//! Write side of the library: deterministic identifiers and batch upserts.
//!
//! ## Why identifiers are derived, not generated
//!
//! Every id in the catalogue is `prefix + 96 bits of SHA-256` over a
//! *canonical identity*. That makes the scanner idempotent: re-indexing a file
//! after a crash, a rename of the library folder, or a second machine pointed
//! at the same share always lands on the same row. Playlists, play history and
//! ratings survive rescans because the primary key never changes.
//!
//! ```text
//! artist  → ar:<sha256(lower(name))>
//! album   → al:<sha256(lower(title) "|" lower(album_artist))>
//! track   → tr:<sha256(normalised_path "|" source "|" cue_start_ms)>
//! cue     → cs:<sha256(normalised_sheet_path)>
//! ```
//!
//! 96 bits keeps the birthday collision probability below 1e-17 for a 500k
//! track library, and keeps ids compact enough to inline in `SQLite` b-trees.
//!
//! ## Why batches
//!
//! One transaction per batch of [`BATCH_ROWS`] rows amortises the `WAL` fsync
//! and the trigger work for the FTS5 index. A 100k-file scan therefore performs
//! ~400 commits instead of 100 000 — the difference between 8 s and a minute on
//! spinning rust.

use std::sync::LazyLock;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::Row;
use sqlx::{QueryBuilder, Sqlite, SqlitePool, Transaction};

use crate::error::PrismError;

/// Rows per write transaction. 256 keeps each `SQLite` variable list well under
/// the 32 766 parameter ceiling (256 × 44 ≈ 11 264) while still amortising
/// commits.
pub const BATCH_ROWS: usize = 256;

/// Derive a stable identifier from a textual identity.
#[must_use]
pub fn stable_id(prefix: &str, identity: &str) -> String {
    let digest = Sha256::digest(identity.as_bytes());
    let mut id = String::with_capacity(prefix.len() + 1 + 24);
    id.push_str(prefix);
    id.push(':');
    for byte in &digest[..12] {
        use std::fmt::Write;
        let _ = write!(id, "{byte:02x}");
    }
    id
}

/// Canonical form of a filesystem path, identical to the expression used by
/// `ux_tracks_identity` in the schema: forward slashes, collapsed duplicate
/// separators, case-folded.
#[must_use]
pub fn path_key(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let mut previous_separator = false;
    for character in path.chars() {
        let character = if character == '\\' { '/' } else { character };
        if character == '/' {
            if previous_separator {
                continue;
            }
            previous_separator = true;
        } else {
            previous_separator = false;
        }
        out.extend(character.to_lowercase());
    }
    out
}

/// Identity of an artist row.
#[must_use]
pub fn artist_key(name: &str) -> String {
    name.trim().to_lowercase()
}

/// Identity of an album row (title + album artist, both case-folded).
#[must_use]
pub fn album_key(title: &str, album_artist: &str) -> String {
    format!(
        "{}|{}",
        title.trim().to_lowercase(),
        album_artist.trim().to_lowercase()
    )
}

/// Identity of a physical file row.
#[must_use]
pub fn file_track_key(path: &str) -> String {
    format!("{}|file|-1", path_key(path))
}

/// Identity of a CUE virtual track.
#[must_use]
pub fn cue_track_key(path: &str, start_ms: i64) -> String {
    format!("{}|cue|{start_ms}", path_key(path))
}

/// One row of the `tracks` table, fully materialised.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackRecord {
    pub id: String,
    pub path: String,
    pub folder: String,
    pub filename: String,
    pub source: TrackSource,
    pub title: String,
    pub title_sort: String,
    pub artist_name: String,
    pub album_title: String,
    pub album_artist_name: String,
    pub composer: String,
    pub genre: String,
    pub comment: String,
    pub year: Option<i64>,
    pub track_no: Option<i64>,
    pub disc_no: Option<i64>,
    pub track_total: Option<i64>,
    pub disc_total: Option<i64>,
    pub format: String,
    pub container: Option<String>,
    pub codec_profile: Option<String>,
    pub bit_depth: Option<i64>,
    pub sample_rate: Option<i64>,
    pub channels: Option<i64>,
    pub bitrate_kbps: Option<i64>,
    pub dsd_rate: Option<i64>,
    pub duration_ms: i64,
    pub file_size: i64,
    pub replaygain_track_gain_db: Option<f64>,
    pub replaygain_track_peak: Option<f64>,
    pub replaygain_album_gain_db: Option<f64>,
    pub replaygain_album_peak: Option<f64>,
    pub musicbrainz_track_id: Option<String>,
    pub musicbrainz_release_track_id: Option<String>,
    pub isrc: Option<String>,
    pub file_mtime: i64,
    pub cue_sheet_id: Option<String>,
    pub cue_start_ms: Option<i64>,
    pub cue_end_ms: Option<i64>,
    pub has_embedded_art: bool,
    pub extra_tags: String,
    /// Values copied into the album row the first time it is created.
    pub album: AlbumMeta,
}

