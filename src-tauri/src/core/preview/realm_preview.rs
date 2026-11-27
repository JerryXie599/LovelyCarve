//! Realm native-type preview for files inside a forensic image.
//!
//! The vendored `realm-db-reader` has been extended to read from immutable
//! in-memory bytes, so this module never materialises the database to a host
//! temp file. Commands keep a single-slot byte cache and pass the shared slice
//! here for table discovery and paged row reads.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;

use anyhow::Context;
use realm_db_reader::{Column, Group, Realm, RealmFileError, Table, Value};
use serde::Serialize;

use super::realm_tolerant;

const MAX_PAGE_ROWS: usize = 500;
const MAX_TEXT_CHARS: usize = 500;
const MAX_BINARY_PREVIEW_BYTES: usize = 64;
const MAX_LINKS: usize = 24;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RealmDigest {
    pub tables: Vec<RealmTable>,
    pub bytes_read: u64,
    pub truncated_bytes: bool,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RealmTable {
    pub name: String,
    pub table_number: usize,
    pub columns: Vec<RealmColumn>,
    pub row_count: Option<usize>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RealmColumn {
    pub index: usize,
    pub name: Option<String>,
    pub nullable: bool,
    pub indexed: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RealmRows {
    pub columns: Vec<RealmColumn>,
    pub rows: Vec<RealmRow>,
    pub total: usize,
    pub offset: usize,
    pub returned: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RealmRow {
    pub index: usize,
    pub cells: Vec<RealmCell>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum RealmCell {
    Null,
    Int(i64),
    Bool(bool),
    String {
        text: String,
        truncated: bool,
    },
    Binary(RealmBinary),
    Table {
        rows: usize,
    },
    Timestamp(String),
    Float(f32),
    Double(f64),
    Link(RealmLink),
    LinkList {
        links: Vec<RealmLink>,
        truncated: bool,
    },
    Backlink(RealmBacklink),
    Unsupported(String),
    Error(String),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RealmBinary {
    pub size: usize,
    pub hex: String,
    pub truncated: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RealmLink {
    pub target_table_number: usize,
    pub row_number: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RealmBacklink {
    pub origin_table_number: usize,
    pub origin_column_number: usize,
    pub row_numbers: Vec<usize>,
    pub truncated: bool,
}

pub fn preview(
    bytes: Arc<[u8]>,
    _file_size: u64,
    truncated_bytes: bool,
) -> anyhow::Result<RealmDigest> {
    let bytes_read = bytes.len() as u64;
    let group = match open_group(Arc::clone(&bytes)) {
        Ok(group) => group,
        Err(_) => return Ok(tolerant_digest(&bytes, bytes_read, truncated_bytes)),
    };
    let names = group.get_table_names().to_vec();
    let mut tables = Vec::with_capacity(names.len());

    for (table_number, name) in names.into_iter().enumerate() {
        let table_result =
            catch_parser(|| group.get_table(table_number).map_err(anyhow::Error::from));
        let table = match table_result {
            Ok(table) => table,
            Err(err) => {
                tables.push(RealmTable {
                    name,
                    table_number,
                    columns: Vec::new(),
                    row_count: None,
                    error: Some(err.to_string()),
                });
                continue;
            }
        };

        let columns = columns_for(&table);
        let row_count = catch_parser(|| table.row_count().map_err(anyhow::Error::from));
        let (row_count, error) = match row_count {
            Ok(count) => (Some(count), None),
            Err(err) => (None, Some(err.to_string())),
        };
        tables.push(RealmTable {
            name,
            table_number,
            columns,
            row_count,
            error,
        });
    }

    let mut notes = Vec::new();
    if truncated_bytes {
        notes.push("文件超过 Realm 预览读取上限，当前解析的是前缀字节。".to_string());
    }

    // 精读无任何可用表（新格式布局差异，如 11.x / 24.x）→ 容错解析兜底。
    let usable = tables
        .iter()
        .filter(|table| table.error.is_none() && !table.columns.is_empty())
        .count();
    if usable == 0 {
        let fallback = tolerant_digest(&bytes, bytes_read, truncated_bytes);
        if !fallback.tables.is_empty() {
            return Ok(fallback);
        }
    }

    Ok(RealmDigest {
        tables,
        bytes_read,
        truncated_bytes,
        notes,
    })
}

/// 容错解析（11.x / 24.x 新格式）：列值字符串化，表名按 class_* 顺序。
fn tolerant_digest(bytes: &[u8], bytes_read: u64, truncated_bytes: bool) -> RealmDigest {
    let tables = realm_tolerant::parse_tables(bytes);
    let mapped = tables
        .iter()
        .enumerate()
        .map(|(table_number, table)| RealmTable {
            name: table.name.clone(),
            table_number,
            columns: table
                .columns
                .iter()
                .enumerate()
                .map(|(index, (name, _))| RealmColumn {
                    index,
                    name: Some(name.clone()),
                    nullable: true,
                    indexed: false,
                })
                .collect(),
            row_count: Some(
                table
                    .columns
                    .iter()
                    .map(|(_, values)| values.len())
                    .max()
                    .unwrap_or(0),
            ),
            error: None,
        })
        .collect();
    RealmDigest {
        tables: mapped,
        bytes_read,
        truncated_bytes,
        notes: vec![
            "该 Realm 文件为 11.x / 24.x 新格式，已切换容错解析器：列值以字符串呈现，时间戳为 UTC+8。".to_string(),
        ],
    }
}

pub fn table_rows(
    bytes: Arc<[u8]>,
    table_number: usize,
    offset: usize,
    limit: usize,
) -> anyhow::Result<RealmRows> {
    // 与 preview 同口径：精读打不开该表时走容错解析（保持表序号一致）。
    let vendored_usable = open_group(Arc::clone(&bytes)).ok().and_then(|group| {
        catch_parser(|| group.get_table(table_number).map_err(anyhow::Error::from))
            .ok()
            .filter(|table| !table.get_column_specs().is_empty())
            .map(|_| ())
    });
    if vendored_usable.is_none() {
        return tolerant_rows(&bytes, table_number, offset, limit);
    }
    let group = open_group(bytes)?;
    let table = catch_parser(|| group.get_table(table_number).map_err(anyhow::Error::from))
        .with_context(|| format!("打开 Realm 表 #{table_number} 失败"))?;
    let total = catch_parser(|| table.row_count().map_err(anyhow::Error::from))
        .context("读取 Realm 表行数失败")?;
    let columns = columns_for(&table);
    let offset = offset.min(total);
    let limit = limit.clamp(1, MAX_PAGE_ROWS);
    let end = offset.saturating_add(limit).min(total);
    let mut rows = Vec::with_capacity(end.saturating_sub(offset));

    for row_index in offset..end {
        let mut cells = Vec::with_capacity(columns.len());
        for column_index in 0..columns.len() {
            let cell = match catch_parser(|| {
                table
                    .get_value(row_index, column_index)
                    .map(value_to_cell)
                    .map_err(anyhow::Error::from)
            }) {
                Ok(cell) => cell,
                Err(err) => RealmCell::Error(err.to_string()),
            };
            cells.push(cell);
        }
        rows.push(RealmRow {
            index: row_index,
            cells,
        });
    }

    let returned = rows.len();
    Ok(RealmRows {
        columns,
        rows,
        total,
        offset,
        returned,
    })
}

fn open_group(bytes: Arc<[u8]>) -> anyhow::Result<Group> {
    catch_parser(|| {
        Realm::from_arc_bytes(bytes)
            .and_then(Realm::into_group)
            .map_err(anyhow::Error::from)
    })
    .context("打开 Realm 数据库失败")
}

fn tolerant_rows(
    bytes: &[u8],
    table_number: usize,
    offset: usize,
    limit: usize,
) -> anyhow::Result<RealmRows> {
    let tables = realm_tolerant::parse_tables(bytes);
    let table = tables.get(table_number).context("容错解析未找到该表")?;
    let columns: Vec<RealmColumn> = table
        .columns
        .iter()
        .enumerate()
        .map(|(index, (name, _))| RealmColumn {
            index,
            name: Some(name.clone()),
            nullable: true,
            indexed: false,
        })
        .collect();
    let total = table
        .columns
        .iter()
        .map(|(_, values)| values.len())
        .max()
        .unwrap_or(0);
    let offset = offset.min(total);
    let limit = limit.clamp(1, MAX_PAGE_ROWS);
    let end = offset.saturating_add(limit).min(total);
    let mut rows = Vec::with_capacity(end.saturating_sub(offset));
    for row_index in offset..end {
        let cells = table
            .columns
            .iter()
            .map(|(_, values)| match values.get(row_index) {
                Some(text) if text.is_empty() => RealmCell::Null,
                Some(text) => RealmCell::String {
                    text: text.chars().take(MAX_TEXT_CHARS).collect::<String>(),
                    truncated: text.chars().count() > MAX_TEXT_CHARS,
                },
                None => RealmCell::Null,
            })
            .collect();
        rows.push(RealmRow {
            index: row_index,
            cells,
        });
    }
    let returned = rows.len();
    Ok(RealmRows {
        columns,
        rows,
        total,
        offset,
        returned,
    })
}

fn columns_for(table: &Table) -> Vec<RealmColumn> {
    table
        .get_column_specs()
        .iter()
        .enumerate()
        .map(|(index, column)| column_meta(index, column.as_ref()))
        .collect()
}

fn column_meta(index: usize, column: &dyn Column) -> RealmColumn {
    RealmColumn {
        index,
        name: column.name().map(ToOwned::to_owned),
        nullable: column.nullable(),
        indexed: column.is_indexed(),
    }
}

fn value_to_cell(value: Value) -> RealmCell {
    match value {
        Value::Int(value) => RealmCell::Int(value),
        Value::Bool(value) => RealmCell::Bool(value),
        Value::String(value) => {
            let (text, truncated) = truncate_chars(&value, MAX_TEXT_CHARS);
            RealmCell::String { text, truncated }
        }
        Value::OldStringEnum(value) => RealmCell::Unsupported(format!("OldStringEnum({value})")),
        Value::Binary(bytes) => {
            let take = bytes.len().min(MAX_BINARY_PREVIEW_BYTES);
            RealmCell::Binary(RealmBinary {
                size: bytes.len(),
                hex: hex::encode(&bytes[..take]),
                truncated: take < bytes.len(),
            })
        }
        Value::Table(rows) => RealmCell::Table { rows: rows.len() },
        Value::OldMixed => RealmCell::Unsupported("OldMixed".to_string()),
        Value::OldDateTime => RealmCell::Unsupported("OldDateTime".to_string()),
        Value::Timestamp(value) => RealmCell::Timestamp(value.to_rfc3339()),
        Value::Float(value) => RealmCell::Float(value),
        Value::Double(value) => RealmCell::Double(value),
        Value::Reserved4 => RealmCell::Unsupported("Decimal/Reserved4".to_string()),
        Value::Link(link) => RealmCell::Link(RealmLink {
            target_table_number: link.target_table_number,
            row_number: link.row_number,
        }),
        Value::LinkList(links) => {
            let truncated = links.len() > MAX_LINKS;
            RealmCell::LinkList {
                links: links
                    .into_iter()
                    .take(MAX_LINKS)
                    .map(|link| RealmLink {
                        target_table_number: link.target_table_number,
                        row_number: link.row_number,
                    })
                    .collect(),
                truncated,
            }
        }
        Value::BackLink(backlink) => {
            let truncated = backlink.row_numbers.len() > MAX_LINKS;
            RealmCell::Backlink(RealmBacklink {
                origin_table_number: backlink.origin_table_number,
                origin_column_number: backlink.origin_column_number,
                row_numbers: backlink.row_numbers.into_iter().take(MAX_LINKS).collect(),
                truncated,
            })
        }
        Value::None => RealmCell::Null,
    }
}

fn truncate_chars(text: &str, max_chars: usize) -> (String, bool) {
    if text.chars().count() <= max_chars {
        return (text.to_string(), false);
    }
    (text.chars().take(max_chars).collect(), true)
}

fn catch_parser<T>(operation: impl FnOnce() -> anyhow::Result<T>) -> anyhow::Result<T> {
    match catch_unwind(AssertUnwindSafe(operation)) {
        Ok(result) => result,
        Err(payload) => Err(anyhow::Error::from(RealmFileError::InvalidRealmFile {
            reason: format!("Realm parser panicked: {}", panic_message(payload)),
        })),
    }
}

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        return (*message).to_string();
    }
    if let Some(message) = payload.downcast_ref::<String>() {
        return message.clone();
    }
    "unknown panic".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncates_string_cells_by_char_count() {
        let long = "数".repeat(MAX_TEXT_CHARS + 2);
        let (text, truncated) = truncate_chars(&long, MAX_TEXT_CHARS);
        assert!(truncated);
        assert_eq!(text.chars().count(), MAX_TEXT_CHARS);
    }

    #[test]
    fn renders_link_list_with_cap() {
        let links = (0..(MAX_LINKS + 3))
            .map(|row_number| realm_db_reader::Link::new(2, row_number))
            .collect();
        let cell = value_to_cell(Value::LinkList(links));
        let RealmCell::LinkList { links, truncated } = cell else {
            panic!("expected link list cell");
        };
        assert!(truncated);
        assert_eq!(links.len(), MAX_LINKS);
    }
}

#[cfg(test)]
mod real_fixture_tests {
    use super::*;
    use std::path::Path;
    use std::sync::Arc;

    /// 真实检材回归（本机路径，`cargo test --lib realm -- --include-ignored`）：
    /// 25 美亚杯 CHAN_MH 手机镜像里的两个 v11.11 Realm 文件。验证放宽版本
    /// 门禁后 group→表→列遍历完整可用（音频库与 SendAnywhere 传输库）。
    #[test]
    #[ignore = "本机检材路径"]
    fn opens_v11_realm_files_from_chan_mh() {
        // 25 美亚 CHAN_MH 检材的 paprika audio.realm（快照）。注意：该检材目录
        // 曾被 Realm Studio 以读写方式打开过——Realm Core 打开旧格式会原地升级
        // （11.11 → 24.11、16KB → 64KB），取证应当使用原始文件副本，勿用
        // Realm Studio 直接开检材内文件。
        // 走 preview() 公共入口：vendored 精读对新布局失败时自动降级容错解析。
        let snapshot =
            Path::new("E:/software/04-Forensics/LovelyCarve/_audit/audio_realm_snapshot.realm");
        let bytes = std::fs::read(snapshot)
            .unwrap_or_else(|error| panic!("快照不存在 {}: {error}", snapshot.display()));
        let byte_len = bytes.len();
        let arc: Arc<[u8]> = Arc::from(bytes.into_boxed_slice());
        let digest = preview(Arc::clone(&arc), byte_len as u64, false).expect("preview 失败");
        assert!(!digest.tables.is_empty(), "预览未发现任何表");
        assert!(
            digest.notes.iter().any(|note| note.contains("容错解析")),
            "v24 快照应走容错解析路径，notes={:?}",
            digest.notes
        );
        for table in &digest.tables {
            tracing::info!(
                "table #{}/>{} cols={} rows={:?}",
                table.table_number,
                table.name,
                table.columns.len(),
                table.row_count
            );
        }

        // 分页读第一张表：降级路径的行读取也要可用。
        let rows = table_rows(Arc::clone(&arc), 0, 0, 10).expect("容错行读取失败");
        assert!(!rows.rows.is_empty(), "第一张表无行");
    }
}
