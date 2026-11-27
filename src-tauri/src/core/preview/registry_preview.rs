//! Windows Registry Hive (`regf`) native preview for files inside a forensic image.
//!
//! Uses vendored [`notatin`] to parse primary hive files (`BBI`, `DEFAULT`, `DRIVERS`,
//! `ELAM`, `SAM`, `SECURITY`, `SYSTEM`, `SOFTWARE`, `COMPONENTS`, `NTUSER.DAT`,
//! `UsrClass.dat`, `Amcache.hve`, etc.), automatically discovers and replays sibling
//! `.LOG1` / `.LOG2` / `.LOG` transaction logs when present, recovers deleted keys and
//! values on bounded hives, and exposes lazy key navigation plus bounded search.

use std::collections::BTreeSet;
use std::io::Cursor;

use chrono::{DateTime, TimeZone, Utc};
use notatin::cell_key_node::CellKeyNode;
use notatin::cell_key_value::{CellKeyValue, CellKeyValueDataTypes};
use notatin::cell_value::CellValue;
use notatin::parser::{Parser, ParserIteratorContext};
use notatin::parser_builder::ParserBuilder;
use serde::{Deserialize, Serialize};

pub const REGISTRY_MAGIC: &[u8] = b"regf";
/// 512 MiB cap covers large `SOFTWARE` / `COMPONENTS` hives without unbounded memory allocation.
pub const MAX_REGISTRY_BYTES: usize = 512 * 1024 * 1024;
/// Scan unallocated/slack cells for deleted keys/values up to 32 MiB (`SYSTEM`, `BBI`, `DRIVERS`, etc.).
const RECOVER_DELETED_MAX_BYTES: usize = 32 * 1024 * 1024;
/// Maximum raw value bytes rendered as hex in a single IPC response.
const MAX_HEX_PREVIEW_BYTES: usize = 4096;
/// Hard safety limit on visited keys per search query so massive hives stay responsive.
const MAX_SEARCH_VISITED_KEYS: usize = 250_000;
const MAX_SEARCH_LIMIT: usize = 1000;

/// Checks whether a byte slice begins with a primary Windows Registry Hive header
/// (`"regf"` magic at offset 0 and `FileType::Primary == 0` at offset `0x1c`),
/// distinguishing primary hives from `.LOG` / `.LOG1` / `.LOG2` transaction logs (`type == 6`).
pub fn is_primary_registry_hive(bytes: &[u8]) -> bool {
    bytes.len() >= 32
        && &bytes[0..4] == REGISTRY_MAGIC
        && u32::from_le_bytes([bytes[28], bytes[29], bytes[30], bytes[31]]) == 0
}

#[derive(Debug, Clone)]
struct RegfHeaderMeta {
    primary_seq: u32,
    secondary_seq: u32,
    last_written: Option<String>,
    major_version: u32,
    minor_version: u32,
    hive_bins_size: u32,
    embedded_filename: String,
}

fn parse_regf_header(bytes: &[u8]) -> Result<RegfHeaderMeta, String> {
    if bytes.len() < 512 || &bytes[0..4] != REGISTRY_MAGIC {
        return Err("文件缺少 regf 签名，不是有效的 Windows 注册表配置单元".to_string());
    }
    let file_type = u32::from_le_bytes(bytes[28..32].try_into().unwrap());
    if file_type != 0 {
        return Err(format!(
            "该文件是注册表事务日志或其他辅助类型 (FileType={file_type})，请直接预览对应主配置单元文件"
        ));
    }

    let primary_seq = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    let secondary_seq = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
    let filetime = u64::from_le_bytes(bytes[12..20].try_into().unwrap());
    let major_version = u32::from_le_bytes(bytes[20..24].try_into().unwrap());
    let minor_version = u32::from_le_bytes(bytes[24..28].try_into().unwrap());
    let hive_bins_size = u32::from_le_bytes(bytes[40..44].try_into().unwrap());

    let name_units: Vec<u16> = bytes[48..112]
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .take_while(|&u| u != 0)
        .collect();
    let embedded_filename = String::from_utf16_lossy(&name_units).trim().to_string();

    Ok(RegfHeaderMeta {
        primary_seq,
        secondary_seq,
        last_written: filetime_to_rfc3339(filetime),
        major_version,
        minor_version,
        hive_bins_size,
        embedded_filename,
    })
}

