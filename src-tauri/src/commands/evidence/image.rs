//! Image lifecycle (load / info / unload / list / raw export) and RAID
//! auto-detection. The exporter emits `image-export-progress` events so
//! the frontend renders a progress bar without polling.

use crate::core::common::ids::ImageId;
use crate::host::AppHandle;
use crate::host::State;

use crate::app::error::AppError;
use crate::core::evidence::image::ImageInfo;
use crate::core::evidence::image::ReadFaultPolicy;
use crate::core::evidence::image::ReadFaultReport;
use crate::core::evidence::physical_disk::PhysicalDisk;
use crate::core::evidence::raid;
use crate::core::evidence::raid::RaidStack;
use crate::core::filesystem::evidence_session::ImageExportMode;
use crate::state::AppState;

#[cfg_attr(feature = "desktop", tauri::command)]
#[allow(
    clippy::too_many_arguments,
    reason = "Flat IPC fields are shared by desktop and the generated CLI"
)]
/// Opens on a blocking worker; `task_id` opts into a caller-owned task identity.
/// Emits `image-load-progress` for the legacy loader and `task-progress` for
/// the task center. `task_cancel` interrupts materialisation and drops tempfiles.
pub async fn image_load(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    qcow_password: Option<String>,
    qcow_key_hex: Option<String>,
    task_id: Option<String>,
    case_dir: Option<String>,
    raw_read_mode: Option<crate::core::evidence::image::RawReadMode>,
) -> Result<ImageInfo, AppError> {
    let raw_read_mode = raw_read_mode.unwrap_or_default();
    if raw_read_mode == crate::core::evidence::image::RawReadMode::Force
        && (qcow_password.is_some() || qcow_key_hex.is_some())
    {
        return Err(AppError::invalid_input(
            "forced RAW mode cannot use QCOW credentials",
        ));
    }
    let password = qcow_password.map(zeroize::Zeroizing::new);
    let key_hex = qcow_key_hex.map(zeroize::Zeroizing::new);
    let key = match (password, key_hex) {
        (Some(_), Some(_)) => {
            return Err(AppError::invalid_input(
                "choose QCOW password or key, not both",
            ))
        }
        (Some(password), None) => Some(
            qcow2::EncryptionKey::from_password(&password)
                .map_err(|error| AppError::invalid_input(error.to_string()))?,
        ),
        (None, Some(key_hex)) => {
            let bytes = zeroize::Zeroizing::new(hex::decode(key_hex.as_str()).map_err(|_| {
                AppError::invalid_input("QCOW key must use hexadecimal characters")
            })?);
            let key: [u8; 16] = bytes
                .as_slice()
                .try_into()
                .map_err(|_| AppError::invalid_input("QCOW AES key must be exactly 16 bytes"))?;
            Some(qcow2::EncryptionKey::from_bytes(key))
        }
        (None, None) => None,
    };
    crate::app::workflows::evidence::image_load(
        app,
        state,
        path,
        key,
        task_id,
        case_dir,
        raw_read_mode,
    )
    .await
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn image_info(
    state: State<'_, AppState>,
    image_id: ImageId,
) -> Result<ImageInfo, AppError> {
    crate::app::workflows::evidence::image_info(state, image_id).await
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn image_unload(state: State<'_, AppState>, image_id: ImageId) -> Result<(), AppError> {
    crate::app::workflows::evidence::image_unload(state, image_id).await
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn image_list(state: State<'_, AppState>) -> Result<Vec<ImageInfo>, AppError> {
    crate::app::workflows::evidence::image_list(state).await
}

/// Detect LVM volume groups and Linux md arrays (metadata 0.90 / 1.x, linear
/// and RAID0/1/4/5/6/10) across every loaded image, plus metadata-less
/// RAID0/5 member sets, and register each assembled array — and each LV that
/// is complete and not consumed as an md member — as a synthetic `DiskImage` so it shows up
/// in the FS tree, hex view and image switcher like a regular image.
///
/// `degraded_sets` lists md set UUIDs the examiner chose to assemble without
/// the members their metadata still expects. Idempotent: re-running on the
/// same image set reuses the previously registered synthetic images.
/// `image_ids` restricts detection to loaded source images, isolating cloned
/// hosts with identical on-disk UUIDs; an empty selection is invalid.
/// Emits `raid-detect-progress` with `stage`, `current` and nullable `total`;
/// counts describe only that stage, never a percentage of the full scan.
#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn raid_auto_detect(
    app: AppHandle,
    state: State<'_, AppState>,
    degraded_sets: Option<Vec<String>>,
    image_ids: Option<Vec<ImageId>>,
) -> Result<RaidStack, AppError> {
    crate::app::workflows::evidence::raid_auto_detect(
        app,
        state,
        degraded_sets.unwrap_or_default(),
        image_ids,
    )
    .await
}

/// Assemble a RAID device from an explicit, user-supplied geometry (the manual
/// "RAID builder" fallback for arrays the automatic heuristic cannot crack) and
/// register it as a synthetic image so the rest of the app can browse / export
/// it. Idempotent on the geometry: re-running with the same parameters reuses
/// the previously registered synthetic image.
#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn raid_manual_assemble(
    state: State<'_, AppState>,
    params: raid::ManualRaidParams,
) -> Result<ManualRaidResult, AppError> {
    crate::app::workflows::evidence::raid_manual_assemble(state, params).await
}

/// Stream an entire image (regular or assembled) to a raw `.dd` file at
/// `destination`. Emits `image-export-progress` events with a
/// `{ task_id, bytes_done, bytes_total }` payload at every chunk.
///
/// Cancellation: the caller passes a `task_id` it has just registered via
/// the existing `task_cancel` IPC. The exporter checks that token between
/// chunks and aborts cleanly with a partial file (the partial result is
/// not deleted — leaving artefact recovery to the user).
/// `mode: unlockedPartitions` snapshots all currently unlocked partitions and
/// writes their plaintext into the original disk layout. Omitted mode is source.
#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn image_export_raw(
    app: AppHandle,
    state: State<'_, AppState>,
    image_id: ImageId,
    destination: String,
    task_id: String,
    read_fault_policy: Option<ReadFaultPolicy>,
    mode: Option<ImageExportMode>,
) -> Result<ImageExportResult, AppError> {
    crate::app::workflows::evidence::image_export_raw(
        app,
        state,
        image_id,
        destination,
        task_id,
        read_fault_policy,
        mode,
    )
    .await
}

