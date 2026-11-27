//! Offline LevelDB & Chromium IndexedDB forensic parser (`*.ldb`, `*.sst`, `*.log`, `MANIFEST-*`).
//!
//! Supports:
//! - SSTable (`*.ldb` / `*.sst`) block & index parsing with prefix-compressed keys,
//!   pure-Rust Snappy decompression, and Zstd decompression
//! - Write-Ahead Log (`*.log`) 32 KiB block record reassembly & `WriteBatch` decoding
//! - Recovery of both live entries (`ValueType = 1`) and deleted tombstones (`ValueType = 0`)
//! - Automatic decoding of JSON, UTF-8, UTF-16LE, nested Protobuf/bplist (`blob_decode`),
//!   MetaMask vault payloads, and Fernet (`gAAAAA...`) tokens

use std::sync::Arc;

use anyhow::ensure;
use serde::Serialize;

use crate::core::filesystem::evidence_session::EvidenceSession;
use crate::core::filesystem::tree::fs_tree::{self, NodeKind};
use crate::core::preview::pgdata::{detect_fernet_token, FernetDetection};
use crate::core::preview::sqlite::blob_decode::{self, DecodedBlob};
use crate::core::preview::sqlite::sqlite_preview::{sidecar_nodes, truncate_text};

const LEVELDB_TABLE_MAGIC: u64 = 0xdb47_7524_8b80_fb57;
const LOG_BLOCK_SIZE: usize = 32768;
const LOG_HEADER_SIZE: usize = 7;
const MAX_FILE_READ: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LevelDbEntry {
    pub source_file: String,
    pub sequence: u64,
    pub deleted: bool,
    pub key: String,
    pub value_preview: String,
    pub value_size: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decoded_blob: Option<DecodedBlob>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fernet: Option<FernetDetection>,
    pub metamask_vault: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LevelDbDigest {
    pub files_parsed: Vec<String>,
    pub total_entries: usize,
    pub live_entries: usize,
    pub deleted_entries: usize,
    pub metamask_vaults: usize,
    pub fernet_tokens: usize,
    pub entries: Vec<LevelDbEntry>,
}

/// Check if `bytes` ends with the 8-byte LevelDB SSTable footer magic.
pub fn looks_like_leveldb_sst(bytes: &[u8]) -> bool {
    if bytes.len() < 48 {
        return false;
    }
    let tail = &bytes[bytes.len() - 8..];
    u64::from_le_bytes(tail.try_into().unwrap_or([0; 8])) == LEVELDB_TABLE_MAGIC
}

/// Parse a LevelDB directory or individual `.ldb`/`.sst`/`.log` file node inside `image`.
pub fn preview_leveldb(
    image: &Arc<EvidenceSession>,
    node_id: &str,
    parent_id: Option<&str>,
    query: Option<&str>,
    include_deleted: bool,
    limit: usize,
) -> anyhow::Result<LevelDbDigest> {
    let mut target_files: Vec<(String, String)> = Vec::new(); // (name, node_id)

    let children = fs_tree::children_for_image(image, node_id).unwrap_or_default();
    if !children.is_empty() {
        for c in children {
            if c.kind == NodeKind::File && is_leveldb_data_file(&c.name) {
                target_files.push((c.name, c.id));
            }
        }
    } else if let Some(pid) = parent_id {
        let siblings = fs_tree::children_for_image(image, pid).unwrap_or_default();
        for s in siblings {
            if s.kind == NodeKind::File && is_leveldb_data_file(&s.name) {
                target_files.push((s.name, s.id));
            }
        }
    }

    if target_files.is_empty() {
        let _ = sidecar_nodes;
        target_files.push((node_id.to_string(), node_id.to_string()));
    }

    target_files.sort_by(|a, b| a.0.cmp(&b.0));

    let mut all_entries: Vec<LevelDbEntry> = Vec::new();
    let mut files_parsed = Vec::new();

    for (name, fid) in target_files {
        let Ok(bytes) = fs_tree::read_file_limited(image, &fid, MAX_FILE_READ) else {
            continue;
        };
        if bytes.is_empty() {
            continue;
        }
        if looks_like_leveldb_sst(&bytes) {
            files_parsed.push(name.clone());
            parse_sstable(&name, &bytes, &mut all_entries);
        } else if name.to_ascii_lowercase().ends_with(".log") || fid == node_id {
            let mut log_entries = Vec::new();
            parse_wal_log(&name, &bytes, &mut log_entries);
            if !log_entries.is_empty() {
                files_parsed.push(name.clone());
                all_entries.extend(log_entries);
            }
        }
    }

    ensure!(
        !files_parsed.is_empty(),
        "未识别到有效的 LevelDB SSTable (.ldb/.sst) 或 WAL (.log) 数据"
    );

    // Sort newest sequence number first
    all_entries.sort_by(|a, b| b.sequence.cmp(&a.sequence).then(a.key.cmp(&b.key)));

    let total_entries = all_entries.len();
    let live_entries = all_entries.iter().filter(|e| !e.deleted).count();
    let deleted_entries = all_entries.iter().filter(|e| e.deleted).count();
    let metamask_vaults = all_entries.iter().filter(|e| e.metamask_vault).count();
    let fernet_tokens = all_entries.iter().filter(|e| e.fernet.is_some()).count();

    let q_lower = query
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_lowercase);
    let cap = limit.clamp(1, 2000);
    let entries: Vec<LevelDbEntry> = all_entries
        .into_iter()
        .filter(|e| include_deleted || !e.deleted)
        .filter(|e| {
            let Some(ref q) = q_lower else {
                return true;
            };
            e.key.to_lowercase().contains(q)
                || e.value_preview.to_lowercase().contains(q)
                || e.decoded_blob
                    .as_ref()
                    .is_some_and(|d| d.summary.to_lowercase().contains(q))
        })
        .take(cap)
        .collect();

    Ok(LevelDbDigest {
        files_parsed,
        total_entries,
        live_entries,
        deleted_entries,
        metamask_vaults,
        fernet_tokens,
        entries,
    })
}

