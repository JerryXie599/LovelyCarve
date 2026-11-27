//! Native binary `systemd-journal` (`*.journal`, magic `LPKSHHRH`) parser.
//!
//! Supports:
//! - Both classic 64-bit (`!COMPACT`) and modern `HEADER_INCOMPATIBLE_COMPACT` (`systemd >= 252`)
//!   object layouts
//! - Uncompressed, LZ4-block-compressed (`OBJECT_COMPRESSED_LZ4`), and ZSTD-compressed
//!   (`OBJECT_COMPRESSED_ZSTD`) `OBJECT_DATA` payloads
//! - Full `OBJECT_ENTRY` reconstruction with `seqnum`, `realtime` (UTC+8), `PRIORITY`,
//!   `_SYSTEMD_UNIT`, `SYSLOG_IDENTIFIER`, `_COMM`, `_PID`, `_UID`, and `MESSAGE`

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::ensure;
use serde::Serialize;

use crate::core::filesystem::evidence_session::EvidenceSession;
use crate::core::filesystem::tree::fs_tree;
use crate::core::preview::sqlite::sqlite_preview::truncate_text;

pub const JOURNAL_MAGIC: &[u8; 8] = b"LPKSHHRH";
const HEADER_INCOMPATIBLE_COMPACT: u32 = 1 << 4;
const OBJECT_DATA: u8 = 1;
const OBJECT_ENTRY: u8 = 3;
const OBJECT_COMPRESSED_XZ: u8 = 1 << 0;
const OBJECT_COMPRESSED_LZ4: u8 = 1 << 1;
const OBJECT_COMPRESSED_ZSTD: u8 = 1 << 2;
const MAX_JOURNAL_BYTES: usize = 128 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JournalRecord {
    pub seqnum: u64,
    pub timestamp_us: u64,
    pub timestamp_utc8: String,
    pub priority: u8,
    pub unit: String,
    pub identifier: String,
    pub pid: String,
    pub uid: String,
    pub hostname: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JournalDigest {
    pub state: String,
    pub machine_id: String,
    pub boot_id: String,
    pub compact_mode: bool,
    pub declared_entries: u64,
    pub parsed_entries: usize,
    pub head_time_utc8: Option<String>,
    pub tail_time_utc8: Option<String>,
    pub records: Vec<JournalRecord>,
}

pub fn looks_like_systemd_journal(bytes: &[u8]) -> bool {
    bytes.len() >= 208 && &bytes[0..8] == JOURNAL_MAGIC
}

pub fn preview_systemd_journal(
    image: &Arc<EvidenceSession>,
    node_id: &str,
    query: Option<&str>,
    priority_max: Option<u8>,
    limit: usize,
) -> anyhow::Result<JournalDigest> {
    let bytes = fs_tree::read_file_limited(image, node_id, MAX_JOURNAL_BYTES)?;
    parse_systemd_journal_bytes(&bytes, query, priority_max, limit)
}

pub fn parse_systemd_journal_bytes(
    bytes: &[u8],
    query: Option<&str>,
    priority_max: Option<u8>,
    limit: usize,
) -> anyhow::Result<JournalDigest> {
    ensure!(
        looks_like_systemd_journal(bytes),
        "不是有效的 systemd-journal 二进制日志 (缺少 LPKSHHRH 魔数)"
    );

    let incompat = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
    let compact_mode = (incompat & HEADER_INCOMPATIBLE_COMPACT) != 0;
    let state_code = bytes[16];
    let state = match state_code {
        0 => "OFFLINE",
        1 => "ONLINE",
        2 => "ARCHIVED",
        _ => "UNKNOWN",
    }
    .to_string();

    let machine_id = hex::encode(&bytes[40..56]);
    let boot_id = hex::encode(&bytes[56..72]);
    let header_size = u64::from_le_bytes(bytes[88..96].try_into().unwrap_or([0; 8])) as usize;
    let arena_size = u64::from_le_bytes(bytes[96..104].try_into().unwrap_or([0; 8])) as usize;
    let declared_entries = u64::from_le_bytes(bytes[152..160].try_into().unwrap_or([0; 8]));
    let head_rt = u64::from_le_bytes(bytes[184..192].try_into().unwrap_or([0; 8]));
    let tail_rt = u64::from_le_bytes(bytes[192..200].try_into().unwrap_or([0; 8]));

    let start_off = header_size.max(208);
    let end_off = start_off
        .saturating_add(arena_size)
        .min(bytes.len())
        .max(bytes.len());

    // Pass 1: Index all OBJECT_DATA offsets -> decoded `KEY=VALUE` string
    let mut data_objects: HashMap<u64, String> = HashMap::new();
    let mut entry_offsets: Vec<usize> = Vec::new();

    let mut off = start_off;
    while off + 16 <= end_off && off + 16 <= bytes.len() {
        let obj_type = bytes[off];
        let obj_flags = bytes[off + 1];
        let obj_size =
            u64::from_le_bytes(bytes[off + 8..off + 16].try_into().unwrap_or([0; 8])) as usize;
        if obj_size < 16 || off + obj_size > bytes.len() {
            break;
        }
        match obj_type {
            OBJECT_DATA => {
                if let Some(kv) =
                    decode_data_object(&bytes[off..off + obj_size], obj_flags, compact_mode)
                {
                    data_objects.insert(off as u64, kv);
                }
            }
            OBJECT_ENTRY => {
                entry_offsets.push(off);
            }
            _ => {}
        }
        off = (off + obj_size + 7) & !7;
    }

    // Pass 2: Reconstruct OBJECT_ENTRY records
    let mut all_records = Vec::with_capacity(entry_offsets.len());
    for e_off in entry_offsets {
        let obj_size =
            u64::from_le_bytes(bytes[e_off + 8..e_off + 16].try_into().unwrap_or([0; 8])) as usize;
        if obj_size < 64 || e_off + obj_size > bytes.len() {
            continue;
        }
        let entry = &bytes[e_off..e_off + obj_size];
        let seqnum = u64::from_le_bytes(entry[16..24].try_into().unwrap_or([0; 8]));
        let realtime = u64::from_le_bytes(entry[24..32].try_into().unwrap_or([0; 8]));
        let items_slice = &entry[64..];

        let mut priority = 6u8;
        let mut unit = String::new();
        let mut identifier = String::new();
        let mut pid = String::new();
        let mut uid = String::new();
        let mut hostname = String::new();
        let mut message = String::new();

        let data_refs: Vec<u64> = if compact_mode {
            items_slice
                .chunks_exact(4)
                .map(|c| u64::from(u32::from_le_bytes([c[0], c[1], c[2], c[3]])))
                .collect()
        } else {
            items_slice
                .chunks_exact(16)
                .map(|c| u64::from_le_bytes(c[0..8].try_into().unwrap_or([0; 8])))
                .collect()
        };

        for d_off in data_refs {
            let Some(kv) = data_objects.get(&d_off) else {
                continue;
            };
            let Some((k, v)) = kv.split_once('=') else {
                continue;
            };
            match k {
                "MESSAGE" => message = truncate_text(v),
                "PRIORITY" => priority = v.trim().parse::<u8>().unwrap_or(6),
                "_SYSTEMD_UNIT" | "UNIT" if unit.is_empty() => unit = v.to_string(),
                "SYSLOG_IDENTIFIER" | "_COMM" if identifier.is_empty() => {
                    identifier = v.to_string()
                }
                "_PID" | "SYSLOG_PID" if pid.is_empty() => pid = v.to_string(),
                "_UID" if uid.is_empty() => uid = v.to_string(),
                "_HOSTNAME" if hostname.is_empty() => hostname = v.to_string(),
                _ => {}
            }
        }

        if message.is_empty() && unit.is_empty() && identifier.is_empty() {
            continue;
        }

        all_records.push(JournalRecord {
            seqnum,
            timestamp_us: realtime,
            timestamp_utc8: format_us_utc8(realtime).unwrap_or_default(),
            priority,
            unit,
            identifier,
            pid,
            uid,
            hostname,
            message,
        });
    }

    let parsed_entries = all_records.len();
    let q_lower = query
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_lowercase);
    let cap = limit.clamp(1, 2000);

    let records: Vec<JournalRecord> = all_records
        .into_iter()
        .filter(|r| priority_max.is_none_or(|m| r.priority <= m))
        .filter(|r| {
            let Some(ref q) = q_lower else {
                return true;
            };
            r.message.to_lowercase().contains(q)
                || r.unit.to_lowercase().contains(q)
                || r.identifier.to_lowercase().contains(q)
                || r.pid.contains(q)
        })
        .take(cap)
        .collect();

    Ok(JournalDigest {
        state,
        machine_id,
        boot_id,
        compact_mode,
        declared_entries,
        parsed_entries,
        head_time_utc8: (head_rt > 0).then(|| format_us_utc8(head_rt)).flatten(),
        tail_time_utc8: (tail_rt > 0).then(|| format_us_utc8(tail_rt)).flatten(),
        records,
    })
}