/// Physical file or virtual CUE track.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackSource {
    File,
    Cue,
}

impl TrackSource {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Cue => "cue",
        }
    }
}

/// Album-level metadata carried by every track of the same album.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AlbumMeta {
    pub year: Option<i64>,
    pub original_year: Option<i64>,
    pub release_date: Option<String>,
    pub release_type: Option<String>,
    pub is_compilation: bool,
    pub label: Option<String>,
    pub catalog_number: Option<String>,
    pub barcode: Option<String>,
    pub musicbrainz_release_id: Option<String>,
    pub musicbrainz_release_group_id: Option<String>,
}

/// One virtual track expanded from a CUE sheet.
///
/// This is the read model for the album detail view: it carries the span in
/// both milliseconds (for display) and frames (for the audio engine) so the UI
/// never has to re-parse the sheet to start playback at the right sample.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CueTrackInfo {
    pub number: u8,
    pub title: String,
    pub performer: String,
    pub start_ms: i64,
    pub end_ms: Option<i64>,
    pub duration_ms: i64,
    pub track_id: Option<String>,
}

/// A parsed `.cue` sheet ready to be stored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CueSheetRecord {
    pub id: String,
    pub path: String,
    pub album_image: String,
    pub sha256: String,
    pub catalog: Option<String>,
    pub performer: Option<String>,
    pub title: Option<String>,
    pub songwriter: Option<String>,
    pub genre: Option<String>,
    pub date: Option<String>,
    pub disc_id: Option<String>,
    pub comment: Option<String>,
    pub track_count: i64,
    pub total_ms: i64,
    pub content: String,
    /// Tracks expanded from this sheet, in sheet order.
    pub tracks: Vec<CueTrackInfo>,
}

/// What a batch did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchOutcome {
    pub tracks_written: usize,
    pub albums_touched: usize,
    pub artists_touched: usize,
}

/// Column order of `tracks`. Single source of truth for both the `VALUES`
/// tuple and the `DO UPDATE SET` clause, so the two can never drift.
const TRACK_COLUMNS: [&str; 44] = [
    "id",
    "path",
    "folder",
    "filename",
    "source",
    "title",
    "title_sort",
    "artist_id",
    "artist_name",
    "album_id",
    "album_title",
    "album_artist_id",
    "album_artist_name",
    "composer",
    "genre",
    "comment",
    "year",
    "track_no",
    "disc_no",
    "track_total",
    "disc_total",
    "format",
    "container",
    "codec_profile",
    "bit_depth",
    "sample_rate",
    "channels",
    "bitrate_kbps",
    "dsd_rate",
    "duration_ms",
    "file_size",
    "replaygain_track_gain_db",
    "replaygain_track_peak",
    "replaygain_album_gain_db",
    "replaygain_album_peak",
    "musicbrainz_track_id",
    "musicbrainz_release_track_id",
    "isrc",
    "file_mtime",
    "cue_sheet_id",
    "cue_start_ms",
    "cue_end_ms",
    "has_embedded_art",
    "extra_tags",
];