fn is_leveldb_data_file(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".ldb") || lower.ends_with(".sst") || lower.ends_with(".log")
}

fn read_varint64(bytes: &[u8], pos: &mut usize) -> Option<u64> {
    let mut result = 0u64;
    let mut shift = 0u32;
    while shift <= 63 {
        let b = *bytes.get(*pos)?;
        *pos += 1;
        result |= u64::from(b & 0x7F) << shift;
        if (b & 0x80) == 0 {
            return Some(result);
        }
        shift += 7;
    }
    None
}

fn read_varint32(bytes: &[u8], pos: &mut usize) -> Option<u32> {
    read_varint64(bytes, pos).and_then(|v| u32::try_from(v).ok())
}

fn decode_block_handle(bytes: &[u8], pos: &mut usize) -> Option<(usize, usize)> {
    let offset = usize::try_from(read_varint64(bytes, pos)?).ok()?;
    let size = usize::try_from(read_varint64(bytes, pos)?).ok()?;
    Some((offset, size))
}

/// Parse an SSTable (`*.ldb` / `*.sst`) file into `out`.
pub fn parse_sstable(source_file: &str, bytes: &[u8], out: &mut Vec<LevelDbEntry>) {
    if !looks_like_leveldb_sst(bytes) {
        return;
    }
    let footer = &bytes[bytes.len() - 48..];
    let mut fpos = 0usize;
    let Some(_metaindex_handle) = decode_block_handle(footer, &mut fpos) else {
        return;
    };
    let Some((index_offset, index_size)) = decode_block_handle(footer, &mut fpos) else {
        return;
    };
    let Some(index_block) = read_table_block(bytes, index_offset, index_size) else {
        return;
    };

    for (_last_key, handle_bytes) in iter_prefix_block(&index_block) {
        let mut hpos = 0usize;
        let Some((data_offset, data_size)) = decode_block_handle(&handle_bytes, &mut hpos) else {
            continue;
        };
        let Some(data_block) = read_table_block(bytes, data_offset, data_size) else {
            continue;
        };
        for (ikey, val_bytes) in iter_prefix_block(&data_block) {
            if ikey.len() < 8 {
                continue;
            }
            let user_key = &ikey[..ikey.len() - 8];
            let tag = u64::from_le_bytes(ikey[ikey.len() - 8..].try_into().unwrap_or([0; 8]));
            let value_type = (tag & 0xFF) as u8;
            let seq = tag >> 8;
            let deleted = value_type == 0;
            out.push(build_entry(source_file, seq, deleted, user_key, &val_bytes));
        }
    }
}

