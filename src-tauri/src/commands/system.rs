//! System commands: build information, runtime health and log access.
//!
//! These are the only commands exposed in FASE 1. They are intentionally
//! small, fully implemented and covered by the frontend's IPC layer; later
//! phases add `library`, `playback`, `playlist` and `settings` modules.

use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_opener::OpenerExt;

use crate::db::{self, DatabaseHealth};
use crate::error::PrismError;
use crate::state::AppState;

/// Build and location metadata rendered in Settings → About.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: &'static str,
    pub version: &'static str,
    pub tauri_version: &'static str,
    pub target_os: &'static str,
    pub target_arch: &'static str,
    pub build_profile: &'static str,
    pub data_dir: String,
    pub database_path: String,
    pub log_dir: String,
}

/// Aggregate health payload for the diagnostics panel.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeHealth {
    pub status: &'static str,
    pub uptime_ms: u64,
    pub database: DatabaseHealth,
}

/// Return version, target and path information.
///
/// `AppHandle`/`State` are injected by value by Tauri's command macro; the
/// owned-handle signature is required by the framework, not a style choice.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn get_app_info(state: State<'_, AppState>) -> Result<AppInfo, PrismError> {
    Ok(AppInfo {
        name: "Prism",
        version: env!("CARGO_PKG_VERSION"),
        tauri_version: tauri::VERSION,
        target_os: std::env::consts::OS,
        target_arch: std::env::consts::ARCH,
        build_profile: if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        data_dir: state.paths().data_dir.to_string_lossy().into_owned(),
        database_path: state.database_path().to_string_lossy().into_owned(),
        log_dir: state.paths().log_dir.to_string_lossy().into_owned(),
    })
}

/// Database + process health. Runs a bounded `PRAGMA quick_check(1)`.
#[tauri::command]
pub async fn get_runtime_health(state: State<'_, AppState>) -> Result<RuntimeHealth, PrismError> {
    let database = db::health(state.pool()).await?;
    Ok(RuntimeHealth {
        status: if database.integrity == "ok" {
            "ok"
        } else {
            "degraded"
        },
        uptime_ms: state.uptime_ms(),
        database,
    })
}

/// Open the log directory in the platform file manager.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn open_log_folder(app: AppHandle) -> Result<(), PrismError> {
    let log_dir = app.path().app_log_dir()?;
    std::fs::create_dir_all(&log_dir).map_err(|err| PrismError::io(&log_dir, err))?;

    app.opener()
        .open_path(log_dir.to_string_lossy().into_owned(), None::<&str>)
        .map_err(|err| PrismError::Internal(format!("could not open log folder: {err}")))
}

/// Open the library database directory in the platform file manager.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn open_data_folder(app: AppHandle) -> Result<(), PrismError> {
    let data_dir = app.path().app_data_dir()?;
    std::fs::create_dir_all(&data_dir).map_err(|err| PrismError::io(&data_dir, err))?;

    app.opener()
        .open_path(data_dir.to_string_lossy().into_owned(), None::<&str>)
        .map_err(|err| PrismError::Internal(format!("could not open data folder: {err}")))
}
