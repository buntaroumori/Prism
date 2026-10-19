//! Library command surface: scanning, browsing, search and artwork.
//!
//! Every command returns `Result<T, PrismError>`, so the TypeScript layer always
//! receives a typed `{ code, message }` instead of a stringly-typed failure.
//! Long-running work (a scan) returns immediately and reports through the two
//! event channels declared at the bottom of this module.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use sqlx::Row;
use tauri::{Emitter, State};

use crate::artwork::{CoverAsset, CoverHint};
use crate::db::catalog::{path_key, stable_id};
use crate::db::search::{
    self, AlbumRow, ArtistRow, Facets, GlobalSearch, Page, TrackQuery, TrackRow, TrackSort,
};
use crate::error::PrismError;
use crate::metadata::scanner::{ScanConfig, ScanOutcome, ScanProgress, ScanTrigger};
use crate::state::AppState;

/// Event emitted on every throttled progress tick (≤ 5 Hz).
pub const SCAN_PROGRESS_EVENT: &str = "library://scan-progress";
/// Event emitted once a scan completes, is cancelled or fails.
pub const SCAN_COMPLETE_EVENT: &str = "library://scan-complete";

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Scanning
// ---------------------------------------------------------------------------

/// Options accepted by `library_scan_start`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ScanOptions {
    /// Roots to scan; empty means "every enabled library root".
    pub roots: Vec<String>,
    /// Ignore the mtime/size index and re-read everything.
    pub full: bool,
    /// Delete albums/artists left without tracks (implies a full scan).
    pub prune: bool,
}

/// Acknowledged scan request.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanStarted {
    pub accepted: bool,
    pub roots: usize,
    pub full: bool,
}

/// Live scanner status for the settings panel.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanStatus {
    pub running: bool,
    pub progress: Option<ScanProgress>,
    pub last_run: Option<ScanRunRow>,
}

/// One row of the scan history.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanRunRow {
    pub id: String,
    pub status: String,
    pub trigger: String,
    pub files_seen: i64,
    pub files_indexed: i64,
    pub files_skipped: i64,
    pub files_failed: i64,
    pub tracks_removed: i64,
    pub bytes_read: i64,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub duration_ms: Option<i64>,
    pub error_message: Option<String>,
    pub errors: i64,
}

/// One recorded per-file failure.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanErrorRow {
    pub path: String,
    pub error_code: String,
    pub message: String,
    pub occurred_at: String,
}

/// Start a scan in the background.
///
/// Returns as soon as the scanner accepted the job; progress arrives through
/// [`SCAN_PROGRESS_EVENT`] and completion through [`SCAN_COMPLETE_EVENT`].
#[tauri::command]
pub async fn library_scan_start(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    options: Option<ScanOptions>,
) -> Result<ScanStarted, PrismError> {
    if state.scanner().is_running() {
        return Err(PrismError::invalid("a library scan is already running"));
    }

    let options = options.unwrap_or_default();
    let mut roots: Vec<PathBuf> = Vec::with_capacity(options.roots.len());
    for root in &options.roots {
        let path = PathBuf::from(root);
        if !path.is_dir() {
            return Err(PrismError::invalid(format!(
                "{root} is not a readable directory"
            )));
        }
        roots.push(path);
    }

    if roots.is_empty() {
        roots = state.scanner().roots().await?;
    }
    if roots.is_empty() {
        return Err(PrismError::invalid(
            "the library has no roots; add a folder before scanning",
        ));
    }

    let config = ScanConfig {
        roots: roots.clone(),
        full: options.full || options.prune,
        prune: options.prune,
        ..ScanConfig::default()
    };
    let full = config.full;

    let scanner = state.scanner().clone();
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let progress_handle = handle.clone();
        let outcome: Result<ScanOutcome, PrismError> = scanner
            .scan(config, ScanTrigger::Manual, move |progress| {
                if let Err(error) = progress_handle.emit(SCAN_PROGRESS_EVENT, &progress) {
                    tracing::debug!(%error, "cannot forward scan progress");
                }
            })
            .await;

        match &outcome {
            Ok(result) => {
                if let Err(error) = handle.emit(SCAN_COMPLETE_EVENT, result) {
                    tracing::debug!(%error, "cannot forward scan completion");
                }
            }
            Err(error) => {
                if let Err(emit_error) = handle.emit(SCAN_COMPLETE_EVENT, error) {
                    tracing::debug!(%emit_error, "cannot forward scan failure");
                }
            }
        }
    });

    Ok(ScanStarted {
        accepted: true,
        roots: roots.len(),
        full,
    })
}

