//! Virtual filesystem entry builders, O(1) slice reader, and TAR indexer for HarmonyOS NEXT backups.

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use tokio_util::sync::CancellationToken;

use super::harmony_crypto::aes256_gcm_ctr_read_at;
use super::*;

#[derive(Debug, Clone)]
pub(super) enum HarmonyBacking {
    Memory(Vec<u8>),
    PlainRange {
        path: PathBuf,
        offset: u64,
        len: u64,
    },
    EncryptedRange {
        path: PathBuf,
        key: [u8; 32],
        j0: [u8; 16],
        base_plain_offset: u64,
        len: u64,
    },
}

impl HarmonyBacking {
    pub(super) fn read_range(&self, offset: u64, max_bytes: usize) -> Result<Vec<u8>> {
        if max_bytes == 0 {
            return Ok(Vec::new());
        }
        match self {
            Self::Memory(bytes) => {
                let start = usize::try_from(offset)
                    .unwrap_or(bytes.len())
                    .min(bytes.len());
                let end = start.saturating_add(max_bytes).min(bytes.len());
                Ok(bytes[start..end].to_vec())
            }
            Self::PlainRange {
                path,
                offset: base,
                len,
            } => read_plain_slice(path, *base, *len, offset, max_bytes),
            Self::EncryptedRange {
                path,
                key,
                j0,
                base_plain_offset,
                len,
            } => aes256_gcm_ctr_read_at(key, j0, path, *base_plain_offset, *len, offset, max_bytes),
        }
    }

    pub(super) fn size(&self) -> u64 {
        match self {
            Self::Memory(bytes) => bytes.len() as u64,
            Self::PlainRange { len, .. } | Self::EncryptedRange { len, .. } => *len,
        }
    }
}

pub(super) fn sanitize_rel_path(raw: &str) -> Option<String> {
    let mut clean = Vec::new();
    let normalized = raw.replace('\\', "/");
    for seg in normalized.split('/') {
        let s = seg.trim();
        if s.is_empty() || s == "." {
            continue;
        }
        if s == ".." || s.contains([':', '\0']) {
            return None;
        }
        clean.push(s);
    }
    if clean.is_empty() {
        None
    } else {
        Some(clean.join("/"))
    }
}

