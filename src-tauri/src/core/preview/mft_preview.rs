//! NTFS `$MFT` native preview and `$SI` / `$FN` forensic inspection for files inside a
//! forensic image (live NTFS volumes, opened Volume Shadow Copies, or exported `$MFT` files).
//!
//! Uses random-access [`FsSession::read_file_range`] for O(1) page browsing even on 1+ GiB
//! `$MFT` files, and bounded 4 MiB sequential chunk streaming when building a compact searchable
//! summary index for deleted files, ADS streams, and `$SI` vs `$FN` timestomping anomalies.

use std::collections::HashMap;

use chrono::{DateTime, TimeZone, Utc};
use ntfs_core::{
    apply_fixup, decode_runlist, parse_attributes, AttributeBody, FileName, MftRecordHeader,
    StandardInformation,
};
use serde::{Deserialize, Serialize};

use crate::core::filesystem::tree::fs_tree::FsSession;

pub const MFT_MAGIC: &[u8] = b"FILE";
const ROOT_RECORD: u64 = 5;
const EPOCH_DIFF_100NS: u64 = 116_444_736_000_000_000;
const STREAM_CHUNK_RECORDS: usize = 4096; // ~4 MiB per read for 1024-byte records
const MAX_INDEX_RECORDS: usize = 2_000_000;
const MAX_RESIDENT_HEX_BYTES: usize = 4096;

/// Checks if a byte prefix matches a valid NTFS `$MFT` / `$MFTMirr` record `#0`.
pub fn looks_like_mft_header(bytes: &[u8]) -> bool {
    if bytes.len() < 48 || &bytes[0..4] != MFT_MAGIC {
        return false;
    }
    let Ok(header) = MftRecordHeader::parse(&bytes[..48]) else {
        return false;
    };
    if header.record_number != 0 || !header.is_in_use() || header.base_record != 0 {
        return false;
    }
    let alloc = header.allocated_size as usize;
    if !(512..=65536).contains(&alloc) || !alloc.is_power_of_two() || header.usa_count <= 1 {
        return false;
    }
    if header.used_size < 48
        || header.used_size > header.allocated_size
        || usize::from(header.first_attribute_offset) < 48
        || u32::from(header.first_attribute_offset) >= header.used_size
    {
        return false;
    }
    if bytes.len() >= alloc {
        let mut slot = bytes[..alloc].to_vec();
        if apply_fixup(&mut slot, 512).is_err() {
            return false;
        }
        let Ok(attrs) = parse_attributes(
            &slot[..header.used_size as usize],
            usize::from(header.first_attribute_offset),
        ) else {
            return false;
        };
        return attrs.iter().any(|attr| {
            attr.type_code == 0x30
                && attr
                    .resident_content(&slot)
                    .and_then(|v| FileName::parse(v).ok())
                    .is_some_and(|n| n.name == "$MFT")
        });
    }
    true
}

pub fn ntfs_time_to_rfc3339_ns(ft: u64) -> Option<String> {
    if ft < EPOCH_DIFF_100NS {
        return None;
    }
    let since_unix_100ns = ft - EPOCH_DIFF_100NS;
    let secs = (since_unix_100ns / 10_000_000) as i64;
    let nanos = ((since_unix_100ns % 10_000_000) * 100) as u32;
    Utc.timestamp_opt(secs, nanos)
        .single()
        .map(|dt: DateTime<Utc>| dt.to_rfc3339())
}

pub fn decode_file_attributes(flags: u32) -> Vec<String> {
    const TABLE: &[(u32, &str)] = &[
        (0x0001, "READ_ONLY"),
        (0x0002, "HIDDEN"),
        (0x0004, "SYSTEM"),
        (0x0020, "ARCHIVE"),
        (0x0040, "DEVICE"),
        (0x0080, "NORMAL"),
        (0x0100, "TEMPORARY"),
        (0x0200, "SPARSE_FILE"),
        (0x0400, "REPARSE_POINT"),
        (0x0800, "COMPRESSED"),
        (0x1000, "OFFLINE"),
        (0x2000, "NOT_CONTENT_INDEXED"),
        (0x4000, "ENCRYPTED"),
    ];
    TABLE
        .iter()
        .filter_map(|(bit, name)| (flags & bit != 0).then(|| (*name).to_string()))
        .collect()
}

