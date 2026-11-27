//! 竞赛题目、证据标注（Pin）、Markdown Writeup 导出与 Desktop↔CLI 同步命令。

use std::path::PathBuf;

use crate::app::error::AppError;
use crate::app::workflows::cli_sync::{self, CliSyncStatus};
use crate::core::casework::case::EvidenceRef;
use crate::core::casework::contest::{
    self, ContestExhibit, ContestQuestion, ContestWorkspace, EvidencePin, ExportResult,
    PinAddInput, PinAnnotation, QuestionNote, QuestionUpsertInput,
};
use crate::host::{AppHandle, State};
use crate::state::AppState;

async fn active_case_dir_and_evidence(
    state: &AppState,
) -> Result<(PathBuf, Vec<EvidenceRef>), AppError> {
    let guard = state.active_case.lock().await;
    let Some(case) = guard.as_ref() else {
        return Err(AppError::invalid_input("no active case"));
    };
    let info = case.info();
    Ok((case.dir.clone(), info.evidence))
}

fn emit_contest_updated(app: &AppHandle, source: &str) {
    let _ = app.emit(
        "contest-updated",
        serde_json::json!({
            "source": source,
            "at": contest::now_iso(),
        }),
    );
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn contest_get(state: State<'_, AppState>) -> Result<ContestWorkspace, AppError> {
    let (case_dir, evidence) = active_case_dir_and_evidence(&state).await?;
    let _guard = state.contest_lock.lock().await;
    tokio::task::spawn_blocking(move || -> anyhow::Result<ContestWorkspace> {
        let mut ws = contest::load_workspace(&case_dir)?;
        let before: Vec<Option<String>> = ws.exhibits.iter().map(|e| e.image_id.clone()).collect();
        contest::auto_bind_exhibits(&mut ws, &evidence);
        let after: Vec<Option<String>> = ws.exhibits.iter().map(|e| e.image_id.clone()).collect();
        if before != after && !ws.exhibits.is_empty() {
            contest::save_workspace(&case_dir, &mut ws)?;
        }
        Ok(ws)
    })
    .await
    .map_err(|err| AppError::task_panicked("contest_get", err))?
    .map_err(AppError::from)
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn contest_import(
    app: AppHandle,
    state: State<'_, AppState>,
    content: String,
    format: Option<String>,
    title: Option<String>,
    merge: Option<bool>,
) -> Result<ContestWorkspace, AppError> {
    let (case_dir, evidence) = active_case_dir_and_evidence(&state).await?;
    let _guard = state.contest_lock.lock().await;
    let fmt = format.unwrap_or_else(|| "auto".to_string());
    let merge = merge.unwrap_or(false);
    let ws = tokio::task::spawn_blocking(move || -> anyhow::Result<ContestWorkspace> {
        let existing = contest::load_workspace(&case_dir)?;
        let mut next = contest::import_content(&existing, &content, &fmt, title.as_deref(), merge)?;
        contest::auto_bind_exhibits(&mut next, &evidence);
        contest::save_workspace(&case_dir, &mut next)?;
        Ok(next)
    })
    .await
    .map_err(|err| AppError::task_panicked("contest_import", err))?
    .map_err(AppError::from)?;

    emit_contest_updated(&app, "desktop");
    Ok(ws)
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn contest_clear(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<ContestWorkspace, AppError> {
    let (case_dir, _) = active_case_dir_and_evidence(&state).await?;
    let _guard = state.contest_lock.lock().await;
    let ws = tokio::task::spawn_blocking(move || contest::clear_workspace(&case_dir))
        .await
        .map_err(|err| AppError::task_panicked("contest_clear", err))?
        .map_err(AppError::from)?;
    emit_contest_updated(&app, "desktop");
    Ok(ws)
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn contest_exhibit_bind(
    app: AppHandle,
    state: State<'_, AppState>,
    exhibit_id: String,
    image_id: Option<String>,
) -> Result<ContestExhibit, AppError> {
    let (case_dir, evidence) = active_case_dir_and_evidence(&state).await?;
    if let Some(ref img_id) = image_id {
        if !evidence.iter().any(|e| &e.image_id == img_id) {
            return Err(AppError::not_found(format!(
                "evidence {img_id} is not in the active case"
            )));
        }
    }
    let _guard = state.contest_lock.lock().await;
    let updated = tokio::task::spawn_blocking(move || -> anyhow::Result<ContestExhibit> {
        let mut ws = contest::load_workspace(&case_dir)?;
        let idx = contest::resolve_exhibit_index(&ws, &exhibit_id)
            .ok_or_else(|| anyhow::anyhow!("未找到检材: {exhibit_id}"))?;
        ws.exhibits[idx].image_id = image_id;
        let cloned = ws.exhibits[idx].clone();
        contest::save_workspace(&case_dir, &mut ws)?;
        Ok(cloned)
    })
    .await
    .map_err(|err| AppError::task_panicked("contest_exhibit_bind", err))?
    .map_err(AppError::from)?;

    emit_contest_updated(&app, "desktop");
    Ok(updated)
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn contest_question_upsert(
    app: AppHandle,
    state: State<'_, AppState>,
    input: QuestionUpsertInput,
) -> Result<ContestQuestion, AppError> {
    let (case_dir, _) = active_case_dir_and_evidence(&state).await?;
    let _guard = state.contest_lock.lock().await;
    let q = tokio::task::spawn_blocking(move || -> anyhow::Result<ContestQuestion> {
        let mut ws = contest::load_workspace(&case_dir)?;
        let q = contest::upsert_question(&mut ws, input)?;
        contest::save_workspace(&case_dir, &mut ws)?;
        Ok(q)
    })
    .await
    .map_err(|err| AppError::task_panicked("contest_question_upsert", err))?
    .map_err(AppError::from)?;

    emit_contest_updated(&app, "desktop");
    Ok(q)
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn contest_question_note_add(
    app: AppHandle,
    state: State<'_, AppState>,
    question_id: String,
    author: Option<String>,
    kind: Option<String>,
    command: Option<String>,
    content: String,
) -> Result<QuestionNote, AppError> {
    let (case_dir, _) = active_case_dir_and_evidence(&state).await?;
    let _guard = state.contest_lock.lock().await;
    let note = tokio::task::spawn_blocking(move || -> anyhow::Result<QuestionNote> {
        let mut ws = contest::load_workspace(&case_dir)?;
        let note = contest::add_question_note(
            &mut ws,
            &question_id,
            author.as_deref().unwrap_or("user"),
            kind.as_deref().unwrap_or("note"),
            command.filter(|c| !c.trim().is_empty()),
            content,
        )?;
        contest::save_workspace(&case_dir, &mut ws)?;
        Ok(note)
    })
    .await
    .map_err(|err| AppError::task_panicked("contest_question_note_add", err))?
    .map_err(AppError::from)?;

    emit_contest_updated(&app, "desktop");
    Ok(note)
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn contest_question_note_remove(
    app: AppHandle,
    state: State<'_, AppState>,
    question_id: String,
    note_id: String,
) -> Result<ContestWorkspace, AppError> {
    let (case_dir, _) = active_case_dir_and_evidence(&state).await?;
    let _guard = state.contest_lock.lock().await;
    let ws = tokio::task::spawn_blocking(move || -> anyhow::Result<ContestWorkspace> {
        let mut ws = contest::load_workspace(&case_dir)?;
        if let Some(idx) = contest::resolve_question_index(&ws, &question_id) {
            ws.questions[idx].notes.retain(|n| n.id != note_id);
            ws.questions[idx].updated_at = contest::now_iso();
            contest::save_workspace(&case_dir, &mut ws)?;
        }
        Ok(ws)
    })
    .await
    .map_err(|err| AppError::task_panicked("contest_question_note_remove", err))?
    .map_err(AppError::from)?;

    emit_contest_updated(&app, "desktop");
    Ok(ws)
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn contest_pin_add(
    app: AppHandle,
    state: State<'_, AppState>,
    input: PinAddInput,
) -> Result<EvidencePin, AppError> {
    let (case_dir, _) = active_case_dir_and_evidence(&state).await?;
    let _guard = state.contest_lock.lock().await;
    let pin = tokio::task::spawn_blocking(move || -> anyhow::Result<EvidencePin> {
        let mut ws = contest::load_workspace(&case_dir)?;
        let pin = contest::add_pin(&mut ws, input)?;
        contest::save_workspace(&case_dir, &mut ws)?;
        Ok(pin)
    })
    .await
    .map_err(|err| AppError::task_panicked("contest_pin_add", err))?
    .map_err(AppError::from)?;

    emit_contest_updated(&app, "desktop");
    Ok(pin)
}

#[allow(clippy::too_many_arguments)]
#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn contest_pin_update(
    app: AppHandle,
    state: State<'_, AppState>,
    pin_id: String,
    title: Option<String>,
    summary: Option<String>,
    question_id: Option<String>,
    exhibit_id: Option<String>,
    command: Option<String>,
    annotation: Option<PinAnnotation>,
) -> Result<EvidencePin, AppError> {
    let (case_dir, _) = active_case_dir_and_evidence(&state).await?;
    let _guard = state.contest_lock.lock().await;
    let pin = tokio::task::spawn_blocking(move || -> anyhow::Result<EvidencePin> {
        let mut ws = contest::load_workspace(&case_dir)?;
        let idx = ws
            .pins
            .iter()
            .position(|p| p.id == pin_id)
            .ok_or_else(|| anyhow::anyhow!("未找到证据 Pin: {pin_id}"))?;
        if let Some(t) = title {
            ws.pins[idx].title = t;
        }
        if let Some(s) = summary {
            ws.pins[idx].summary = s;
        }
        if let Some(q) = question_id {
            ws.pins[idx].question_id = if q.trim().is_empty() { None } else { Some(q) };
        }
        if let Some(e) = exhibit_id {
            ws.pins[idx].exhibit_id = if e.trim().is_empty() { None } else { Some(e) };
        }
        if let Some(c) = command {
            ws.pins[idx].command = if c.trim().is_empty() { None } else { Some(c) };
        }
        if annotation.is_some() {
            ws.pins[idx].annotation = annotation;
        }
        let cloned = ws.pins[idx].clone();
        contest::save_workspace(&case_dir, &mut ws)?;
        Ok(cloned)
    })
    .await
    .map_err(|err| AppError::task_panicked("contest_pin_update", err))?
    .map_err(AppError::from)?;

    emit_contest_updated(&app, "desktop");
    Ok(pin)
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn contest_pin_remove(
    app: AppHandle,
    state: State<'_, AppState>,
    pin_id: String,
) -> Result<ContestWorkspace, AppError> {
    let (case_dir, _) = active_case_dir_and_evidence(&state).await?;
    let _guard = state.contest_lock.lock().await;
    let ws = tokio::task::spawn_blocking(move || -> anyhow::Result<ContestWorkspace> {
        let mut ws = contest::load_workspace(&case_dir)?;
        ws.pins.retain(|p| p.id != pin_id);
        contest::save_workspace(&case_dir, &mut ws)?;
        Ok(ws)
    })
    .await
    .map_err(|err| AppError::task_panicked("contest_pin_remove", err))?
    .map_err(AppError::from)?;

    emit_contest_updated(&app, "desktop");
    Ok(ws)
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn contest_wp_update(
    app: AppHandle,
    state: State<'_, AppState>,
    case_wp_md: Option<String>,
    exhibit_id: Option<String>,
    exhibit_wp_md: Option<String>,
    question_id: Option<String>,
    question_wp_md: Option<String>,
) -> Result<ContestWorkspace, AppError> {
    let (case_dir, _) = active_case_dir_and_evidence(&state).await?;
    let _guard = state.contest_lock.lock().await;
    let ws = tokio::task::spawn_blocking(move || -> anyhow::Result<ContestWorkspace> {
        let mut ws = contest::load_workspace(&case_dir)?;
        if let Some(md) = case_wp_md {
            ws.case_wp_md = md;
        }
        if let (Some(ex_id), Some(md)) = (exhibit_id, exhibit_wp_md) {
            if let Some(idx) = contest::resolve_exhibit_index(&ws, &ex_id) {
                ws.exhibits[idx].wp_md = md;
            }
        }
        if let (Some(q_id), Some(md)) = (question_id, question_wp_md) {
            if let Some(idx) = contest::resolve_question_index(&ws, &q_id) {
                ws.questions[idx].wp_section_md = md;
                ws.questions[idx].updated_at = contest::now_iso();
            }
        }
        contest::save_workspace(&case_dir, &mut ws)?;
        Ok(ws)
    })
    .await
    .map_err(|err| AppError::task_panicked("contest_wp_update", err))?
    .map_err(AppError::from)?;

    emit_contest_updated(&app, "desktop");
    Ok(ws)
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn contest_asset_read(
    state: State<'_, AppState>,
    rel_path: String,
) -> Result<String, AppError> {
    let (case_dir, _) = active_case_dir_and_evidence(&state).await?;
    let bytes =
        tokio::task::spawn_blocking(move || contest::read_asset_bytes(&case_dir, &rel_path))
            .await
            .map_err(|err| AppError::task_panicked("contest_asset_read", err))?
            .map_err(AppError::from)?;
    Ok(format!("data:image/png;base64,{}", encode_base64(&bytes)))
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn contest_wp_preview(
    state: State<'_, AppState>,
    exhibit_id: Option<String>,
) -> Result<String, AppError> {
    let (case_dir, _) = active_case_dir_and_evidence(&state).await?;
    let _guard = state.contest_lock.lock().await;
    tokio::task::spawn_blocking(move || -> anyhow::Result<String> {
        let ws = contest::load_workspace(&case_dir)?;
        Ok(contest::render_writeup_markdown(&ws, exhibit_id.as_deref()))
    })
    .await
    .map_err(|err| AppError::task_panicked("contest_wp_preview", err))?
    .map_err(AppError::from)
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn contest_wp_export(
    state: State<'_, AppState>,
    output_md_path: String,
    exhibit_id: Option<String>,
) -> Result<ExportResult, AppError> {
    let (case_dir, _) = active_case_dir_and_evidence(&state).await?;
    let _guard = state.contest_lock.lock().await;
    let out_path = PathBuf::from(output_md_path);
    tokio::task::spawn_blocking(move || -> anyhow::Result<ExportResult> {
        let ws = contest::load_workspace(&case_dir)?;
        contest::export_writeup(&case_dir, &ws, &out_path, exhibit_id.as_deref())
    })
    .await
    .map_err(|err| AppError::task_panicked("contest_wp_export", err))?
    .map_err(AppError::from)
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn cli_sync_status(state: State<'_, AppState>) -> Result<CliSyncStatus, AppError> {
    Ok(cli_sync::status(&state).await)
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn cli_sync_set(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<CliSyncStatus, AppError> {
    cli_sync::set_enabled(&app, &state, enabled).await
}

fn encode_base64(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((triple >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(triple & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}
