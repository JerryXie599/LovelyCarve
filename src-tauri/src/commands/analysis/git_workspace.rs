//! Git / GitHub repository workspace IPC commands (read-only evidence view & case sandbox).

use crate::core::common::ids::ImageId;
use crate::host::State;

use crate::{
    app::{access::loaded_image_app_error, error::AppError, workflows::casework::active_case_dir},
    core::analysis::git_workspace::{
        self,
        types::{
            GitDiffResult, GitExportResult, GitFileContent, GitRepoOverview,
            GitSandboxActionResult, GitSandboxFsResult, GitTreeListing,
        },
    },
    state::AppState,
};

#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn git_workspace_overview(
    state: State<'_, AppState>,
    image_id: ImageId,
    repo_path: String,
    use_sandbox: Option<bool>,
) -> Result<GitRepoOverview, AppError> {
    let image = loaded_image_app_error(&state, &image_id).await?;
    let case_dir = active_case_dir(&state).await?;
    tokio::task::spawn_blocking(move || {
        git_workspace::repo_overview(
            &image,
            &case_dir,
            &image_id,
            &repo_path,
            use_sandbox.unwrap_or(false),
        )
    })
    .await
    .map_err(|error| AppError::task_panicked("git_workspace_overview", error))?
    .map_err(AppError::from)
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn git_workspace_tree(
    state: State<'_, AppState>,
    image_id: ImageId,
    repo_path: String,
    rev: Option<String>,
    dir_path: Option<String>,
    use_sandbox: Option<bool>,
) -> Result<GitTreeListing, AppError> {
    let image = loaded_image_app_error(&state, &image_id).await?;
    let case_dir = active_case_dir(&state).await?;
    tokio::task::spawn_blocking(move || {
        git_workspace::repo_tree(
            &image,
            &case_dir,
            &image_id,
            &repo_path,
            rev.as_deref().unwrap_or("WORKTREE"),
            dir_path.as_deref().unwrap_or(""),
            use_sandbox.unwrap_or(false),
        )
    })
    .await
    .map_err(|error| AppError::task_panicked("git_workspace_tree", error))?
    .map_err(AppError::from)
}

#[cfg_attr(feature = "desktop", tauri::command)]
#[allow(clippy::too_many_arguments)]
pub async fn git_workspace_file_read(
    state: State<'_, AppState>,
    image_id: ImageId,
    repo_path: String,
    rev: Option<String>,
    file_path: Option<String>,
    blob_sha: Option<String>,
    use_sandbox: Option<bool>,
) -> Result<GitFileContent, AppError> {
    let image = loaded_image_app_error(&state, &image_id).await?;
    let case_dir = active_case_dir(&state).await?;
    tokio::task::spawn_blocking(move || {
        git_workspace::repo_file_read(
            &image,
            &case_dir,
            &image_id,
            &repo_path,
            rev.as_deref().unwrap_or("WORKTREE"),
            file_path.as_deref().unwrap_or(""),
            blob_sha.as_deref(),
            use_sandbox.unwrap_or(false),
        )
    })
    .await
    .map_err(|error| AppError::task_panicked("git_workspace_file_read", error))?
    .map_err(AppError::from)
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn git_workspace_diff(
    state: State<'_, AppState>,
    image_id: ImageId,
    repo_path: String,
    target_rev: String,
    base_rev: Option<String>,
    use_sandbox: Option<bool>,
) -> Result<GitDiffResult, AppError> {
    let image = loaded_image_app_error(&state, &image_id).await?;
    let case_dir = active_case_dir(&state).await?;
    tokio::task::spawn_blocking(move || {
        git_workspace::repo_diff(
            &image,
            &case_dir,
            &image_id,
            &repo_path,
            &target_rev,
            base_rev.as_deref(),
            use_sandbox.unwrap_or(false),
        )
    })
    .await
    .map_err(|error| AppError::task_panicked("git_workspace_diff", error))?
    .map_err(AppError::from)
}

#[cfg_attr(feature = "desktop", tauri::command)]
#[allow(clippy::too_many_arguments)]
pub async fn git_workspace_export(
    state: State<'_, AppState>,
    image_id: ImageId,
    repo_path: String,
    rev: Option<String>,
    sub_path: Option<String>,
    destination: String,
    include_git_dir: Option<bool>,
    use_sandbox: Option<bool>,
) -> Result<GitExportResult, AppError> {
    let image = loaded_image_app_error(&state, &image_id).await?;
    let case_dir = active_case_dir(&state).await?;
    tokio::task::spawn_blocking(move || {
        git_workspace::repo_export(
            &image,
            &case_dir,
            &image_id,
            &repo_path,
            rev.as_deref().unwrap_or("WORKTREE"),
            sub_path.as_deref(),
            &destination,
            include_git_dir.unwrap_or(false),
            use_sandbox.unwrap_or(false),
        )
    })
    .await
    .map_err(|error| AppError::task_panicked("git_workspace_export", error))?
    .map_err(AppError::from)
}

#[cfg_attr(feature = "desktop", tauri::command)]
#[allow(clippy::too_many_arguments)]
pub async fn git_workspace_sandbox_git(
    state: State<'_, AppState>,
    image_id: ImageId,
    repo_path: String,
    action: String,
    target_ref: Option<String>,
    branch_name: Option<String>,
    commit_message: Option<String>,
    file_paths: Option<Vec<String>>,
    reset_from_evidence: Option<bool>,
) -> Result<GitSandboxActionResult, AppError> {
    let image = loaded_image_app_error(&state, &image_id).await?;
    let case_dir = active_case_dir(&state).await?;
    tokio::task::spawn_blocking(move || {
        git_workspace::sandbox_git_action(
            &image,
            &case_dir,
            &image_id,
            &repo_path,
            &action,
            target_ref.as_deref(),
            branch_name.as_deref(),
            commit_message.as_deref(),
            &file_paths.unwrap_or_default(),
            reset_from_evidence.unwrap_or(false),
        )
    })
    .await
    .map_err(|error| AppError::task_panicked("git_workspace_sandbox_git", error))?
    .map_err(AppError::from)
}

#[cfg_attr(feature = "desktop", tauri::command)]
#[allow(clippy::too_many_arguments)]
pub async fn git_workspace_sandbox_fs(
    state: State<'_, AppState>,
    image_id: ImageId,
    repo_path: String,
    action: String,
    target_path: String,
    new_path: Option<String>,
    content: Option<String>,
) -> Result<GitSandboxFsResult, AppError> {
    let image = loaded_image_app_error(&state, &image_id).await?;
    let case_dir = active_case_dir(&state).await?;
    tokio::task::spawn_blocking(move || {
        git_workspace::sandbox_fs_action(
            &image,
            &case_dir,
            &image_id,
            &repo_path,
            &action,
            &target_path,
            new_path.as_deref(),
            content.as_deref(),
        )
    })
    .await
    .map_err(|error| AppError::task_panicked("git_workspace_sandbox_fs", error))?
    .map_err(AppError::from)
}