pub(super) fn ensure_parent_dirs(
    entries: &mut BTreeMap<String, FolderEntry>,
    rel_path: &str,
) -> Result<()> {
    let mut cur = rel_path;
    while let Some((prefix, _)) = cur.rsplit_once('/') {
        if let Some(existing) = entries.get(prefix) {
            if existing.kind != FolderEntryKind::Dir {
                bail!("HarmonyOS backup parent path conflict: {prefix}");
            }
        } else {
            entries.insert(prefix.to_owned(), directory_entry(prefix.to_owned()));
        }
        cur = prefix;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn insert_harmony_file(
    entries: &mut BTreeMap<String, FolderEntry>,
    total_size: &mut u64,
    file_count: &mut usize,
    rel: String,
    size: u64,
    modified: Option<SystemTime>,
    accessed: Option<SystemTime>,
    mode: Option<u32>,
    source: FolderEntrySource,
) -> Result<()> {
    ensure_parent_dirs(entries, &rel)?;
    if entries.len() >= MAX_ENTRIES {
        bail!("HarmonyOS backup exceeded {MAX_ENTRIES} entries limit");
    }
    *total_size = total_size.saturating_add(size);
    *file_count += 1;
    entries.insert(
        rel.clone(),
        FolderEntry {
            rel_path: rel,
            kind: FolderEntryKind::File,
            size,
            modified,
            created: None,
            accessed,
            status_changed: None,
            inode: None,
            mode,
            link_target: None,
            source: Some(source),
        },
    );
    Ok(())
}

pub(super) fn insert_host_if_absent(
    entries: &mut BTreeMap<String, FolderEntry>,
    total_size: &mut u64,
    file_count: &mut usize,
    rel: String,
    path: &Path,
    meta: &fs::Metadata,
) -> Result<()> {
    if entries.contains_key(&rel) {
        return Ok(());
    }
    ensure_parent_dirs(entries, &rel)?;
    if entries.len() >= MAX_ENTRIES {
        bail!("HarmonyOS backup exceeded {MAX_ENTRIES} entries limit");
    }
    let size = meta.len();
    *total_size = total_size.saturating_add(size);
    *file_count += 1;
    entries.insert(rel.clone(), host_file_entry(rel, path, meta));
    Ok(())
}

pub(super) fn host_file_entry(rel_path: String, path: &Path, meta: &fs::Metadata) -> FolderEntry {
    FolderEntry {
        rel_path,
        kind: FolderEntryKind::File,
        size: meta.len(),
        modified: meta.modified().ok(),
        created: meta.created().ok(),
        accessed: meta.accessed().ok(),
        status_changed: None,
        inode: None,
        mode: None,
        link_target: None,
        source: Some(FolderEntrySource::Host(path.to_path_buf())),
    }
}

pub(super) fn directory_entry(rel_path: String) -> FolderEntry {
    FolderEntry {
        rel_path,
        kind: FolderEntryKind::Dir,
        size: 0,
        modified: None,
        created: None,
        accessed: None,
        status_changed: None,
        inode: None,
        mode: None,
        link_target: None,
        source: None,
    }
}

pub(super) fn unix_time(seconds: Option<i64>) -> Option<SystemTime> {
    let seconds = seconds?;
    if seconds >= 0 {
        UNIX_EPOCH.checked_add(Duration::from_secs(seconds as u64))
    } else {
        UNIX_EPOCH.checked_sub(Duration::from_secs(seconds.unsigned_abs()))
    }
}

fn read_plain_slice(
    path: &Path,
    base_offset: u64,
    len: u64,
    offset: u64,
    max_bytes: usize,
) -> Result<Vec<u8>> {
    if offset >= len || max_bytes == 0 {
        return Ok(Vec::new());
    }
    let want = usize::try_from((len - offset).min(max_bytes as u64)).unwrap_or(max_bytes);
    let abs = base_offset
        .checked_add(offset)
        .context("HarmonyOS slice offset overflow")?;
    let mut f = File::open(path).with_context(|| format!("open {}", path.display()))?;
    f.seek(SeekFrom::Start(abs))?;
    let mut buf = vec![0u8; want];
    let mut filled = 0;
    while filled < buf.len() {
        let n = f.read(&mut buf[filled..])?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    buf.truncate(filled);
    Ok(buf)
}

fn read_tar_chunk(
    path: &Path,
    cipher: Option<([u8; 32], [u8; 16])>,
    plain_len: u64,
    offset: u64,
    len: usize,
) -> Result<Vec<u8>> {
    match cipher {
        Some((key, j0)) => aes256_gcm_ctr_read_at(&key, &j0, path, 0, plain_len, offset, len),
        None => read_plain_slice(path, 0, plain_len, offset, len),
    }
}

fn parse_tar_octal_or_base256(field: &[u8]) -> Option<u64> {
    if field.first().is_some_and(|&b| (b & 0x80) != 0) {
        let mut val = u64::from(field[0] & 0x7f);
        for &b in &field[1..] {
            val = (val << 8) | u64::from(b);
        }
        return Some(val);
    }
    let s = std::str::from_utf8(field)
        .ok()?
        .trim_matches(|c: char| c == '\0' || c.is_ascii_whitespace());
    if s.is_empty() {
        Some(0)
    } else {
        u64::from_str_radix(s, 8).ok()
    }
}

fn cstr_utf8(slice: &[u8]) -> String {
    let end = slice.iter().position(|&b| b == 0).unwrap_or(slice.len());
    String::from_utf8_lossy(&slice[..end]).into_owned()
}

fn validate_tar_header_checksum(hdr: &[u8]) -> bool {
    if hdr.len() < 512 {
        return false;
    }
    let Some(expected) = parse_tar_octal_or_base256(&hdr[148..156]) else {
        return false;
    };
    let unsigned_sum: u64 = hdr
        .iter()
        .enumerate()
        .map(|(i, &b)| u64::from(if (148..156).contains(&i) { b' ' } else { b }))
        .sum();
    if expected == unsigned_sum {
        return true;
    }
    let signed_sum: i64 = hdr
        .iter()
        .enumerate()
        .map(|(i, &b)| i64::from(if (148..156).contains(&i) { b' ' } else { b } as i8))
        .sum();
    i64::try_from(expected).is_ok_and(|e| e == signed_sum)
}

fn parse_pax_fields(data: &[u8]) -> (Option<String>, Option<String>, Option<u64>) {
    let mut path = None;
    let mut linkpath = None;
    let mut size = None;
    let mut pos = 0;
    while pos < data.len() {
        let Some(sp) = data[pos..].iter().position(|&b| b == b' ') else {
            break;
        };
        let Ok(rec_len) = std::str::from_utf8(&data[pos..pos + sp])
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .ok_or(())
        else {
            break;
        };
        if rec_len == 0 || pos + rec_len > data.len() {
            break;
        }
        let body = &data[pos + sp + 1..pos + rec_len];
        let body = body.strip_suffix(b"\n").unwrap_or(body);
        if let Some(eq) = body.iter().position(|&b| b == b'=') {
            let k = std::str::from_utf8(&body[..eq]).unwrap_or("");
            let v = String::from_utf8_lossy(&body[eq + 1..]).into_owned();
            match k {
                "path" => path = Some(v),
                "linkpath" => linkpath = Some(v),
                "size" => size = v.parse::<u64>().ok(),
                _ => {}
            }
        }
        pos += rec_len;
    }
    (path, linkpath, size)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn index_tar_into_entries(
    tar_path: &Path,
    disk_len: u64,
    cipher: Option<([u8; 32], [u8; 16])>,
    mod_prefix: &str,
    entries: &mut BTreeMap<String, FolderEntry>,
    backing_files: &mut Vec<HarmonyBacking>,
    total_size: &mut u64,
    file_count: &mut usize,
    cancel: &CancellationToken,
) -> Result<()> {
    let plain_len = if cipher.is_some() {
        disk_len.saturating_sub(16)
    } else {
        disk_len
    };
    let mut offset = 0u64;
    let mut long_name: Option<String> = None;
    let mut long_link: Option<String> = None;
    let mut pax_path: Option<String> = None;
    let mut pax_link: Option<String> = None;
    let mut pax_size: Option<u64> = None;

    while offset + 512 <= plain_len {
        if cancel.is_cancelled() {
            bail!("HarmonyOS backup unlock cancelled");
        }
        let hdr = read_tar_chunk(tar_path, cipher, plain_len, offset, 512)?;
        if hdr.len() < 512 || hdr.iter().all(|&b| b == 0) || !validate_tar_header_checksum(&hdr) {
            break;
        }
        offset += 512;

        let typeflag = hdr[156];
        let mut size = parse_tar_octal_or_base256(&hdr[124..136]).unwrap_or(0);
        if let Some(ps) = pax_size.take() {
            size = ps;
        }
        let mtime = parse_tar_octal_or_base256(&hdr[136..148])
            .and_then(|s| unix_time(i64::try_from(s).ok()));
        let mode = parse_tar_octal_or_base256(&hdr[100..108]).and_then(|m| u32::try_from(m).ok());

        let capped = size.min(plain_len.saturating_sub(offset));
        let padded = capped.div_ceil(512).saturating_mul(512);

        match typeflag {
            b'L' => {
                if capped <= 64 * 1024 {
                    let data =
                        read_tar_chunk(tar_path, cipher, plain_len, offset, capped as usize)?;
                    long_name = Some(cstr_utf8(&data));
                }
                offset += padded;
                continue;
            }
            b'K' => {
                if capped <= 64 * 1024 {
                    let data =
                        read_tar_chunk(tar_path, cipher, plain_len, offset, capped as usize)?;
                    long_link = Some(cstr_utf8(&data));
                }
                offset += padded;
                continue;
            }
            b'x' => {
                if capped <= 256 * 1024 {
                    let data =
                        read_tar_chunk(tar_path, cipher, plain_len, offset, capped as usize)?;
                    let (p, l, s) = parse_pax_fields(&data);
                    pax_path = p;
                    pax_link = l;
                    pax_size = s;
                }
                offset += padded;
                continue;
            }
            b'g' => {
                offset += padded;
                continue;
            }
            _ => {}
        }

        let name_part = cstr_utf8(&hdr[0..100]);
        let prefix_part = cstr_utf8(&hdr[345..500]);
        let raw_name = pax_path
            .take()
            .or_else(|| long_name.take())
            .unwrap_or_else(|| {
                if prefix_part.is_empty() {
                    name_part
                } else {
                    format!("{prefix_part}/{name_part}")
                }
            });
        let link_target = pax_link.take().or_else(|| long_link.take()).or_else(|| {
            let l = cstr_utf8(&hdr[157..257]);
            (!l.is_empty()).then_some(l)
        });

        let has_payload = !matches!(typeflag, b'1' | b'2' | b'3' | b'4' | b'5' | b'6');
        let data_offset = offset;
        if has_payload {
            offset = offset.saturating_add(padded);
        }

        let Some(rel_inner) = sanitize_rel_path(&raw_name) else {
            continue;
        };
        let rel = format!("{mod_prefix}/{rel_inner}");

        if typeflag == b'5' {
            ensure_parent_dirs(entries, &format!("{rel}/.dir"))?;
            continue;
        }
        if typeflag == b'2' {
            ensure_parent_dirs(entries, &rel)?;
            if entries.len() < MAX_ENTRIES {
                entries.insert(
                    rel.clone(),
                    FolderEntry {
                        rel_path: rel,
                        kind: FolderEntryKind::Symlink,
                        size: 0,
                        modified: mtime,
                        created: None,
                        accessed: None,
                        status_changed: None,
                        inode: None,
                        mode,
                        link_target,
                        source: None,
                    },
                );
            }
            continue;
        }
        if !has_payload {
            continue;
        }

        let idx = backing_files.len();
        let backing = match cipher {
            Some((key, j0)) => HarmonyBacking::EncryptedRange {
                path: tar_path.to_path_buf(),
                key,
                j0,
                base_plain_offset: data_offset,
                len: capped,
            },
            None => HarmonyBacking::PlainRange {
                path: tar_path.to_path_buf(),
                offset: data_offset,
                len: capped,
            },
        };
        backing_files.push(backing);
        insert_harmony_file(
            entries,
            total_size,
            file_count,
            rel,
            capped,
            mtime,
            None,
            mode,
            FolderEntrySource::Harmony(idx),
        )?;
    }

    Ok(())
}
