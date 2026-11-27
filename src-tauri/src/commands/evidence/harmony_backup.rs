//! IPC adapter for HarmonyOS NEXT (`.info.json`) backup decryption.

use std::sync::Arc;

use crate::app::{access::loaded_image_app_error, error::AppError};
use crate::core::common::ids::ImageId;
use crate::core::evidence::image::ImageInfo;
use crate::core::filesystem::evidence_session::EvidenceSession;
use crate::host::State;
use crate::state::AppState;
use crate::task_manager::TaskKind;

#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn harmony_backup_unlock(
    app: crate::host::AppHandle,
    state: State<'_, AppState>,
    image_id: ImageId,
    password: String,
    task_id: Option<String>,
) -> Result<ImageInfo, AppError> {
    let source = loaded_image_app_error(&state, &image_id).await?;
    let task_id = task_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let task = state
        .tasks
        .register_if_absent(
            task_id,
            TaskKind::HarmonyBackupUnlock,
            Some(image_id.clone()),
        )
        .ok_or_else(|| AppError::invalid_input("task id is already in use"))?;
    let outcome = async {
        let permit = state.tasks.acquire_resources(&task).await?;
        let token = task.token();
        let source_for_unlock = Arc::clone(&source);
        let token_for_unlock = token.clone();
        let unlocked = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            source_for_unlock.unlock_harmony(password, &token_for_unlock)
        })
        .await
        .map_err(|error| AppError::task_panicked("harmony_backup_unlock", error))?;
        if token.is_cancelled() {
            return Err(AppError::cancelled("HarmonyOS backup unlock cancelled"));
        }
        let unlocked = unlocked.map_err(AppError::from)?;
        let info = unlocked.info();
        let session = Arc::new(EvidenceSession::from(unlocked));
        {
            let mut images = state.images.lock().await;
            if !images
                .get(&image_id)
                .is_some_and(|current| Arc::ptr_eq(current, &source))
            {
                return Err(AppError::source_changed(
                    "HarmonyOS backup unloaded during unlock",
                ));
            }
            images.insert(session.id.clone(), Arc::clone(&session));
        }
        if let Err(error) = crate::app::workflows::snapshot::case_snapshot::register_and_snapshot(
            &app,
            &state,
            Arc::clone(&session),
        )
        .await
        {
            state.images.lock().await.remove(&session.id);
            return Err(error);
        }
        Ok(info)
    }
    .await;
    task.finish_with(&outcome);
    outcome
}