/// Evaluates `$STANDARD_INFORMATION` (`$SI`) vs `$FILE_NAME` (`$FN`) four timestamps for
/// timestomping anomalies (e.g. `SetFileTime` backdating `$SI.Created` before kernel `$FN.Created`
/// or stripping nanosecond precision).
pub fn analyze_si_fn_timestomp(si_ft: Option<[u64; 4]>, fn_ft: Option<[u64; 4]>) -> Vec<String> {
    let mut warnings = Vec::new();
    let Some([si_cr, si_mod, si_mft, _si_acc]) = si_ft else {
        return warnings;
    };

    if si_mft > 0 && si_cr > 0 && si_mft < si_cr {
        warnings.push("$SI MFT变更时间早于创建时间（时间线倒挂）".to_string());
    }

    if let Some([fn_cr, _fn_mod, _fn_mft, _fn_acc]) = fn_ft {
        if si_cr > 0 && fn_cr > 0 && si_cr < fn_cr {
            warnings.push(
                "$SI 创建时间早于 $FN 创建时间（疑似向前伪造 $SI / Timestomping）".to_string(),
            );
        }
        let si_zero_ns_count = [si_cr, si_mod, si_mft]
            .into_iter()
            .filter(|&ft| ft > EPOCH_DIFF_100NS && ft % 10_000_000 == 0)
            .count();
        if si_zero_ns_count >= 2 && fn_cr > EPOCH_DIFF_100NS && fn_cr % 10_000_000 != 0 {
            warnings.push(
                "$SI 多项时间戳纳秒位为 .0000000（$FN 含精细纳秒，疑似 SetFileTime 修改）"
                    .to_string(),
            );
        }
    }

    warnings
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MftDigest {
    pub file_size: u64,
    pub record_size: u32,
    pub total_records: u64,
    pub volume_name: Option<String>,
    pub ntfs_version: Option<String>,
    pub volume_flags: Option<u16>,
    pub index_built: bool,
    pub indexed_records: usize,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MftRecordSummary {
    pub record_number: u64,
    pub sequence: u16,
    pub in_use: bool,
    pub is_dir: bool,
    pub name: String,
    pub full_path: String,
    pub parent_record: u64,
    pub logical_size: u64,
    pub is_resident: bool,
    pub ads_count: usize,
    pub si_created: Option<String>,
    pub si_modified: Option<String>,
    pub si_mft_modified: Option<String>,
    pub si_accessed: Option<String>,
    pub fn_created: Option<String>,
    pub fn_modified: Option<String>,
    pub timestomp_suspect: bool,
    pub timestomp_reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MftPage {
    pub total_records: u64,
    pub filtered_total: usize,
    pub offset: usize,
    pub limit: usize,
    pub indexed: bool,
    pub items: Vec<MftRecordSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MftStandardInfoDetail {
    pub created: Option<String>,
    pub modified: Option<String>,
    pub mft_modified: Option<String>,
    pub accessed: Option<String>,
    pub raw_filetimes: [u64; 4],
    pub file_attributes: u32,
    pub file_attribute_flags: Vec<String>,
    pub owner_id: Option<u32>,
    pub security_id: Option<u32>,
    pub usn: Option<u64>,
    pub quota_charged: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MftFileNameDetail {
    pub name: String,
    pub namespace: String,
    pub parent_record: u64,
    pub parent_sequence: u16,
    pub created: Option<String>,
    pub modified: Option<String>,
    pub mft_modified: Option<String>,
    pub accessed: Option<String>,
    pub raw_filetimes: [u64; 4],
    pub logical_size: u64,
    pub allocated_size: u64,
    pub file_flags: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MftDataRunInfo {
    pub vcn_offset: u64,
    pub clusters: u64,
    pub lcn: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MftAttributeDetail {
    pub type_code: u32,
    pub type_name: String,
    pub instance: u16,
    pub name: String,
    pub resident: bool,
    pub flags: u16,
    pub logical_size: u64,
    pub allocated_size: u64,
    pub resident_preview_text: Option<String>,
    pub resident_preview_hex: Option<String>,
    pub data_runs: Vec<MftDataRunInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MftRecordDetail {
    pub record_number: u64,
    pub sequence: u16,
    pub in_use: bool,
    pub is_dir: bool,
    pub base_record: u64,
    pub base_sequence: u16,
    pub hard_link_count: u16,
    pub lsn: u64,
    pub used_size: u32,
    pub allocated_size: u32,
    pub full_path: String,
    pub standard_info: Option<MftStandardInfoDetail>,
    pub filenames: Vec<MftFileNameDetail>,
    pub attributes: Vec<MftAttributeDetail>,
    pub timestomp_warnings: Vec<String>,
    pub notes: Vec<String>,
}

/// Compact per-session cache stored in `AppState::mft_session`.
pub struct MftSessionCache {
    pub key: String,
    pub node_id: String,
    pub file_size: u64,
    pub record_size: u32,
    pub total_records: u64,
    pub digest: MftDigest,
    pub parent_cache: HashMap<u64, Option<(u64, String)>>,
    pub full_index: Option<Vec<MftCompactEntry>>,
}

pub struct MftCompactEntry {
    pub record_number: u64,
    pub sequence: u16,
    pub in_use: bool,
    pub is_dir: bool,
    pub parent_record: u64,
    pub name: Box<str>,
    pub logical_size: u64,
    pub is_resident: bool,
    pub ads_count: u16,
    pub si_ft: [u64; 4],
    pub fn_ft: [u64; 4],
    pub has_si: bool,
    pub has_fn: bool,
    pub timestomp_flags: u8,
}

/// Opens `$MFT` digest using only a few KB via ranged reads (instant on 1+ GiB `$MFT`).
pub fn open_mft_digest(
    session: &mut FsSession<'_>,
    node_id: &str,
    file_size: u64,
) -> Result<(u32, u64, MftDigest), String> {
    let head = session
        .read_file_range(node_id, 0, 4096)
        .map_err(|e| format!("读取 $MFT 记录 #0 失败: {e}"))?;
    if !looks_like_mft_header(&head) {
        return Err("文件不是有效的 NTFS $MFT（缺少 FILE 签名或记录结构不合法）".to_string());
    }
    let header0 =
        MftRecordHeader::parse(&head[..48]).map_err(|e| format!("解析 $MFT 头失败: {e}"))?;
    let record_size = header0.allocated_size;
    let total_records = file_size / u64::from(record_size);

    let mut volume_name = None;
    let mut ntfs_version = None;
    let mut volume_flags = None;
    let mut notes = Vec::new();

    if total_records > 3 {
        if let Ok(mut vol_bytes) =
            session.read_file_range(node_id, 3 * u64::from(record_size), record_size as usize)
        {
            if vol_bytes.len() == record_size as usize && &vol_bytes[0..4] == MFT_MAGIC {
                if let Ok(h3) = MftRecordHeader::parse(&vol_bytes[..48]) {
                    if !h3.is_corrupt() && apply_fixup(&mut vol_bytes, 512).is_ok() {
                        if let Ok(attrs) = parse_attributes(
                            &vol_bytes[..h3.used_size as usize],
                            usize::from(h3.first_attribute_offset),
                        ) {
                            for attr in attrs {
                                let Some(content) = attr.resident_content(&vol_bytes) else {
                                    continue;
                                };
                                match attr.type_code {
                                    0x60 if content.len().is_multiple_of(2) => {
                                        let words: Vec<u16> = content
                                            .chunks_exact(2)
                                            .map(|c| u16::from_le_bytes([c[0], c[1]]))
                                            .collect();
                                        let s = String::from_utf16_lossy(&words);
                                        if !s.is_empty() {
                                            volume_name = Some(s);
                                        }
                                    }
                                    0x70 if content.len() >= 12 => {
                                        ntfs_version =
                                            Some(format!("{}.{}", content[8], content[9]));
                                        volume_flags =
                                            Some(u16::from_le_bytes([content[10], content[11]]));
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if file_size % u64::from(record_size) != 0 {
        notes.push("文件长度不是记录大小的整数倍，末尾存在不完整尾部字节。".to_string());
    }

    let digest = MftDigest {
        file_size,
        record_size,
        total_records,
        volume_name,
        ntfs_version,
        volume_flags,
        index_built: false,
        indexed_records: 0,
        notes,
    };

    Ok((record_size, total_records, digest))
}

/// Serves one paginated view of `$MFT`.
/// When unfiltered (`query` empty and `filter == "all"` or `"index"`), reads only the exact byte
/// window via `read_file_range` (< 30ms). When filtering/searching, builds & caches the compact index once.
pub fn list_mft_records(
    session: &mut FsSession<'_>,
    cache: &mut MftSessionCache,
    query: Option<&str>,
    filter: Option<&str>,
    offset: usize,
    limit: usize,
) -> Result<MftPage, String> {
    let q = query.unwrap_or("").trim().to_ascii_lowercase();
    let f = filter.unwrap_or("all").trim();
    let cap = limit.clamp(1, 500);

    // O(1) direct slice browse when no search/filter is requested and full index isn't forced.
    if q.is_empty() && f == "all" && cache.full_index.is_none() {
        let start_rec = offset as u64;
        if start_rec >= cache.total_records {
            return Ok(MftPage {
                total_records: cache.total_records,
                filtered_total: cache.total_records as usize,
                offset,
                limit: cap,
                indexed: false,
                items: Vec::new(),
            });
        }
        let want_recs = (cache.total_records - start_rec).min(cap as u64) as usize;
        let rec_sz = cache.record_size as usize;
        let raw = session
            .read_file_range(
                &cache.node_id,
                start_rec * rec_sz as u64,
                want_recs * rec_sz,
            )
            .map_err(|e| format!("读取 $MFT 分页记录失败: {e}"))?;

        let mut items = Vec::with_capacity(want_recs);
        for (i, chunk) in raw.chunks_exact(rec_sz).enumerate() {
            let rec_no = start_rec + i as u64;
            if let Some(summary) = summarize_raw_slot(chunk, rec_no, session, cache) {
                items.push(summary);
            } else {
                items.push(blank_record_summary(rec_no));
            }
        }
        return Ok(MftPage {
            total_records: cache.total_records,
            filtered_total: cache.total_records as usize,
            offset,
            limit: cap,
            indexed: false,
            items,
        });
    }

    ensure_full_index(session, cache)?;
    let index = cache.full_index.as_ref().expect("index populated");
    let rec_num_query = q.strip_prefix('#').unwrap_or(&q).parse::<u64>().ok();

    let matching: Vec<&MftCompactEntry> = index
        .iter()
        .filter(|e| match f {
            "deleted" => !e.in_use,
            "timestomped" => e.timestomp_flags != 0,
            "ads" => e.ads_count > 0,
            "directories" => e.is_dir && e.in_use,
            "resident" => e.is_resident && !e.is_dir && e.in_use,
            "allocated" => e.in_use,
            _ => true,
        })
        .filter(|e| {
            if q.is_empty() {
                return true;
            }
            if rec_num_query == Some(e.record_number) {
                return true;
            }
            if e.name.to_ascii_lowercase().contains(&q) {
                return true;
            }
            let path = resolve_path_from_cache(e.parent_record, &e.name, &cache.parent_cache);
            path.to_ascii_lowercase().contains(&q)
        })
        .collect();

    let filtered_total = matching.len();
    let slice = matching.into_iter().skip(offset).take(cap);
    let items = slice
        .map(|e| summary_from_compact(e, &cache.parent_cache))
        .collect();

    Ok(MftPage {
        total_records: cache.total_records,
        filtered_total,
        offset,
        limit: cap,
        indexed: true,
        items,
    })
}

/// Builds complete forensic details (`$SI`, all `$FN` namespaces, all attributes, Data Runs,
/// resident preview, and `$SI` vs `$FN` timestomping analysis) for a single MFT record slot.
pub fn read_mft_record_detail(
    session: &mut FsSession<'_>,
    cache: &mut MftSessionCache,
    record_number: u64,
) -> Result<MftRecordDetail, String> {
    if record_number >= cache.total_records {
        return Err(format!(
            "MFT 记录号 #{record_number} 超出范围（共 {} 条）",
            cache.total_records
        ));
    }
    let rec_sz = cache.record_size as usize;
    let mut slot = session
        .read_file_range(&cache.node_id, record_number * rec_sz as u64, rec_sz)
        .map_err(|e| format!("读取 MFT 记录 #{record_number} 失败: {e}"))?;
    if slot.len() < 48 {
        return Err("读取到的 MFT 记录字节不足 48 B".to_string());
    }
    if &slot[0..4] != MFT_MAGIC {
        let sig = String::from_utf8_lossy(&slot[0..4]).to_string();
        return Err(format!(
            "MFT 记录 #{record_number} 无效或未使用（签名: {sig:?}）"
        ));
    }

    let header = MftRecordHeader::parse(&slot[..48])
        .map_err(|e| format!("解析 MFT 记录 #{record_number} 头部失败: {e}"))?;
    let mut notes = Vec::new();
    if header.is_corrupt() {
        notes.push("该记录标记为 BAAD（Fixup 校验失败或已损坏）。".to_string());
    } else if apply_fixup(&mut slot, 512).is_err() {
        notes.push("USA Fixup 校验数组还原失败，部分尾部字节可能不一致。".to_string());
    }

    let used_end = (header.used_size as usize).min(slot.len());
    let first_attr = usize::from(header.first_attribute_offset).min(used_end);
    let attrs = parse_attributes(&slot[..used_end], first_attr)
        .map_err(|e| format!("解析 MFT 属性表失败: {e}"))?;

    let mut standard_info: Option<MftStandardInfoDetail> = None;
    let mut filenames: Vec<MftFileNameDetail> = Vec::new();
    let mut attr_details: Vec<MftAttributeDetail> = Vec::new();

    for attr in attrs {
        let type_name = attr_type_label(attr.type_code).to_string();
        let name = attr.name.clone().unwrap_or_default();
        let (logical_size, allocated_size, data_runs) = match &attr.body {
            AttributeBody::Resident { content_length, .. } => (
                u64::from(*content_length),
                u64::from(*content_length),
                Vec::new(),
            ),
            AttributeBody::NonResident {
                real_size,
                allocated_size,
                runs_offset,
                ..
            } => {
                let mut parsed_runs = Vec::new();
                let start = attr.offset.saturating_add(usize::from(*runs_offset));
                let end = attr.offset.saturating_add(attr.length as usize);
                if let Some(run_slice) = slot.get(start..end) {
                    if let Ok(decoded) = decode_runlist(run_slice) {
                        let mut vcn = 0u64;
                        for r in decoded {
                            parsed_runs.push(MftDataRunInfo {
                                vcn_offset: vcn,
                                clusters: r.length,
                                lcn: r.lcn,
                            });
                            vcn = vcn.saturating_add(r.length);
                        }
                    }
                }
                (*real_size, *allocated_size, parsed_runs)
            }
        };

        let mut resident_preview_text = None;
        let mut resident_preview_hex = None;
        if let Some(content) = attr.resident_content(&slot) {
            if attr.type_code == 0x80 && !content.is_empty() {
                let slice = &content[..content.len().min(MAX_RESIDENT_HEX_BYTES)];
                resident_preview_hex = Some(format_hex_lines(slice));
                if let Ok(txt) = std::str::from_utf8(slice) {
                    if txt
                        .chars()
                        .all(|c| !c.is_control() || c == '\n' || c == '\r' || c == '\t')
                    {
                        resident_preview_text = Some(txt.to_string());
                    }
                }
            }
            match attr.type_code {
                0x10 => {
                    if let Ok(info) = StandardInformation::parse(content) {
                        let raw_ft = [
                            info.created.0,
                            info.modified.0,
                            info.mft_modified.0,
                            info.accessed.0,
                        ];
                        let (owner_id, quota_charged) = if content.len() >= 72 {
                            (
                                content[48..52].try_into().ok().map(u32::from_le_bytes),
                                content[56..64].try_into().ok().map(u64::from_le_bytes),
                            )
                        } else {
                            (None, None)
                        };
                        standard_info = Some(MftStandardInfoDetail {
                            created: ntfs_time_to_rfc3339_ns(raw_ft[0]),
                            modified: ntfs_time_to_rfc3339_ns(raw_ft[1]),
                            mft_modified: ntfs_time_to_rfc3339_ns(raw_ft[2]),
                            accessed: ntfs_time_to_rfc3339_ns(raw_ft[3]),
                            raw_filetimes: raw_ft,
                            file_attributes: info.file_attributes,
                            file_attribute_flags: decode_file_attributes(info.file_attributes),
                            owner_id,
                            security_id: info.security_id,
                            usn: info.usn,
                            quota_charged,
                        });
                    }
                }
                0x20 => {
                    notes.push(
                        "该记录包含 $ATTRIBUTE_LIST (0x20)，部分属性可能存放在扩展 MFT 记录中。"
                            .to_string(),
                    );
                }
                0x30 => {
                    if let Ok(fn_attr) = FileName::parse(content) {
                        let raw_ft = [
                            fn_attr.created.0,
                            fn_attr.modified.0,
                            fn_attr.mft_modified.0,
                            fn_attr.accessed.0,
                        ];
                        filenames.push(MftFileNameDetail {
                            name: fn_attr.name,
                            namespace: namespace_label(fn_attr.namespace).to_string(),
                            parent_record: fn_attr.parent.record_number,
                            parent_sequence: fn_attr.parent.sequence,
                            created: ntfs_time_to_rfc3339_ns(raw_ft[0]),
                            modified: ntfs_time_to_rfc3339_ns(raw_ft[1]),
                            mft_modified: ntfs_time_to_rfc3339_ns(raw_ft[2]),
                            accessed: ntfs_time_to_rfc3339_ns(raw_ft[3]),
                            raw_filetimes: raw_ft,
                            logical_size: fn_attr.real_size,
                            allocated_size: fn_attr.allocated_size,
                            file_flags: fn_attr.flags,
                        });
                    }
                }
                _ => {}
            }
        }

        attr_details.push(MftAttributeDetail {
            type_code: attr.type_code,
            type_name,
            instance: attr.attribute_id,
            name,
            resident: !attr.non_resident,
            flags: attr.flags,
            logical_size,
            allocated_size,
            resident_preview_text,
            resident_preview_hex,
            data_runs,
        });
    }

    let primary_fn = pick_primary_fn_detail(&filenames);
    let full_path = if let Some(pfn) = primary_fn {
        resolve_path_lazy(pfn.parent_record, &pfn.name, session, cache)
    } else {
        format!("<unnamed #{record_number}>")
    };

    let timestomp_warnings = analyze_si_fn_timestomp(
        standard_info.as_ref().map(|s| s.raw_filetimes),
        primary_fn.map(|f| f.raw_filetimes),
    );

    Ok(MftRecordDetail {
        record_number,
        sequence: header.sequence_number,
        in_use: header.is_in_use(),
        is_dir: header.is_directory(),
        base_record: header.base_record & 0xffff_ffff_ffff,
        base_sequence: (header.base_record >> 48) as u16,
        hard_link_count: header.hard_link_count,
        lsn: header.lsn,
        used_size: header.used_size,
        allocated_size: header.allocated_size,
        full_path,
        standard_info,
        filenames,
        attributes: attr_details,
        timestomp_warnings,
        notes,
    })
}

fn ensure_full_index(
    session: &mut FsSession<'_>,
    cache: &mut MftSessionCache,
) -> Result<(), String> {
    if cache.full_index.is_some() {
        return Ok(());
    }
    let rec_sz = cache.record_size as usize;
    let total = (cache.total_records as usize).min(MAX_INDEX_RECORDS);
    let chunk_bytes = STREAM_CHUNK_RECORDS * rec_sz;
    let mut entries = Vec::with_capacity(total / 2);

    let mut rec_no = 0u64;
    while (rec_no as usize) < total {
        let want_recs = (total - rec_no as usize).min(STREAM_CHUNK_RECORDS);
        let bytes = session
            .read_file_range(
                &cache.node_id,
                rec_no * rec_sz as u64,
                (want_recs * rec_sz).min(chunk_bytes),
            )
            .map_err(|e| format!("流式扫描 $MFT 偏移 #{rec_no} 失败: {e}"))?;
        if bytes.is_empty() {
            break;
        }
        for slot in bytes.chunks_exact(rec_sz) {
            if let Some(compact) = parse_compact_entry(slot, rec_no) {
                if compact.is_dir && compact.in_use && !compact.name.is_empty() {
                    cache.parent_cache.insert(
                        compact.record_number,
                        Some((compact.parent_record, compact.name.to_string())),
                    );
                }
                entries.push(compact);
            }
            rec_no += 1;
        }
    }

    cache.digest.index_built = true;
    cache.digest.indexed_records = entries.len();
    cache.full_index = Some(entries);
    Ok(())
}

/// Minimal parsed fields for streaming MFT index and VSS diff.
pub(crate) fn parse_compact_entry(slot: &[u8], record_number: u64) -> Option<MftCompactEntry> {
    if slot.len() < 48 || &slot[0..4] != MFT_MAGIC {
        return None;
    }
    let header = MftRecordHeader::parse(&slot[..48]).ok()?;
    if header.is_corrupt() || header.used_size < 48 || (header.used_size as usize) > slot.len() {
        return None;
    }
    let mut fixed = slot.to_vec();
    if apply_fixup(&mut fixed, 512).is_err() {
        return None;
    }
    let first_attr = usize::from(header.first_attribute_offset).min(header.used_size as usize);
    let attrs = parse_attributes(&fixed[..header.used_size as usize], first_attr).ok()?;

    let mut has_si = false;
    let mut si_ft = [0u64; 4];
    let mut best_fn: Option<(u8, u64, String, [u64; 4], u64)> = None;
    let mut logical_size = 0u64;
    let mut has_default_data = false;
    let mut is_resident = false;
    let mut ads_count = 0u16;

    for attr in attrs {
        match attr.type_code {
            0x10 => {
                if let Some(content) = attr.resident_content(&fixed) {
                    if let Ok(info) = StandardInformation::parse(content) {
                        has_si = true;
                        si_ft = [
                            info.created.0,
                            info.modified.0,
                            info.mft_modified.0,
                            info.accessed.0,
                        ];
                    }
                }
            }
            0x30 => {
                if let Some(content) = attr.resident_content(&fixed) {
                    if let Ok(fn_attr) = FileName::parse(content) {
                        let score = namespace_priority(fn_attr.namespace);
                        let ft = [
                            fn_attr.created.0,
                            fn_attr.modified.0,
                            fn_attr.mft_modified.0,
                            fn_attr.accessed.0,
                        ];
                        let replace = best_fn.as_ref().is_none_or(|(prev_ns, _, _, _, _)| {
                            score > namespace_priority(*prev_ns)
                        });
                        if replace {
                            best_fn = Some((
                                fn_attr.namespace,
                                fn_attr.parent.record_number,
                                fn_attr.name,
                                ft,
                                fn_attr.real_size,
                            ));
                        }
                    }
                }
            }
            0x80 => {
                if attr.name.as_deref().unwrap_or("").is_empty() {
                    if !has_default_data {
                        has_default_data = true;
                        is_resident = !attr.non_resident;
                        logical_size = match attr.body {
                            AttributeBody::Resident { content_length, .. } => {
                                u64::from(content_length)
                            }
                            AttributeBody::NonResident { real_size, .. } => real_size,
                        };
                    }
                } else {
                    ads_count = ads_count.saturating_add(1);
                }
            }
            _ => {}
        }
    }

    let (_, parent_record, name, fn_ft, fn_size) = best_fn
        .map(|(ns, p, n, ft, sz)| (ns, p, n, ft, sz))
        .unwrap_or((0, ROOT_RECORD, String::new(), [0u64; 4], 0));
    let has_fn = !name.is_empty();
    let final_size = if has_default_data {
        logical_size
    } else {
        fn_size
    };
    let anomalies = analyze_si_fn_timestomp(has_si.then_some(si_ft), has_fn.then_some(fn_ft));

    Some(MftCompactEntry {
        record_number,
        sequence: header.sequence_number,
        in_use: header.is_in_use(),
        is_dir: header.is_directory(),
        parent_record,
        name: name.into_boxed_str(),
        logical_size: final_size,
        is_resident,
        ads_count,
        si_ft,
        fn_ft,
        has_si,
        has_fn,
        timestomp_flags: if anomalies.is_empty() { 0 } else { 1 },
    })
}

fn summarize_raw_slot(
    slot: &[u8],
    record_number: u64,
    session: &mut FsSession<'_>,
    cache: &mut MftSessionCache,
) -> Option<MftRecordSummary> {
    let e = parse_compact_entry(slot, record_number)?;
    let full_path = resolve_path_lazy(e.parent_record, &e.name, session, cache);
    let timestomp_reasons =
        analyze_si_fn_timestomp(e.has_si.then_some(e.si_ft), e.has_fn.then_some(e.fn_ft));
    Some(MftRecordSummary {
        record_number: e.record_number,
        sequence: e.sequence,
        in_use: e.in_use,
        is_dir: e.is_dir,
        name: e.name.into_string(),
        full_path,
        parent_record: e.parent_record,
        logical_size: e.logical_size,
        is_resident: e.is_resident,
        ads_count: e.ads_count as usize,
        si_created: e
            .has_si
            .then(|| ntfs_time_to_rfc3339_ns(e.si_ft[0]))
            .flatten(),
        si_modified: e
            .has_si
            .then(|| ntfs_time_to_rfc3339_ns(e.si_ft[1]))
            .flatten(),
        si_mft_modified: e
            .has_si
            .then(|| ntfs_time_to_rfc3339_ns(e.si_ft[2]))
            .flatten(),
        si_accessed: e
            .has_si
            .then(|| ntfs_time_to_rfc3339_ns(e.si_ft[3]))
            .flatten(),
        fn_created: e
            .has_fn
            .then(|| ntfs_time_to_rfc3339_ns(e.fn_ft[0]))
            .flatten(),
        fn_modified: e
            .has_fn
            .then(|| ntfs_time_to_rfc3339_ns(e.fn_ft[1]))
            .flatten(),
        timestomp_suspect: !timestomp_reasons.is_empty(),
        timestomp_reasons,
    })
}

fn summary_from_compact(
    e: &MftCompactEntry,
    parent_cache: &HashMap<u64, Option<(u64, String)>>,
) -> MftRecordSummary {
    let timestomp_reasons =
        analyze_si_fn_timestomp(e.has_si.then_some(e.si_ft), e.has_fn.then_some(e.fn_ft));
    MftRecordSummary {
        record_number: e.record_number,
        sequence: e.sequence,
        in_use: e.in_use,
        is_dir: e.is_dir,
        name: e.name.to_string(),
        full_path: resolve_path_from_cache(e.parent_record, &e.name, parent_cache),
        parent_record: e.parent_record,
        logical_size: e.logical_size,
        is_resident: e.is_resident,
        ads_count: e.ads_count as usize,
        si_created: e
            .has_si
            .then(|| ntfs_time_to_rfc3339_ns(e.si_ft[0]))
            .flatten(),
        si_modified: e
            .has_si
            .then(|| ntfs_time_to_rfc3339_ns(e.si_ft[1]))
            .flatten(),
        si_mft_modified: e
            .has_si
            .then(|| ntfs_time_to_rfc3339_ns(e.si_ft[2]))
            .flatten(),
        si_accessed: e
            .has_si
            .then(|| ntfs_time_to_rfc3339_ns(e.si_ft[3]))
            .flatten(),
        fn_created: e
            .has_fn
            .then(|| ntfs_time_to_rfc3339_ns(e.fn_ft[0]))
            .flatten(),
        fn_modified: e
            .has_fn
            .then(|| ntfs_time_to_rfc3339_ns(e.fn_ft[1]))
            .flatten(),
        timestomp_suspect: !timestomp_reasons.is_empty(),
        timestomp_reasons,
    }
}

fn blank_record_summary(record_number: u64) -> MftRecordSummary {
    MftRecordSummary {
        record_number,
        sequence: 0,
        in_use: false,
        is_dir: false,
        name: "(空记录槽)".to_string(),
        full_path: String::new(),
        parent_record: ROOT_RECORD,
        logical_size: 0,
        is_resident: false,
        ads_count: 0,
        si_created: None,
        si_modified: None,
        si_mft_modified: None,
        si_accessed: None,
        fn_created: None,
        fn_modified: None,
        timestomp_suspect: false,
        timestomp_reasons: Vec::new(),
    }
}

fn resolve_path_lazy(
    parent_record: u64,
    leaf_name: &str,
    session: &mut FsSession<'_>,
    cache: &mut MftSessionCache,
) -> String {
    if leaf_name.is_empty() {
        return String::new();
    }
    if leaf_name == "." && parent_record == ROOT_RECORD {
        return "\\".to_string();
    }
    let mut parts = vec![leaf_name.to_string()];
    let mut cur = parent_record;
    let rec_sz = cache.record_size as usize;

    for _ in 0..32 {
        if cur == ROOT_RECORD {
            break;
        }
        let cached = if let Some(entry) = cache.parent_cache.get(&cur) {
            entry.clone()
        } else {
            let fetched = if cur < cache.total_records {
                session
                    .read_file_range(&cache.node_id, cur * rec_sz as u64, rec_sz)
                    .ok()
                    .and_then(|raw| parse_compact_entry(&raw, cur))
                    .filter(|e| !e.name.is_empty())
                    .map(|e| (e.parent_record, e.name.into_string()))
            } else {
                None
            };
            cache.parent_cache.insert(cur, fetched.clone());
            fetched
        };
        let Some((next_parent, name)) = cached else {
            parts.push(format!("<MFT#{cur}>"));
            break;
        };
        if name == "." && next_parent == ROOT_RECORD {
            break;
        }
        parts.push(name);
        if next_parent == cur {
            break;
        }
        cur = next_parent;
    }

    parts.reverse();
    format!("\\{}", parts.join("\\"))
}

fn resolve_path_from_cache(
    parent_record: u64,
    leaf_name: &str,
    parent_cache: &HashMap<u64, Option<(u64, String)>>,
) -> String {
    if leaf_name.is_empty() {
        return String::new();
    }
    if leaf_name == "." && parent_record == ROOT_RECORD {
        return "\\".to_string();
    }
    let mut parts = vec![leaf_name.to_string()];
    let mut cur = parent_record;
    for _ in 0..32 {
        if cur == ROOT_RECORD {
            break;
        }
        let Some(Some((next_parent, name))) = parent_cache.get(&cur) else {
            parts.push(format!("<MFT#{cur}>"));
            break;
        };
        if name == "." && *next_parent == ROOT_RECORD {
            break;
        }
        parts.push(name.clone());
        if *next_parent == cur {
            break;
        }
        cur = *next_parent;
    }
    parts.reverse();
    format!("\\{}", parts.join("\\"))
}

pub fn namespace_priority(ns: u8) -> u8 {
    match ns {
        1 => 4, // Win32
        3 => 3, // Win32AndDos
        0 => 2, // Posix
        2 => 1, // Dos
        _ => 0,
    }
}

pub fn namespace_label(ns: u8) -> &'static str {
    match ns {
        0 => "POSIX",
        1 => "Win32",
        2 => "DOS",
        3 => "Win32&DOS",
        _ => "Unknown",
    }
}

fn pick_primary_fn_detail(fns: &[MftFileNameDetail]) -> Option<&MftFileNameDetail> {
    fns.iter().max_by_key(|f| match f.namespace.as_str() {
        "Win32" => 4,
        "Win32&DOS" => 3,
        "POSIX" => 2,
        "DOS" => 1,
        _ => 0,
    })
}

pub fn attr_type_label(code: u32) -> &'static str {
    match code {
        0x10 => "$STANDARD_INFORMATION",
        0x20 => "$ATTRIBUTE_LIST",
        0x30 => "$FILE_NAME",
        0x40 => "$OBJECT_ID",
        0x50 => "$SECURITY_DESCRIPTOR",
        0x60 => "$VOLUME_NAME",
        0x70 => "$VOLUME_INFORMATION",
        0x80 => "$DATA",
        0x90 => "$INDEX_ROOT",
        0xA0 => "$INDEX_ALLOCATION",
        0xB0 => "$BITMAP",
        0xC0 => "$REPARSE_POINT",
        0x100 => "$LOGGED_UTILITY_STREAM",
        _ => "UNKNOWN_ATTR",
    }
}

fn format_hex_lines(bytes: &[u8]) -> String {
    bytes
        .chunks(16)
        .enumerate()
        .map(|(row, chunk)| {
            let cols: Vec<String> = (0..16)
                .map(|i| {
                    chunk
                        .get(i)
                        .map(|b| format!("{b:02x}"))
                        .unwrap_or_else(|| "  ".to_string())
                })
                .collect();
            let ascii: String = chunk
                .iter()
                .map(|&b| {
                    if (0x20..=0x7e).contains(&b) {
                        b as char
                    } else {
                        '.'
                    }
                })
                .collect();
            format!(
                "{:08x}  {} {}  |{ascii}|",
                row * 16,
                cols[..8].join(" "),
                cols[8..].join(" ")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NtfsStreamExtentView {
    pub logical_offset: u64,
    pub storage_length: u64,
    pub volume_offset: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NtfsAttributeForensicView {
    pub record_number: u64,
    pub instance: u16,
    pub attribute_type: u32,
    pub type_name: String,
    pub name: String,
    pub flags: u16,
    pub resident: bool,
    pub logical_size: u64,
    pub initialized_size: u64,
    pub allocated_size: u64,
    pub lowest_vcn: u64,
    pub compression_unit_exponent: u16,
    pub extents: Vec<NtfsStreamExtentView>,
}

/// Complete forensic metadata for a single NTFS MFT record (`$SI` + `$FN` + attributes),
/// surfaced in every NTFS file/folder's detail panel.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NtfsRecordForensicView {
    pub record_number: u64,
    pub sequence: u16,
    pub allocated: bool,
    pub directory: bool,
    pub base_record: u64,
    pub base_sequence: u16,
    pub hard_link_count: u16,
    pub logfile_sequence_number: u64,
    pub standard_info: Option<MftStandardInfoDetail>,
    pub filenames: Vec<MftFileNameDetail>,
    pub attributes: Vec<NtfsAttributeForensicView>,
    pub timestomp_warnings: Vec<String>,
    pub reparse_tag: Option<u32>,
    pub link_target: Option<String>,
    pub reparse_print_name: Option<String>,
}

/// Converts low-level `NtfsRecordMetadata` into a JSON/UI-friendly `$SI` + `$FN` forensic view.
pub fn build_ntfs_record_forensic_view(
    meta: &crate::core::filesystem::volumes::ntfs::NtfsRecordMetadata,
) -> NtfsRecordForensicView {
    let standard_info = meta.standard_filetimes.map(|raw| {
        let flags = meta.file_flags.unwrap_or(0);
        MftStandardInfoDetail {
            created: ntfs_time_to_rfc3339_ns(raw[0]),
            modified: ntfs_time_to_rfc3339_ns(raw[1]),
            mft_modified: ntfs_time_to_rfc3339_ns(raw[2]),
            accessed: ntfs_time_to_rfc3339_ns(raw[3]),
            raw_filetimes: raw,
            file_attributes: flags,
            file_attribute_flags: decode_file_attributes(flags),
            owner_id: meta.owner_id,
            security_id: meta.security_id,
            usn: meta.usn,
            quota_charged: meta.quota_charged,
        }
    });

    let filenames: Vec<MftFileNameDetail> = meta
        .filenames
        .iter()
        .map(|f| MftFileNameDetail {
            name: f.name.clone(),
            namespace: namespace_label(f.namespace).to_string(),
            parent_record: f.parent_record,
            parent_sequence: f.parent_sequence,
            created: ntfs_time_to_rfc3339_ns(f.filetimes[0]),
            modified: ntfs_time_to_rfc3339_ns(f.filetimes[1]),
            mft_modified: ntfs_time_to_rfc3339_ns(f.filetimes[2]),
            accessed: ntfs_time_to_rfc3339_ns(f.filetimes[3]),
            raw_filetimes: f.filetimes,
            logical_size: f.logical_size,
            allocated_size: f.allocated_size,
            file_flags: f.file_flags,
        })
        .collect();

    let primary_fn_raw = meta
        .filenames
        .iter()
        .max_by_key(|f| namespace_priority(f.namespace))
        .map(|f| f.filetimes);
    let timestomp_warnings = analyze_si_fn_timestomp(meta.standard_filetimes, primary_fn_raw);

    let attributes = meta
        .attributes
        .iter()
        .map(|a| NtfsAttributeForensicView {
            record_number: a.record_number,
            instance: a.instance,
            attribute_type: a.attribute_type,
            type_name: attr_type_label(a.attribute_type).to_string(),
            name: a.name.clone(),
            flags: a.flags,
            resident: a.resident,
            logical_size: a.logical_size,
            initialized_size: a.initialized_size,
            allocated_size: a.allocated_size,
            lowest_vcn: a.lowest_vcn,
            compression_unit_exponent: a.compression_unit_exponent,
            extents: a
                .extents
                .iter()
                .map(|e| NtfsStreamExtentView {
                    logical_offset: e.logical_offset,
                    storage_length: e.storage_length,
                    volume_offset: e.volume_offset,
                })
                .collect(),
        })
        .collect();

    NtfsRecordForensicView {
        record_number: meta.record_number,
        sequence: meta.sequence,
        allocated: meta.allocated,
        directory: meta.directory,
        base_record: meta.base_record,
        base_sequence: meta.base_sequence,
        hard_link_count: meta.hard_link_count,
        logfile_sequence_number: meta.logfile_sequence_number,
        standard_info,
        filenames,
        attributes,
        timestomp_warnings,
        reparse_tag: meta.reparse_tag,
        link_target: meta.link_target.clone(),
        reparse_print_name: meta.reparse_print_name.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestomp_detector_flags_si_before_fn_and_zero_subseconds() {
        let fn_ts = 134_300_000_001_234_567u64; // has non-zero subseconds
        let si_backdated = 134_200_000_000_000_000u64; // earlier + exact second boundary
        let warnings = analyze_si_fn_timestomp(
            Some([si_backdated, si_backdated, si_backdated, si_backdated]),
            Some([fn_ts, fn_ts, fn_ts, fn_ts]),
        );
        assert!(warnings.iter().any(|w| w.contains("$SI 创建时间早于 $FN")));
        assert!(warnings.iter().any(|w| w.contains("纳秒位为 .0000000")));
    }

    #[test]
    fn parses_real_mft_header_if_image_exists() {
        let path = std::path::Path::new(r"C:\hlnet\1-1794883754\COM-01.e01\分区1\$MFT");
        if !path.exists() {
            return;
        }
        use std::io::Read;
        let mut f = std::fs::File::open(path).unwrap();
        let mut buf = vec![0u8; 4096];
        f.read_exact(&mut buf).unwrap();
        assert!(looks_like_mft_header(&buf));
        let e0 = parse_compact_entry(&buf[..1024], 0).unwrap();
        assert_eq!(&*e0.name, "$MFT");
        assert!(e0.in_use);
    }
}