/// Ask the running scan to stop at the next batch boundary.
#[tauri::command]
pub fn library_scan_cancel(state: State<'_, AppState>) -> bool {
    state.scanner().cancel();
    true
}

/// Current scanner status plus the most recent run.
#[tauri::command]
pub async fn library_scan_status(state: State<'_, AppState>) -> Result<ScanStatus, PrismError> {
    let last_run = scan_run_rows(state.pool(), 1).await?.into_iter().next();
    Ok(ScanStatus {
        running: state.scanner().is_running(),
        progress: state.scanner().progress(),
        last_run,
    })
}

/// Scan history, newest first.
#[tauri::command]
pub async fn library_scan_runs(
    state: State<'_, AppState>,
    limit: Option<u32>,
) -> Result<Vec<ScanRunRow>, PrismError> {
    scan_run_rows(state.pool(), limit.unwrap_or(20).clamp(1, 200)).await
}

async fn scan_run_rows(pool: &sqlx::SqlitePool, limit: u32) -> Result<Vec<ScanRunRow>, PrismError> {
    let rows = sqlx::query(
        "SELECT r.id, r.status, r.trigger, r.files_seen, r.files_indexed, r.files_skipped, \
                r.files_failed, r.tracks_removed, r.bytes_read, r.started_at, r.finished_at, \
                r.duration_ms, r.error_message, \
                (SELECT COUNT(*) FROM scan_errors WHERE run_id = r.id) AS errors \
         FROM scan_runs AS r ORDER BY r.started_at DESC LIMIT ?1",
    )
    .bind(i64::from(limit))
    .fetch_all(pool)
    .await?;

    rows.iter()
        .map(|row| {
            Ok(ScanRunRow {
                id: row.try_get("id")?,
                status: row.try_get("status")?,
                trigger: row.try_get("trigger")?,
                files_seen: row.try_get("files_seen")?,
                files_indexed: row.try_get("files_indexed")?,
                files_skipped: row.try_get("files_skipped")?,
                files_failed: row.try_get("files_failed")?,
                tracks_removed: row.try_get("tracks_removed")?,
                bytes_read: row.try_get("bytes_read")?,
                started_at: row.try_get("started_at")?,
                finished_at: row.try_get("finished_at")?,
                duration_ms: row.try_get("duration_ms")?,
                error_message: row.try_get("error_message")?,
                errors: row.try_get("errors")?,
            })
        })
        .collect()
}

