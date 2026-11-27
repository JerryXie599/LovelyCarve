//! Offline PostgreSQL `PGDATA` cluster & heap page parser for forensic analysis.
//!
//! Supports:
//! - Full `PGDATA` directory parsing (`PG_VERSION`, `global/pg_filenode.map`,
//!   `pg_database` OID 1262, `pg_class` OID 1259, `pg_attribute` OID 1249)
//! - Standalone 8 KiB heap segment parsing (`base/<db_oid>/<relfilenode>`)
//! - Live AND dead/deleted tuple recovery (`t_xmin`, `t_xmax`, `LP_DEAD` / `HEAP_XMAX_*`)
//! - PostgreSQL base-10000 `numeric` (short & long), `timestamptz`/`timestamp`/`date`,
//!   `uuid`, `json`/`jsonb`, `pglz` inline compression, and external `pg_toast` chunk reassembly
//! - Automatic Fernet token (`gAAAAA...`) detection & timestamp extraction

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use anyhow::{anyhow, bail, ensure};
use base64::Engine;
use serde::Serialize;

use crate::core::filesystem::evidence_session::EvidenceSession;
use crate::core::filesystem::tree::fs_tree::{self, FsNode, NodeKind};
use crate::core::preview::sqlite::sqlite_preview::truncate_text;

