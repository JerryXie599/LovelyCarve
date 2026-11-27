//! Desktop ↔ CLI 实时同步桥接。
//!
//! 桌面端打开案件且开启 CLI 同步时，由桌面进程托管标准的 `cli-backends/case-<sha>.json`
//! 服务端入口，与桌面端共享同一个 `AppState` 与 `CaseLock`：
//! - 彻底消除桌面端与 CLI 的 `case.lock` 冲突；
//! - AI 或用户在命令行执行的 `contest` / `pin` / `wp` / `ls` / `sqlite` 等操作实时作用于桌面状态，
//!   并在变更时向前端推送 `cli-sync-event` 与 `contest-updated`；
//! - 用户可通过开关随时启用或停用同步。

use std::path::Path;
use std::sync::Arc;

use serde::Serialize;
use tokio::sync::watch;

use crate::app::error::AppError;
use crate::cli::backend::{prepare_with_mode, serve_with_options, ServerOptions};
use crate::cli::background::Background;
use crate::cli::exec::{Exec, Mode};
use crate::cli::progress::{Options as ProgressOptions, Reporter};
use crate::cli::workspace::Workspace;
use crate::core::casework::{case_ledger, case_paths};
use crate::host::AppHandle;
use crate::state::AppState;

pub const SETTING_KEY: &str = "cli.sync.enabled";

pub struct CliSyncRuntime {
    pub case_dir: String,
    pub port: u16,
    shutdown_tx: watch::Sender<bool>,
    join_handle: tokio::task::JoinHandle<()>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliSyncStatus {
    pub enabled: bool,
    pub active: bool,
    pub case_dir: Option<String>,
    pub port: Option<u16>,
    pub pid: u32,
    pub last_error: Option<String>,
}

pub fn read_enabled_setting(case_dir: &Path) -> bool {
    let path = case_paths::ledger_path(case_dir);
    let Ok(conn) = case_ledger::open(&path) else {
        return true;
    };
    match case_ledger::setting(&conn, SETTING_KEY) {
        Ok(Some(v)) => !matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "false" | "0" | "off" | "no"
        ),
        _ => true,
    }
}

pub async fn start_if_enabled(app: &AppHandle, state: &AppState, case_dir: &Path) {
    let dir_buf = case_dir.to_path_buf();
    let enabled = tokio::task::spawn_blocking(move || read_enabled_setting(&dir_buf))
        .await
        .unwrap_or(true);
    if !enabled {
        stop(Some(app), state).await;
        return;
    }
    if let Err(err) = start_for_case(app, state, &case_dir.to_string_lossy()).await {
        tracing::warn!(
            "cli_sync: failed to start server for {}: {err}",
            case_dir.display()
        );
    }
}

pub fn start_for_case<'a>(
    app: &'a AppHandle,
    state: &'a AppState,
    case_dir: &'a str,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<CliSyncStatus, AppError>> + Send + 'a>>
{
    Box::pin(async move {
        stop(None, state).await;

        let case_dir_owned = case_dir.to_string();
        let prepared =
            tokio::task::spawn_blocking(move || prepare_with_mode(&case_dir_owned, true))
                .await
                .map_err(|err| AppError::task_panicked("cli_sync_prepare", err))?
                .map_err(|err| AppError::internal(err.message))?;

        let port = prepared.port();
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        let reporter = Reporter::new(ProgressOptions {
            quiet: true,
            events: false,
        });
        let exec = Exec {
            host: app.clone(),
            state: Arc::new(state.clone()),
            mode: Mode::Backend,
            reporter,
            background: Background::default(),
            workspace: Workspace::default(),
        };

        let app_emit = app.clone();
        let case_for_emit = case_dir.to_string();
        let on_mutation: Arc<dyn Fn(&str) + Send + Sync> = Arc::new(move |command_path: &str| {
            let payload = serde_json::json!({
                "command": command_path,
                "caseDir": case_for_emit,
                "at": crate::core::casework::contest::now_iso(),
            });
            let _ = app_emit.emit("cli-sync-event", payload);
            if command_path.starts_with("contest ")
                || command_path.starts_with("pin ")
                || command_path.starts_with("wp ")
            {
                let _ = app_emit.emit(
                    "contest-updated",
                    serde_json::json!({
                        "source": "cli",
                        "command": command_path,
                    }),
                );
            }
        });

        let join_handle = tokio::spawn(async move {
            let options = ServerOptions {
                desktop_hosted: true,
                shutdown: Some(shutdown_rx),
                on_mutation: Some(on_mutation),
            };
            if let Err(err) = serve_with_options(&exec, prepared, options).await {
                tracing::warn!("cli_sync server stopped with error: {}", err.message);
            }
        });

        {
            let mut guard = state.cli_sync.lock().await;
            *guard = Some(CliSyncRuntime {
                case_dir: case_dir.to_string(),
                port,
                shutdown_tx,
                join_handle,
                last_error: None,
            });
        }

        let st = status(state).await;
        let _ = app.emit("cli-sync-status", &st);
        Ok(st)
    })
}

pub async fn stop(app: Option<&AppHandle>, state: &AppState) {
    let runtime = {
        let mut guard = state.cli_sync.lock().await;
        guard.take()
    };
    if let Some(rt) = runtime {
        let _ = rt.shutdown_tx.send(true);
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), rt.join_handle).await;
    }
    if let Some(app) = app {
        let st = status(state).await;
        let _ = app.emit("cli-sync-status", &st);
    }
}

pub async fn status(state: &AppState) -> CliSyncStatus {
    let case_dir = {
        let guard = state.active_case.lock().await;
        guard.as_ref().map(|c| c.dir.clone())
    };
    let enabled = match case_dir {
        Some(ref dir) => {
            let dir_clone = dir.clone();
            tokio::task::spawn_blocking(move || read_enabled_setting(&dir_clone))
                .await
                .unwrap_or(true)
        }
        None => false,
    };
    let sync_guard = state.cli_sync.lock().await;
    let (active, active_dir, port, last_error) = match sync_guard.as_ref() {
        Some(rt) => (
            !rt.join_handle.is_finished(),
            Some(rt.case_dir.clone()),
            Some(rt.port),
            rt.last_error.clone(),
        ),
        None => (
            false,
            case_dir.map(|d| d.to_string_lossy().to_string()),
            None,
            None,
        ),
    };
    CliSyncStatus {
        enabled,
        active,
        case_dir: active_dir,
        port,
        pid: std::process::id(),
        last_error,
    }
}

pub async fn set_enabled(
    app: &AppHandle,
    state: &AppState,
    enabled: bool,
) -> Result<CliSyncStatus, AppError> {
    let case_dir = {
        let guard = state.active_case.lock().await;
        guard
            .as_ref()
            .map(|c| c.dir.clone())
            .ok_or_else(|| AppError::invalid_input("no active case"))?
    };

    let dir_clone = case_dir.clone();
    let val = if enabled { "true" } else { "false" }.to_string();
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let ledger = case_ledger::open(&case_paths::ledger_path(&dir_clone))?;
        case_ledger::settings_save(&ledger, &[(SETTING_KEY.to_string(), val)])?;
        Ok(())
    })
    .await
    .map_err(|err| AppError::task_panicked("cli_sync_set", err))?
    .map_err(AppError::from)?;

    if enabled {
        start_for_case(app, state, &case_dir.to_string_lossy()).await
    } else {
        stop(Some(app), state).await;
        Ok(status(state).await)
    }
}