/// Return every source span known to have been synthesized as zero bytes.
/// When a case is active this also loads/merges the persisted audit, so the
/// report survives app restarts and evidence reloads.
#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn image_bad_regions(
    state: State<'_, AppState>,
    image_id: ImageId,
) -> Result<ReadFaultReport, AppError> {
    crate::app::workflows::evidence::image_bad_regions(state, image_id).await
}

/// Enumerate physical disks attached to the host. Returned as a best-effort
/// list — disks that fail to open (permission denied, removed mid-scan) are
/// silently skipped so the UI still shows the rest. The enumeration runs on
/// a blocking thread because Windows' IOCTL calls are synchronous.
#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn physical_disks_list() -> Result<Vec<PhysicalDisk>, AppError> {
    crate::app::workflows::evidence::physical_disks_list().await
}

/// Mount an explicit sector offset (optionally overriding sector size and filesystem kind)
/// on a loaded image as a new zero-copy derived image. Useful for X-Ways cleaned images
/// or damaged/shifted partition tables.
#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn image_mount_offset(
    state: State<'_, AppState>,
    image_id: ImageId,
    offset_sectors: u64,
    sector_size: Option<usize>,
    kind: Option<String>,
) -> Result<ImageInfo, AppError> {
    use crate::core::evidence::image::{DerivedImage, DiskImage};
    use crate::core::evidence::partitions::{
        probe_partition_at_offset, PartitionKind, SectorSource, TableScheme,
    };
    use crate::core::filesystem::evidence_session::EvidenceSession;
    use std::sync::Arc;

    struct ParentSource(Arc<DiskImage>);
    impl SectorSource for ParentSource {
        fn size(&self) -> u64 {
            self.0.len()
        }
        fn read(&self, offset: u64, len: usize) -> anyhow::Result<Vec<u8>> {
            self.0.read_range(offset, len)
        }
    }

    let source = crate::app::access::loaded_image_app_error(&state, &image_id).await?;
    let sec = sector_size.unwrap_or(source.sector_size);
    if !matches!(sec, 512 | 1024 | 2048 | 4096) {
        return Err(AppError::invalid_input(
            "sectorSize 必须为 512 / 1024 / 2048 / 4096",
        ));
    }
    let forced_kind = kind
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("auto"))
        .map(|s| match s.to_ascii_lowercase().as_str() {
            "ntfs" => Ok(PartitionKind::Ntfs),
            "ext4" | "ext3" | "ext2" => Ok(PartitionKind::Ext4),
            "apfs" | "apfscontainer" => Ok(PartitionKind::ApfsContainer),
            "fat32" => Ok(PartitionKind::Fat32),
            "exfat" => Ok(PartitionKind::ExFat),
            "btrfs" => Ok(PartitionKind::Btrfs),
            "xfs" => Ok(PartitionKind::Xfs),
            _ => Err(AppError::invalid_input(format!(
                "不支持强制指定的文件系统类型：{s}"
            ))),
        })
        .transpose()?;

    let parent_img = Arc::clone(source.disk_image());
    let parent_id = source.id.clone();
    let mounted = tokio::task::spawn_blocking(move || -> anyhow::Result<DiskImage> {
        let src = Arc::new(ParentSource(parent_img));
        let offset_bytes = offset_sectors
            .checked_mul(sec as u64)
            .ok_or_else(|| anyhow::anyhow!("sector offset overflow"))?;
        let part = probe_partition_at_offset(src.as_ref(), sec, offset_bytes, forced_kind)?;
        let mut disk = DiskImage::from_assembled(
            format!("offset:{parent_id}@{offset_sectors}"),
            src,
            vec![part],
            sec,
        )?;
        disk.path = format!("offset://{}@sector-{}", parent_id, offset_sectors).into();
        disk.table_scheme = Some(TableScheme::Volume);
        disk.derived = Some(DerivedImage::Decrypted { parent: parent_id });
        Ok(disk)
    })
    .await
    .map_err(|e| AppError::task_panicked("image_mount_offset", e))?
    .map_err(AppError::from)?;

    let info = mounted.info();
    state
        .images
        .lock()
        .await
        .insert(mounted.id.clone(), Arc::new(EvidenceSession::from(mounted)));
    Ok(info)
}

pub use crate::app::workflows::evidence::{
    ImageExportProgress, ImageExportResult, ImageLoadProgress, ManualPartitionInfo,
    ManualRaidResult,
};