fn decode_data_object(obj: &[u8], flags: u8, compact_mode: bool) -> Option<String> {
    let hdr_len = if compact_mode && obj.len() >= 72 {
        // Check whether payload starts at 64 or 72 in compact mode
        if obj.get(64..72).is_some_and(|s| s.contains(&b'=')) {
            64
        } else {
            72
        }
    } else {
        64
    };
    let raw = obj.get(hdr_len..)?;
    if raw.is_empty() {
        return None;
    }

    let decompressed = if (flags & OBJECT_COMPRESSED_ZSTD) != 0 {
        zstd::stream::decode_all(raw).ok()?
    } else if (flags & OBJECT_COMPRESSED_LZ4) != 0 {
        if raw.len() < 8 {
            return None;
        }
        let uncomp_len = u64::from_le_bytes(raw[0..8].try_into().ok()?) as usize;
        if uncomp_len > 4 * 1024 * 1024 {
            return None;
        }
        decompress_lz4_raw(&raw[8..], uncomp_len)?
    } else if (flags & OBJECT_COMPRESSED_XZ) != 0 {
        return None;
    } else {
        raw.to_vec()
    };

    Some(String::from_utf8_lossy(&decompressed).into_owned())
}

fn decompress_lz4_raw(src: &[u8], max_len: usize) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(max_len.min(65536));
    let mut pos = 0usize;
    while pos < src.len() && out.len() <= max_len {
        let token = src[pos];
        pos += 1;
        let mut lit_len = (token >> 4) as usize;
        if lit_len == 15 {
            loop {
                let extra = *src.get(pos)?;
                pos += 1;
                lit_len = lit_len.checked_add(extra as usize)?;
                if extra != 255 {
                    break;
                }
            }
        }
        let lits = src.get(pos..pos + lit_len)?;
        pos += lit_len;
        out.extend_from_slice(lits);
        if pos >= src.len() {
            break;
        }
        let offset = u16::from_le_bytes([*src.get(pos)?, *src.get(pos + 1)?]) as usize;
        pos += 2;
        if offset == 0 || offset > out.len() {
            return None;
        }
        let mut match_len = ((token & 0x0F) as usize) + 4;
        if (token & 0x0F) == 15 {
            loop {
                let extra = *src.get(pos)?;
                pos += 1;
                match_len = match_len.checked_add(extra as usize)?;
                if extra != 255 {
                    break;
                }
            }
        }
        for _ in 0..match_len {
            let b = out[out.len() - offset];
            out.push(b);
        }
    }
    Some(out)
}

