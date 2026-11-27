//! Case lifecycle IPC.
//!
//! | 命令 | 语义 |
//! |---|---|
//! | `case_create` | 建案件目录与台账 `case.db`,加进程锁 |
//! | `case_open` | 打开案件(进程锁 + 自愈)并登记到本地案件列表 |
//! | `case_close` | 记审计后释放台账连接与进程锁,卸载运行态镜像 |
//! | `case_lock` / `case_unlock` | 翻转逻辑锁 |
//! | `case_remove` | 删除案件自己的文件并注销登记 |
//! | `case_list` | 本地登记簿 + 台账档案摘要 |
//! | `case_rename` | 改名并同步登记簿 |
//! | `case_option_query` / `case_option_save` | 台账 `setting` 表的案件级 KV |
//! | `case_link` | 案件或其中某处的 `lovelycarve://` 链接 |
//! | `case_evidence_platform_set` | 手动指定检材平台,优先于自动识别;`auto` 恢复自动结论 |
//!
//! Snapshot orchestration and the `snapshot_*` commands live in
//! `commands::casework::case_snapshot`.

use crate::core::common::ids::ImageId;
use crate::host::AppHandle;
use crate::host::State;

use crate::app::error::AppError;
use crate::core::casework::case::CaseInfo;
use crate::core::casework::case::EvidenceRef;
use crate::state::AppState;

#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn case_create(
    app: AppHandle,
    state: State<'_, AppState>,
    dir: String,
    name: String,
    case_number: Option<String>,
    description: Option<String>,
    examiner: Option<String>,
) -> Result<CaseInfo, AppError> {
    crate::app::workflows::casework::case_create(
        app,
        state,
        dir,
        name,
        case_number,
        description,
        examiner,
    )
    .await
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn case_open(
    app: AppHandle,
    state: State<'_, AppState>,
    dir: String,
) -> Result<CaseInfo, AppError> {
    crate::app::workflows::casework::case_open(app, state, dir).await
}

/// Exit the active case: journal the close, release the ledger connection and
/// the process lock, and unload the case's runtime images.
#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn case_close(state: State<'_, AppState>) -> Result<(), AppError> {
    crate::app::workflows::casework::case_close(state).await
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn case_active(state: State<'_, AppState>) -> Result<Option<CaseInfo>, AppError> {
    crate::app::workflows::casework::case_active(state).await
}

/// Locally registered cases, most recently opened first, each with its ledger
/// profile.
#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn case_list(app: AppHandle) -> Result<Vec<CaseListItem>, AppError> {
    crate::app::workflows::casework::case_list(app).await
}

/// Removes a case: closes it first when it is the active one, deletes only the
/// case's own files (ledger, lock, evidence and staging directories) and drops
/// the registry entry. A directory that no longer exists is only
/// unregistered; a directory without a case ledger is refused — arbitrary
/// directories are never deleted.
#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn case_remove(
    app: AppHandle,
    state: State<'_, AppState>,
    dir: String,
) -> Result<(), AppError> {
    crate::app::workflows::casework::case_remove(app, state, dir).await
}

/// Lock the active case (logical lock).
#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn case_lock(state: State<'_, AppState>) -> Result<CaseInfo, AppError> {
    crate::app::workflows::casework::case_lock(state).await
}

/// Unlock the active case.
#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn case_unlock(state: State<'_, AppState>) -> Result<CaseInfo, AppError> {
    crate::app::workflows::casework::case_unlock(state).await
}

/// Rename the active case and its registry entry. Locked cases refuse edits.
#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn case_rename(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
) -> Result<CaseInfo, AppError> {
    crate::app::workflows::casework::case_rename(app, state, name).await
}

/// Case-level settings; an empty `prefix` returns all of them.
#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn case_option_query(
    state: State<'_, AppState>,
    prefix: Option<String>,
) -> Result<Vec<CaseOption>, AppError> {
    crate::app::workflows::casework::case_option_query(state, prefix).await
}

/// Upsert case-level settings.
#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn case_option_save(
    state: State<'_, AppState>,
    entries: Vec<CaseOption>,
) -> Result<(), AppError> {
    crate::app::workflows::casework::case_option_save(state, entries).await
}

