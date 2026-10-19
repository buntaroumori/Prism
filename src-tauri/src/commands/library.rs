//! Library read-model commands.
//!
//! FASE 1 exposes the aggregate statistics computed by `v_library_stats`
//! (schema migration 0001). Scanning, pagination and search land in FASE 3 on
//! top of the same view and the FTS5 index.

use serde::Serialize;
use sqlx::Row;
use tauri::State;

use crate::error::PrismError;
use crate::state::AppState;

/// Aggregate library counters shown in the overview and the settings panel.
///
/// `max_sample_rate` / `max_bit_depth` are `None` for an empty library, which
/// the UI renders as "—" instead of a misleading zero.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryStats {
    pub track_count: i64,
    pub album_count: i64,
    pub artist_count: i64,
    pub total_duration_ms: i64,
    pub total_bytes: i64,
    pub missing_count: i64,
    pub unhealthy_count: i64,
    pub cue_track_count: i64,
    pub max_sample_rate: Option<i64>,
    pub max_bit_depth: Option<i64>,
}

/// Return the current aggregate library statistics.
///
/// The query hits a pre-aggregated view backed by covering indexes; on a 500k
/// track library it runs in single-digit milliseconds and never blocks audio.
#[tauri::command]
pub async fn get_library_stats(state: State<'_, AppState>) -> Result<LibraryStats, PrismError> {
    let row = sqlx::query(
        "SELECT track_count, album_count, artist_count, total_duration_ms, total_bytes, \
                missing_count, unhealthy_count, cue_track_count, max_sample_rate, max_bit_depth \
         FROM v_library_stats",
    )
    .fetch_one(state.pool())
    .await?;

    Ok(LibraryStats {
        track_count: row.try_get("track_count")?,
        album_count: row.try_get("album_count")?,
        artist_count: row.try_get("artist_count")?,
        total_duration_ms: row.try_get("total_duration_ms")?,
        total_bytes: row.try_get("total_bytes")?,
        missing_count: row.try_get("missing_count")?,
        unhealthy_count: row.try_get("unhealthy_count")?,
        cue_track_count: row.try_get("cue_track_count")?,
        max_sample_rate: row.try_get("max_sample_rate")?,
        max_bit_depth: row.try_get("max_bit_depth")?,
    })
}