fn format_us_utc8(us: u64) -> Option<String> {
    use chrono::{FixedOffset, TimeZone};
    let secs = (us / 1_000_000) as i64;
    let nanos = ((us % 1_000_000) as u32) * 1_000;
    let tz = FixedOffset::east_opt(8 * 3600)?;
    let dt = tz.timestamp_opt(secs, nanos).single()?;
    Some(dt.format("%Y-%m-%d %H:%M:%S%.3f UTC+08").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_synthetic_systemd_journal_objects_and_entries() {
        let mut buf = vec![0u8; 240];
        buf[0..8].copy_from_slice(JOURNAL_MAGIC);
        buf[16] = 1; // ONLINE
        buf[40..56].copy_from_slice(&[0xAA; 16]); // machine_id
        buf[56..72].copy_from_slice(&[0xBB; 16]); // boot_id
        buf[88..96].copy_from_slice(&240u64.to_le_bytes()); // header_size
        buf[152..160].copy_from_slice(&1u64.to_le_bytes()); // n_entries = 1

        // Object 1 at offset 240: OBJECT_DATA "MESSAGE=Accepted password for root from 10.0.0.5"
        let off_msg = buf.len() as u64;
        let msg_payload = b"MESSAGE=Accepted password for root from 10.0.0.5";
        let obj1_size = 64 + msg_payload.len();
        let mut obj1 = vec![0u8; (obj1_size + 7) & !7];
        obj1[0] = OBJECT_DATA;
        obj1[8..16].copy_from_slice(&(obj1_size as u64).to_le_bytes());
        obj1[64..64 + msg_payload.len()].copy_from_slice(msg_payload);
        buf.extend_from_slice(&obj1);

        // Object 2: OBJECT_DATA "_SYSTEMD_UNIT=sshd.service"
        let off_unit = buf.len() as u64;
        let unit_payload = b"_SYSTEMD_UNIT=sshd.service";
        let obj2_size = 64 + unit_payload.len();
        let mut obj2 = vec![0u8; (obj2_size + 7) & !7];
        obj2[0] = OBJECT_DATA;
        obj2[8..16].copy_from_slice(&(obj2_size as u64).to_le_bytes());
        obj2[64..64 + unit_payload.len()].copy_from_slice(unit_payload);
        buf.extend_from_slice(&obj2);

        // Object 3: OBJECT_ENTRY referencing off_msg and off_unit
        let entry_size = 64 + 16 * 2;
        let mut entry = vec![0u8; entry_size];
        entry[0] = OBJECT_ENTRY;
        entry[8..16].copy_from_slice(&(entry_size as u64).to_le_bytes());
        entry[16..24].copy_from_slice(&42u64.to_le_bytes()); // seqnum = 42
        entry[24..32].copy_from_slice(&1_700_000_000_000_000u64.to_le_bytes()); // realtime
        entry[64..72].copy_from_slice(&off_msg.to_le_bytes());
        entry[80..88].copy_from_slice(&off_unit.to_le_bytes());
        buf.extend_from_slice(&entry);

        let digest = parse_systemd_journal_bytes(&buf, Some("root"), None, 100).unwrap();
        assert_eq!(digest.parsed_entries, 1);
        assert_eq!(digest.records.len(), 1);
        assert_eq!(digest.records[0].seqnum, 42);
        assert_eq!(digest.records[0].unit, "sshd.service");
        assert!(digest.records[0]
            .message
            .contains("Accepted password for root"));
    }
}