/// The `lovelycarve://` link of the active case, or of a place in one of its
/// evidence items: a node by ID with an optional byte range, or a path below
/// one of the evidence's root nodes. Opening it in the desktop app shows and
/// highlights the place.
#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn case_link(
    state: State<'_, AppState>,
    place: Option<LinkPlace>,
) -> Result<String, AppError> {
    crate::app::workflows::case_link::case_link(state, place).await
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn case_evidence_list(state: State<'_, AppState>) -> Result<Vec<EvidenceRef>, AppError> {
    crate::app::workflows::casework::case_evidence_list(state).await
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn case_evidence_remove(
    state: State<'_, AppState>,
    image_id: ImageId,
) -> Result<CaseInfo, AppError> {
    crate::app::workflows::casework::case_evidence_remove(state, image_id).await
}

/// Set the examiner's platform for case evidence; `auto` restores detection.
#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn case_evidence_platform_set(
    state: State<'_, AppState>,
    image_id: ImageId,
    platform: String,
) -> Result<EvidenceRef, AppError> {
    crate::app::workflows::casework::case_evidence_platform_set(state, image_id, platform).await
}

/// Aggregate Cross-Image IOCs across the active case's `records.db` stores and loaded images.
#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn case_iocs_list(
    state: State<'_, AppState>,
    kind: Option<String>,
    query: Option<String>,
) -> Result<crate::core::casework::case_intel::CaseIocsReport, AppError> {
    let (case_dir, evidence_ids) = {
        let guard = state.active_case.lock().await;
        if let Some(c) = guard.as_ref() {
            let info = c.info();
            let ev: Vec<(String, String)> = info
                .evidence
                .iter()
                .map(|e| (e.image_id.clone(), e.path.clone()))
                .collect();
            (Some(std::path::PathBuf::from(&info.dir)), ev)
        } else {
            (None, Vec::new())
        }
    };
    let loaded: Vec<(
        String,
        std::sync::Arc<crate::core::filesystem::evidence_session::EvidenceSession>,
    )> = {
        let guard = state.images.lock().await;
        guard
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    };
    tokio::task::spawn_blocking(move || {
        Ok(crate::core::casework::case_intel::build_case_iocs(
            case_dir.as_deref(),
            &evidence_ids,
            &loaded,
            kind.as_deref(),
            query.as_deref(),
        ))
    })
    .await
    .map_err(|err| AppError::task_panicked("case_iocs_list", err))?
}

/// Generate a case-specific password wordlist from user hints, accounts, SSIDs, and note files.
#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn case_wordlist_get(
    state: State<'_, AppState>,
    destination: Option<String>,
) -> Result<crate::core::casework::case_intel::CaseWordlistReport, AppError> {
    let (case_dir, evidence_ids) = {
        let guard = state.active_case.lock().await;
        if let Some(c) = guard.as_ref() {
            let info = c.info();
            let ev: Vec<(String, String)> = info
                .evidence
                .iter()
                .map(|e| (e.image_id.clone(), e.path.clone()))
                .collect();
            (Some(std::path::PathBuf::from(&info.dir)), ev)
        } else {
            (None, Vec::new())
        }
    };
    let loaded: Vec<(
        String,
        std::sync::Arc<crate::core::filesystem::evidence_session::EvidenceSession>,
    )> = {
        let guard = state.images.lock().await;
        guard
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    };
    tokio::task::spawn_blocking(move || {
        crate::core::casework::case_intel::generate_case_wordlist(
            case_dir.as_deref(),
            &evidence_ids,
            &loaded,
            destination.as_deref(),
        )
        .map_err(AppError::from)
    })
    .await
    .map_err(|err| AppError::task_panicked("case_wordlist_get", err))?
}

/// Run the built-in multi-core password & hash cracker (`ntlm`, `md5`, `sha1`, `sha256`, `fernet`, `metamask`, `zip`).
#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn cracker_start(
    request: crate::core::analysis::cracker::CrackRequest,
) -> Result<crate::core::analysis::cracker::CrackResult, AppError> {
    tokio::task::spawn_blocking(move || {
        crate::core::analysis::cracker::run_crack(&request).map_err(AppError::from)
    })
    .await
    .map_err(|err| AppError::task_panicked("cracker_start", err))?
}

pub use crate::app::workflows::case_link::LinkPlace;
pub use crate::app::workflows::casework::{CaseListItem, CaseOption};

const BOOKMARKS_SETTING_KEY: &str = "bookmarks.list";

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PathBookmark {
    pub id: String,
    pub image_id: String,
    pub root_id: String,
    pub path: String,
    #[serde(default)]
    pub node_id: Option<String>,
    pub kind: String,
    pub label: String,
    pub created_at: String,
}

fn normalize_bookmark_path_str(raw: &str) -> String {
    let trimmed = raw.replace('\\', "/");
    let trimmed = trimmed.trim().trim_matches('/');
    trimmed.to_string()
}

fn load_bookmarks_from_dir(case_dir: &std::path::Path) -> Result<Vec<PathBookmark>, AppError> {
    let ledger = crate::core::casework::case_ledger::open(
        &crate::core::casework::case_paths::ledger_path(case_dir),
    )
    .map_err(AppError::from)?;
    let rows = crate::core::casework::case_ledger::settings_query(&ledger, BOOKMARKS_SETTING_KEY)
        .map_err(AppError::from)?;
    let Some((_, raw_json)) = rows.into_iter().find(|(k, _)| k == BOOKMARKS_SETTING_KEY) else {
        return Ok(Vec::new());
    };
    if raw_json.trim().is_empty() {
        return Ok(Vec::new());
    }
    Ok(serde_json::from_str(&raw_json).unwrap_or_default())
}