/// `missing`, `indexed_at` and `modified_at` are not part of [`TRACK_COLUMNS`]
/// because they are literals, not bound values.
static TRACK_UPSERT_TAIL: LazyLock<String> = LazyLock::new(|| {
    let assignments = TRACK_COLUMNS
        .iter()
        .filter(|column| **column != "id")
        .map(|column| format!("{column} = excluded.{column}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{assignments}, missing = 0, \
         indexed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), \
         modified_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')"
    )
});

/// Write side of the catalogue, borrowing the shared pool.
#[derive(Debug, Clone)]
pub struct CatalogWriter {
    pool: SqlitePool,
}

impl CatalogWriter {
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Write one batch atomically: artists → albums → tracks → album totals.
    pub async fn write_batch(&self, batch: &[TrackRecord]) -> Result<BatchOutcome, PrismError> {
        if batch.is_empty() {
            return Ok(BatchOutcome::default());
        }

        let mut transaction = self.pool.begin().await?;

        let artists = ensure_artists(&mut transaction, batch).await?;
        let albums = ensure_albums(&mut transaction, batch).await?;
        upsert_tracks(&mut transaction, batch, &artists, &albums).await?;
        refresh_albums(&mut transaction, &albums).await?;
        link_album_artists(&mut transaction, &albums).await?;

        transaction.commit().await?;

        Ok(BatchOutcome {
            tracks_written: batch.len(),
            albums_touched: albums.len(),
            artists_touched: artists.len(),
        })
    }

    /// Insert or update a parsed CUE sheet and return its canonical row id.
    pub async fn upsert_cue_sheet(&self, sheet: &CueSheetRecord) -> Result<(), PrismError> {
        sqlx::query(
            "INSERT INTO cue_sheets (\
                id, path, album_image, sha256, catalog, performer, title, songwriter, genre, \
                date, disc_id, comment, track_count, total_ms, content\
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15) \
             ON CONFLICT(id) DO UPDATE SET \
                path = excluded.path, album_image = excluded.album_image, \
                sha256 = excluded.sha256, catalog = excluded.catalog, \
                performer = excluded.performer, title = excluded.title, \
                songwriter = excluded.songwriter, genre = excluded.genre, \
                date = excluded.date, disc_id = excluded.disc_id, \
                comment = excluded.comment, track_count = excluded.track_count, \
                total_ms = excluded.total_ms, content = excluded.content, \
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
        )
        .bind(&sheet.id)
        .bind(&sheet.path)
        .bind(&sheet.album_image)
        .bind(&sheet.sha256)
        .bind(&sheet.catalog)
        .bind(&sheet.performer)
        .bind(&sheet.title)
        .bind(&sheet.songwriter)
        .bind(&sheet.genre)
        .bind(&sheet.date)
        .bind(&sheet.disc_id)
        .bind(&sheet.comment)
        .bind(sheet.track_count)
        .bind(sheet.total_ms)
        .bind(&sheet.content)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Attach a CUE sheet to the album its tracks resolved to.
    pub async fn link_cue_album(
        &self,
        cue_id: &str,
        album_title: &str,
        album_artist: &str,
    ) -> Result<(), PrismError> {
        sqlx::query(
            "UPDATE cue_sheets SET album_id = (\
                 SELECT id FROM albums \
                 WHERE title = ?2 COLLATE NOCASE AND album_artist_name = ?3 COLLATE NOCASE\
             ) WHERE id = ?1",
        )
        .bind(cue_id)
        .bind(album_title)
        .bind(album_artist)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Delete a CUE sheet that disappeared from disk, cascading its virtual
    /// tracks (`ON DELETE CASCADE` on `tracks.cue_sheet_id`).
    pub async fn remove_cue_sheet(&self, id: &str) -> Result<u64, PrismError> {
        let result = sqlx::query("DELETE FROM cue_sheets WHERE id = ?1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }

    /// Load `(path, mtime, size)` for every indexed file under the given roots.
    ///
    /// Used to skip unchanged files before parsing them. The query is served by
    /// the identity index and never touches the heap for narrow rows.
    pub async fn fingerprint_index(
        &self,
        roots: &[String],
    ) -> Result<Vec<FileFingerprint>, PrismError> {
        let mut out = Vec::new();
        for chunk in roots.chunks(64) {
            let mut query = QueryBuilder::<Sqlite>::new(
                "SELECT path, file_mtime, file_size FROM tracks WHERE source = 'file' AND (",
            );
            for (index, root) in chunk.iter().enumerate() {
                if index > 0 {
                    query.push(" OR ");
                }
                query
                    .push("path LIKE ")
                    .push_bind(format!("{}%", escape_like(root)));
                query.push(" ESCAPE '\\'");
            }
            query.push(")");
            let rows = query
                .build_query_as::<(String, i64, i64)>()
                .fetch_all(&self.pool)
                .await?;
            out.extend(rows.into_iter().map(|(path, mtime, size)| FileFingerprint {
                key: path_key(&path),
                mtime,
                size,
            }));
        }
        Ok(out)
    }

    /// Digest of every stored CUE sheet, keyed by canonical sheet path.
    ///
    /// A sheet is re-expanded only when its bytes change; the SHA-256 was
    /// already computed for the sheet row, so the incremental check costs one
    /// narrow table scan instead of re-parsing every `.cue` in the library.
    pub async fn cue_digests(
        &self,
    ) -> Result<std::collections::HashMap<String, String>, PrismError> {
        let rows: Vec<(String, String)> = sqlx::query_as("SELECT path, sha256 FROM cue_sheets")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(|(path, digest)| (path_key(&path), digest))
            .collect())
    }

    /// Mark rows whose files were not seen in this scan as missing.
    ///
    /// `seen` holds canonical path keys. The diff happens inside `SQLite`
    /// through a temp table, so memory stays proportional to the page cache
    /// rather than to the library size.
    pub async fn mark_missing(&self, roots: &[String], seen: &[String]) -> Result<u64, PrismError> {
        if roots.is_empty() {
            return Ok(0);
        }
        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            "CREATE TEMP TABLE IF NOT EXISTS scan_seen (path TEXT PRIMARY KEY) WITHOUT ROWID",
        )
        .execute(&mut *transaction)
        .await?;
        sqlx::query("DELETE FROM scan_seen")
            .execute(&mut *transaction)
            .await?;

        for chunk in seen.chunks(500) {
            let mut insert = QueryBuilder::<Sqlite>::new("INSERT OR IGNORE INTO scan_seen (path) ");
            insert.push_values(chunk, |mut row, path| {
                row.push_bind(path);
            });
            insert.build().execute(&mut *transaction).await?;
        }

        let mut update = QueryBuilder::<Sqlite>::new(
            "UPDATE tracks SET missing = 1, indexed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
             WHERE missing = 0 AND (",
        );
        for (index, root) in roots.iter().enumerate() {
            if index > 0 {
                update.push(" OR ");
            }
            update
                .push("path LIKE ")
                .push_bind(format!("{}%", escape_like(root)))
                .push(" ESCAPE '\\'");
        }
        update.push(
            ") AND lower(replace(replace(path, '\\', '/'), '//', '/')) \
             NOT IN (SELECT path FROM scan_seen)",
        );
        let result = update.build().execute(&mut *transaction).await?;

        // Albums that just lost a track must not keep advertising it.
        sqlx::query(
            "UPDATE albums SET \
                track_count = (SELECT COUNT(*) FROM tracks WHERE album_id = albums.id AND missing = 0), \
                duration_ms = (SELECT COALESCE(SUM(duration_ms), 0) FROM tracks \
                                WHERE album_id = albums.id AND missing = 0), \
                updated_at  = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
             WHERE id IN (SELECT DISTINCT album_id FROM tracks \
                          WHERE missing = 1 AND album_id IS NOT NULL)",
        )
        .execute(&mut *transaction)
        .await?;

        sqlx::query("DROP TABLE IF EXISTS scan_seen")
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(result.rows_affected())
    }

    /// Recompute the denormalised counters of every album.
    ///
    /// Run once at the end of a *full* scan: per-batch refreshes keep touched
    /// albums correct, but only a full sweep repairs albums whose last track
    /// was deleted while the app was closed.
    pub async fn reconcile_albums(&self) -> Result<u64, PrismError> {
        let result = sqlx::query(
            "UPDATE albums SET \
                track_count = (SELECT COUNT(*) FROM tracks WHERE album_id = albums.id AND missing = 0), \
                duration_ms = (SELECT COALESCE(SUM(duration_ms), 0) FROM tracks \
                                WHERE album_id = albums.id AND missing = 0), \
                disc_count  = MAX(1, (SELECT COALESCE(MAX(disc_no), 1) FROM tracks \
                                WHERE album_id = albums.id)), \
                updated_at  = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
        )
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    /// Drop album and artist rows that no longer own any track.
    ///
    /// Only called on an explicit "prune" full scan, never automatically:
    /// keeping orphans preserves the user's playlists and history.
    /// Remove every track that lives under `root`.
    ///
    /// Removing a library folder is a statement about the collection, not about
    /// a list: the files are no longer scanned, so their rows must stop being
    /// part of the library the moment the folder is removed. Doing it lazily
    /// (on the next scan) would leave a hole in the catalogue that only a full
    /// rescan could explain, so the purge happens inside the same call that
    /// removes the root.
    ///
    /// Matching is done on the stored path and folder strings with an explicit
    /// separator, so `/music` never matches `/music-archive`. CUE sheets are
    /// removed with their images, and orphaned albums/artists are pruned after.
    pub async fn purge_root(&self, root: &str) -> Result<PurgeOutcome, PrismError> {
        let base = root.trim_end_matches(['/', '\\']);
        let separator = if base.contains('\\') { '\\' } else { '/' };
        let prefix = format!("{base}{separator}%");

        let mut transaction = self.pool.begin().await?;
        let tracks = sqlx::query(
            "DELETE FROM tracks WHERE path = ?1 OR path LIKE ?2 ESCAPE '\\' \
             OR folder = ?1 OR folder LIKE ?2 ESCAPE '\\'",
        )
        .bind(base)
        .bind(&prefix)
        .execute(&mut *transaction)
        .await?
        .rows_affected();

        let sheets =
            sqlx::query("DELETE FROM cue_sheets WHERE path = ?1 OR path LIKE ?2 ESCAPE '\\'")
                .bind(base)
                .bind(&prefix)
                .execute(&mut *transaction)
                .await?
                .rows_affected();

        // Orphans created by the purge, resolved in the same transaction so the
        // catalogue is never observably inconsistent between two statements.
        let albums = sqlx::query(
            "DELETE FROM albums WHERE NOT EXISTS (SELECT 1 FROM tracks WHERE album_id = albums.id)",
        )
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        let artists = sqlx::query(
            "DELETE FROM artists WHERE NOT EXISTS (SELECT 1 FROM tracks WHERE artist_id = artists.id) \
                AND NOT EXISTS (SELECT 1 FROM tracks WHERE album_artist_id = artists.id) \
                AND NOT EXISTS (SELECT 1 FROM albums WHERE artist_id = artists.id) \
                AND NOT EXISTS (SELECT 1 FROM albums WHERE album_artist_id = artists.id)",
        )
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        transaction.commit().await?;

        Ok(PurgeOutcome {
            tracks,
            cue_sheets: sheets,
            albums,
            artists,
        })
    }

    /// Albums still waiting for cover extraction, in identifier order.
    ///
    /// Returns the album identity plus the first playable track, which is what
    /// the artwork resolver needs to find embedded or folder artwork. The limit
    /// keeps memory bounded, and `after` is a keyset cursor: an album whose
    /// artwork cannot be resolved keeps its `cover_id` null by design, so an
    /// offset-based sweep would either re-examine it forever or stop early the
    /// moment a page happened to contain only such albums.
    pub async fn albums_without_cover(
        &self,
        limit: u32,
        after: Option<&str>,
    ) -> Result<Vec<CoverCandidate>, PrismError> {
        let rows = sqlx::query(
            "SELECT al.id, al.title, al.album_artist_name, \
                    (SELECT t.path FROM tracks t \
                      WHERE t.album_id = al.id AND t.missing = 0 AND t.source = 'file' \
                      ORDER BY t.disc_no ASC, t.track_no ASC LIMIT 1) AS track_path \
             FROM albums al \
             WHERE al.cover_id IS NULL AND (?1 IS NULL OR al.id > ?1) \
             ORDER BY al.id ASC LIMIT ?2",
        )
        .bind(after)
        .bind(i64::from(limit.clamp(1, 500)))
        .fetch_all(&self.pool)
        .await?;

        let mut out = Vec::with_capacity(rows.len());
        for row in &rows {
            out.push(CoverCandidate {
                album_id: row.try_get("id")?,
                album: row.try_get("title")?,
                artist: row.try_get("album_artist_name")?,
                track_path: row.try_get("track_path")?,
            });
        }
        Ok(out)
    }

    /// Attach a resolved cover to an album.
    pub async fn attach_cover(&self, album_id: &str, cover_id: &str) -> Result<(), PrismError> {
        sqlx::query("UPDATE albums SET cover_id = ?2 WHERE id = ?1")
            .bind(album_id)
            .bind(cover_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Record that a track started playing.
    ///
    /// The counters live on `tracks` (so the album view can aggregate them) and
    /// the event itself lives in `play_history` (so the history is auditable and
    /// a future stats view does not need a schema change). Both writes are one
    /// transaction: a play is either recorded or it is not.
    pub async fn note_play(
        &self,
        track_id: &str,
        ms_played: i64,
        completed: bool,
    ) -> Result<bool, PrismError> {
        let mut transaction = self.pool.begin().await?;
        let updated = sqlx::query(
            "UPDATE tracks SET play_count = play_count + 1, \
                    last_played_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') \
             WHERE id = ?1",
        )
        .bind(track_id)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if updated == 0 {
            return Ok(false);
        }
        sqlx::query(
            "INSERT INTO play_history (id, track_id, ms_played, completed, source) \
             VALUES (?1, ?2, ?3, ?4, 'queue')",
        )
        .bind(stable_id("ph", &format!("{track_id}:{}", now_nanos())))
        .bind(track_id)
        .bind(ms_played.max(0))
        .bind(i64::from(u8::from(completed)))
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(true)
    }

    pub async fn prune_orphans(&self) -> Result<(u64, u64), PrismError> {
        let mut transaction = self.pool.begin().await?;
        let albums = sqlx::query(
            "DELETE FROM albums WHERE NOT EXISTS (SELECT 1 FROM tracks WHERE album_id = albums.id)",
        )
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        let artists = sqlx::query(
            "DELETE FROM artists WHERE NOT EXISTS (SELECT 1 FROM tracks WHERE artist_id = artists.id) \
                AND NOT EXISTS (SELECT 1 FROM tracks WHERE album_artist_id = artists.id) \
                AND NOT EXISTS (SELECT 1 FROM albums WHERE artist_id = artists.id) \
                AND NOT EXISTS (SELECT 1 FROM albums WHERE album_artist_id = artists.id)",
        )
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        transaction.commit().await?;
        Ok((albums, artists))
    }
}

/// Result of removing a library root from the catalogue.
#[derive(Debug, Clone, Copy, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PurgeOutcome {
    pub tracks: u64,
    pub cue_sheets: u64,
    pub albums: u64,
    pub artists: u64,
}

impl PurgeOutcome {
    /// True when the purge changed the catalogue at all.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.tracks == 0 && self.cue_sheets == 0 && self.albums == 0 && self.artists == 0
    }
}

/// One album waiting for cover extraction.
#[derive(Debug, Clone)]
pub struct CoverCandidate {
    pub album_id: String,
    pub album: String,
    pub artist: String,
    pub track_path: Option<String>,
}

/// Monotonic-enough unique suffix for a history row.
fn now_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
}

/// Stored mtime/size pair used for incremental scans.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileFingerprint {
    pub key: String,
    pub mtime: i64,
    pub size: i64,
}

pub(crate) fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

/// Insert (or find) every artist referenced by the batch.
async fn ensure_artists(
    transaction: &mut Transaction<'_, Sqlite>,
    batch: &[TrackRecord],
) -> Result<Vec<(String, String)>, PrismError> {
    let mut wanted: Vec<(String, String)> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for track in batch {
        for name in [&track.artist_name, &track.album_artist_name] {
            let trimmed = name.trim();
            if trimmed.is_empty() {
                continue;
            }
            let key = artist_key(trimmed);
            if seen.insert(key) {
                wanted.push((trimmed.to_string(), String::new()));
            }
        }
    }
    if wanted.is_empty() {
        return Ok(Vec::new());
    }

    for chunk in wanted.chunks(200) {
        let mut query =
            QueryBuilder::<Sqlite>::new("INSERT OR IGNORE INTO artists (id, name, sort_name) ");
        query.push_values(chunk.iter(), |mut row, (name, _)| {
            row.push_bind(stable_id("ar", &artist_key(name)));
            row.push_bind(clamp(name, 512));
            row.push_bind(crate::metadata::sort_key(name));
        });
        query.build().execute(&mut **transaction).await?;
    }

    // Read back the canonical ids: an artist may already exist under the same
    // name but a different id (imported database), and the FK must point at it.
    let mut resolved = Vec::with_capacity(wanted.len());
    for chunk in wanted.chunks(200) {
        let mut query = QueryBuilder::<Sqlite>::new("SELECT id, name FROM artists WHERE name IN (");
        let mut separated = query.separated(", ");
        for (name, _) in chunk {
            separated.push_bind(clamp(name, 512));
        }
        separated.push_unseparated(")");
        let rows = query
            .build_query_as::<(String, String)>()
            .fetch_all(&mut **transaction)
            .await?;
        resolved.extend(rows);
    }
    Ok(resolved)
}

/// Insert (or find) every album referenced by the batch.
async fn ensure_albums(
    transaction: &mut Transaction<'_, Sqlite>,
    batch: &[TrackRecord],
) -> Result<Vec<(String, String, String)>, PrismError> {
    // (id, title, album_artist_name)
    let mut wanted: Vec<(String, String, String)> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut meta_by_key: std::collections::HashMap<String, AlbumMeta> =
        std::collections::HashMap::new();

    for track in batch {
        if track.album_title.trim().is_empty() {
            continue;
        }
        let key = album_key(&track.album_title, &track.album_artist_name);
        // Prefer the richest metadata variant: a disc 2 row often repeats the
        // album without the MusicBrainz ids the disc 1 row carries.
        let entry = meta_by_key.entry(key.clone()).or_default();
        merge_meta(entry, &track.album);
        if seen.insert(key.clone()) {
            wanted.push((
                stable_id("al", &key),
                clamp(&track.album_title, 1024),
                clamp(&track.album_artist_name, 512),
            ));
        }
    }
    if wanted.is_empty() {
        return Ok(Vec::new());
    }

    for chunk in wanted.chunks(150) {
        let mut query = QueryBuilder::<Sqlite>::new(
            "INSERT OR IGNORE INTO albums (\
                id, title, sort_title, album_artist_name, year, original_year, release_date, \
                release_type, is_compilation, label, catalog_number, barcode, \
                musicbrainz_release_id, musicbrainz_release_group_id\
             ) ",
        );
        query.push_values(chunk.iter(), |mut row, (id, title, album_artist)| {
            let meta = meta_by_key
                .get(&album_key(title, album_artist))
                .cloned()
                .unwrap_or_default();
            row.push_bind(id.clone());
            row.push_bind(title.clone());
            row.push_bind(crate::metadata::sort_key(title));
            row.push_bind(album_artist.clone());
            row.push_bind(meta.year);
            row.push_bind(meta.original_year);
            row.push_bind(meta.release_date);
            row.push_bind(meta.release_type);
            row.push_bind(i64::from(meta.is_compilation));
            row.push_bind(meta.label);
            row.push_bind(meta.catalog_number);
            row.push_bind(meta.barcode);
            row.push_bind(meta.musicbrainz_release_id);
            row.push_bind(meta.musicbrainz_release_group_id);
        });
        query.build().execute(&mut **transaction).await?;
    }

    let mut resolved = Vec::with_capacity(wanted.len());
    for chunk in wanted.chunks(100) {
        let mut query =
            QueryBuilder::<Sqlite>::new("SELECT id, title, album_artist_name FROM albums WHERE ");
        let mut separated = query.separated(" OR ");
        for (_, title, album_artist) in chunk {
            separated.push("(title = ");
            separated.push_bind_unseparated(title.clone());
            separated.push_unseparated(" COLLATE NOCASE AND album_artist_name = ");
            separated.push_bind_unseparated(album_artist.clone());
            separated.push_unseparated(" COLLATE NOCASE)");
        }
        let rows = query
            .build_query_as::<(String, String, String)>()
            .fetch_all(&mut **transaction)
            .await?;
        resolved.extend(rows);
    }
    Ok(resolved)
}

/// Multi-row upsert of the batch, resolving artist/album foreign keys.
async fn upsert_tracks(
    transaction: &mut Transaction<'_, Sqlite>,
    batch: &[TrackRecord],
    artists: &[(String, String)],
    albums: &[(String, String, String)],
) -> Result<(), PrismError> {
    let artist_ids: std::collections::HashMap<String, &str> = artists
        .iter()
        .map(|(id, name)| (artist_key(name), id.as_str()))
        .collect();
    let album_ids: std::collections::HashMap<String, &str> = albums
        .iter()
        .map(|(id, title, album_artist)| (album_key(title, album_artist), id.as_str()))
        .collect();

    let mut query = QueryBuilder::<Sqlite>::new("INSERT INTO tracks (");
    query.push(TRACK_COLUMNS.join(", "));
    query.push(") ");
    query.push_values(batch.iter(), |mut row, track| {
        let artist_id = artist_ids.get(&artist_key(&track.artist_name)).copied();
        let album_artist_id = artist_ids
            .get(&artist_key(&track.album_artist_name))
            .copied();
        let album_id = album_ids
            .get(&album_key(&track.album_title, &track.album_artist_name))
            .copied();

        row.push_bind(&track.id);
        row.push_bind(&track.path);
        row.push_bind(&track.folder);
        row.push_bind(&track.filename);
        row.push_bind(track.source.as_str());
        row.push_bind(&track.title);
        row.push_bind(&track.title_sort);
        row.push_bind(artist_id);
        row.push_bind(&track.artist_name);
        row.push_bind(album_id);
        row.push_bind(&track.album_title);
        row.push_bind(album_artist_id);
        row.push_bind(&track.album_artist_name);
        row.push_bind(&track.composer);
        row.push_bind(&track.genre);
        row.push_bind(&track.comment);
        row.push_bind(track.year);
        row.push_bind(track.track_no);
        row.push_bind(track.disc_no);
        row.push_bind(track.track_total);
        row.push_bind(track.disc_total);
        row.push_bind(&track.format);
        row.push_bind(track.container.as_deref());
        row.push_bind(track.codec_profile.as_deref());
        row.push_bind(track.bit_depth);
        row.push_bind(track.sample_rate);
        row.push_bind(track.channels);
        row.push_bind(track.bitrate_kbps);
        row.push_bind(track.dsd_rate);
        row.push_bind(track.duration_ms);
        row.push_bind(track.file_size);
        row.push_bind(track.replaygain_track_gain_db);
        row.push_bind(track.replaygain_track_peak);
        row.push_bind(track.replaygain_album_gain_db);
        row.push_bind(track.replaygain_album_peak);
        row.push_bind(track.musicbrainz_track_id.as_deref());
        row.push_bind(track.musicbrainz_release_track_id.as_deref());
        row.push_bind(track.isrc.as_deref());
        row.push_bind(track.file_mtime);
        row.push_bind(track.cue_sheet_id.as_deref());
        row.push_bind(track.cue_start_ms);
        row.push_bind(track.cue_end_ms);
        row.push_bind(i64::from(track.has_embedded_art));
        row.push_bind(&track.extra_tags);
    });
    query.push(" ON CONFLICT(id) DO UPDATE SET ");
    query.push(TRACK_UPSERT_TAIL.as_str());
    query.build().execute(&mut **transaction).await?;
    Ok(())
}

/// Point every album at the artist rows that own it.
///
/// `ensure_albums` inserts a record by title and album-artist *name*, without
/// foreign keys — the artist rows exist by then, but the album has no way to
/// know their ids. Without this statement `albums.album_artist_id` stayed null
/// for the whole catalogue, so `v_album_summary.artist_id` was null too and no
/// album tile could link to its artist at all.
async fn link_album_artists(
    transaction: &mut Transaction<'_, Sqlite>,
    albums: &[(String, String, String)],
) -> Result<(), PrismError> {
    for chunk in albums.chunks(100) {
        let mut query = QueryBuilder::<Sqlite>::new(
            "UPDATE albums SET \
                album_artist_id = COALESCE(album_artist_id, \
                    (SELECT ar.id FROM artists ar \
                      WHERE ar.name = albums.album_artist_name COLLATE NOCASE LIMIT 1)), \
                artist_id = COALESCE(artist_id, album_artist_id, \
                    (SELECT ar.id FROM artists ar \
                      WHERE ar.name = albums.album_artist_name COLLATE NOCASE LIMIT 1)) \
             WHERE id IN (",
        );
        let mut separated = query.separated(", ");
        for (id, _, _) in chunk {
            separated.push_bind(id.clone());
        }
        separated.push_unseparated(")");
        query.build().execute(&mut **transaction).await?;
    }
    Ok(())
}

/// Refresh `track_count` / `duration_ms` / `disc_count` for touched albums.
async fn refresh_albums(
    transaction: &mut Transaction<'_, Sqlite>,
    albums: &[(String, String, String)],
) -> Result<(), PrismError> {
    for chunk in albums.chunks(100) {
        let mut query = QueryBuilder::<Sqlite>::new(
            "UPDATE albums SET \
                track_count = (SELECT COUNT(*) FROM tracks WHERE album_id = albums.id AND missing = 0), \
                duration_ms = (SELECT COALESCE(SUM(duration_ms), 0) FROM tracks \
                                WHERE album_id = albums.id AND missing = 0), \
                disc_count  = MAX(1, (SELECT COALESCE(MAX(disc_no), 1) FROM tracks \
                                WHERE album_id = albums.id)), \
                updated_at  = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), \
                artist_id   = COALESCE(artist_id, album_artist_id) \
             WHERE id IN (",
        );
        let mut separated = query.separated(", ");
        for (id, _, _) in chunk {
            separated.push_bind(id.clone());
        }
        separated.push_unseparated(")");
        query.build().execute(&mut **transaction).await?;
    }
    Ok(())
}

/// Fill the empty fields of `target` with the non-empty fields of `source`.
fn merge_meta(target: &mut AlbumMeta, source: &AlbumMeta) {
    macro_rules! fill {
        ($($field:ident),+ $(,)?) => {
            $(if target.$field.is_none() {
                target.$field = source.$field.clone();
            })+
        };
    }
    fill!(
        year,
        original_year,
        release_date,
        release_type,
        label,
        catalog_number,
        barcode,
        musicbrainz_release_id,
        musicbrainz_release_group_id,
    );
    target.is_compilation |= source.is_compilation;
}

fn clamp(value: &str, max: usize) -> String {
    let trimmed = value.trim();
    if trimmed.chars().count() <= max {
        return trimmed.to_string();
    }
    trimmed.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_stable_and_prefixed() {
        let first = stable_id("ar", "radiohead");
        let second = stable_id("ar", "radiohead");
        assert_eq!(first, second);
        assert!(first.starts_with("ar:"));
        assert_eq!(first.len(), 3 + 24);
        assert_ne!(first, stable_id("al", "radiohead"));
        assert_ne!(first, stable_id("ar", "Radiohead"));
    }

    #[test]
    fn ids_are_hex_and_collision_resistant() {
        let a = stable_id("tr", "/music/a.flac|file|-1");
        let b = stable_id("tr", "/music/b.flac|file|-1");
        assert_ne!(a, b);
        assert!(a[3..].chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn path_keys_match_the_sql_identity_expression() {
        assert_eq!(
            path_key(r"C:\Music\Album\Track.flac"),
            "c:/music/album/track.flac"
        );
        assert_eq!(
            path_key("//music//album///track.flac"),
            "/music/album/track.flac"
        );
        assert_eq!(
            path_key("/Music/Beyoncé/01.flac"),
            path_key("/music/beyoncé/01.flac")
        );
        assert_eq!(file_track_key("/a/b.flac"), "/a/b.flac|file|-1");
        assert_eq!(cue_track_key("/a/b.cue", 1_234), "/a/b.cue|cue|1234");
    }

    #[test]
    fn album_keys_fold_case_and_trim() {
        assert_eq!(album_key(" Kid A ", "Radiohead"), "kid a|radiohead");
        assert_eq!(
            album_key("kid a", "RADIOHEAD"),
            album_key("Kid A", "Radiohead")
        );
        assert_ne!(
            album_key("Kid A", "Radiohead"),
            album_key("Kid A", "Portishead")
        );
    }

    #[test]
    fn like_patterns_escape_wildcards() {
        assert_eq!(
            escape_like(r"C:\Music\100%_hits"),
            r"C:\\Music\\100\%\_hits"
        );
    }

    #[test]
    fn clamping_never_splits_a_unicode_scalar() {
        assert_eq!(clamp("  Kid A  ", 16), "Kid A");
        assert_eq!(clamp("ééééé", 3), "ééé");
    }

    #[test]
    fn upsert_tail_updates_every_column_except_the_key() {
        let tail = TRACK_UPSERT_TAIL.as_str();
        assert!(!tail.contains("id = excluded.id"));
        assert!(tail.contains("title = excluded.title"));
        assert!(tail.contains("extra_tags = excluded.extra_tags"));
        assert!(tail.contains("missing = 0"));
        assert!(tail.contains("modified_at = strftime"));
    }
}