fn filetime_to_rfc3339(ft: u64) -> Option<String> {
    const EPOCH_DIFF_100NS: u64 = 116_444_736_000_000_000;
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryDigest {
    pub file_size: u64,
    pub hive_name: String,
    pub embedded_path: String,
    pub version: String,
    pub primary_seq: u32,
    pub secondary_seq: u32,
    pub is_dirty: bool,
    pub hive_bins_size: u32,
    pub last_written: Option<String>,
    pub root_name: String,
    pub root_subkey_count: u32,
    pub root_value_count: u32,
    pub sidecars: Vec<String>,
    pub deleted_scan_enabled: bool,
    pub notes: Vec<String>,
    pub root_children: Vec<RegistryKeySummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryKeySummary {
    pub name: String,
    pub path: String,
    pub cell_offset: u64,
    pub subkey_count: u32,
    pub value_count: u32,
    pub has_subkeys: bool,
    pub last_written: Option<String>,
    pub is_deleted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryKeyDetail {
    pub name: String,
    pub path: String,
    pub cell_offset: u64,
    pub last_written: Option<String>,
    pub class_name_size: u16,
    pub subkey_count: u32,
    pub value_count: u32,
    pub is_deleted: bool,
    pub subkeys: Vec<RegistryKeySummary>,
    pub values: Vec<RegistryValueItem>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryValueItem {
    pub name: String,
    pub is_default: bool,
    pub data_type: String,
    pub value: String,
    pub raw_size: usize,
    pub data_size_raw: u32,
    pub raw_hex: String,
    pub hex_truncated: bool,
    pub utf16_hint: Option<String>,
    pub is_deleted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistrySearchHit {
    pub kind: String,
    pub key_path: String,
    pub key_offset: u64,
    pub value_name: Option<String>,
    pub data_type: Option<String>,
    pub preview: String,
    pub last_written: Option<String>,
    pub is_deleted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistrySearchResult {
    pub query: String,
    pub hits: Vec<RegistrySearchHit>,
    pub truncated: bool,
}

/// Parses a primary Windows Registry Hive in memory, automatically replaying sibling transaction logs
/// when valid and producing the top-level [`RegistryDigest`] plus a warm stateful [`Parser`].
pub fn open_registry_hive(
    primary_bytes: Vec<u8>,
    file_size: u64,
    file_name: &str,
    sidecar_logs: Vec<(String, Vec<u8>)>,
) -> Result<(Parser, RegistryDigest), String> {
    let header = parse_regf_header(&primary_bytes)?;
    let is_dirty = header.primary_seq != header.secondary_seq;
    let recover_deleted = primary_bytes.len() <= RECOVER_DELETED_MAX_BYTES;

    let mut notes = Vec::new();
    let mut replayed_sidecars = Vec::new();

    let mut parser = if !sidecar_logs.is_empty() && is_dirty {
        let mut builder = ParserBuilder::from_file(Cursor::new(primary_bytes.clone()));
        builder.recover_deleted(recover_deleted);
        let mut names = Vec::new();
        for (log_name, log_bytes) in &sidecar_logs {
            if names.len() >= 2 {
                break;
            }
            builder.with_transaction_log(Cursor::new(log_bytes.clone()));
            names.push(log_name.clone());
        }
        match builder.build() {
            Ok(p) => {
                replayed_sidecars = names;
                p
            }
            Err(err) => {
                notes.push(format!(
                    "事务日志重放失败（{err}），已按主配置单元原始内容只读打开。"
                ));
                let mut fb = ParserBuilder::from_file(Cursor::new(primary_bytes));
                fb.recover_deleted(recover_deleted);
                fb.build()
                    .map_err(|e| format!("无法解析注册表配置单元: {e}"))?
            }
        }
    } else {
        if is_dirty && sidecar_logs.is_empty() {
            notes.push(format!(
                "配置单元序列号不一致（{} ≠ {}），且同目录未找到可用的 .LOG/.LOG1/.LOG2 事务日志。",
                header.primary_seq, header.secondary_seq
            ));
        }
        let mut builder = ParserBuilder::from_file(Cursor::new(primary_bytes));
        builder.recover_deleted(recover_deleted);
        builder
            .build()
            .map_err(|e| format!("无法解析注册表配置单元: {e}"))?
    };

    if !recover_deleted {
        notes.push(format!(
            "文件超过 {} MiB 删除扫描阈值，已跳过未分配单元的已删除项恢复扫描以确保浏览速度。",
            RECOVER_DELETED_MAX_BYTES / (1024 * 1024)
        ));
    }

    let mut root = parser
        .get_root_key()
        .map_err(|e| format!("读取注册表根键失败: {e}"))?
        .ok_or_else(|| "注册表中不存在根键节点".to_string())?;

    let root_children = collect_child_summaries(&mut root, &mut parser, "");
    let root_name = root.key_name.clone();
    let root_subkey_count = root.detail.number_of_sub_keys();
    let root_value_count = root.detail.number_of_key_values();

    let digest = RegistryDigest {
        file_size,
        hive_name: if file_name.is_empty() {
            infer_hive_label(&header.embedded_filename)
        } else {
            file_name.to_string()
        },
        embedded_path: header.embedded_filename,
        version: format!("{}.{}", header.major_version, header.minor_version),
        primary_seq: header.primary_seq,
        secondary_seq: header.secondary_seq,
        is_dirty,
        hive_bins_size: header.hive_bins_size,
        last_written: header.last_written,
        root_name,
        root_subkey_count,
        root_value_count,
        sidecars: replayed_sidecars,
        deleted_scan_enabled: recover_deleted,
        notes,
        root_children,
    };

    Ok((parser, digest))
}

fn infer_hive_label(embedded: &str) -> String {
    embedded
        .rsplit(['\\', '/'])
        .find(|s| !s.is_empty())
        .unwrap_or("Registry Hive")
        .to_string()
}

/// Reads details (subkeys and all values) for a given relative registry key path.
/// Passing an empty `path` resolves the hive root key.
pub fn read_key_detail(
    parser: &mut Parser,
    path: &str,
    cell_offset: Option<u64>,
) -> Result<RegistryKeyDetail, String> {
    let trimmed = path.trim_matches('\\');
    let mut key = resolve_key(parser, trimmed, cell_offset)?;

    let subkeys = collect_child_summaries(&mut key, parser, trimmed);
    let mut values: Vec<RegistryValueItem> = key.value_iter().map(format_value_item).collect();
    values.sort_by(|a, b| {
        a.is_deleted
            .cmp(&b.is_deleted)
            .then_with(|| b.is_default.cmp(&a.is_default))
            .then_with(|| {
                a.name
                    .to_ascii_lowercase()
                    .cmp(&b.name.to_ascii_lowercase())
            })
    });

    let mut notes = Vec::new();
    if key.cell_state.is_deleted() {
        notes.push("此注册表键恢复自未分配单元或事务日志 slack 空间（已删除）。".to_string());
    }
    if let Some(logs) = key.logs.get() {
        for log in logs.iter().take(4) {
            notes.push(format!("{:?}: {}", log.code, log.text));
        }
    }

    Ok(RegistryKeyDetail {
        name: key.key_name.clone(),
        path: trimmed.to_string(),
        cell_offset: key.file_offset_absolute as u64,
        last_written: Some(key.last_key_written_date_and_time().to_rfc3339()),
        class_name_size: key.detail.class_name_size(),
        subkey_count: key.detail.number_of_sub_keys(),
        value_count: key.detail.number_of_key_values(),
        is_deleted: key.cell_state.is_deleted(),
        subkeys,
        values,
        notes,
    })
}

/// Searches key names, paths, value names, and formatted value data across the hive.
pub fn search_hive(parser: &mut Parser, query: &str, limit: usize) -> RegistrySearchResult {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return RegistrySearchResult {
            query: String::new(),
            hits: Vec::new(),
            truncated: false,
        };
    }

    let needle = trimmed.to_ascii_lowercase();
    let cap = limit.clamp(1, MAX_SEARCH_LIMIT);
    let mut hits = Vec::new();
    let mut truncated = false;
    let mut visited = 0usize;

    let Ok(Some(root)) = parser.get_root_key() else {
        return RegistrySearchResult {
            query: trimmed.to_string(),
            hits,
            truncated: false,
        };
    };
    let root_prefix = format!("\\{}", root.key_name);
    let mut ctx = ParserIteratorContext::from_parser(parser, false, None);

    while let Some(node) = parser.next_key_preorder(&mut ctx) {
        visited += 1;
        if visited > MAX_SEARCH_VISITED_KEYS {
            truncated = true;
            break;
        }

        let rel_path = strip_root_prefix(&node.path, &root_prefix);
        let is_deleted_key = node.cell_state.is_deleted();
        let last_written = Some(node.last_key_written_date_and_time().to_rfc3339());

        if !rel_path.is_empty()
            && (node.key_name.to_ascii_lowercase().contains(&needle)
                || rel_path.to_ascii_lowercase().contains(&needle))
        {
            hits.push(RegistrySearchHit {
                kind: "key".to_string(),
                key_path: rel_path.clone(),
                key_offset: node.file_offset_absolute as u64,
                value_name: None,
                data_type: None,
                preview: format!(
                    "{} 个子键 · {} 个值",
                    node.detail.number_of_sub_keys(),
                    node.detail.number_of_key_values()
                ),
                last_written: last_written.clone(),
                is_deleted: is_deleted_key,
            });
            if hits.len() >= cap {
                truncated = true;
                break;
            }
        }

        for val in node.value_iter() {
            let formatted = format_value_item(val);
            let matched = formatted.name.to_ascii_lowercase().contains(&needle)
                || formatted.value.to_ascii_lowercase().contains(&needle)
                || formatted
                    .utf16_hint
                    .as_deref()
                    .is_some_and(|h| h.to_ascii_lowercase().contains(&needle));
            if matched {
                hits.push(RegistrySearchHit {
                    kind: "value".to_string(),
                    key_path: rel_path.clone(),
                    key_offset: node.file_offset_absolute as u64,
                    value_name: Some(formatted.name),
                    data_type: Some(formatted.data_type),
                    preview: truncate_text(&formatted.value, 180),
                    last_written: last_written.clone(),
                    is_deleted: is_deleted_key || formatted.is_deleted,
                });
                if hits.len() >= cap {
                    truncated = true;
                    break;
                }
            }
        }

        if truncated {
            break;
        }
    }

    RegistrySearchResult {
        query: trimmed.to_string(),
        hits,
        truncated,
    }
}

fn strip_root_prefix(raw_path: &str, root_prefix: &str) -> String {
    if raw_path.eq_ignore_ascii_case(root_prefix) {
        return String::new();
    }
    if let Some(rest) = raw_path.strip_prefix(root_prefix).or_else(|| {
        if raw_path
            .get(..root_prefix.len())
            .is_some_and(|p| p.eq_ignore_ascii_case(root_prefix))
        {
            Some(&raw_path[root_prefix.len()..])
        } else {
            None
        }
    }) {
        return rest.trim_start_matches('\\').to_string();
    }
    raw_path.trim_start_matches('\\').to_string()
}

fn resolve_key(
    parser: &mut Parser,
    rel_path: &str,
    cell_offset: Option<u64>,
) -> Result<CellKeyNode, String> {
    let mut current = parser
        .get_root_key()
        .map_err(|e| format!("读取注册表根键失败: {e}"))?
        .ok_or_else(|| "注册表根键缺失".to_string())?;

    if rel_path.is_empty() {
        if let Some(off) = cell_offset {
            if current.file_offset_absolute as u64 != off {
                if let Some(dk) = current
                    .deleted_keys
                    .iter()
                    .find(|k| k.file_offset_absolute as u64 == off)
                {
                    return Ok(dk.clone());
                }
            }
        }
        return Ok(current);
    }

    let segments: Vec<&str> = rel_path.split('\\').filter(|s| !s.is_empty()).collect();
    for (idx, seg) in segments.iter().enumerate() {
        let is_last = idx + 1 == segments.len();
        let active_children = current.read_sub_keys(parser);

        let found_deleted = current
            .deleted_keys
            .iter()
            .find(|k| {
                if is_last {
                    if let Some(off) = cell_offset {
                        return k.file_offset_absolute as u64 == off;
                    }
                }
                k.key_name.eq_ignore_ascii_case(seg)
            })
            .cloned();

        let found_active = active_children
            .into_iter()
            .find(|k| {
                if is_last {
                    if let Some(off) = cell_offset {
                        return k.file_offset_absolute as u64 == off;
                    }
                }
                k.key_name.eq_ignore_ascii_case(seg)
            })
            .or_else(|| {
                if is_last {
                    // Fallback match by name if offset shifted after transaction log replay.
                    let retry = current.read_sub_keys(parser);
                    retry
                        .into_iter()
                        .find(|k| k.key_name.eq_ignore_ascii_case(seg))
                } else {
                    None
                }
            });

        current = match (is_last, cell_offset, found_active, found_deleted) {
            (true, Some(_), _, Some(d))
                if d.file_offset_absolute as u64 == cell_offset.unwrap() =>
            {
                d
            }
            (_, _, Some(a), _) => a,
            (_, _, None, Some(d)) => d,
            (false, _, None, None) => {
                // Handle rare edge case where key name itself contains '\' via direct prefix lookup on remaining path.
                let rest = segments[idx..].join("\\");
                if let Some(direct) = parser.get_key(&rest, false).ok().flatten() {
                    return Ok(direct);
                }
                return Err(format!("找不到注册表键: {rel_path}"));
            }
            (true, _, None, None) => return Err(format!("找不到注册表键: {rel_path}")),
        };
    }

    Ok(current)
}

fn collect_child_summaries(
    node: &mut CellKeyNode,
    parser: &mut Parser,
    parent_rel_path: &str,
) -> Vec<RegistryKeySummary> {
    let active = node.read_sub_keys(parser);
    let mut seen_offsets = BTreeSet::new();
    let mut out = Vec::with_capacity(active.len() + node.deleted_keys.len());

    for child in active {
        seen_offsets.insert(child.file_offset_absolute);
        out.push(summarize_key(&child, parent_rel_path));
    }
    for deleted in &node.deleted_keys {
        if seen_offsets.insert(deleted.file_offset_absolute) {
            out.push(summarize_key(deleted, parent_rel_path));
        }
    }

    out.sort_by(|a, b| {
        a.is_deleted.cmp(&b.is_deleted).then_with(|| {
            a.name
                .to_ascii_lowercase()
                .cmp(&b.name.to_ascii_lowercase())
        })
    });
    out
}

fn summarize_key(key: &CellKeyNode, parent_rel_path: &str) -> RegistryKeySummary {
    let rel_path = if parent_rel_path.is_empty() {
        key.key_name.clone()
    } else {
        format!("{parent_rel_path}\\{}", key.key_name)
    };
    let subkey_count = key.detail.number_of_sub_keys();
    let deleted_child_count = key.deleted_keys.len() as u32;
    RegistryKeySummary {
        name: key.key_name.clone(),
        path: rel_path,
        cell_offset: key.file_offset_absolute as u64,
        subkey_count: subkey_count.saturating_add(deleted_child_count),
        value_count: key.detail.number_of_key_values(),
        has_subkeys: subkey_count > 0 || deleted_child_count > 0,
        last_written: Some(key.last_key_written_date_and_time().to_rfc3339()),
        is_deleted: key.cell_state.is_deleted(),
    }
}

fn format_value_item(val: CellKeyValue) -> RegistryValueItem {
    let is_deleted = val.cell_state.is_deleted();
    let data_size_raw = val.detail.data_size_raw();
    let raw_name = val.detail.value_name();
    let is_default = raw_name.is_empty();
    let name = if is_default {
        "(默认)".to_string()
    } else {
        raw_name
    };
    let data_type = format_reg_type(val.data_type);
    let raw_bytes = extract_raw_bytes(&val);
    let raw_size = raw_bytes.len();
    let hex_truncated = raw_size > MAX_HEX_PREVIEW_BYTES;
    let raw_hex = format_hex_dump(&raw_bytes[..raw_size.min(MAX_HEX_PREVIEW_BYTES)]);

    let (content, _logs) = val.get_content();
    let utf16_hint = if matches!(
        val.data_type,
        CellKeyValueDataTypes::REG_BIN | CellKeyValueDataTypes::REG_NONE
    ) {
        detect_utf16_text_hint(&raw_bytes)
    } else {
        None
    };

    let value = format_cell_value(&val.data_type, &content, &raw_bytes, utf16_hint.as_deref());

    RegistryValueItem {
        name,
        is_default,
        data_type,
        value,
        raw_size,
        data_size_raw,
        raw_hex,
        hex_truncated,
        utf16_hint,
        is_deleted,
    }
}

fn extract_raw_bytes(val: &CellKeyValue) -> Vec<u8> {
    if let Some(raw) = val.detail.value_bytes() {
        return raw.clone();
    }
    match val.get_content().0 {
        CellValue::Binary(b) => b,
        CellValue::String(s) => s.encode_utf16().flat_map(u16::to_le_bytes).collect(),
        CellValue::U32(n) => n.to_le_bytes().to_vec(),
        CellValue::I32(n) => n.to_le_bytes().to_vec(),
        CellValue::U64(n) => n.to_le_bytes().to_vec(),
        CellValue::I64(n) => n.to_le_bytes().to_vec(),
        _ => Vec::new(),
    }
}

fn format_reg_type(dt: CellKeyValueDataTypes) -> String {
    match dt {
        CellKeyValueDataTypes::REG_BIN => "REG_BINARY".to_string(),
        other => format!("{other:?}"),
    }
}

fn format_cell_value(
    dt: &CellKeyValueDataTypes,
    content: &CellValue,
    raw_bytes: &[u8],
    utf16_hint: Option<&str>,
) -> String {
    match content {
        CellValue::None => {
            if raw_bytes.is_empty() {
                "(未设置值 / 空)".to_string()
            } else {
                format_hex_inline(raw_bytes)
            }
        }
        CellValue::String(s) => s.clone(),
        CellValue::MultiString(items) => items.join("\n"),
        CellValue::U32(v) => format!("0x{v:08x} ({v})"),
        CellValue::I32(v) => format!("0x{:08x} ({v})", *v as u32),
        CellValue::U64(v) => {
            if matches!(dt, CellKeyValueDataTypes::REG_FILETIME) {
                if let Some(rfc) = filetime_to_rfc3339(*v) {
                    return format!("{rfc} (0x{v:016x})");
                }
            }
            format!("0x{v:016x} ({v})")
        }
        CellValue::I64(v) => format!("0x{:016x} ({v})", *v as u64),
        CellValue::Binary(b) => {
            let bytes = if b.is_empty() {
                raw_bytes
            } else {
                b.as_slice()
            };
            let inline = format_hex_inline(bytes);
            if let Some(hint) = utf16_hint {
                format!("{inline}  [UTF-16LE: \"{hint}\"]")
            } else {
                inline
            }
        }
        CellValue::Error => {
            if raw_bytes.is_empty() {
                "(解析错误)".to_string()
            } else {
                format_hex_inline(raw_bytes)
            }
        }
    }
}

/// Detects short printable UTF-16LE text inside `REG_BINARY` cells (e.g. SAM `UserPasswordHint`).
fn detect_utf16_text_hint(bytes: &[u8]) -> Option<String> {
    if bytes.len() < 2 || bytes.len() > 512 || !bytes.len().is_multiple_of(2) {
        return None;
    }
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    let trimmed = match units.iter().rposition(|&u| u != 0) {
        Some(last) => &units[..=last],
        None => return None,
    };
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.iter().any(|&u| u < 0x20 || u == 0x7f) {
        return None;
    }
    // Require NUL high-bytes (ASCII/Latin) or common CJK characters so arbitrary binary hashes don't false-trigger.
    let plausible_text = trimmed.iter().all(|&u| {
        (0x0020..=0x007e).contains(&u)
            || (0x4e00..=0x9fff).contains(&u)
            || (0x3000..=0x303f).contains(&u)
            || (0xff01..=0xff5e).contains(&u)
    });
    if !plausible_text {
        return None;
    }
    String::from_utf16(trimmed).ok()
}

fn format_hex_inline(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return "(零长度二进制数据)".to_string();
    }
    const LIMIT: usize = 48;
    let shown: Vec<String> = bytes
        .iter()
        .take(LIMIT)
        .map(|b| format!("{b:02x}"))
        .collect();
    if bytes.len() > LIMIT {
        format!("{} … (共 {} B)", shown.join(" "), bytes.len())
    } else {
        shown.join(" ")
    }
}

fn format_hex_dump(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    let mut lines = Vec::new();
    for (row, chunk) in bytes.chunks(16).enumerate() {
        let offset = row * 16;
        let hex_cols: Vec<String> = (0..16)
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
        lines.push(format!(
            "{offset:08x}  {} {}  |{ascii}|",
            hex_cols[..8].join(" "),
            hex_cols[8..].join(" "),
        ));
    }
    lines.join("\n")
}

fn truncate_text(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let prefix: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{prefix}…")
    } else {
        prefix
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinguishes_primary_hive_from_transaction_log() {
        let mut primary = vec![0u8; 64];
        primary[0..4].copy_from_slice(b"regf");
        primary[28..32].copy_from_slice(&0u32.to_le_bytes());
        assert!(is_primary_registry_hive(&primary));

        let mut tx_log = primary.clone();
        tx_log[28..32].copy_from_slice(&6u32.to_le_bytes());
        assert!(!is_primary_registry_hive(&tx_log));
    }

    #[test]
    fn detects_utf16_password_hint_and_blocks_random_binary() {
        assert_eq!(
            detect_utf16_text_hint(&[0x32, 0x00, 0x38, 0x00]).as_deref(),
            Some("28")
        );
        assert_eq!(detect_utf16_text_hint(&[0xff, 0x00, 0x10, 0x80]), None);
    }

    #[test]
    fn parses_real_windows_config_hives_when_present() {
        let cfg =
            std::path::Path::new(r"C:\hlnet\1-1794883754\COM-01.e01\分区1\Windows\System32\config");
        if !cfg.exists() {
            return;
        }
        for name in [
            "BBI", "DEFAULT", "DRIVERS", "ELAM", "SAM", "SECURITY", "SYSTEM", "userdiff",
        ] {
            let path = cfg.join(name);
            let bytes = std::fs::read(&path).expect("read hive");
            assert!(
                is_primary_registry_hive(&bytes),
                "{name} should match regf primary"
            );
            let mut logs = Vec::new();
            for ext in ["LOG1", "LOG2"] {
                let lp = cfg.join(format!("{name}.{ext}"));
                if lp.exists() {
                    let lb = std::fs::read(&lp).unwrap_or_default();
                    if !lb.is_empty() {
                        logs.push((format!("{name}.{ext}"), lb));
                    }
                }
            }
            if logs.is_empty() {
                let lp = cfg.join(format!("{name}.LOG"));
                if lp.exists() {
                    let lb = std::fs::read(&lp).unwrap_or_default();
                    if !lb.is_empty() {
                        logs.push((format!("{name}.LOG"), lb));
                    }
                }
            }
            let file_size = bytes.len() as u64;
            let (mut parser, digest) = open_registry_hive(bytes, file_size, name, logs)
                .unwrap_or_else(|e| panic!("failed to open {name}: {e}"));
            let detail = read_key_detail(&mut parser, "", None)
                .unwrap_or_else(|e| panic!("failed to read root of {name}: {e}"));
            assert_eq!(detail.name, digest.root_name);
        }
    }
}