pub const PG_PAGE_SIZE: usize = 8192;
const RELMAP_MAGIC: u32 = 0x592717;
const PG_EPOCH_UNIX_SECS: i64 = 946_684_800; // 2000-01-01 00:00:00 UTC
const MAX_HEAP_BYTES: usize = 128 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PgColumn {
    pub attnum: i16,
    pub name: String,
    pub type_oid: u32,
    pub type_name: String,
    pub attlen: i16,
    pub attalign: u8,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PgTableSummary {
    pub db_oid: u32,
    pub db_name: String,
    pub rel_oid: u32,
    pub relfilenode: u32,
    pub toast_relid: u32,
    pub name: String,
    pub node_id: String,
    pub size_bytes: u64,
    pub live_tuples: usize,
    pub dead_tuples: usize,
    pub columns: Vec<PgColumn>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PgDigest {
    pub pg_version: Option<String>,
    pub mode: String, // "cluster" | "single_heap"
    pub root_node_id: String,
    pub databases: Vec<String>,
    pub tables: Vec<PgTableSummary>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PgTupleRow {
    pub page: u32,
    pub item_index: u16,
    pub xmin: u32,
    pub xmax: u32,
    pub deleted: bool,
    pub values: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fernet_tokens: Vec<FernetDetection>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FernetDetection {
    pub column: String,
    pub token_prefix: String,
    pub timestamp_utc8: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PgTableRows {
    pub table_name: String,
    pub columns: Vec<PgColumn>,
    pub rows: Vec<PgTupleRow>,
    pub total_live: usize,
    pub total_dead: usize,
    pub offset: usize,
    pub returned: usize,
}

/// Parse `global/pg_filenode.map` or `base/<dboid>/pg_filenode.map` (512-byte RelMapFile).
pub fn parse_relmap(bytes: &[u8]) -> BTreeMap<u32, u32> {
    let mut map = BTreeMap::new();
    if bytes.len() < 512 {
        return map;
    }
    let magic_le = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let num_le = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
    if magic_le != RELMAP_MAGIC || num_le > 62 {
        return map;
    }
    for i in 0..num_le {
        let off = 8 + i * 8;
        if let Some(entry) = bytes.get(off..off + 8) {
            let oid = u32::from_le_bytes([entry[0], entry[1], entry[2], entry[3]]);
            let fn_node = u32::from_le_bytes([entry[4], entry[5], entry[6], entry[7]]);
            if oid != 0 && fn_node != 0 {
                map.insert(oid, fn_node);
            }
        }
    }
    map
}

/// Detect whether `bytes` is a valid 8 KiB PostgreSQL heap page (`PageHeaderData`).
pub fn looks_like_pg_heap(bytes: &[u8]) -> bool {
    if bytes.len() < PG_PAGE_SIZE || !bytes.len().is_multiple_of(PG_PAGE_SIZE) {
        return false;
    }
    let pd_lower = u16::from_le_bytes([bytes[12], bytes[13]]) as usize;
    let pd_upper = u16::from_le_bytes([bytes[14], bytes[15]]) as usize;
    let pd_special = u16::from_le_bytes([bytes[16], bytes[17]]) as usize;
    let pd_pagesize_ver = u16::from_le_bytes([bytes[18], bytes[19]]);
    let page_size = (pd_pagesize_ver & 0xFF00) as usize;
    let ver = pd_pagesize_ver & 0x00FF;
    page_size == PG_PAGE_SIZE
        && ver == 4
        && (24..=PG_PAGE_SIZE).contains(&pd_lower)
        && pd_upper >= pd_lower
        && pd_special >= pd_upper
        && pd_special <= PG_PAGE_SIZE
}

/// Inspect a `PGDATA` directory or single heap file node inside `image`.
pub fn preview_pgdata(
    image: &Arc<EvidenceSession>,
    node_id: &str,
    parent_id: Option<&str>,
) -> anyhow::Result<PgDigest> {
    let children = fs_tree::children_for_image(image, node_id).unwrap_or_default();
    let has_base = children
        .iter()
        .any(|c| c.name == "base" && c.kind == NodeKind::Dir);
    let has_pg_ver = children.iter().any(|c| c.name == "PG_VERSION");

    if has_base || has_pg_ver {
        return inspect_cluster_dir(image, node_id, &children);
    }

    // Check if parent or grandparent is PGDATA when user clicked `PG_VERSION` or a file under `base/<db>/<rel>`
    if let Some(pid) = parent_id {
        let p_children = fs_tree::children_for_image(image, pid).unwrap_or_default();
        if p_children
            .iter()
            .any(|c| c.name == "base" && c.kind == NodeKind::Dir)
        {
            return inspect_cluster_dir(image, pid, &p_children);
        }
    }

    // Otherwise parse as a single standalone PostgreSQL heap file.
    let bytes = fs_tree::read_file_limited(image, node_id, MAX_HEAP_BYTES)?;
    ensure!(
        looks_like_pg_heap(&bytes),
        "不是有效的 PostgreSQL 8 KiB 堆表页或 PGDATA 目录"
    );
    let (rows, cols) = parse_heap_pages(&bytes, &[], &HashMap::new());
    let live = rows.iter().filter(|r| !r.deleted).count();
    let dead = rows.iter().filter(|r| r.deleted).count();
    Ok(PgDigest {
        pg_version: None,
        mode: "single_heap".to_string(),
        root_node_id: node_id.to_string(),
        databases: vec!["standalone".to_string()],
        tables: vec![PgTableSummary {
            db_oid: 0,
            db_name: "standalone".to_string(),
            rel_oid: 0,
            relfilenode: 0,
            toast_relid: 0,
            name: node_id.to_string(),
            node_id: node_id.to_string(),
            size_bytes: bytes.len() as u64,
            live_tuples: live,
            dead_tuples: dead,
            columns: cols,
        }],
        notes: vec!["单文件无系统目录模式：已按元组字节布局启发式推断列".to_string()],
    })
}

/// Read rows from a specific table (`table_node_id` or `relfilenode`) in a `PGDATA` cluster or heap file.
#[allow(clippy::too_many_arguments)]
pub fn read_pgdata_table_rows(
    image: &Arc<EvidenceSession>,
    node_id: &str,
    parent_id: Option<&str>,
    table_node_id: Option<&str>,
    include_dead: bool,
    offset: usize,
    limit: usize,
) -> anyhow::Result<PgTableRows> {
    let digest = preview_pgdata(image, node_id, parent_id)?;
    let table = if let Some(tid) = table_node_id.filter(|s| !s.is_empty()) {
        digest
            .tables
            .into_iter()
            .find(|t| t.node_id == tid || t.name == tid)
            .ok_or_else(|| anyhow!("未找到指定的 PostgreSQL 表：{tid}"))?
    } else {
        digest
            .tables
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("未在该 PGDATA 中发现可读用户表"))?
    };

    let bytes =
        if let Some(sibling_files) = find_db_dir_files(image, &digest.root_node_id, table.db_oid) {
            let base_name = sibling_files
                .iter()
                .find(|f| f.id == table.node_id)
                .map(|f| f.name.clone())
                .unwrap_or_else(|| table.relfilenode.to_string());
            read_multi_segment_heap(image, &table.node_id, &base_name, &sibling_files)?
        } else {
            read_multi_segment_heap(image, &table.node_id, "", &[])?
        };
    let pg_major = parse_pg_major(digest.pg_version.as_deref());
    let toast_chunks = load_toast_map_for_table(image, &digest.root_node_id, &table, pg_major);
    let (all_rows, inferred_cols) = parse_heap_pages(&bytes, &table.columns, &toast_chunks);
    let total_live = all_rows.iter().filter(|r| !r.deleted).count();
    let total_dead = all_rows.iter().filter(|r| r.deleted).count();

    let filtered: Vec<PgTupleRow> = all_rows
        .into_iter()
        .filter(|r| include_dead || !r.deleted)
        .skip(offset)
        .take(limit.clamp(1, 1000))
        .collect();
    let returned = filtered.len();
    let columns = if table.columns.is_empty() {
        inferred_cols
    } else {
        table.columns
    };

    Ok(PgTableRows {
        table_name: table.name,
        columns,
        rows: filtered,
        total_live,
        total_dead,
        offset,
        returned,
    })
}

fn inspect_cluster_dir(
    image: &Arc<EvidenceSession>,
    root_id: &str,
    children: &[FsNode],
) -> anyhow::Result<PgDigest> {
    let mut notes = Vec::new();
    let pg_version = children
        .iter()
        .find(|c| c.name == "PG_VERSION")
        .and_then(|c| fs_tree::read_file_limited(image, &c.id, 64).ok())
        .map(|b| String::from_utf8_lossy(&b).trim().to_string());
    let pg_major = parse_pg_major(pg_version.as_deref());

    // 1. Parse global/pg_filenode.map & pg_database (OID 1262)
    let mut db_names: BTreeMap<u32, String> = BTreeMap::new();
    if let Some(global_dir) = children
        .iter()
        .find(|c| c.name == "global" && c.kind == NodeKind::Dir)
    {
        let g_children = fs_tree::children_for_image(image, &global_dir.id).unwrap_or_default();
        let g_relmap = g_children
            .iter()
            .find(|c| c.name == "pg_filenode.map")
            .and_then(|c| fs_tree::read_file_limited(image, &c.id, 1024).ok())
            .map(|b| parse_relmap(&b))
            .unwrap_or_default();
        let db_filenode = g_relmap.get(&1262).copied().unwrap_or(1262);
        if let Some(db_node) = g_children
            .iter()
            .find(|c| c.name == db_filenode.to_string())
        {
            if let Ok(db_bytes) = fs_tree::read_file_limited(image, &db_node.id, 4 * 1024 * 1024) {
                for (oid, datname) in parse_pg_database(&db_bytes) {
                    db_names.insert(oid, datname);
                }
            }
        }
    }

    // 2. Enumerate base/<db_oid>
    let Some(base_dir) = children
        .iter()
        .find(|c| c.name == "base" && c.kind == NodeKind::Dir)
    else {
        bail!("PGDATA 目录缺少 base 子目录");
    };
    let db_dirs = fs_tree::children_for_image(image, &base_dir.id).unwrap_or_default();
    let mut databases = Vec::new();
    let mut tables = Vec::new();

    for db_dir in db_dirs.into_iter().filter(|d| d.kind == NodeKind::Dir) {
        let Ok(db_oid) = db_dir.name.parse::<u32>() else {
            continue;
        };
        let db_name = db_names
            .get(&db_oid)
            .cloned()
            .unwrap_or_else(|| format!("db_{db_oid}"));
        if matches!(db_name.as_str(), "template0" | "template1") {
            continue;
        }
        databases.push(db_name.clone());

        let files = fs_tree::children_for_image(image, &db_dir.id).unwrap_or_default();
        let relmap = files
            .iter()
            .find(|c| c.name == "pg_filenode.map")
            .and_then(|c| fs_tree::read_file_limited(image, &c.id, 1024).ok())
            .map(|b| parse_relmap(&b))
            .unwrap_or_default();

        let class_fn = relmap.get(&1259).copied().unwrap_or(1259);
        let attr_fn = relmap.get(&1249).copied().unwrap_or(1249);

        let class_name = class_fn.to_string();
        let attr_name = attr_fn.to_string();

        let pg_classes = files
            .iter()
            .find(|c| c.name == class_name)
            .and_then(|c| read_multi_segment_heap(image, &c.id, &class_name, &files).ok())
            .map(|b| parse_pg_class(&b, pg_major))
            .unwrap_or_default();

        let mut pg_attrs = files
            .iter()
            .find(|c| c.name == attr_name)
            .and_then(|c| read_multi_segment_heap(image, &c.id, &attr_name, &files).ok())
            .map(|b| parse_pg_attribute(&b, pg_major))
            .unwrap_or_default();

        for cls in pg_classes {
            // Only ordinary tables ('r') in user namespaces (exclude pg_catalog=11, pg_toast=99, information_schema)
            if cls.relkind != b'r' || cls.relnamespace == 11 || cls.relnamespace == 99 {
                continue;
            }
            if cls.relname.starts_with("pg_") || cls.relname.starts_with("sql_") {
                continue;
            }
            let effective_fn = if cls.relfilenode != 0 {
                cls.relfilenode
            } else {
                relmap.get(&cls.oid).copied().unwrap_or(cls.oid)
            };
            let Some(file_node) = files.iter().find(|f| f.name == effective_fn.to_string()) else {
                continue;
            };
            let mut cols = pg_attrs.remove(&cls.oid).unwrap_or_default();
            cols.sort_by_key(|c| c.attnum);

            let (live, dead) = fs_tree::read_file_limited(image, &file_node.id, 16 * 1024 * 1024)
                .ok()
                .map(|b| count_heap_tuples(&b))
                .unwrap_or((0, 0));

            tables.push(PgTableSummary {
                db_oid,
                db_name: db_name.clone(),
                rel_oid: cls.oid,
                relfilenode: effective_fn,
                toast_relid: cls.reltoastrelid,
                name: cls.relname,
                node_id: file_node.id.clone(),
                size_bytes: file_node.size,
                live_tuples: live,
                dead_tuples: dead,
                columns: cols,
            });
        }
    }

    if tables.is_empty() {
        notes.push("系统目录未匹配到用户表，或当前目录仅包含空表".to_string());
    }

    Ok(PgDigest {
        pg_version,
        mode: "cluster".to_string(),
        root_node_id: root_id.to_string(),
        databases,
        tables,
        notes,
    })
}

fn parse_pg_major(pg_version: Option<&str>) -> Option<u32> {
    pg_version
        .and_then(|v| v.trim().split('.').next())
        .and_then(|major| major.parse::<u32>().ok())
}

fn find_db_dir_files(
    image: &Arc<EvidenceSession>,
    root_id: &str,
    db_oid: u32,
) -> Option<Vec<FsNode>> {
    if db_oid == 0 {
        return None;
    }
    let children = fs_tree::children_for_image(image, root_id).ok()?;
    let base_dir = children
        .iter()
        .find(|c| c.name == "base" && c.kind == NodeKind::Dir)?;
    let db_dirs = fs_tree::children_for_image(image, &base_dir.id).ok()?;
    let target_oid = db_oid.to_string();
    let db_dir = db_dirs.iter().find(|d| d.name == target_oid)?;
    fs_tree::children_for_image(image, &db_dir.id).ok()
}

fn read_multi_segment_heap(
    image: &Arc<EvidenceSession>,
    node_id: &str,
    base_name: &str,
    sibling_files: &[FsNode],
) -> anyhow::Result<Vec<u8>> {
    let mut bytes = fs_tree::read_file_limited(image, node_id, MAX_HEAP_BYTES)?;
    if base_name.is_empty() || sibling_files.is_empty() {
        return Ok(bytes);
    }
    for seg in 1u32.. {
        if bytes.len() >= MAX_HEAP_BYTES {
            break;
        }
        let seg_name = format!("{base_name}.{seg}");
        let Some(seg_node) = sibling_files
            .iter()
            .find(|f| f.kind != NodeKind::Dir && f.name == seg_name)
        else {
            break;
        };
        let remaining = MAX_HEAP_BYTES - bytes.len();
        let Ok(chunk) = fs_tree::read_file_limited(image, &seg_node.id, remaining) else {
            break;
        };
        if chunk.is_empty() {
            break;
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn load_toast_map_for_table(
    image: &Arc<EvidenceSession>,
    root_id: &str,
    table: &PgTableSummary,
    pg_major: Option<u32>,
) -> HashMap<u32, Vec<u8>> {
    let mut map = HashMap::new();
    if table.toast_relid == 0 || table.db_oid == 0 {
        return map;
    }
    let Some(files) = find_db_dir_files(image, root_id, table.db_oid) else {
        return map;
    };
    // Find pg_class entry for toast_relid to map to its relfilenode
    let relmap = files
        .iter()
        .find(|c| c.name == "pg_filenode.map")
        .and_then(|c| fs_tree::read_file_limited(image, &c.id, 1024).ok())
        .map(|b| parse_relmap(&b))
        .unwrap_or_default();
    let class_fn = relmap.get(&1259).copied().unwrap_or(1259);
    let class_name = class_fn.to_string();
    let toast_fn = files
        .iter()
        .find(|c| c.name == class_name)
        .and_then(|c| read_multi_segment_heap(image, &c.id, &class_name, &files).ok())
        .map(|b| parse_pg_class(&b, pg_major))
        .unwrap_or_default()
        .into_iter()
        .find(|cls| cls.oid == table.toast_relid)
        .map(|cls| {
            if cls.relfilenode != 0 {
                cls.relfilenode
            } else {
                cls.oid
            }
        })
        .unwrap_or(table.toast_relid);

    let toast_name = toast_fn.to_string();
    let Some(toast_file) = files.iter().find(|f| f.name == toast_name) else {
        return map;
    };
    let Ok(toast_bytes) = read_multi_segment_heap(image, &toast_file.id, &toast_name, &files)
    else {
        return map;
    };
    parse_toast_heap(&toast_bytes, &mut map);
    map
}

fn parse_toast_heap(bytes: &[u8], out: &mut HashMap<u32, Vec<u8>>) {
    let mut chunks: BTreeMap<(u32, i32), Vec<u8>> = BTreeMap::new();
    for page in bytes.chunks_exact(PG_PAGE_SIZE) {
        for (_idx, _xmin, _xmax, _deleted, _natts, data, _nulls) in iter_page_tuples(page) {
            if data.len() < 12 {
                continue;
            }
            let chunk_id = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
            let chunk_seq = i32::from_le_bytes([data[4], data[5], data[6], data[7]]);
            if let Some((payload, _consumed)) = read_varlena(&data[8..]) {
                chunks.insert((chunk_id, chunk_seq), payload);
            }
        }
    }
    for ((cid, _seq), part) in chunks {
        out.entry(cid).or_default().extend_from_slice(&part);
    }
}

struct PgClassRow {
    oid: u32,
    relname: String,
    relnamespace: u32,
    relfilenode: u32,
    reltoastrelid: u32,
    relkind: u8,
}

fn parse_c_name64(slice: &[u8]) -> String {
    let end = slice.iter().position(|&b| b == 0).unwrap_or(slice.len());
    String::from_utf8_lossy(&slice[..end]).into_owned()
}

fn is_valid_pg_class_pair(persistence: u8, relkind: u8) -> bool {
    matches!(persistence, b'p' | b'u' | b't')
        && matches!(
            relkind,
            b'r' | b'i' | b'S' | b't' | b'v' | b'm' | b'c' | b'f' | b'p' | b'I'
        )
}

fn pg_type_default_align(oid: u32) -> u8 {
    match oid {
        16 | 18 | 19 | 2950 => b'c',
        21 => b's',
        20 | 701 | 1114 | 1184 => b'd',
        _ => b'i',
    }
}

fn parse_pg_database(bytes: &[u8]) -> Vec<(u32, String)> {
    let mut out = Vec::new();
    for page in bytes.chunks_exact(PG_PAGE_SIZE) {
        for (_idx, _xmin, _xmax, deleted, _natts, data, _nulls) in iter_page_tuples(page) {
            if deleted {
                continue;
            }
            // PG 12+: oid (4B) + datname (64B); PG <=11: datname (64B) with OID in header
            if data.len() >= 68 {
                let oid = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let name = parse_c_name64(&data[4..68]);
                if oid > 0
                    && !name.is_empty()
                    && name.chars().all(|c| c.is_ascii_graphic() || c == '_')
                {
                    out.push((oid, name));
                }
            }
        }
    }
    out
}

fn parse_pg_class(bytes: &[u8], pg_major: Option<u32>) -> Vec<PgClassRow> {
    let mut rows: Vec<(u32, PgClassRow)> = Vec::new();
    let mut by_oid: HashMap<u32, usize> = HashMap::new();

    for page in bytes.chunks_exact(PG_PAGE_SIZE) {
        for (_idx, xmin, _xmax, deleted, _natts, data, _nulls) in iter_page_tuples(page) {
            if deleted || data.len() < 116 {
                continue;
            }
            // PG 12..17 FormData_pg_class:
            //   108..112: reltoastrelid, 114: relpersistence, 115: relkind
            // PG 18+ inserted `relallfrozen: i32` at 108..112:
            //   112..116: reltoastrelid, 118: relpersistence, 119: relkind
            let use_pg18 = match pg_major {
                Some(v) if v >= 18 => true,
                Some(_) => false,
                None => {
                    data.len() >= 120
                        && is_valid_pg_class_pair(data[118], data[119])
                        && !is_valid_pg_class_pair(data[114], data[115])
                }
            };
            let (toast_off, kind_off, min_len) = if use_pg18 {
                (112, 119, 120)
            } else {
                (108, 115, 116)
            };
            if data.len() < min_len {
                continue;
            }
            let oid = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
            let relname = parse_c_name64(&data[4..68]);
            let relnamespace = u32::from_le_bytes([data[68], data[69], data[70], data[71]]);
            let relfilenode = u32::from_le_bytes([data[88], data[89], data[90], data[91]]);
            let reltoastrelid = u32::from_le_bytes([
                data[toast_off],
                data[toast_off + 1],
                data[toast_off + 2],
                data[toast_off + 3],
            ]);
            let relkind = data[kind_off];
            if oid == 0 || relname.is_empty() {
                continue;
            }
            let row = PgClassRow {
                oid,
                relname,
                relnamespace,
                relfilenode,
                reltoastrelid,
                relkind,
            };
            match by_oid.get(&oid).copied() {
                Some(slot) => {
                    if xmin >= rows[slot].0 {
                        rows[slot] = (xmin, row);
                    }
                }
                None => {
                    by_oid.insert(oid, rows.len());
                    rows.push((xmin, row));
                }
            }
        }
    }
    rows.into_iter().map(|(_, row)| row).collect()
}

fn parse_pg_attribute(bytes: &[u8], pg_major: Option<u32>) -> HashMap<u32, Vec<PgColumn>> {
    let mut best: HashMap<(u32, i16), (u32, PgColumn)> = HashMap::new();

    for page in bytes.chunks_exact(PG_PAGE_SIZE) {
        for (_idx, xmin, _xmax, deleted, _natts, data, _nulls) in iter_page_tuples(page) {
            if deleted || data.len() < 92 {
                continue;
            }
            let attrelid = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
            let attname = parse_c_name64(&data[4..68]);
            let atttypid = u32::from_le_bytes([data[68], data[69], data[70], data[71]]);

            // Resolve version-specific FormData_pg_attribute offsets:
            // - PG 17/18 (removed attstattarget & attcacheoff; attndims i16):
            //   attlen@72..74, attnum@74..76, attalign@83, attstorage@84, attisdropped@91 (min 92B)
            // - PG 16 (removed attcacheoff):
            //   attlen@76..78, attnum@78..80, attalign@87, attstorage@88, attisdropped@95 (min 96B)
            // - PG 12..15:
            //   attlen@76..78, attnum@78..80, attalign@93 (legacy fallback 89/90) (min 95B)
            let layout = match pg_major {
                Some(v) if v >= 17 => 17,
                Some(16) => 16,
                Some(_) => 15,
                None => {
                    if matches!(data.get(83), Some(b'c' | b's' | b'i' | b'd'))
                        && matches!(data.get(84), Some(b'p' | b'e' | b'x' | b'm'))
                        && i16::from_le_bytes([data[74], data[75]]) > 0
                    {
                        17
                    } else if data.len() >= 96
                        && matches!(data.get(87), Some(b'c' | b's' | b'i' | b'd'))
                        && matches!(data.get(88), Some(b'p' | b'e' | b'x' | b'm'))
                        && i16::from_le_bytes([data[78], data[79]]) > 0
                    {
                        16
                    } else {
                        15
                    }
                }
            };

            let (attlen, attnum, raw_align, attisdropped) = match layout {
                17 => (
                    i16::from_le_bytes([data[72], data[73]]),
                    i16::from_le_bytes([data[74], data[75]]),
                    Some(data[83]),
                    data[91] != 0,
                ),
                16 => {
                    if data.len() < 96 {
                        continue;
                    }
                    (
                        i16::from_le_bytes([data[76], data[77]]),
                        i16::from_le_bytes([data[78], data[79]]),
                        Some(data[87]),
                        data[95] != 0,
                    )
                }
                _ => {
                    if data.len() < 95 {
                        continue;
                    }
                    let align = [93usize, 94, 89, 90]
                        .into_iter()
                        .filter_map(|off| data.get(off).copied())
                        .find(|b| matches!(b, b'c' | b's' | b'i' | b'd'));
                    let dropped = data.get(101).copied().is_some_and(|b| b != 0) || atttypid == 0;
                    (
                        i16::from_le_bytes([data[76], data[77]]),
                        i16::from_le_bytes([data[78], data[79]]),
                        align,
                        dropped,
                    )
                }
            };

            let attalign = match raw_align {
                Some(b @ (b'c' | b's' | b'i' | b'd')) => b,
                _ => pg_type_default_align(atttypid),
            };

            if attisdropped
                || attnum <= 0
                || atttypid == 0
                || attname.is_empty()
                || attname.starts_with("........pg.dropped.")
            {
                continue;
            }

            let col = PgColumn {
                attnum,
                name: attname,
                type_oid: atttypid,
                type_name: pg_type_name(atttypid).to_string(),
                attlen,
                attalign,
            };
            match best.get(&(attrelid, attnum)) {
                Some((prev_xmin, _)) if xmin < *prev_xmin => {}
                _ => {
                    best.insert((attrelid, attnum), (xmin, col));
                }
            }
        }
    }

    let mut map: HashMap<u32, Vec<PgColumn>> = HashMap::new();
    for ((attrelid, _attnum), (_xmin, col)) in best {
        map.entry(attrelid).or_default().push(col);
    }
    for cols in map.values_mut() {
        cols.sort_by_key(|c| c.attnum);
    }
    map
}

fn pg_type_name(oid: u32) -> &'static str {
    match oid {
        16 => "bool",
        17 => "bytea",
        18 => "char",
        19 => "name",
        20 => "int8",
        21 => "int2",
        23 => "int4",
        25 => "text",
        26 => "oid",
        114 => "json",
        700 => "float4",
        701 => "float8",
        1042 => "bpchar",
        1043 => "varchar",
        1082 => "date",
        1114 => "timestamp",
        1184 => "timestamptz",
        1700 => "numeric",
        2950 => "uuid",
        3802 => "jsonb",
        _ => "varlena",
    }
}

fn count_heap_tuples(bytes: &[u8]) -> (usize, usize) {
    let mut live = 0usize;
    let mut dead = 0usize;
    for page in bytes.chunks_exact(PG_PAGE_SIZE) {
        for (_idx, _xmin, _xmax, is_deleted, _natts, _data, _nulls) in iter_page_tuples(page) {
            if is_deleted {
                dead += 1;
            } else {
                live += 1;
            }
        }
    }
    (live, dead)
}

type RawTupleSlice<'a> = (u16, u32, u32, bool, usize, &'a [u8], Option<&'a [u8]>);

fn iter_page_tuples(page: &[u8]) -> Vec<RawTupleSlice<'_>> {
    let mut out = Vec::new();
    if page.len() < PG_PAGE_SIZE {
        return out;
    }
    let pd_lower = u16::from_le_bytes([page[12], page[13]]) as usize;
    if !(24..=PG_PAGE_SIZE).contains(&pd_lower) {
        return out;
    }
    let num_items = (pd_lower - 24) / 4;
    for idx in 0..num_items {
        let lp_pos = 24 + idx * 4;
        let Some(lp_bytes) = page.get(lp_pos..lp_pos + 4) else {
            break;
        };
        let raw_lp = u32::from_le_bytes([lp_bytes[0], lp_bytes[1], lp_bytes[2], lp_bytes[3]]);
        let lp_off = (raw_lp & 0x7FFF) as usize;
        let lp_flags = (raw_lp >> 15) & 0x03;
        let lp_len = (raw_lp >> 17) as usize;
        if lp_flags == 0 || lp_flags == 2 || lp_len < 23 || lp_off + lp_len > PG_PAGE_SIZE {
            continue;
        }
        let tuple = &page[lp_off..lp_off + lp_len];
        let t_xmin = u32::from_le_bytes([tuple[0], tuple[1], tuple[2], tuple[3]]);
        let t_xmax = u32::from_le_bytes([tuple[4], tuple[5], tuple[6], tuple[7]]);
        let t_infomask2 = u16::from_le_bytes([tuple[18], tuple[19]]);
        let t_infomask = u16::from_le_bytes([tuple[20], tuple[21]]);
        let t_hoff = tuple[22] as usize;
        if t_hoff < 23 || t_hoff > lp_len {
            continue;
        }
        let natts = (t_infomask2 & 0x07FF) as usize;
        let has_null = (t_infomask & 0x0001) != 0;
        let xmax_invalid = (t_infomask & 0x0800) != 0;
        let xmin_invalid = (t_infomask & 0x0200) != 0;
        // PostgreSQL htup_details.h HEAP_XMAX_IS_LOCKED_ONLY(infomask):
        // HEAP_XMAX_LOCK_ONLY (0x0080) or (infomask & (HEAP_XMAX_IS_MULTI 0x1000 | HEAP_LOCK_MASK 0x00D0)) == HEAP_XMAX_EXCL_LOCK (0x0040)
        let xmax_lock_only =
            (t_infomask & 0x0080) != 0 || (t_infomask & (0x1000 | 0x00D0)) == 0x0040;
        let deleted =
            lp_flags == 3 || xmin_invalid || (t_xmax != 0 && !xmax_invalid && !xmax_lock_only);
        let null_bitmap = if has_null && t_hoff >= 23 + natts.div_ceil(8) {
            Some(&tuple[23..t_hoff])
        } else {
            None
        };
        out.push((
            (idx + 1) as u16,
            t_xmin,
            t_xmax,
            deleted,
            natts,
            &tuple[t_hoff..],
            null_bitmap,
        ));
    }
    out
}

pub fn parse_heap_pages(
    bytes: &[u8],
    schema_cols: &[PgColumn],
    toast_map: &HashMap<u32, Vec<u8>>,
) -> (Vec<PgTupleRow>, Vec<PgColumn>) {
    let mut rows = Vec::new();
    let mut max_inferred = 0usize;

    for (page_idx, page) in bytes.chunks_exact(PG_PAGE_SIZE).enumerate() {
        for (item_index, xmin, xmax, deleted, natts, data, null_bitmap) in iter_page_tuples(page) {
            let values = if schema_cols.is_empty() {
                let vals = decode_schemaless_tuple(data);
                max_inferred = max_inferred.max(vals.len());
                vals
            } else {
                decode_schema_tuple(data, schema_cols, natts, null_bitmap, toast_map)
            };
            let mut fernet_tokens = Vec::new();
            for (ci, val) in values.iter().enumerate() {
                if let Some(det) = detect_fernet_token(
                    val,
                    schema_cols
                        .get(ci)
                        .map(|c| c.name.as_str())
                        .unwrap_or("col"),
                ) {
                    fernet_tokens.push(det);
                }
            }
            rows.push(PgTupleRow {
                page: page_idx as u32,
                item_index,
                xmin,
                xmax,
                deleted,
                values,
                fernet_tokens,
            });
        }
    }

    let inferred_cols = (0..max_inferred)
        .map(|i| PgColumn {
            attnum: (i + 1) as i16,
            name: format!("col_{}", i + 1),
            type_oid: 25,
            type_name: "auto".to_string(),
            attlen: -1,
            attalign: b'i',
        })
        .collect();

    (rows, inferred_cols)
}

fn align_offset(off: usize, align: u8) -> usize {
    match align {
        b's' => (off + 1) & !1,
        b'i' => (off + 3) & !3,
        b'd' => (off + 7) & !7,
        _ => off,
    }
}

fn decode_schema_tuple(
    data: &[u8],
    cols: &[PgColumn],
    natts: usize,
    null_bitmap: Option<&[u8]>,
    toast_map: &HashMap<u32, Vec<u8>>,
) -> Vec<String> {
    let mut out = Vec::with_capacity(cols.len());
    let mut pos = 0usize;

    for (idx, col) in cols.iter().enumerate() {
        if idx >= natts {
            out.push("NULL".to_string());
            continue;
        }
        if let Some(bits) = null_bitmap {
            let byte_idx = idx / 8;
            let bit_mask = 1u8 << (idx % 8);
            if bits.get(byte_idx).is_some_and(|b| (b & bit_mask) == 0) {
                out.push("NULL".to_string());
                continue;
            }
        }
        if pos >= data.len() {
            out.push("NULL".to_string());
            continue;
        }
        if col.attlen > 0 {
            pos = align_offset(pos, col.attalign);
            let len = col.attlen as usize;
            let Some(slice) = data.get(pos..pos + len) else {
                out.push("NULL".to_string());
                break;
            };
            pos += len;
            out.push(format_fixed_attr(col.type_oid, slice));
        } else if col.attlen == -1 {
            // varlena: 1-byte short header requires NO alignment; 4-byte header requires align
            let first = data[pos];
            if (first & 0x01) == 0 {
                pos = align_offset(pos, col.attalign);
            }
            let Some((raw_bytes, consumed)) = read_varlena_with_toast(&data[pos..], toast_map)
            else {
                out.push("NULL".to_string());
                break;
            };
            pos += consumed;
            out.push(format_varlena_attr(col.type_oid, &raw_bytes));
        } else {
            // cstring (-2)
            let end = data[pos..]
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(data.len() - pos);
            let s = String::from_utf8_lossy(&data[pos..pos + end]).into_owned();
            pos += (end + 1).min(data.len() - pos);
            out.push(truncate_text(&s));
        }
    }

    out
}

fn format_fixed_attr(type_oid: u32, slice: &[u8]) -> String {
    match (type_oid, slice.len()) {
        (16, 1) => (slice[0] != 0).to_string(),
        (18, 1) => (slice[0] as char).to_string(),
        (19, 64) => parse_c_name64(slice),
        (21, 2) => i16::from_le_bytes([slice[0], slice[1]]).to_string(),
        (23, 4) => i32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]).to_string(),
        (26, 4) => u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]).to_string(),
        (20, 8) => i64::from_le_bytes(slice.try_into().unwrap_or([0; 8])).to_string(),
        (700, 4) => f32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]).to_string(),
        (701, 8) => f64::from_le_bytes(slice.try_into().unwrap_or([0; 8])).to_string(),
        (1082, 4) => {
            let days = i32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]) as i64;
            let unix = PG_EPOCH_UNIX_SECS + days * 86_400;
            chrono::DateTime::from_timestamp(unix, 0)
                .map(|dt| dt.format("%Y-%m-%d").to_string())
                .unwrap_or_else(|| days.to_string())
        }
        (1114 | 1184, 8) => {
            let us = i64::from_le_bytes(slice.try_into().unwrap_or([0; 8]));
            let unix_us = PG_EPOCH_UNIX_SECS * 1_000_000 + us;
            let secs = unix_us.div_euclid(1_000_000);
            let nanos = (unix_us.rem_euclid(1_000_000) as u32) * 1_000;
            use chrono::{FixedOffset, TimeZone};
            FixedOffset::east_opt(8 * 3600)
                .and_then(|tz| tz.timestamp_opt(secs, nanos).single())
                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC+08").to_string())
                .unwrap_or_else(|| us.to_string())
        }
        (2950, 16) => {
            let b: [u8; 16] = slice.try_into().unwrap_or([0; 16]);
            uuid::Uuid::from_bytes(b).to_string()
        }
        _ => format!("0x{}", hex::encode(slice)),
    }
}

