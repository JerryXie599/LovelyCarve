//! Native comparison between a Volume Shadow Copy snapshot and the live NTFS partition.
//!
//! Streams `$MFT` records sequentially in 4 MiB chunks from both volumes via `ntfs_core`,
//! reconstructs full directory paths in memory, and highlights files deleted since the
//! snapshot, modified files, and newly created files.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{anyhow, bail, ensure, Context};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::core::evidence::vss_image::{self, ShadowCopyOrigin};
use crate::core::filesystem::evidence_session::EvidenceSession;
use crate::core::filesystem::tree::fs_tree::open_ntfs_volume_for_key;
use crate::core::filesystem::volumes::ntfs::NtfsVolume;
use crate::core::preview::mft_preview::{ntfs_time_to_rfc3339_ns, parse_compact_entry};

const ROOT_RECORD: u64 = 5;
const STREAM_CHUNK_RECORDS: usize = 4096;
const MAX_DIFF_RESULTS: usize = 2000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShadowCopyDiffItem {
    /// `"deleted"` (present in shadow copy, missing/deleted in live), `"modified"`, or `"added"`.
    pub kind: String,
    pub path: String,
    pub is_dir: bool,
    pub shadow_record: Option<u64>,
    pub live_record: Option<u64>,
    pub shadow_size: Option<u64>,
    pub live_size: Option<u64>,
    pub shadow_modified: Option<String>,
    pub live_modified: Option<String>,
    pub shadow_node_id: Option<String>,
    pub live_node_id: Option<String>,
    pub is_forensic_artifact: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShadowCopyDiffReport {
    pub partition_id: u8,
    pub snapshot_index: usize,
    pub store_id: String,
    pub shadow_copy_id: Option<String>,
    pub snapshot_created_unix_ms: Option<i64>,
    pub shadow_record_count: usize,
    pub live_record_count: usize,
    pub deleted_count: usize,
    pub modified_count: usize,
    pub added_count: usize,
    pub truncated: bool,
    pub items: Vec<ShadowCopyDiffItem>,
}

struct CompactSnapshotEntry {
    record_number: u64,
    parent_record: u64,
    name: Box<str>,
    is_dir: bool,
    size: u64,
    modified_ft: u64,
}

struct SnapshotTreeFile {
    record_number: u64,
    path: String,
    is_dir: bool,
    size: u64,
    modified_ft: u64,
}

/// Compares NTFS snapshot `index` on `partition_id` against the current live volume.
pub fn diff_shadow_copy(
    source: &Arc<EvidenceSession>,
    partition_id: u8,
    index: usize,
    query: Option<&str>,
    kind_filter: Option<&str>,
    limit: usize,
    cancel: &CancellationToken,
) -> anyhow::Result<ShadowCopyDiffReport> {
    let live_key = format!("part:{partition_id}");
    let live_vol = open_ntfs_volume_for_key(source, &live_key)
        .ok_or_else(|| anyhow!("分区 {partition_id} 不是可浏览的 NTFS 卷"))??;

    let shadow_disk = vss_image::open(source, partition_id, index, cancel)?;
    let origin: Option<ShadowCopyOrigin> = match &shadow_disk.derived {
        Some(crate::core::evidence::image::DerivedImage::ShadowCopy { origin, .. }) => {
            Some(origin.clone())
        }
        _ => None,
    };
    let origin = origin.context("卷影副本来源信息缺失")?;
    let shadow_part = shadow_disk
        .partitions
        .first()
        .context("卷影副本中未发现 NTFS 分区")?;
    let shadow_vol = NtfsVolume::open(&shadow_disk, shadow_part)?;

    let live_map = collect_ntfs_snapshot_map(&live_vol, cancel)?;
    let shadow_map = collect_ntfs_snapshot_map(&shadow_vol, cancel)?;

    let live_count = live_map.len();
    let shadow_count = shadow_map.len();

    let mut deleted_count = 0usize;
    let mut modified_count = 0usize;
    let mut added_count = 0usize;

    let needle = query.unwrap_or("").trim().to_ascii_lowercase();
    let want_kind = kind_filter.unwrap_or("all").trim();
    let cap = limit.clamp(1, MAX_DIFF_RESULTS);

    let mut candidates: Vec<ShadowCopyDiffItem> = Vec::new();

    for (lower_path, s_entry) in &shadow_map {
        if let Some(l_entry) = live_map.get(lower_path) {
            if !s_entry.is_dir
                && !l_entry.is_dir
                && (s_entry.size != l_entry.size || s_entry.modified_ft != l_entry.modified_ft)
            {
                modified_count += 1;
                if matches!(want_kind, "all" | "modified")
                    && (needle.is_empty() || lower_path.contains(&needle))
                {
                    candidates.push(ShadowCopyDiffItem {
                        kind: "modified".to_string(),
                        path: l_entry.path.clone(),
                        is_dir: false,
                        shadow_record: Some(s_entry.record_number),
                        live_record: Some(l_entry.record_number),
                        shadow_size: Some(s_entry.size),
                        live_size: Some(l_entry.size),
                        shadow_modified: ntfs_time_to_rfc3339_ns(s_entry.modified_ft),
                        live_modified: ntfs_time_to_rfc3339_ns(l_entry.modified_ft),
                        shadow_node_id: Some(format!("ntfs:part:0:{}", s_entry.record_number)),
                        live_node_id: Some(format!(
                            "ntfs:part:{partition_id}:{}",
                            l_entry.record_number
                        )),
                        is_forensic_artifact: is_forensic_path(lower_path),
                    });
                }
            }
        } else {
            deleted_count += 1;
            if matches!(want_kind, "all" | "deleted")
                && (needle.is_empty() || lower_path.contains(&needle))
            {
                candidates.push(ShadowCopyDiffItem {
                    kind: "deleted".to_string(),
                    path: s_entry.path.clone(),
                    is_dir: s_entry.is_dir,
                    shadow_record: Some(s_entry.record_number),
                    live_record: None,
                    shadow_size: Some(s_entry.size),
                    live_size: None,
                    shadow_modified: ntfs_time_to_rfc3339_ns(s_entry.modified_ft),
                    live_modified: None,
                    shadow_node_id: Some(format!("ntfs:part:0:{}", s_entry.record_number)),
                    live_node_id: None,
                    is_forensic_artifact: is_forensic_path(lower_path),
                });
            }
        }
    }

    for (lower_path, l_entry) in &live_map {
        if !shadow_map.contains_key(lower_path) {
            added_count += 1;
            if matches!(want_kind, "all" | "added")
                && (needle.is_empty() || lower_path.contains(&needle))
            {
                candidates.push(ShadowCopyDiffItem {
                    kind: "added".to_string(),
                    path: l_entry.path.clone(),
                    is_dir: l_entry.is_dir,
                    shadow_record: None,
                    live_record: Some(l_entry.record_number),
                    shadow_size: None,
                    live_size: Some(l_entry.size),
                    shadow_modified: None,
                    live_modified: ntfs_time_to_rfc3339_ns(l_entry.modified_ft),
                    shadow_node_id: None,
                    live_node_id: Some(format!(
                        "ntfs:part:{partition_id}:{}",
                        l_entry.record_number
                    )),
                    is_forensic_artifact: is_forensic_path(lower_path),
                });
            }
        }
    }

    // Prioritize deleted files first, then high-signal forensic artifact paths, then alphabetical.
    candidates.sort_by(|a, b| {
        diff_kind_rank(&a.kind)
            .cmp(&diff_kind_rank(&b.kind))
            .then_with(|| b.is_forensic_artifact.cmp(&a.is_forensic_artifact))
            .then_with(|| a.is_dir.cmp(&b.is_dir))
            .then_with(|| a.path.cmp(&b.path))
    });

    let truncated = candidates.len() > cap;
    candidates.truncate(cap);

    Ok(ShadowCopyDiffReport {
        partition_id,
        snapshot_index: index,
        store_id: origin.store_id,
        shadow_copy_id: origin.shadow_copy_id,
        snapshot_created_unix_ms: origin.created_unix_ms,
        shadow_record_count: shadow_count,
        live_record_count: live_count,
        deleted_count,
        modified_count,
        added_count,
        truncated,
        items: candidates,
    })
}

fn diff_kind_rank(kind: &str) -> u8 {
    match kind {
        "deleted" => 0,
        "modified" => 1,
        _ => 2,
    }
}

fn is_forensic_path(lower_path: &str) -> bool {
    lower_path.contains("\\users\\")
        || lower_path.contains("\\windows\\system32\\config\\")
        || lower_path.contains("\\windows\\system32\\winevt\\")
        || lower_path.contains("\\windows\\prefetch\\")
        || lower_path.contains("\\windows\\tasks\\")
        || lower_path.contains("\\programdata\\")
        || lower_path.contains("\\$recycle.bin\\")
}

fn collect_ntfs_snapshot_map(
    volume: &NtfsVolume<'_>,
    cancel: &CancellationToken,
) -> anyhow::Result<HashMap<String, SnapshotTreeFile>> {
    let meta = volume.volume_metadata()?;
    let rec_sz = meta.record_size.max(512) as usize;
    ensure!(rec_sz.is_power_of_two(), "invalid NTFS record size");
    let total_bytes = meta.mft_records.saturating_mul(rec_sz as u64);

    let mut dirs: HashMap<u64, (u64, String)> = HashMap::new();
    let mut entries: Vec<CompactSnapshotEntry> = Vec::with_capacity(meta.mft_records as usize / 2);

    let chunk_bytes = STREAM_CHUNK_RECORDS * rec_sz;
    let mut offset = 0u64;
    let mut rec_no = 0u64;
    while offset < total_bytes {
        if cancel.is_cancelled() {
            bail!("卷影副本对比已取消");
        }
        let want = ((total_bytes - offset) as usize).min(chunk_bytes);
        let chunk = volume.read_range(0, offset, want)?;
        if chunk.is_empty() {
            break;
        }
        for slot in chunk.chunks_exact(rec_sz) {
            if let Some(parsed) = parse_compact_entry(slot, rec_no) {
                if parsed.in_use && !parsed.name.is_empty() {
                    let mod_ft = if parsed.has_si && parsed.si_ft[1] > 0 {
                        parsed.si_ft[1]
                    } else {
                        parsed.fn_ft[1]
                    };
                    if parsed.is_dir {
                        dirs.insert(
                            parsed.record_number,
                            (parsed.parent_record, parsed.name.to_string()),
                        );
                    }
                    entries.push(CompactSnapshotEntry {
                        record_number: parsed.record_number,
                        parent_record: parsed.parent_record,
                        name: parsed.name,
                        is_dir: parsed.is_dir,
                        size: parsed.logical_size,
                        modified_ft: mod_ft,
                    });
                }
            }
            rec_no += 1;
        }
        offset += chunk.len() as u64;
    }

    let mut map = HashMap::with_capacity(entries.len());
    for entry in entries {
        if entry.record_number < 16 || (&*entry.name == "." && entry.parent_record == ROOT_RECORD) {
            continue;
        }
        let path = build_path(&dirs, entry.parent_record, &entry.name);
        let key = path.to_ascii_lowercase();
        map.insert(
            key,
            SnapshotTreeFile {
                record_number: entry.record_number,
                path,
                is_dir: entry.is_dir,
                size: entry.size,
                modified_ft: entry.modified_ft,
            },
        );
    }

    Ok(map)
}

fn build_path(dirs: &HashMap<u64, (u64, String)>, parent_record: u64, leaf: &str) -> String {
    let mut parts = vec![leaf.to_string()];
    let mut cur = parent_record;
    for _ in 0..32 {
        if cur == ROOT_RECORD {
            break;
        }
        let Some((next_parent, name)) = dirs.get(&cur) else {
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