fn read_table_block(file: &[u8], offset: usize, size: usize) -> Option<Vec<u8>> {
    let block_plus_trailer = file.get(offset..offset.checked_add(size)?.checked_add(5)?)?;
    let raw = &block_plus_trailer[..size];
    let compression_type = block_plus_trailer[size];
    match compression_type {
        0 => Some(raw.to_vec()),
        1 => decompress_snappy(raw),
        2 => zstd::stream::decode_all(raw).ok(),
        _ => None,
    }
}

/// Pure-Rust Snappy block decompressor (`compression_type == 1`).
pub fn decompress_snappy(src: &[u8]) -> Option<Vec<u8>> {
    let mut pos = 0usize;
    let uncompressed_len = read_varint32(src, &mut pos)? as usize;
    if uncompressed_len > 16 * 1024 * 1024 {
        return None;
    }
    let mut out = Vec::with_capacity(uncompressed_len.min(65536));
    while pos < src.len() && out.len() < uncompressed_len {
        let tag = src[pos];
        pos += 1;
        match tag & 0x03 {
            0 => {
                // Literal
                let len_code = (tag >> 2) as usize;
                let lit_len = if len_code < 60 {
                    len_code + 1
                } else {
                    let extra_bytes = len_code - 59;
                    let slice = src.get(pos..pos + extra_bytes)?;
                    pos += extra_bytes;
                    let mut n = 0usize;
                    for (i, &b) in slice.iter().enumerate() {
                        n |= (b as usize) << (i * 8);
                    }
                    n + 1
                };
                let lits = src.get(pos..pos + lit_len)?;
                pos += lit_len;
                out.extend_from_slice(lits);
            }
            1 => {
                // Copy with 1-byte offset (length 4..11, offset 0..2047)
                let length = (((tag >> 2) & 0x07) as usize) + 4;
                let next = *src.get(pos)? as usize;
                pos += 1;
                let offset = (((tag >> 5) as usize) << 8) | next;
                copy_from_history(&mut out, offset, length)?;
            }
            2 => {
                // Copy with 2-byte offset (length 1..64)
                let length = ((tag >> 2) as usize) + 1;
                let lo = *src.get(pos)? as usize;
                let hi = *src.get(pos + 1)? as usize;
                pos += 2;
                let offset = lo | (hi << 8);
                copy_from_history(&mut out, offset, length)?;
            }
            3 => {
                // Copy with 4-byte offset
                let length = ((tag >> 2) as usize) + 1;
                let s = src.get(pos..pos + 4)?;
                pos += 4;
                let offset = u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as usize;
                copy_from_history(&mut out, offset, length)?;
            }
            _ => unreachable!(),
        }
    }
    (out.len() == uncompressed_len).then_some(out)
}

fn copy_from_history(out: &mut Vec<u8>, offset: usize, length: usize) -> Option<()> {
    if offset == 0 || offset > out.len() {
        return None;
    }
    for _ in 0..length {
        let b = out[out.len() - offset];
        out.push(b);
    }
    Some(())
}