fn format_varlena_attr(type_oid: u32, bytes: &[u8]) -> String {
    match type_oid {
        1700 => decode_pg_numeric(bytes).unwrap_or_else(|| format!("0x{}", hex::encode(bytes))),
        17 => format!("0x{}", hex::encode(&bytes[..bytes.len().min(64)])),
        3802 if !bytes.is_empty() => {
            // jsonb starts with version byte 0x01 or 0x01 header
            let text_start = bytes
                .iter()
                .position(|&b| matches!(b, b'{' | b'[' | b'"'))
                .unwrap_or(0);
            truncate_text(&String::from_utf8_lossy(&bytes[text_start..]))
        }
        _ => {
            if let Ok(s) = std::str::from_utf8(bytes) {
                truncate_text(s)
            } else {
                format!("0x{}", hex::encode(&bytes[..bytes.len().min(64)]))
            }
        }
    }
}

/// Decode PostgreSQL base-10000 `numeric` (`NumericShort` & `NumericLong`).
pub fn decode_pg_numeric(bytes: &[u8]) -> Option<String> {
    if bytes.len() < 2 {
        return None;
    }
    let header = u16::from_le_bytes([bytes[0], bytes[1]]);
    let is_short = (header & 0x8000) != 0;
    let is_special = (header & 0xC000) == 0xC000;
    if is_special {
        return Some("NaN".to_string());
    }

    let (weight, sign_neg, dscale, digits_slice) = if is_short {
        let sign_neg = (header & 0x2000) != 0;
        let dscale = ((header & 0x1F80) >> 7) as usize;
        let raw_w = (header & 0x007F) as i16;
        let weight = if (raw_w & 0x0040) != 0 {
            raw_w | !0x007F
        } else {
            raw_w
        };
        (weight, sign_neg, dscale, &bytes[2..])
    } else {
        if bytes.len() < 4 {
            return None;
        }
        let weight = header as i16;
        let flag = u16::from_le_bytes([bytes[2], bytes[3]]);
        let sign_neg = (flag & 0xC000) == 0x4000;
        let dscale = (flag & 0x3FFF) as usize;
        (weight, sign_neg, dscale, &bytes[4..])
    };

    let mut digits = Vec::with_capacity(digits_slice.len() / 2);
    for chunk in digits_slice.chunks_exact(2) {
        digits.push(i16::from_le_bytes([chunk[0], chunk[1]]));
    }
    if digits.is_empty() {
        return Some(if dscale > 0 {
            format!("0.{}", "0".repeat(dscale.min(18)))
        } else {
            "0".to_string()
        });
    }

    let mut out = String::new();
    if sign_neg {
        out.push('-');
    }
    // Integer part: indices 0..=weight
    if weight < 0 {
        out.push('0');
    } else {
        for d_idx in 0..=(weight as usize) {
            let dig = digits.get(d_idx).copied().unwrap_or(0).clamp(0, 9999);
            if d_idx == 0 {
                out.push_str(&dig.to_string());
            } else {
                out.push_str(&format!("{dig:04}"));
            }
        }
    }
    if dscale > 0 {
        out.push('.');
        let mut frac = String::new();
        if weight < -1 {
            for _ in 0..((-weight - 1) as usize) {
                frac.push_str("0000");
            }
        }
        let start_idx = (weight + 1).max(0) as usize;
        for &dig in digits.iter().skip(start_idx) {
            frac.push_str(&format!("{:04}", dig.clamp(0, 9999)));
        }
        while frac.len() < dscale {
            frac.push('0');
        }
        frac.truncate(dscale);
        out.push_str(&frac);
    }
    Some(out)
}

