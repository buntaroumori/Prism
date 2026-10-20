//! Plugin command surface.
//!
//! Discovery, enabling and configuration. All heavy work (compiling a module,
//! executing a hook) happens on the plugin manager; these commands are thin so
//! the UI can call them on every toggle without worrying about latency.

use serde_json::Value;
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::error::PrismError;
use crate::plugins::{PluginDescriptor, PluginManager};
use crate::state::AppState;

/// Everything Settings → Plugins renders, plus which runtimes this build has.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginSnapshot {
    pub plugins: Vec<PluginDescriptor>,
    /// `{ lua: bool, wasm: bool, api: u32 }`.
    pub runtimes: Value,
    pub directory: String,
}

fn snapshot(manager: &PluginManager, plugins: Vec<PluginDescriptor>) -> PluginSnapshot {
    PluginSnapshot {
        plugins,
        runtimes: crate::plugins::runtime_support(),
        directory: manager.directory().to_string_lossy().into_owned(),
    }
}

/// List installed plugins.
#[tauri::command]
pub async fn plugins_list(state: State<'_, AppState>) -> Result<PluginSnapshot, PrismError> {
    let manager = state.plugins();
    let plugins = manager.descriptors().await;
    Ok(snapshot(manager.as_ref(), plugins))
}

/// Rescan the plugin directory. Configuration survives a reload.
#[tauri::command]
pub async fn plugins_reload(state: State<'_, AppState>) -> Result<PluginSnapshot, PrismError> {
    let manager = state.plugins();
    manager.reload().await?;
    let plugins = manager.descriptors().await;
    Ok(snapshot(manager.as_ref(), plugins))
}

/// Enable or disable one plugin. The choice is persisted.
#[tauri::command]
pub async fn plugins_set_enabled(
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
) -> Result<PluginSnapshot, PrismError> {
    let manager = state.plugins();
    manager.set_enabled(&id, enabled).await?;
    let plugins = manager.descriptors().await;
    Ok(snapshot(manager.as_ref(), plugins))
}

/// Write one configuration value, validated against the manifest schema.
#[tauri::command]
pub async fn plugins_set_config(
    state: State<'_, AppState>,
    id: String,
    key: String,
    value: Value,
) -> Result<PluginSnapshot, PrismError> {
    let manager = state.plugins();
    manager.set_config(&id, &key, value).await?;
    let plugins = manager.descriptors().await;
    Ok(snapshot(manager.as_ref(), plugins))
}

/// Open the plugin folder so the user can drop a directory in.
#[tauri::command]
pub async fn plugins_open_folder(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), PrismError> {
    let directory = state.plugins().directory().to_path_buf();
    std::fs::create_dir_all(&directory).map_err(|error| PrismError::io(&directory, error))?;
    app.opener()
        .open_path(directory.to_string_lossy().into_owned(), None::<&str>)
        .map_err(|error| PrismError::Internal(format!("could not open the plugin folder: {error}")))
}
