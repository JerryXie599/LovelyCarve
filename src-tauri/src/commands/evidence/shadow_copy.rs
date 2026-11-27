//! List an image's Volume Shadow Copies, open one as a session/case image, or diff against live.

use crate::core::common::ids::{ImageId, NodeId};
use crate::host::{AppHandle, State};
use std::path::Path;
use std::sync::Arc;

use crate::core::evidence::image::{DerivedImage, ImageInfo};
use crate::core::evidence::vss_diff::{self, ShadowCopyDiffReport};
use crate::core::evidence::vss_image::{self, ShadowVolumeInfo};
use crate::core::filesystem::evidence_session::EvidenceSession;
use crate::core::filesystem::tree::fs_tree::{self, FsExportResult};
use crate::state::AppState;
use crate::task_manager::TaskKind;
use {
    crate::app::access::loaded_image_app_error, crate::app::access::reject_if_folder,
    crate::app::error::AppError,
};

/// Shadow copies of every NTFS volume of the image, oldest first, with any
/// session image already open for each. An unlocked encrypted partition is
/// read through its plaintext view.
#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn shadow_copy_list(
    state: State<'_, AppState>,
    image_id: ImageId,
) -> Result<Vec<ShadowVolumeInfo>, AppError> {
    let image = loaded_image_app_error(&state, &image_id).await?;
    reject_if_folder(&image)?;
    let mut volumes = tokio::task::spawn_blocking(move || {
        let mut volumes = vss_image::list(image.disk_image());
        for view in image.decrypted_views() {
            volumes.extend(vss_image::list(view.disk_image()));
        }
        volumes.sort_by_key(|volume| volume.partition_id);
        volumes
    })
    .await
    .map_err(|error| AppError::task_panicked("shadow copy listing", error))?;
    let images = state.images.lock().await;
    for volume in &mut volumes {
        for copy in &mut volume.shadow_copies {
            let prefix =
                vss_image::session_path_prefix(&image_id, volume.partition_id, &copy.store_id);
            copy.open_image_id = images
                .values()
                .find(|image| image.path.to_string_lossy().starts_with(&prefix))
                .map(|image| image.id.to_string());
        }
    }
    Ok(volumes)
}

/// Reconstruct shadow copy `index` of the NTFS volume in `partition_id` and
/// register it as a read-only session/case image. Reuses an already open session
/// for the same shadow copy.
#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn shadow_copy_open(
    app: AppHandle,
    state: State<'_, AppState>,
    image_id: ImageId,
    partition_id: u8,
    index: usize,
    task_id: Option<String>,
) -> Result<ImageInfo, AppError> {
    let source = loaded_image_app_error(&state, &image_id).await?;
    reject_if_folder(&source)?;
    let task_id = task_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let task = state
        .tasks
        .register_if_absent(task_id, TaskKind::ShadowCopyOpen, Some(image_id.clone()))
        .ok_or_else(|| AppError::invalid_input("task id is already in use"))?;
    let permit = state.tasks.acquire_resources(&task).await?;
    let token = task.token();
    let source_for_open = Arc::clone(&source);
    let token_for_open = token.clone();
    let result = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let _task = task;
        let volume = source_for_open.filesystem_session(&format!("part:{partition_id}"));
        vss_image::open(volume.disk_image(), partition_id, index, &token_for_open)
    })
    .await
    .map_err(|error| AppError::task_panicked("shadow copy open", error))?;
    if token.is_cancelled() {
        return Err(AppError::cancelled("shadow copy open cancelled"));
    }
    let snapshot = result.map_err(AppError::from)?;
    crate::app::workflows::evidence::publish_shadow_copy(
        &app,
        &state,
        snapshot,
        Some((&image_id, &source)),
    )
    .await
}

/// Compare Volume Shadow Copy `index` on `partition_id` against the current live NTFS volume,
/// surfacing deleted files still preserved in the shadow copy, modified files, and newly added files.
#[cfg_attr(feature = "desktop", tauri::command)]
#[allow(clippy::too_many_arguments)]
pub async fn shadow_copy_diff(
    state: State<'_, AppState>,
    image_id: ImageId,
    partition_id: u8,
    index: usize,
    query: Option<String>,
    kind_filter: Option<String>,
    limit: Option<usize>,
    task_id: Option<String>,
) -> Result<ShadowCopyDiffReport, AppError> {
    let source = loaded_image_app_error(&state, &image_id).await?;
    reject_if_folder(&source)?;
    let task_id = task_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let task = state
        .tasks
        .register_if_absent(task_id, TaskKind::ShadowCopyOpen, Some(image_id))
        .ok_or_else(|| AppError::invalid_input("task id is already in use"))?;
    let permit = state.tasks.acquire_resources(&task).await?;
    let token = task.token();
    let source_for_diff = Arc::clone(&source);
    let token_for_diff = token.clone();
    let result = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let _task = task;
        vss_diff::diff_shadow_copy(
            &source_for_diff,
            partition_id,
            index,
            query.as_deref(),
            kind_filter.as_deref(),
            limit.unwrap_or(500),
            &token_for_diff,
        )
    })
    .await
    .map_err(|error| AppError::task_panicked("shadow copy diff", error))?;
    if token.is_cancelled() {
        return Err(AppError::cancelled("shadow copy diff cancelled"));
    }
    result.map_err(AppError::from)
}