fn read_varlena(slice: &[u8]) -> Option<(Vec<u8>, usize)> {
    read_varlena_with_toast(slice, &HashMap::new())
}

fn read_varlena_with_toast(
    slice: &[u8],
    toast_map: &HashMap<u32, Vec<u8>>,
) -> Option<(Vec<u8>, usize)> {
    let b0 = *slice.first()?;
    if b0 == 0x01 {
        // 1-byte external varatt_external (18 bytes total: tag 1B + type 1B + 16B payload)
        let tag = *slice.get(1)?;
        if tag == 18 && slice.len() >= 18 {
            // va_rawsize (4B), va_extinfo (4B), va_valueid (4B), va_toastrelid (4B)
            let value_id = u32::from_le_bytes([slice[10], slice[11], slice[12], slice[13]]);
            let extinfo = u32::from_le_bytes([slice[6], slice[7], slice[8], slice[9]]);
            let is_compressed = (extinfo >> 30) != 0;
            if let Some(reassembled) = toast_map.get(&value_id) {
                let data = if is_compressed {
                    decompress_pglz(reassembled).unwrap_or_else(|| reassembled.clone())
                } else {
                    reassembled.clone()
                };
                return Some((data, 18));
            }
            return Some((format!("<TOAST value_id={value_id}>").into_bytes(), 18));
        }
    }
    if (b0 & 0x01) != 0 {
        // 1-byte short varlena (`va_header >> 1` total length including 1B header)
        let total = (b0 >> 1) as usize;
        if total == 0 || total > slice.len() {
            return None;
        }
        return Some((slice[1..total].to_vec(), total));
    }
    if slice.len() < 4 {
        return None;
    }
    let hdr = u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]);
    let total = (hdr >> 2) as usize;
    if total < 4 || total > slice.len() {
        return None;
    }
    if (b0 & 0x03) == 0x02 {
        // 4-byte compressed varlena (`va_tcinfo` at 4..8)
        if total >= 8 {
            if let Some(dec) = decompress_pglz(&slice[8..total]) {
                return Some((dec, total));
            }
        }
        return Some((slice[8.min(total)..total].to_vec(), total));
    }
    Some((slice[4..total].to_vec(), total))
}

