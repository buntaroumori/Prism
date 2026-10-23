//! User-owned playlists.
//!
//! Playlists live in the catalogue, not in browser storage: they survive a
//! rescan, a restart and a moved webview profile, and a deleted audio file is
//! removed through the existing foreign-key cascade.

use std::path::PathBuf;

use serde::Serialize;
use sqlx::Row;
use tauri::State;

use crate::db::catalog::stable_id;
use crate::db::search::{self, Page, TrackRow};
use crate::error::PrismError;
use crate::state::AppState;

const PLAYLIST_COLUMNS: &str = "id, name, description, kind, created_at, updated_at, \
     track_count, duration_ms, cover_id, cover_remote_url, cover_local_path";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistRow {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub kind: String,
    pub created_at: String,
    pub updated_at: String,
    pub track_count: i64,
    pub duration_ms: i64,
    pub cover_id: Option<String>,
    pub cover_path: Option<String>,
    pub cover_url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistDetail {
    pub playlist: PlaylistRow,
    pub tracks: Page<TrackRow>,
}

fn from_row(row: &sqlx::sqlite::SqliteRow) -> Result<PlaylistRow, sqlx::Error> {
    Ok(PlaylistRow {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        description: row.try_get("description")?,
        kind: row.try_get("kind")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        track_count: row.try_get("track_count")?,
        duration_ms: row.try_get("duration_ms")?,
        cover_id: row.try_get("cover_id")?,
        cover_url: row.try_get("cover_remote_url")?,
        cover_path: row.try_get("cover_local_path")?,
    })
}

async fn one(pool: &sqlx::SqlitePool, id: &str) -> Result<PlaylistRow, PrismError> {
    let sql = format!("SELECT {PLAYLIST_COLUMNS} FROM v_playlist_summary WHERE id = ?1");
    let row = sqlx::query(&sql)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| PrismError::invalid("unknown playlist"))?;
    from_row(&row).map_err(Into::into)
}

#[tauri::command]
pub async fn playlist_list(state: State<'_, AppState>) -> Result<Vec<PlaylistRow>, PrismError> {
    let sql = format!(
        "SELECT {PLAYLIST_COLUMNS} FROM v_playlist_summary WHERE kind = 'static' \
         ORDER BY name COLLATE NOCASE ASC"
    );
    let rows = sqlx::query(&sql).fetch_all(state.pool()).await?;
    rows.iter()
        .map(from_row)
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}

#[tauri::command]
pub async fn playlist_create(
    state: State<'_, AppState>,
    name: String,
) -> Result<PlaylistRow, PrismError> {
    create_in(state.pool(), name).await
}

/// Pool-level implementation, shared by the command and the real-database
/// integration tests (no Tauri handle or mock repository required).
pub async fn create_in(pool: &sqlx::SqlitePool, name: String) -> Result<PlaylistRow, PrismError> {
    let name = name.trim().to_string();
    if name.is_empty() || name.chars().count() > 128 {
        return Err(PrismError::invalid(
            "playlist names must contain between 1 and 128 characters",
        ));
    }
    let exists: Option<String> =
        sqlx::query_scalar("SELECT id FROM playlists WHERE name = ?1 COLLATE NOCASE LIMIT 1")
            .bind(&name)
            .fetch_optional(pool)
            .await?;
    if exists.is_some() {
        return Err(PrismError::invalid(
            "a playlist with that name already exists",
        ));
    }

    let id = stable_id("pl", &uuid::Uuid::new_v4().to_string());
    sqlx::query("INSERT INTO playlists (id, name, kind) VALUES (?1, ?2, 'static')")
        .bind(&id)
        .bind(&name)
        .execute(pool)
        .await?;
    one(pool, &id).await
}

/// Add tracks in the order supplied. Existing entries are left in place.
#[tauri::command]
pub async fn playlist_add_tracks(
    state: State<'_, AppState>,
    playlist_id: String,
    track_ids: Vec<String>,
) -> Result<PlaylistRow, PrismError> {
    add_tracks_in(state.pool(), playlist_id, track_ids).await
}

pub async fn add_tracks_in(
    pool: &sqlx::SqlitePool,
    playlist_id: String,
    track_ids: Vec<String>,
) -> Result<PlaylistRow, PrismError> {
    if track_ids.is_empty() || track_ids.len() > 500 {
        return Err(PrismError::invalid(
            "choose between 1 and 500 tracks to add at once",
        ));
    }
    let kind: Option<String> = sqlx::query_scalar("SELECT kind FROM playlists WHERE id = ?1")
        .bind(&playlist_id)
        .fetch_optional(pool)
        .await?;
    if kind.as_deref() != Some("static") {
        return Err(PrismError::invalid("unknown or read-only playlist"));
    }

    let mut transaction = pool.begin().await?;
    for track_id in track_ids {
        sqlx::query(
            "INSERT INTO playlist_items (id, playlist_id, track_id, position) \
             SELECT ?1, ?2, t.id, \
                 COALESCE((SELECT MAX(position) + 1.0 FROM playlist_items \
                           WHERE playlist_id = ?2), 0.0) \
             FROM tracks AS t WHERE t.id = ?3 \
             ON CONFLICT(playlist_id, track_id) DO NOTHING",
        )
        .bind(stable_id("pli", &format!("{playlist_id}:{track_id}")))
        .bind(&playlist_id)
        .bind(&track_id)
        .execute(&mut *transaction)
        .await?;
    }
    sqlx::query(
        "UPDATE playlists SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), \
             cover_id = COALESCE(cover_id, ( \
                SELECT al.cover_id FROM playlist_items pi \
                JOIN tracks t ON t.id = pi.track_id \
                JOIN albums al ON al.id = t.album_id \
                WHERE pi.playlist_id = ?1 AND al.cover_id IS NOT NULL \
                ORDER BY pi.position ASC LIMIT 1 \
            )) WHERE id = ?1",
    )
    .bind(&playlist_id)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    one(pool, &playlist_id).await
}