const SHADOW_PREVIEW_CAP: usize = 1024 * 1024;

async fn resolve_shadow_session(
    state: &State<'_, AppState>,
    image_id: &str,
    partition_id: u8,
    index: usize,
) -> Result<Arc<EvidenceSession>, AppError> {
    {
        let images = state.images.lock().await;
        for session in images.values() {
            if let Some(DerivedImage::ShadowCopy { origin, .. }) = &session.derived {
                if origin.parent_image_id == image_id
                    && origin.partition_id == partition_id
                    && origin.index == index
                {
                    return Ok(Arc::clone(session));
                }
            }
        }
    }

    let cache_key = format!("{image_id}\u{1f}{partition_id}\u{1f}{index}");
    {
        let cached = state.vss_preview_sessions.lock().await;
        if let Some((k, session)) = cached.as_ref() {
            if k == &cache_key {
                return Ok(Arc::clone(session));
            }
        }
    }

    let source = loaded_image_app_error(state, image_id).await?;
    reject_if_folder(&source)?;
    let source_clone = Arc::clone(&source);
    let cancel = tokio_util::sync::CancellationToken::new();
    let disk = tokio::task::spawn_blocking(move || {
        vss_image::open(&source_clone, partition_id, index, &cancel)
    })
    .await
    .map_err(|error| AppError::task_panicked("shadow copy resolve", error))?
    .map_err(AppError::from)?;

    let session = Arc::new(EvidenceSession::from(disk));
    let mut cached = state.vss_preview_sessions.lock().await;
    *cached = Some((cache_key, Arc::clone(&session)));
    Ok(session)
}

fn resolve_shadow_node_by_path(
    session: &Arc<EvidenceSession>,
    raw_path: &str,
) -> anyhow::Result<String> {
    let norm = raw_path.trim().replace('\\', "/");
    let rel = norm
        .trim_start_matches('/')
        .strip_prefix("part:0/")
        .unwrap_or_else(|| norm.trim_start_matches('/'));
    if rel.is_empty() {
        return Ok("part:0".to_string());
    }
    let mut tree = fs_tree::FsSession::new(session);
    let mut current = "part:0".to_string();
    for segment in rel.split('/').filter(|s| !s.is_empty()) {
        let children = tree.children(&current)?;
        let found = children
            .into_iter()
            .find(|c| c.name.eq_ignore_ascii_case(segment))
            .ok_or_else(|| anyhow::anyhow!("卷影副本内未找到路径：{raw_path}"))?;
        current = found.id;
    }
    Ok(current)
}

/// Read bounded file bytes directly from a Volume Shadow Copy without opening a workspace tab.
#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn shadow_copy_file_preview(
    state: State<'_, AppState>,
    image_id: ImageId,
    partition_id: u8,
    index: usize,
    node_id: NodeId,
    max_bytes: Option<usize>,
) -> Result<Vec<u8>, AppError> {
    let shadow = resolve_shadow_session(&state, &image_id, partition_id, index).await?;
    let cap = max_bytes.unwrap_or(256 * 1024).clamp(1, SHADOW_PREVIEW_CAP);
    tokio::task::spawn_blocking(move || {
        let mut session = fs_tree::FsSession::new(&shadow);
        session
            .read_file_limited(&node_id, cap)
            .map_err(AppError::from)
    })
    .await
    .map_err(|error| AppError::task_panicked("shadow copy file preview", error))?
}

/// Export a single file directly from Volume Shadow Copy `index` by `nodeId` or in-volume `filePath`.
#[cfg_attr(feature = "desktop", tauri::command)]
#[allow(clippy::too_many_arguments)]
pub async fn shadow_copy_file_export(
    state: State<'_, AppState>,
    image_id: ImageId,
    partition_id: u8,
    index: usize,
    node_id: Option<NodeId>,
    file_path: Option<String>,
    destination: String,
) -> Result<FsExportResult, AppError> {
    let shadow = resolve_shadow_session(&state, &image_id, partition_id, index).await?;
    tokio::task::spawn_blocking(move || -> Result<FsExportResult, AppError> {
        let target_id = match node_id.filter(|s| !s.trim().is_empty()) {
            Some(nid) => nid.to_string(),
            None => {
                let fp = file_path
                    .filter(|s| !s.trim().is_empty())
                    .ok_or_else(|| AppError::invalid_input("需要提供 nodeId 或 filePath"))?;
                resolve_shadow_node_by_path(&shadow, &fp).map_err(AppError::from)?
            }
        };
        fs_tree::export_file_with_fault_report(&shadow, &target_id, Path::new(&destination))
            .map_err(AppError::from)
    })
    .await
    .map_err(|error| AppError::task_panicked("shadow copy file export", error))?
}