fn decompress_pglz(src: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos < src.len() && out.len() < 4 * 1024 * 1024 {
        let ctrl = src[pos];
        pos += 1;
        for bit in 0..8 {
            if pos >= src.len() {
                break;
            }
            if (ctrl & (1 << bit)) != 0 {
                let b1 = *src.get(pos)?;
                let b2 = *src.get(pos + 1)?;
                pos += 2;
                let mut len = ((b1 & 0x0F) as usize) + 3;
                let off = (((b1 & 0xF0) as usize) << 4) | (b2 as usize);
                if len == 18 {
                    let extra = *src.get(pos)? as usize;
                    pos += 1;
                    len += extra;
                }
                if off == 0 || off > out.len() {
                    return None;
                }
                for _ in 0..len {
                    let b = out[out.len() - off];
                    out.push(b);
                }
            } else {
                out.push(src[pos]);
                pos += 1;
            }
        }
    }
    Some(out)
}

fn decode_schemaless_tuple(data: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos < data.len() && out.len() < 64 {
        let b0 = data[pos];
        if (b0 & 0x01) != 0 && (b0 >> 1) >= 2 && pos + ((b0 >> 1) as usize) <= data.len() {
            let total = (b0 >> 1) as usize;
            let body = &data[pos + 1..pos + total];
            if let Ok(s) = std::str::from_utf8(body) {
                if s.chars().all(|c| !c.is_control() || c.is_whitespace()) {
                    out.push(truncate_text(s));
                    pos += total;
                    continue;
                }
            }
            if let Some(num) = decode_pg_numeric(body) {
                out.push(num);
                pos += total;
                continue;
            }
        }
        if pos + 4 <= data.len() {
            let v = i32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
            out.push(v.to_string());
            pos += 4;
        } else {
            break;
        }
    }
    out
}

