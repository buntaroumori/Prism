//! Lyrics command surface.
//!
//! Two commands, both cheap: the now-playing panel requests lyrics whenever the
//! track changes, and the refresh button asks for a re-read (a user who just
//! dropped a `.lrc` next to the file expects to see it immediately).

use tauri::State;

use crate::error::PrismError;
use crate::lyrics::{LyricsDocument, LyricsService};
use crate::state::AppState;

/// Resolve the lyrics for a track.
///
/// Order: embedded tag → sidecar `.lrc` → empty document. `refresh` bypasses the
/// cache; a miss is never stored, so a fresh `.lrc` is picked up either way.
#[tauri::command]
pub async fn lyrics_get(
    state: State<'_, AppState>,
    track_id: String,
    refresh: Option<bool>,
) -> Result<LyricsDocument, PrismError> {
    if track_id.trim().is_empty() {
        return Err(PrismError::invalid("track id cannot be empty"));
    }
    LyricsService::new(state.pool().clone())
        .get(&track_id, refresh.unwrap_or(false))
        .await
}

/// Drop the cached lyrics for a track. Returns `true` when a row existed.
#[tauri::command]
pub async fn lyrics_clear(
    state: State<'_, AppState>,
    track_id: String,
) -> Result<bool, PrismError> {
    if track_id.trim().is_empty() {
        return Err(PrismError::invalid("track id cannot be empty"));
    }
    LyricsService::new(state.pool().clone())
        .clear(&track_id)
        .await
}