fn iter_prefix_block(block: &[u8]) -> Vec<(Vec<u8>, Vec<u8>)> {
    let mut entries = Vec::new();
    if block.len() < 4 {
        return entries;
    }
    let num_restarts =
        u32::from_le_bytes(block[block.len() - 4..].try_into().unwrap_or([0; 4])) as usize;
    let restarts_bytes = num_restarts.saturating_mul(4).saturating_add(4);
    if restarts_bytes > block.len() {
        return entries;
    }
    let limit = block.len() - restarts_bytes;
    let mut pos = 0usize;
    let mut last_key: Vec<u8> = Vec::new();

    while pos < limit {
        let Some(shared) = read_varint32(block, &mut pos) else {
            break;
        };
        let Some(non_shared) = read_varint32(block, &mut pos) else {
            break;
        };
        let Some(value_len) = read_varint32(block, &mut pos) else {
            break;
        };
        let shared = shared as usize;
        let non_shared = non_shared as usize;
        let value_len = value_len as usize;
        if shared > last_key.len() || pos + non_shared + value_len > limit {
            break;
        }
        last_key.truncate(shared);
        last_key.extend_from_slice(&block[pos..pos + non_shared]);
        pos += non_shared;
        let val = block[pos..pos + value_len].to_vec();
        pos += value_len;
        entries.push((last_key.clone(), val));
    }

    entries
}

/// Parse a LevelDB Write-Ahead Log (`*.log`) into `out`.
pub fn parse_wal_log(source_file: &str, bytes: &[u8], out: &mut Vec<LevelDbEntry>) {
    let mut batch_buf: Vec<u8> = Vec::new();
    for block in bytes.chunks(LOG_BLOCK_SIZE) {
        let mut pos = 0usize;
        while pos + LOG_HEADER_SIZE <= block.len() {
            let length = u16::from_le_bytes([block[pos + 4], block[pos + 5]]) as usize;
            let rec_type = block[pos + 6];
            pos += LOG_HEADER_SIZE;
            let Some(fragment) = block.get(pos..pos + length) else {
                break;
            };
            pos += length;
            match rec_type {
                1 => {
                    // Full
                    parse_write_batch(source_file, fragment, out);
                }
                2 => {
                    // First
                    batch_buf.clear();
                    batch_buf.extend_from_slice(fragment);
                }
                3 => {
                    // Middle
                    batch_buf.extend_from_slice(fragment);
                }
                4 => {
                    // Last
                    batch_buf.extend_from_slice(fragment);
                    parse_write_batch(source_file, &batch_buf, out);
                    batch_buf.clear();
                }
                _ => break,
            }
        }
    }
}

fn parse_write_batch(source_file: &str, batch: &[u8], out: &mut Vec<LevelDbEntry>) {
    if batch.len() < 12 {
        return;
    }
    let base_seq = u64::from_le_bytes(batch[0..8].try_into().unwrap_or([0; 8]));
    let count = u32::from_le_bytes(batch[8..12].try_into().unwrap_or([0; 4]));
    let mut pos = 12usize;
    let mut seq = base_seq;

    for _ in 0..count.min(100_000) {
        let Some(&tag) = batch.get(pos) else {
            break;
        };
        pos += 1;
        let Some(klen) = read_varint32(batch, &mut pos) else {
            break;
        };
        let klen = klen as usize;
        let Some(key) = batch.get(pos..pos + klen) else {
            break;
        };
        pos += klen;

        match tag {
            1 => {
                // kTypeValue
                let Some(vlen) = read_varint32(batch, &mut pos) else {
                    break;
                };
                let vlen = vlen as usize;
                let Some(val) = batch.get(pos..pos + vlen) else {
                    break;
                };
                pos += vlen;
                out.push(build_entry(source_file, seq, false, key, val));
            }
            0 => {
                // kTypeDeletion
                out.push(build_entry(source_file, seq, true, key, &[]));
            }
            _ => break,
        }
        seq = seq.saturating_add(1);
    }
}