/// Detect a Fernet v1 token (`gAAAAA...`) inside `text` and extract its embedded
/// 64-bit big-endian creation timestamp in UTC+8.
pub fn detect_fernet_token(text: &str, column: &str) -> Option<FernetDetection> {
    let idx = text.find("gAAAAA")?;
    let candidate: String = text[idx..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_' || *c == '=')
        .collect();
    if candidate.len() < 76 {
        return None;
    }
    let raw = base64::engine::general_purpose::URL_SAFE
        .decode(&candidate)
        .or_else(|_| {
            base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(candidate.trim_end_matches('='))
        })
        .or_else(|_| base64::engine::general_purpose::STANDARD.decode(&candidate))
        .ok()?;
    if raw.len() < 57 || raw[0] != 0x80 || !(raw.len() - 57).is_multiple_of(16) {
        return None;
    }
    let ts_secs = i64::from_be_bytes(raw[1..9].try_into().ok()?);
    if !(946_684_800..=4_102_444_800).contains(&ts_secs) {
        return None;
    }
    use chrono::{FixedOffset, TimeZone};
    let tz = FixedOffset::east_opt(8 * 3600)?;
    let dt = tz.timestamp_opt(ts_secs, 0).single()?;
    Some(FernetDetection {
        column: column.to_string(),
        token_prefix: candidate.chars().take(32).collect(),
        timestamp_utc8: dt.format("%Y-%m-%d %H:%M:%S UTC+08").to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_synthetic_pg_heap_page_live_and_dead_tuples_numeric_and_fernet() {
        let mut page = vec![0u8; PG_PAGE_SIZE];
        // PageHeaderData: pd_lower = 32 (2 items), pd_upper = 7900, pd_special = 8192, pagesize_ver = 8192 | 4
        page[12..14].copy_from_slice(&32u16.to_le_bytes());
        page[14..16].copy_from_slice(&7900u16.to_le_bytes());
        page[16..18].copy_from_slice(&8192u16.to_le_bytes());
        page[18..20].copy_from_slice(&((8192u16) | 4).to_le_bytes());

        // Construct a valid Fernet token: version 0x80 + timestamp 1700000000 (BE) + 16B IV + 16B ciphertext + 32B HMAC
        let mut fernet_raw = vec![0x80u8];
        fernet_raw.extend_from_slice(&1_700_000_000i64.to_be_bytes());
        fernet_raw.extend_from_slice(&[0x11u8; 16]); // IV
        fernet_raw.extend_from_slice(&[0x22u8; 16]); // Ciphertext
        fernet_raw.extend_from_slice(&[0x33u8; 32]); // HMAC
        let fernet_str = base64::engine::general_purpose::URL_SAFE.encode(&fernet_raw);

        // Tuple 1 (Live, xmin=100, xmax=0): col0 int4=42, col1 short numeric 12.34, col2 text=fernet_str
        let mut tup1 = vec![0u8; 24];
        tup1[0..4].copy_from_slice(&100u32.to_le_bytes()); // xmin
        tup1[4..8].copy_from_slice(&0u32.to_le_bytes()); // xmax (live)
        tup1[18..20].copy_from_slice(&3u16.to_le_bytes()); // natts=3
        tup1[20..22].copy_from_slice(&0x0102u16.to_le_bytes()); // XMIN_COMMITTED | HASVARWIDTH
        tup1[22] = 24; // t_hoff
        tup1.extend_from_slice(&42i32.to_le_bytes());
        // Short numeric 12.34: header = 0x8000 | (dscale=2 << 7) | (weight=0) = 0x8100, digits=[12, 3400]
        let num_body = [0x00u8, 0x81, 12, 0, 0x48, 0x0D];
        tup1.push(((num_body.len() + 1) as u8) << 1 | 1);
        tup1.extend_from_slice(&num_body);
        // Short varlena text for fernet_str
        let fbytes = fernet_str.as_bytes();
        tup1.push(((fbytes.len() + 1) as u8) << 1 | 1);
        tup1.extend_from_slice(fbytes);

        let off1 = 8000usize;
        page[off1..off1 + tup1.len()].copy_from_slice(&tup1);
        let lp1 = (off1 as u32) | (1u32 << 15) | ((tup1.len() as u32) << 17);
        page[24..28].copy_from_slice(&lp1.to_le_bytes());

        // Tuple 2 (Dead/Deleted, xmin=101, xmax=105): col0 int4=99, col1 numeric 0, col2 text="deleted_row"
        let mut tup2 = vec![0u8; 24];
        tup2[0..4].copy_from_slice(&101u32.to_le_bytes());
        tup2[4..8].copy_from_slice(&105u32.to_le_bytes()); // xmax != 0 => deleted!
        tup2[18..20].copy_from_slice(&3u16.to_le_bytes());
        tup2[20..22].copy_from_slice(&0x0502u16.to_le_bytes());
        tup2[22] = 24;
        tup2.extend_from_slice(&99i32.to_le_bytes());
        tup2.push((3u8 << 1) | 1);
        tup2.extend_from_slice(&[0x00, 0x80]); // numeric 0
        let del_txt = b"deleted_row";
        tup2.push(((del_txt.len() + 1) as u8) << 1 | 1);
        tup2.extend_from_slice(del_txt);

        let off2 = 7900usize;
        page[off2..off2 + tup2.len()].copy_from_slice(&tup2);
        let lp2 = (off2 as u32) | (1u32 << 15) | ((tup2.len() as u32) << 17);
        page[28..32].copy_from_slice(&lp2.to_le_bytes());

        let cols = vec![
            PgColumn {
                attnum: 1,
                name: "id".into(),
                type_oid: 23,
                type_name: "int4".into(),
                attlen: 4,
                attalign: b'i',
            },
            PgColumn {
                attnum: 2,
                name: "amount".into(),
                type_oid: 1700,
                type_name: "numeric".into(),
                attlen: -1,
                attalign: b'i',
            },
            PgColumn {
                attnum: 3,
                name: "secret".into(),
                type_oid: 25,
                type_name: "text".into(),
                attlen: -1,
                attalign: b'i',
            },
        ];
        let (rows, _) = parse_heap_pages(&page, &cols, &HashMap::new());
        assert_eq!(rows.len(), 2);
        assert!(!rows[0].deleted);
        assert_eq!(rows[0].values[0], "42");
        assert_eq!(rows[0].values[1], "12.34");
        assert_eq!(rows[0].fernet_tokens.len(), 1);
        assert!(rows[0].fernet_tokens[0]
            .timestamp_utc8
            .contains("2023-11-15"));

        assert!(rows[1].deleted);
        assert_eq!(rows[1].xmax, 105);
        assert_eq!(rows[1].values[2], "deleted_row");
    }

    fn build_synthetic_pg_page(tuples: &[(&[u8], u32, u32, u16, u16)]) -> Vec<u8> {
        let mut page = vec![0u8; PG_PAGE_SIZE];
        let pd_lower = 24 + tuples.len() * 4;
        page[12..14].copy_from_slice(&(pd_lower as u16).to_le_bytes());
        page[16..18].copy_from_slice(&(PG_PAGE_SIZE as u16).to_le_bytes());
        page[18..20].copy_from_slice(&((PG_PAGE_SIZE as u16) | 4).to_le_bytes());

        let mut cursor = PG_PAGE_SIZE;
        for (idx, (payload, xmin, xmax, infomask2, infomask)) in tuples.iter().enumerate() {
            let len = 24 + payload.len();
            cursor -= (len + 7) & !7;
            let tup = &mut page[cursor..cursor + len];
            tup[0..4].copy_from_slice(&xmin.to_le_bytes());
            tup[4..8].copy_from_slice(&xmax.to_le_bytes());
            tup[18..20].copy_from_slice(&infomask2.to_le_bytes());
            tup[20..22].copy_from_slice(&infomask.to_le_bytes());
            tup[22] = 24;
            tup[24..24 + payload.len()].copy_from_slice(payload);

            let lp = (cursor as u32) | (1u32 << 15) | ((len as u32) << 17);
            let lp_off = 24 + idx * 4;
            page[lp_off..lp_off + 4].copy_from_slice(&lp.to_le_bytes());
        }
        page[14..16].copy_from_slice(&(cursor as u16).to_le_bytes());
        page
    }

    #[test]
    fn parses_pg18_catalog_class_and_attribute_layouts() {
        let mut class_old = vec![0u8; 120];
        class_old[0..4].copy_from_slice(&16395u32.to_le_bytes());
        class_old[4..10].copy_from_slice(b"orders");
        class_old[68..72].copy_from_slice(&2200u32.to_le_bytes());
        class_old[88..92].copy_from_slice(&16395u32.to_le_bytes());
        class_old[108..112].copy_from_slice(&(-1i32).to_le_bytes()); // PG18 relallfrozen
        class_old[112..116].copy_from_slice(&16398u32.to_le_bytes());
        class_old[118] = b'p';
        class_old[119] = b'r';

        let mut class_new = class_old.clone();
        class_new[88..92].copy_from_slice(&26395u32.to_le_bytes()); // updated relfilenode

        let class_page = build_synthetic_pg_page(&[
            (&class_old, 10, 0, 30, 0x0900),
            (&class_new, 20, 0, 30, 0x0900),
        ]);
        let classes = parse_pg_class(&class_page, Some(18));
        assert_eq!(classes.len(), 1);
        assert_eq!(classes[0].oid, 16395);
        assert_eq!(classes[0].relname, "orders");
        assert_eq!(classes[0].reltoastrelid, 16398);
        assert_eq!(classes[0].relfilenode, 26395);
        assert_eq!(classes[0].relkind, b'r');

        // Also verify PG18 auto-detection when PG_VERSION is unavailable.
        assert_eq!(parse_pg_class(&class_page, None).len(), 1);

        let mut attr_col1_old = vec![0u8; 92];
        attr_col1_old[0..4].copy_from_slice(&16395u32.to_le_bytes());
        attr_col1_old[4..6].copy_from_slice(b"id");
        attr_col1_old[68..72].copy_from_slice(&23u32.to_le_bytes());
        attr_col1_old[72..74].copy_from_slice(&4i16.to_le_bytes());
        attr_col1_old[74..76].copy_from_slice(&1i16.to_le_bytes());
        attr_col1_old[83] = b'i';
        attr_col1_old[84] = b'p';

        let mut attr_col1_new = attr_col1_old.clone();
        attr_col1_new[68..72].copy_from_slice(&20u32.to_le_bytes()); // int8
        attr_col1_new[72..74].copy_from_slice(&8i16.to_le_bytes());
        attr_col1_new[83] = b'd';

        let mut attr_dropped = vec![0u8; 92];
        attr_dropped[0..4].copy_from_slice(&16395u32.to_le_bytes());
        attr_dropped[4..12].copy_from_slice(b"old_note");
        attr_dropped[68..72].copy_from_slice(&25u32.to_le_bytes());
        attr_dropped[72..74].copy_from_slice(&(-1i16).to_le_bytes());
        attr_dropped[74..76].copy_from_slice(&2i16.to_le_bytes());
        attr_dropped[83] = b'i';
        attr_dropped[84] = b'x';
        attr_dropped[91] = 1; // attisdropped

        let attr_page = build_synthetic_pg_page(&[
            (&attr_col1_old, 10, 0, 26, 0x0900),
            (&attr_col1_new, 25, 0, 26, 0x0900),
            (&attr_dropped, 25, 0, 26, 0x0900),
        ]);
        let attrs = parse_pg_attribute(&attr_page, Some(18));
        let cols = &attrs[&16395];
        assert_eq!(cols.len(), 1);
        assert_eq!(cols[0].attnum, 1);
        assert_eq!(cols[0].name, "id");
        assert_eq!(cols[0].type_oid, 20);
        assert_eq!(cols[0].attalign, b'd');
        assert_eq!(parse_pg_attribute(&attr_page, None)[&16395].len(), 1);
    }

    #[test]
    fn treats_xmax_lock_only_tuples_as_live_and_handles_alter_table_natts() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&7i32.to_le_bytes());
        let txt = b"pending";
        payload.push(((txt.len() + 1) as u8) << 1 | 1);
        payload.extend_from_slice(txt);

        // infomask = 0x0192 (XMIN_COMMITTED | XMAX_LOCK_ONLY | XMAX_KEYSHR_LOCK | HASVARWIDTH)
        // xmin == xmax == 500, natts = 2 against a 3-column schema (simulating ALTER TABLE ADD COLUMN)
        let page = build_synthetic_pg_page(&[(&payload, 500, 500, 2, 0x0192)]);

        let cols = vec![
            PgColumn {
                attnum: 1,
                name: "id".into(),
                type_oid: 23,
                type_name: "int4".into(),
                attlen: 4,
                attalign: b'i',
            },
            PgColumn {
                attnum: 2,
                name: "status".into(),
                type_oid: 25,
                type_name: "text".into(),
                attlen: -1,
                attalign: b'i',
            },
            PgColumn {
                attnum: 3,
                name: "added_col".into(),
                type_oid: 25,
                type_name: "text".into(),
                attlen: -1,
                attalign: b'i',
            },
        ];
        let (rows, _) = parse_heap_pages(&page, &cols, &HashMap::new());
        assert_eq!(rows.len(), 1);
        assert!(!rows[0].deleted);
        assert_eq!(rows[0].values, vec!["7", "pending", "NULL"]);
    }
}
