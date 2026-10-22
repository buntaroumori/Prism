//! Discord Rich Presence commands.
//!
//! Presence itself is driven by the audio engine (see `lib.rs`), so the UI only
//! needs to read the connection state and change the configuration.

use serde::Serialize;
use tauri::State;

use crate::discord::{DiscordConfig, DiscordState, DiscordStatus};
use crate::error::PrismError;
use crate::state::AppState;

/// Flattened configuration + connection snapshot for the settings panel.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscordStateDto {
    pub enabled: bool,
    /// True when a syntactically valid application id is configured.
    pub configured: bool,
    pub client_id: String,
    pub state: DiscordState,
    pub attempts: u32,
    pub last_error: Option<String>,
    pub last_update_at: Option<String>,
    pub show_cover: bool,
    pub show_timestamps: bool,
    pub show_quality: bool,
    pub show_paused: bool,
}

fn to_dto(config: DiscordConfig, status: DiscordStatus) -> DiscordStateDto {
    DiscordStateDto {
        enabled: config.enabled,
        configured: config.validate().is_ok() && !config.client_id.trim().is_empty(),
        client_id: config.client_id,
        state: status.state,
        attempts: status.attempts,
        last_error: status.last_error,
        last_update_at: status.last_update_at,
        show_cover: config.show_cover,
        show_timestamps: config.show_timestamps,
        show_quality: config.show_quality,
        show_paused: config.show_paused,
    }
}

/// Current rich-presence configuration and socket state.
#[tauri::command]
pub async fn discord_status(state: State<'_, AppState>) -> Result<DiscordStateDto, PrismError> {
    let (config, status) = state.discord().status();
    Ok(to_dto(config, status))
}

/// Apply a new configuration, persisting it only if it validates.
#[tauri::command]
pub async fn discord_configure(
    state: State<'_, AppState>,
    config: DiscordConfig,
) -> Result<DiscordStateDto, PrismError> {
    let status = state
        .discord()
        .configure(config.clone())
        .map_err(PrismError::invalid)?;
    config.save(state.pool()).await?;
    // Apply a changed pause preference to the record that is loaded right now,
    // rather than waiting for the next transport event.
    state.discord().sync(&state.engine().status()).await;
    Ok(to_dto(config, status))
}

/// Clear the published activity without disabling rich presence.
#[tauri::command]
pub async fn discord_clear(state: State<'_, AppState>) -> Result<bool, PrismError> {
    state.discord().publish(None).await;
    Ok(true)
}