#[tauri::command]
pub async fn playlist_detail(
    state: State<'_, AppState>,
    id: String,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<PlaylistDetail, PrismError> {
    let playlist = one(state.pool(), &id).await?;
    let tracks =
        search::playlist_tracks(state.pool(), &id, limit.unwrap_or(500), offset.unwrap_or(0))
            .await?;
    Ok(PlaylistDetail { playlist, tracks })
}

#[tauri::command]
pub async fn playlist_remove_track(
    state: State<'_, AppState>,
    playlist_id: String,
    track_id: String,
) -> Result<PlaylistRow, PrismError> {
    sqlx::query("DELETE FROM playlist_items WHERE playlist_id = ?1 AND track_id = ?2")
        .bind(&playlist_id)
        .bind(&track_id)
        .execute(state.pool())
        .await?;
    sqlx::query(
        "UPDATE playlists SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1",
    )
    .bind(&playlist_id)
    .execute(state.pool())
    .await?;
    one(state.pool(), &playlist_id).await
}

/// Copy and normalise a user-selected image without touching album artwork or
/// the cover-cache identity consumed by Discord.
#[tauri::command]
pub async fn playlist_set_custom_cover(
    state: State<'_, AppState>,
    playlist_id: String,
    path: String,
) -> Result<PlaylistRow, PrismError> {
    let editable: i64 = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM playlists WHERE id = ?1 AND kind = 'static')",
    )
    .bind(&playlist_id)
    .fetch_one(state.pool())
    .await?;
    if editable == 0 {
        return Err(PrismError::invalid("unknown or read-only playlist"));
    }

    let source = PathBuf::from(path);
    let metadata = tokio::fs::metadata(&source)
        .await
        .map_err(|error| PrismError::io(&source, error))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > 20 * 1024 * 1024 {
        return Err(PrismError::invalid(
            "choose a JPEG, PNG or WebP image smaller than 20 MiB",
        ));
    }
    let directory = state.paths().data_dir.join("playlist-covers");
    tokio::fs::create_dir_all(&directory)
        .await
        .map_err(|error| PrismError::io(&directory, error))?;
    let destination = directory.join(format!("{playlist_id}.jpg"));
    let bytes = tokio::fs::read(&source)
        .await
        .map_err(|error| PrismError::io(&source, error))?;
    let write_destination = destination.clone();
    tokio::task::spawn_blocking(move || -> Result<(), PrismError> {
        let decoded = image::load_from_memory(&bytes).map_err(|error| {
            PrismError::invalid(format!("cannot decode playlist cover: {error}"))
        })?;
        let decoded = if decoded.width() > 1_000 || decoded.height() > 1_000 {
            decoded.thumbnail(1_000, 1_000)
        } else {
            decoded
        };
        let mut encoded = Vec::with_capacity(64 * 1024);
        let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut encoded, 88);
        encoder
            .encode_image(&decoded.to_rgb8())
            .map_err(|error| PrismError::Internal(format!("encode playlist cover: {error}")))?;
        let parent = write_destination.parent().ok_or_else(|| {
            PrismError::invalid("the playlist cover destination has no parent directory")
        })?;
        let temporary = tempfile::NamedTempFile::new_in(parent)
            .map_err(|error| PrismError::io(parent, error))?;
        std::fs::write(temporary.path(), encoded)
            .map_err(|error| PrismError::io(temporary.path(), error))?;
        temporary
            .persist(&write_destination)
            .map_err(|error| PrismError::io(&write_destination, error.error))?;
        Ok(())
    })
    .await
    .map_err(|error| PrismError::Internal(format!("playlist cover task failed: {error}")))??;

    let affected = sqlx::query(
        "UPDATE playlists SET custom_cover_path = ?2, \
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') \
         WHERE id = ?1 AND kind = 'static'",
    )
    .bind(&playlist_id)
    .bind(destination.to_string_lossy().into_owned())
    .execute(state.pool())
    .await?
    .rows_affected();
    if affected == 0 {
        return Err(PrismError::invalid("unknown or read-only playlist"));
    }
    one(state.pool(), &playlist_id).await
}

#[tauri::command]
pub async fn playlist_delete(state: State<'_, AppState>, id: String) -> Result<bool, PrismError> {
    let result = sqlx::query("DELETE FROM playlists WHERE id = ?1 AND kind = 'static'")
        .bind(id)
        .execute(state.pool())
        .await?;
    Ok(result.rows_affected() > 0)
}