fn format_readable_bytes(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    if let Ok(s) = std::str::from_utf8(bytes) {
        if s.chars()
            .filter(|c| !c.is_control() || c.is_whitespace())
            .count()
            * 4
            >= s.chars().count() * 3
        {
            return truncate_text(s);
        }
    }
    // Check Chromium IndexedDB / UTF-16LE strings
    let ascii_run: String = bytes
        .iter()
        .copied()
        .map(|b| {
            if (0x20..=0x7E).contains(&b) {
                b as char
            } else {
                '.'
            }
        })
        .collect();
    if ascii_run.chars().filter(|&c| c != '.').count() >= 6 {
        return truncate_text(&ascii_run);
    }
    format!("0x{}", hex::encode(&bytes[..bytes.len().min(64)]))
}

fn build_entry(
    source_file: &str,
    sequence: u64,
    deleted: bool,
    raw_key: &[u8],
    raw_val: &[u8],
) -> LevelDbEntry {
    let key = format_readable_bytes(raw_key);
    let value_preview = if deleted {
        "<TOMBSTONE / 已删除>".to_string()
    } else {
        format_readable_bytes(raw_val)
    };
    let decoded_blob = if !deleted && raw_val.len() <= 256 * 1024 {
        blob_decode::decode_blob(raw_val)
    } else {
        None
    };
    let fernet =
        detect_fernet_token(&value_preview, &key).or_else(|| detect_fernet_token(&key, "key"));
    let metamask_vault = (key.contains("KeyringController")
        || value_preview.contains("KeyringController"))
        || (value_preview.contains("\"data\"")
            && value_preview.contains("\"iv\"")
            && value_preview.contains("\"salt\""));

    LevelDbEntry {
        source_file: source_file.to_string(),
        sequence,
        deleted,
        key,
        value_preview,
        value_size: raw_val.len(),
        decoded_blob,
        fernet,
        metamask_vault,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_leveldb_wal_log_put_delete_metamask_and_fernet() {
        // Construct a valid LevelDB WAL .log record with 2 operations:
        // 1) Put("KeyringController", "{\"data\":\"abc\",\"iv\":\"123\",\"salt\":\"456\"}")
        // 2) Delete("old_session_token")
        let mut batch = Vec::new();
        batch.extend_from_slice(&500u64.to_le_bytes()); // base sequence = 500
        batch.extend_from_slice(&2u32.to_le_bytes()); // count = 2

        let k1 = b"KeyringController";
        let v1 = br#"{"data":"cipher","iv":"1122","salt":"3344"}"#;
        batch.push(1); // Put
        batch.push(k1.len() as u8);
        batch.extend_from_slice(k1);
        batch.push(v1.len() as u8);
        batch.extend_from_slice(v1);

        let k2 = b"old_session_token";
        batch.push(0); // Delete
        batch.push(k2.len() as u8);
        batch.extend_from_slice(k2);

        let mut log = Vec::new();
        log.extend_from_slice(&0u32.to_le_bytes()); // crc
        log.extend_from_slice(&(batch.len() as u16).to_le_bytes());
        log.push(1); // Full record
        log.extend_from_slice(&batch);

        let mut entries = Vec::new();
        parse_wal_log("000003.log", &log, &mut entries);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].key, "KeyringController");
        assert!(entries[0].metamask_vault);
        assert!(entries[0].decoded_blob.is_some());
        assert_eq!(entries[1].key, "old_session_token");
        assert!(entries[1].deleted);
    }

    #[test]
    fn decompresses_snappy_literal_and_backref() {
        // Uncompressed length = 10 ("hellohello"):
        // varint(10) = 0x0A
        // literal len=5 (code=4 => (4<<2)|0 = 0x10), "hello"
        // 1-byte copy len=5 (code=1 => (1<<2)|1 = 0x05), offset=5 (0x05)
        let snappy = [0x0Au8, 0x10, b'h', b'e', b'l', b'l', b'o', 0x05, 0x05];
        let dec = decompress_snappy(&snappy).expect("snappy decompressed");
        assert_eq!(&dec, b"hellohello");
    }
}