fn save_bookmarks_to_dir(
    case_dir: &std::path::Path,
    items: &[PathBookmark],
) -> Result<(), AppError> {
    let ledger = crate::core::casework::case_ledger::open(
        &crate::core::casework::case_paths::ledger_path(case_dir),
    )
    .map_err(AppError::from)?;
    let json = serde_json::to_string(items)
        .map_err(|e| AppError::new(crate::app::error::ErrorCode::Internal, e.to_string()))?;
    crate::core::casework::case_ledger::settings_save(
        &ledger,
        &[(BOOKMARKS_SETTING_KEY.to_owned(), json)],
    )
    .map_err(AppError::from)
}

/// List saved path bookmarks persisted in the active case (optionally filtered by `image_id`).
#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn case_bookmarks_list(
    state: State<'_, AppState>,
    image_id: Option<crate::core::common::ids::ImageId>,
) -> Result<Vec<PathBookmark>, AppError> {
    let case_dir = {
        let guard = state.active_case.lock().await;
        let Some(c) = guard.as_ref() else {
            return Ok(Vec::new());
        };
        std::path::PathBuf::from(&c.info().dir)
    };
    let filter = image_id
        .map(|id| id.as_str().trim().to_string())
        .filter(|s| !s.is_empty());
    tokio::task::spawn_blocking(move || {
        let mut items = load_bookmarks_from_dir(&case_dir)?;
        if let Some(ref img) = filter {
            items.retain(|b| b.image_id == *img);
        }
        Ok(items)
    })
    .await
    .map_err(|err| AppError::task_panicked("case_bookmarks_list", err))?
}

/// Create or update a path bookmark in the unlocked active case.
#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn case_bookmarks_add(
    state: State<'_, AppState>,
    image_id: crate::core::common::ids::ImageId,
    root_id: Option<String>,
    path: String,
    node_id: Option<crate::core::common::ids::NodeId>,
    kind: Option<String>,
    label: Option<String>,
) -> Result<PathBookmark, AppError> {
    let img = image_id.as_str().trim().to_string();
    let root = root_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("root")
        .to_string();
    if img.is_empty() {
        return Err(AppError::invalid_input("image_id must not be empty"));
    }
    let norm_path = normalize_bookmark_path_str(&path);
    let kind_norm = match kind.as_deref().map(str::trim) {
        Some("file") => "file".to_string(),
        _ => "dir".to_string(),
    };
    let label_norm = label
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| {
            if norm_path.is_empty() {
                format!("/{root}")
            } else {
                norm_path
                    .rsplit('/')
                    .next()
                    .unwrap_or(&norm_path)
                    .to_string()
            }
        });
    let node_norm = node_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned);

    let case_dir = {
        let guard = state.active_case.lock().await;
        let Some(c) = guard.as_ref() else {
            return Err(AppError::invalid_input("no active case"));
        };
        c.ensure_unlocked(crate::core::casework::case::LockedEdit::EditBookmarks)?;
        std::path::PathBuf::from(&c.info().dir)
    };

    tokio::task::spawn_blocking(move || {
        let mut items = load_bookmarks_from_dir(&case_dir)?;
        let now = chrono::Utc::now().to_rfc3339();
        if let Some(existing) = items
            .iter_mut()
            .find(|b| b.image_id == img && b.root_id == root && b.path == norm_path)
        {
            existing.label = label_norm;
            existing.kind = kind_norm;
            if node_norm.is_some() {
                existing.node_id = node_norm;
            }
            let updated = existing.clone();
            save_bookmarks_to_dir(&case_dir, &items)?;
            return Ok(updated);
        }
        let created = PathBookmark {
            id: uuid::Uuid::new_v4().to_string(),
            image_id: img,
            root_id: root,
            path: norm_path,
            node_id: node_norm,
            kind: kind_norm,
            label: label_norm,
            created_at: now,
        };
        items.insert(0, created.clone());
        save_bookmarks_to_dir(&case_dir, &items)?;
        Ok(created)
    })
    .await
    .map_err(|err| AppError::task_panicked("case_bookmarks_add", err))?
}

/// Remove a saved path bookmark from the unlocked active case.
#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn case_bookmarks_remove(
    state: State<'_, AppState>,
    bookmark_id: String,
) -> Result<bool, AppError> {
    let bid = bookmark_id.trim().to_string();
    if bid.is_empty() {
        return Err(AppError::invalid_input("bookmark_id must not be empty"));
    }
    let case_dir = {
        let guard = state.active_case.lock().await;
        let Some(c) = guard.as_ref() else {
            return Err(AppError::invalid_input("no active case"));
        };
        c.ensure_unlocked(crate::core::casework::case::LockedEdit::EditBookmarks)?;
        std::path::PathBuf::from(&c.info().dir)
    };
    tokio::task::spawn_blocking(move || {
        let mut items = load_bookmarks_from_dir(&case_dir)?;
        let before = items.len();
        items.retain(|b| b.id != bid);
        let removed = items.len() != before;
        if removed {
            save_bookmarks_to_dir(&case_dir, &items)?;
        }
        Ok(removed)
    })
    .await
    .map_err(|err| AppError::task_panicked("case_bookmarks_remove", err))?
}