/// Per-file failures recorded by one scan.
#[tauri::command]
pub async fn library_scan_errors(
    state: State<'_, AppState>,
    run_id: String,
    limit: Option<u32>,
) -> Result<Vec<ScanErrorRow>, PrismError> {
    let rows = sqlx::query(
        "SELECT path, error_code, message, occurred_at FROM scan_errors \
         WHERE run_id = ?1 ORDER BY occurred_at ASC LIMIT ?2",
    )
    .bind(run_id)
    .bind(i64::from(limit.unwrap_or(100).clamp(1, 1000)))
    .fetch_all(state.pool())
    .await?;

    rows.iter()
        .map(|row| {
            Ok(ScanErrorRow {
                path: row.try_get("path")?,
                error_code: row.try_get("error_code")?,
                message: row.try_get("message")?,
                occurred_at: row.try_get("occurred_at")?,
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Roots
// ---------------------------------------------------------------------------

/// A folder the scanner walks.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryRootRow {
    pub id: String,
    pub path: String,
    pub name: String,
    pub enabled: bool,
    pub recursive: bool,
    pub watch: bool,
    /// False when the folder is currently unreachable (unmounted drive).
    pub available: bool,
    pub tracks: i64,
}

/// The root projection shared by the list and single-row commands.
///
/// Public so the integration tests can assert the wildcard escaping without
/// standing up a Tauri application handle.
///
/// The `tracks` counter escapes `%`, `_` and `\` inside the stored path: a
/// folder literally named `100% Hits` must not turn into a wildcard. This is a
/// full scan per root, which is acceptable for a command the UI calls once per
/// settings visit and never on a scroll path.
pub const ROOT_SELECT: &str = "SELECT r.id, r.path, r.name, r.enabled, r.recursive, r.watch, \
        (SELECT COUNT(*) FROM tracks WHERE path LIKE \
            replace(replace(replace(r.path, '\\', '\\\\'), '%', '\\%'), '_', '\\_') || '%' \
            ESCAPE '\\') AS tracks \
     FROM library_roots AS r";

fn root_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<LibraryRootRow, sqlx::Error> {
    let path: String = row.try_get("path")?;
    Ok(LibraryRootRow {
        id: row.try_get("id")?,
        available: PathBuf::from(&path).is_dir(),
        path,
        name: row.try_get("name")?,
        enabled: row.try_get::<i64, _>("enabled")? != 0,
        recursive: row.try_get::<i64, _>("recursive")? != 0,
        watch: row.try_get::<i64, _>("watch")? != 0,
        tracks: row.try_get("tracks")?,
    })
}

/// List every configured root.
#[tauri::command]
pub async fn library_roots(state: State<'_, AppState>) -> Result<Vec<LibraryRootRow>, PrismError> {
    let sql = format!("{ROOT_SELECT} ORDER BY r.path");
    let rows = sqlx::query(&sql).fetch_all(state.pool()).await?;
    rows.iter()
        .map(root_from_row)
        .collect::<Result<Vec<_>, sqlx::Error>>()
        .map_err(Into::into)
}

/// Add a folder to the library. Idempotent: re-adding an existing root returns
/// the stored row instead of failing.
#[tauri::command]
pub async fn library_add_root(
    state: State<'_, AppState>,
    path: String,
    name: Option<String>,
) -> Result<LibraryRootRow, PrismError> {
    let candidate = PathBuf::from(path.trim());
    let canonical = candidate
        .canonicalize()
        .map_err(|error| PrismError::io(&candidate, error))?;
    if !canonical.is_dir() {
        return Err(PrismError::invalid(format!(
            "{} is not a directory",
            canonical.display()
        )));
    }

    // Indexing the application's own cache would recurse into generated files.
    let data_dir = state.paths().data_dir.clone();
    if canonical.starts_with(&data_dir) {
        return Err(PrismError::invalid(
            "the application data directory cannot be a library root",
        ));
    }

    let path_string = canonical.to_string_lossy().into_owned();
    let id = stable_id("root", &path_key(&path_string));
    let display_name = name
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            canonical
                .file_name()
                .and_then(|value| value.to_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| path_string.clone());

    sqlx::query(
        "INSERT INTO library_roots (id, path, name) VALUES (?1, ?2, ?3) \
         ON CONFLICT(path) DO UPDATE SET name = excluded.name, \
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')",
    )
    .bind(&id)
    .bind(&path_string)
    .bind(&display_name)
    .execute(state.pool())
    .await?;

    let sql = format!("{ROOT_SELECT} WHERE r.path = ?1");
    let row = sqlx::query(&sql)
        .bind(&path_string)
        .fetch_one(state.pool())
        .await?;
    Ok(root_from_row(&row)?)
}

/// Remove a root. Indexed rows are kept: the user may only be moving a drive.
#[tauri::command]
pub async fn library_remove_root(
    state: State<'_, AppState>,
    id: String,
) -> Result<bool, PrismError> {
    let affected = sqlx::query("DELETE FROM library_roots WHERE id = ?1")
        .bind(id)
        .execute(state.pool())
        .await?
        .rows_affected();
    Ok(affected > 0)
}

/// Enable or disable a root without forgetting it.
#[tauri::command]
pub async fn library_set_root_enabled(
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
) -> Result<bool, PrismError> {
    let affected = sqlx::query(
        "UPDATE library_roots SET enabled = ?2, \
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?1",
    )
    .bind(id)
    .bind(i64::from(enabled))
    .execute(state.pool())
    .await?
    .rows_affected();
    Ok(affected > 0)
}

// ---------------------------------------------------------------------------
// Browse and search
// ---------------------------------------------------------------------------

/// Search or browse tracks with any combination of filters.
#[tauri::command]
pub async fn library_search(
    state: State<'_, AppState>,
    query: Option<TrackQuery>,
) -> Result<Page<TrackRow>, PrismError> {
    search::tracks(state.pool(), query.unwrap_or_default()).await
}

/// Album grid, optionally filtered by free text or artist.
#[tauri::command]
pub async fn library_albums(
    state: State<'_, AppState>,
    text: Option<String>,
    artist_id: Option<String>,
    sort: Option<TrackSort>,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<Page<AlbumRow>, PrismError> {
    search::albums(
        state.pool(),
        text.as_deref(),
        artist_id.as_deref(),
        sort.unwrap_or(TrackSort::Title),
        limit.unwrap_or(60),
        offset.unwrap_or(0),
    )
    .await
}

/// Artist grid with aggregate counters.
#[tauri::command]
pub async fn library_artists(
    state: State<'_, AppState>,
    text: Option<String>,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<Page<ArtistRow>, PrismError> {
    search::artists(
        state.pool(),
        text.as_deref(),
        limit.unwrap_or(60),
        offset.unwrap_or(0),
    )
    .await
}

/// Genres, formats and decades present in the library.
#[tauri::command]
pub async fn library_facets(state: State<'_, AppState>) -> Result<Facets, PrismError> {
    search::facets(state.pool()).await
}

/// One-shot search behind the ⌘K overlay.
#[tauri::command]
pub async fn library_global_search(
    state: State<'_, AppState>,
    text: String,
    limit: Option<u32>,
) -> Result<GlobalSearch, PrismError> {
    search::global(state.pool(), &text, limit.unwrap_or(8)).await
}

// ---------------------------------------------------------------------------
// Artwork
// ---------------------------------------------------------------------------

/// Cover for an album, extracting or downloading it when necessary.
///
/// The resolved `cover_cache` row is attached to the album, so the grid's next
/// query joins it directly and never re-enters this command.
#[tauri::command]
pub async fn library_album_cover(
    state: State<'_, AppState>,
    album_id: String,
) -> Result<CoverAsset, PrismError> {
    let row = sqlx::query("SELECT title, album_artist_name FROM albums WHERE id = ?1")
        .bind(&album_id)
        .fetch_optional(state.pool())
        .await?
        .ok_or_else(|| PrismError::invalid("unknown album"))?;
    let album: String = row.try_get("title")?;
    let artist: String = row.try_get("album_artist_name")?;

    let track_path: Option<String> = sqlx::query_scalar(
        "SELECT path FROM tracks WHERE album_id = ?1 AND missing = 0 \
         ORDER BY disc_no ASC, track_no ASC LIMIT 1",
    )
    .bind(&album_id)
    .fetch_optional(state.pool())
    .await?;

    let hint = CoverHint {
        folder: track_path.as_ref().and_then(|path| {
            std::path::Path::new(path)
                .parent()
                .map(std::path::Path::to_path_buf)
        }),
        track_path: track_path.map(PathBuf::from),
    };
    let asset = state.artwork().ensure(&artist, &album, &hint).await;

    if !asset.is_placeholder() {
        let _ = sqlx::query("UPDATE albums SET cover_id = ?2 WHERE id = ?1")
            .bind(&album_id)
            .bind(&asset.key)
            .execute(state.pool())
            .await;
    }

    Ok(asset)
}

/// Cover for the album a track belongs to (player bar).
#[tauri::command]
pub async fn library_track_cover(
    state: State<'_, AppState>,
    track_id: String,
) -> Result<CoverAsset, PrismError> {
    let row = sqlx::query(
        "SELECT artist_name, album_title, album_artist_name, folder, path, album_id \
         FROM tracks WHERE id = ?1",
    )
    .bind(&track_id)
    .fetch_optional(state.pool())
    .await?
    .ok_or_else(|| PrismError::invalid("unknown track"))?;

    let album: String = row.try_get("album_title")?;
    let artist: String = {
        let album_artist: String = row.try_get("album_artist_name")?;
        if album_artist.is_empty() {
            row.try_get("artist_name")?
        } else {
            album_artist
        }
    };
    let path: String = row.try_get("path")?;
    let album_id: Option<String> = row.try_get("album_id")?;

    let hint = CoverHint {
        folder: std::path::Path::new(&path)
            .parent()
            .map(std::path::Path::to_path_buf),
        track_path: Some(PathBuf::from(&path)),
    };
    let asset = state.artwork().ensure(&artist, &album, &hint).await;

    if !asset.is_placeholder()
        && let Some(album_id) = album_id
    {
        let _ = sqlx::query("UPDATE albums SET cover_id = ?2 WHERE id = ?1")
            .bind(album_id)
            .bind(&asset.key)
            .execute(state.pool())
            .await;
    }

    Ok(asset)
}
