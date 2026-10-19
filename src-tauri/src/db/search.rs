//! Read side of the library: browsing, filtering and FTS5 search.
//!
//! ## Query strategy
//!
//! * **Text search** joins `tracks_fts` (external content, `unicode61
//!   remove_diacritics 2`) with `tracks` on `rowid`. Ranking uses `bm25` with
//!   per-column weights that mirror what a listener means by "relevance":
//!   a title hit outweighs a comment hit by 10:1.
//! * **Browse** never touches the FTS index: it walks the covering indexes
//!   declared in migration 0001 (`ix_tracks_title_sort`, `ix_tracks_album`, …)
//!   so scrolling a 500k library stays in the millisecond range.
//! * **Every sort is whitelisted.** The `ORDER BY` fragment comes from a closed
//!   enum, never from user input, so no binding can reach the SQL grammar.
//! * **Pagination is offset-based** because the UI virtualizer needs random
//!   access to row *n*; the tie-breaker on the primary key keeps pages stable
//!   when two rows share a sort value.
//!
//! Highlighting uses the markers `U+0001` / `U+0002` instead of HTML so the
//! React layer can render them through JSX without `dangerouslySetInnerHTML`.

use serde::{Deserialize, Serialize};
use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool};

use crate::error::PrismError;
use crate::metadata::quality_label;

use super::catalog::escape_like;

/// Maximum rows a single page may request. The UI virtualizer asks for 200 at
/// a time; anything larger is a bug or an attempt to DoS the backend.
pub const MAX_PAGE_SIZE: u32 = 500;

/// FTS5 highlight markers, stripped by the TypeScript formatter.
pub const HIGHLIGHT_OPEN: char = '\u{1}';
/// Closing highlight marker.
pub const HIGHLIGHT_CLOSE: char = '\u{2}';

/// Sort orders exposed to the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum TrackSort {
    /// `bm25` ranking; falls back to title order when there is no query text.
    #[default]
    Relevance,
    Title,
    Artist,
    Album,
    Year,
    Added,
    Duration,
    Bitrate,
}

impl TrackSort {
    /// `ORDER BY` fragment. Closed set: no user input ever reaches this string.
    const fn order_by(self, has_text: bool) -> &'static str {
        match self {
            Self::Relevance if has_text => {
                "bm25(tracks_fts, 10.0, 6.0, 4.0, 3.0, 2.0, 1.0, 1.0) ASC, t.title_sort COLLATE NOCASE ASC, t.id ASC"
            }
            Self::Relevance | Self::Title => "t.title_sort COLLATE NOCASE ASC, t.id ASC",
            Self::Artist => {
                "t.artist_name COLLATE NOCASE ASC, t.album_title COLLATE NOCASE ASC, \
                 t.disc_no ASC, t.track_no ASC, t.id ASC"
            }
            Self::Album => {
                "t.album_title COLLATE NOCASE ASC, t.disc_no ASC, t.track_no ASC, t.id ASC"
            }
            Self::Year => {
                "t.year DESC, t.album_title COLLATE NOCASE ASC, t.disc_no ASC, t.track_no ASC, t.id ASC"
            }
            Self::Added => "t.added_at DESC, t.id ASC",
            Self::Duration => "t.duration_ms DESC, t.id ASC",
            Self::Bitrate => "t.sample_rate DESC, t.bit_depth DESC, t.id ASC",
        }
    }
}

/// One row of the track list. Field names and casing match
/// `TrackRowSchema` in `src/types/ipc.ts`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackRow {
    pub id: String,
    pub path: String,
    pub folder: String,
    pub filename: String,
    pub source: String,
    pub title: String,
    pub artist_id: Option<String>,
    pub artist: String,
    pub album_id: Option<String>,
    pub album: String,
    pub album_artist: String,
    pub composer: String,
    pub genre: String,
    pub year: Option<i64>,
    pub track_no: Option<i64>,
    pub disc_no: Option<i64>,
    pub format: String,
    pub codec_profile: Option<String>,
    pub bit_depth: Option<i64>,
    pub sample_rate: Option<i64>,
    pub channels: Option<i64>,
    pub bitrate_kbps: Option<i64>,
    pub dsd_rate: Option<i64>,
    pub duration_ms: i64,
    pub quality: String,
    pub lossless: bool,
    pub has_embedded_art: bool,
    pub missing: bool,
    pub rating: i64,
    pub play_count: i64,
    pub added_at: String,
    pub cue_start_ms: Option<i64>,
    pub cue_end_ms: Option<i64>,
    pub cover_id: Option<String>,
    pub cover_path: Option<String>,
    pub cover_url: Option<String>,
    /// FTS5 snippet with [`HIGHLIGHT_OPEN`]/[`HIGHLIGHT_CLOSE`] markers.
    pub highlight: Option<String>,
}

/// One row of the album grid.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumRow {
    pub id: String,
    pub title: String,
    pub sort_title: String,
    pub year: Option<i64>,
    pub album_artist: String,
    pub is_compilation: bool,
    pub disc_count: i64,
    pub track_count: i64,
    pub duration_ms: i64,
    pub release_type: Option<String>,
    pub cover_id: Option<String>,
    pub cover_path: Option<String>,
    pub cover_url: Option<String>,
    pub available_tracks: i64,
    pub max_sample_rate: Option<i64>,
    pub max_bit_depth: Option<i64>,
    pub format: Option<String>,
    pub quality: String,
}

/// One row of the artist list.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtistRow {
    pub id: String,
    pub name: String,
    pub sort_name: String,
    pub album_count: i64,
    pub track_count: i64,
    pub total_duration_ms: i64,
    /// First album cover, used as the artist tile.
    pub cover_id: Option<String>,
    pub cover_path: Option<String>,
}

/// Generic page envelope.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page<T> {
    pub items: Vec<T>,
    pub total: i64,
    pub limit: u32,
    pub offset: u32,
}

/// Track browser query. Every field is optional; the defaults mean "everything,
/// newest first" which is what an empty library view shows.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TrackQuery {
    /// Free text. Empty/whitespace disables FTS and switches to a pure browse.
    pub text: Option<String>,
    pub artist_id: Option<String>,
    pub album_id: Option<String>,
    pub genre: Option<String>,
    pub format: Option<String>,
    pub year_from: Option<i64>,
    pub year_to: Option<i64>,
    pub min_sample_rate: Option<i64>,
    pub min_bit_depth: Option<i64>,
    pub lossless_only: bool,
    pub include_missing: bool,
    pub sort: TrackSort,
    pub limit: u32,
    pub offset: u32,
}

impl TrackQuery {
    /// Clamp a hostile page request into the supported window.
    #[must_use]
    pub fn normalised(mut self) -> Self {
        self.limit = self.limit.clamp(1, MAX_PAGE_SIZE);
        self.offset = self.offset.min(5_000_000);
        if self
            .text
            .as_deref()
            .is_some_and(|text| text.trim().is_empty())
        {
            self.text = None;
        }
        self
    }

    fn has_text(&self) -> bool {
        self.text
            .as_deref()
            .is_some_and(|text| !text.trim().is_empty())
    }
}

/// Convert user input into a safe FTS5 `MATCH` expression.
///
/// FTS5 treats `-`, `:`, `*`, `"`, `(`, `)`, `^`, `NEAR` and `OR`/`AND`/`NOT`
/// as operators; feeding raw input to `MATCH` is both a correctness bug
/// (`C++` parses as a syntax error) and an injection surface. Here every token
/// is extracted as a run of alphanumeric characters and turned into a quoted
/// phrase; the final token becomes a prefix query so incremental typing works
/// (`beeth` matches `Beethoven`).
///
/// Returns `None` when the input has no indexable token, in which case the
/// caller must fall back to a plain browse instead of querying an empty index.
#[must_use]
pub fn build_match_query(input: &str) -> Option<String> {
    let mut tokens: Vec<String> = Vec::new();
    let mut current = String::new();
    for character in input.chars() {
        if character.is_alphanumeric() {
            current.extend(character.to_lowercase());
        } else if !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }

    // Bound the expression: ten terms is already a very specific query and
    // keeps the FTS query planner from exploding on a pasted paragraph.
    tokens.truncate(10);
    if tokens.is_empty() {
        return None;
    }

    let last = tokens.len() - 1;
    let mut expression = String::new();
    for (index, token) in tokens.iter().enumerate() {
        if index > 0 {
            expression.push(' ');
        }
        // Quotes inside tokens are impossible after the filter above, but the
        // doubling keeps this function correct if the filter ever changes.
        expression.push('"');
        expression.push_str(&token.replace('"', "\"\""));
        if index == last && token.len() >= 2 {
            expression.push_str("\"*");
        } else {
            expression.push('"');
        }
    }
    Some(expression)
}

const TRACK_SELECT: &str = "SELECT \
    t.id, t.path, t.folder, t.filename, t.source, t.title, \
    t.artist_id, t.artist_name, t.album_id, t.album_title, t.album_artist_name, \
    t.composer, t.genre, t.year, t.track_no, t.disc_no, \
    t.format, t.codec_profile, t.bit_depth, t.sample_rate, t.channels, \
    t.bitrate_kbps, t.dsd_rate, t.duration_ms, t.has_embedded_art, t.missing, \
    t.rating, t.play_count, t.added_at, t.cue_start_ms, t.cue_end_ms, \
    al.cover_id, cc.local_path AS cover_local_path, cc.remote_url AS cover_url";

/// Search or browse tracks.
pub async fn tracks(pool: &SqlitePool, query: TrackQuery) -> Result<Page<TrackRow>, PrismError> {
    let query = query.normalised();
    let has_text = query.has_text();
    let from = if has_text {
        " FROM tracks_fts JOIN tracks AS t ON t.rowid = tracks_fts.rowid \
          LEFT JOIN albums AS al ON al.id = t.album_id \
          LEFT JOIN cover_cache AS cc ON cc.id = al.cover_id"
    } else {
        " FROM tracks AS t \
          LEFT JOIN albums AS al ON al.id = t.album_id \
          LEFT JOIN cover_cache AS cc ON cc.id = al.cover_id"
    };

    let total = count_tracks(pool, &query, from).await?;

    let mut builder = QueryBuilder::<Sqlite>::new(TRACK_SELECT);
    if has_text {
        builder.push(", snippet(tracks_fts, -1, char(1), char(2), '…', 12) AS highlight");
    } else {
        builder.push(", NULL AS highlight");
    }
    builder.push(from);
    push_filters(&mut builder, &query);
    builder.push(" ORDER BY ");
    // The fragment is a compile-time constant per sort order.
    builder.push(query.sort.order_by(has_text));
    builder
        .push(" LIMIT ")
        .push_bind(i64::from(query.limit))
        .push(" OFFSET ")
        .push_bind(i64::from(query.offset));

    let rows = builder.build().fetch_all(pool).await?;
    let items = rows
        .iter()
        .map(track_row_from_sql)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Page {
        items,
        total,
        limit: query.limit,
        offset: query.offset,
    })
}

async fn count_tracks(
    pool: &SqlitePool,
    query: &TrackQuery,
    from: &str,
) -> Result<i64, PrismError> {
    let mut builder = QueryBuilder::<Sqlite>::new("SELECT COUNT(*)");
    builder.push(from);
    push_filters(&mut builder, query);
    Ok(builder.build_query_scalar::<i64>().fetch_one(pool).await?)
}

/// Apply the shared `WHERE` clause. Assumes the table alias `t` is in scope.
fn push_filters(builder: &mut QueryBuilder<'_, Sqlite>, query: &TrackQuery) {
    builder.push(" WHERE 1 = 1");
    if let Some(text) = query.text.as_deref().and_then(build_match_query) {
        builder.push(" AND tracks_fts MATCH ").push_bind(text);
    }
    if !query.include_missing {
        builder.push(" AND t.missing = 0");
    }
    if let Some(artist_id) = &query.artist_id {
        builder
            .push(" AND (t.artist_id = ")
            .push_bind(artist_id.clone())
            .push(" OR t.album_artist_id = ")
            .push_bind(artist_id.clone())
            .push(")");
    }
    if let Some(album_id) = &query.album_id {
        builder
            .push(" AND t.album_id = ")
            .push_bind(album_id.clone());
    }
    if let Some(genre) = &query.genre {
        builder
            .push(" AND t.genre = ")
            .push_bind(genre.clone())
            .push(" COLLATE NOCASE");
    }
    if let Some(format) = &query.format {
        builder
            .push(" AND t.format = ")
            .push_bind(format.clone())
            .push(" COLLATE NOCASE");
    }
    if let Some(year) = query.year_from {
        builder.push(" AND t.year >= ").push_bind(year);
    }
    if let Some(year) = query.year_to {
        builder.push(" AND t.year <= ").push_bind(year);
    }
    if let Some(rate) = query.min_sample_rate {
        builder.push(" AND t.sample_rate >= ").push_bind(rate);
    }
    if let Some(depth) = query.min_bit_depth {
        builder.push(" AND t.bit_depth >= ").push_bind(depth);
    }
    if query.lossless_only {
        builder.push(
            " AND t.format IN ('flac','alac','wav','wave','aiff','aif','ape','wv','dsf','dff')",
        );
    }
}

fn track_row_from_sql(row: &sqlx::sqlite::SqliteRow) -> Result<TrackRow, PrismError> {
    let bit_depth: Option<i64> = row.try_get("bit_depth")?;
    let sample_rate: Option<i64> = row.try_get("sample_rate")?;
    let dsd_rate: Option<i64> = row.try_get("dsd_rate")?;
    let format: String = row.try_get("format")?;
    Ok(TrackRow {
        id: row.try_get("id")?,
        path: row.try_get("path")?,
        folder: row.try_get("folder")?,
        filename: row.try_get("filename")?,
        source: row.try_get("source")?,
        title: row.try_get("title")?,
        artist_id: row.try_get("artist_id")?,
        artist: row.try_get("artist_name")?,
        album_id: row.try_get("album_id")?,
        album: row.try_get("album_title")?,
        album_artist: row.try_get("album_artist_name")?,
        composer: row.try_get("composer")?,
        genre: row.try_get("genre")?,
        year: row.try_get("year")?,
        track_no: row.try_get("track_no")?,
        disc_no: row.try_get("disc_no")?,
        codec_profile: row.try_get("codec_profile")?,
        bit_depth,
        sample_rate,
        channels: row.try_get("channels")?,
        bitrate_kbps: row.try_get("bitrate_kbps")?,
        dsd_rate,
        duration_ms: row.try_get("duration_ms")?,
        quality: quality_label(bit_depth, sample_rate, dsd_rate, &format),
        lossless: is_lossless(&format),
        format,
        has_embedded_art: row.try_get::<i64, _>("has_embedded_art")? != 0,
        missing: row.try_get::<i64, _>("missing")? != 0,
        rating: row.try_get("rating")?,
        play_count: row.try_get("play_count")?,
        added_at: row.try_get("added_at")?,
        cue_start_ms: row.try_get("cue_start_ms")?,
        cue_end_ms: row.try_get("cue_end_ms")?,
        cover_id: row.try_get("cover_id")?,
        cover_path: row.try_get("cover_local_path")?,
        cover_url: row.try_get("cover_url")?,
        highlight: row.try_get("highlight")?,
    })
}

/// Container formats whose samples reach the DAC unchanged.
#[must_use]
pub fn is_lossless(format: &str) -> bool {
    matches!(
        format.to_ascii_lowercase().as_str(),
        "flac" | "alac" | "wav" | "wave" | "aiff" | "aif" | "ape" | "wv" | "dsf" | "dff"
    )
}

/// Browse the album grid.
pub async fn albums(
    pool: &SqlitePool,
    text: Option<&str>,
    artist_id: Option<&str>,
    sort: TrackSort,
    limit: u32,
    offset: u32,
) -> Result<Page<AlbumRow>, PrismError> {
    let limit = limit.clamp(1, MAX_PAGE_SIZE);
    let offset = offset.min(5_000_000);

    let mut count = QueryBuilder::<Sqlite>::new("SELECT COUNT(*) FROM albums WHERE 1 = 1");
    if let Some(text) = text.map(str::trim).filter(|text| !text.is_empty()) {
        count
            .push(" AND (title LIKE ")
            .push_bind(format!("%{}%", escape_like(text)))
            .push(" ESCAPE '\\' OR album_artist_name LIKE ")
            .push_bind(format!("%{}%", escape_like(text)))
            .push(" ESCAPE '\\')");
    }
    if let Some(artist_id) = artist_id {
        count
            .push(" AND (artist_id = ")
            .push_bind(artist_id.to_string())
            .push(" OR album_artist_id = ")
            .push_bind(artist_id.to_string())
            .push(")");
    }
    let total: i64 = count.build_query_scalar().fetch_one(pool).await?;

    let order = match sort {
        TrackSort::Year => "year DESC NULLS LAST, sort_title COLLATE NOCASE ASC",
        TrackSort::Added => "added_at DESC",
        TrackSort::Artist => "album_artist_name COLLATE NOCASE ASC, sort_title COLLATE NOCASE ASC",
        _ => "sort_title COLLATE NOCASE ASC",
    };

    let mut builder = QueryBuilder::<Sqlite>::new(
        "SELECT id, title, sort_title, year, album_artist_name, is_compilation, disc_count, \
                track_count, duration_ms, release_type, cover_id, cover_remote_url, \
                cover_local_path, available_tracks, max_sample_rate, max_bit_depth, format \
         FROM v_album_summary WHERE 1 = 1",
    );
    if let Some(text) = text.map(str::trim).filter(|text| !text.is_empty()) {
        builder
            .push(" AND (title LIKE ")
            .push_bind(format!("%{}%", escape_like(text)))
            .push(" ESCAPE '\\' OR album_artist_name LIKE ")
            .push_bind(format!("%{}%", escape_like(text)))
            .push(" ESCAPE '\\')");
    }
    if let Some(artist_id) = artist_id {
        builder
            .push(" AND id IN (SELECT id FROM albums WHERE artist_id = ")
            .push_bind(artist_id.to_string())
            .push(" OR album_artist_id = ")
            .push_bind(artist_id.to_string())
            .push(")");
    }
    builder
        .push(" ORDER BY ")
        .push(order)
        .push(", id ASC LIMIT ")
        .push_bind(i64::from(limit))
        .push(" OFFSET ")
        .push_bind(i64::from(offset));

    let rows = builder.build().fetch_all(pool).await?;
    let mut items = Vec::with_capacity(rows.len());
    for row in &rows {
        let max_sample_rate: Option<i64> = row.try_get("max_sample_rate")?;
        let max_bit_depth: Option<i64> = row.try_get("max_bit_depth")?;
        let format: Option<String> = row.try_get("format")?;
        items.push(AlbumRow {
            id: row.try_get("id")?,
            title: row.try_get("title")?,
            sort_title: row.try_get("sort_title")?,
            year: row.try_get("year")?,
            album_artist: row.try_get("album_artist_name")?,
            is_compilation: row.try_get::<i64, _>("is_compilation")? != 0,
            disc_count: row.try_get("disc_count")?,
            track_count: row.try_get("track_count")?,
            duration_ms: row.try_get("duration_ms")?,
            release_type: row.try_get("release_type")?,
            cover_id: row.try_get("cover_id")?,
            cover_path: row.try_get("cover_local_path")?,
            cover_url: row.try_get("cover_remote_url")?,
            available_tracks: row.try_get("available_tracks")?,
            quality: quality_label(
                max_bit_depth,
                max_sample_rate,
                None,
                format.as_deref().unwrap_or(""),
            ),
            max_sample_rate,
            max_bit_depth,
            format,
        });
    }

    Ok(Page {
        items,
        total,
        limit,
        offset,
    })
}

/// Browse artists with their aggregate counters.
pub async fn artists(
    pool: &SqlitePool,
    text: Option<&str>,
    limit: u32,
    offset: u32,
) -> Result<Page<ArtistRow>, PrismError> {
    let limit = limit.clamp(1, MAX_PAGE_SIZE);
    let offset = offset.min(5_000_000);

    let mut count = QueryBuilder::<Sqlite>::new("SELECT COUNT(*) FROM artists WHERE 1 = 1");
    if let Some(text) = text.map(str::trim).filter(|text| !text.is_empty()) {
        count
            .push(" AND name LIKE ")
            .push_bind(format!("%{}%", escape_like(text)))
            .push(" ESCAPE '\\'");
    }
    let total: i64 = count.build_query_scalar().fetch_one(pool).await?;

    let mut builder = QueryBuilder::<Sqlite>::new(
        "SELECT ar.id, ar.name, ar.sort_name, s.album_count, s.track_count, s.total_duration_ms, \
                (SELECT id FROM albums WHERE artist_id = ar.id OR album_artist_id = ar.id \
                 ORDER BY year ASC LIMIT 1) AS cover_album_id, \
                (SELECT cc.local_path FROM albums al JOIN cover_cache cc ON cc.id = al.cover_id \
                 WHERE al.artist_id = ar.id OR al.album_artist_id = ar.id LIMIT 1) AS cover_path \
         FROM artists AS ar JOIN v_artist_summary AS s ON s.id = ar.id WHERE 1 = 1",
    );
    if let Some(text) = text.map(str::trim).filter(|text| !text.is_empty()) {
        builder
            .push(" AND ar.name LIKE ")
            .push_bind(format!("%{}%", escape_like(text)))
            .push(" ESCAPE '\\'");
    }
    builder
        .push(" ORDER BY ar.sort_name COLLATE NOCASE ASC, ar.id ASC LIMIT ")
        .push_bind(i64::from(limit))
        .push(" OFFSET ")
        .push_bind(i64::from(offset));

    let rows = builder.build().fetch_all(pool).await?;
    let mut items = Vec::with_capacity(rows.len());
    for row in &rows {
        items.push(ArtistRow {
            id: row.try_get("id")?,
            name: row.try_get("name")?,
            sort_name: row.try_get("sort_name")?,
            album_count: row.try_get("album_count")?,
            track_count: row.try_get("track_count")?,
            total_duration_ms: row.try_get("total_duration_ms")?,
            cover_id: row.try_get("cover_album_id")?,
            cover_path: row.try_get("cover_path")?,
        });
    }

    Ok(Page {
        items,
        total,
        limit,
        offset,
    })
}

/// Everything the global search overlay needs in one round trip.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalSearch {
    pub query: String,
    pub match_expression: Option<String>,
    pub tracks: Page<TrackRow>,
    pub albums: Vec<AlbumRow>,
    pub artists: Vec<ArtistRow>,
    pub took_ms: u64,
}

/// One-shot search across tracks, albums and artists (⌘K overlay).
pub async fn global(
    pool: &SqlitePool,
    input: &str,
    limit: u32,
) -> Result<GlobalSearch, PrismError> {
    let started = std::time::Instant::now();
    let trimmed = input.trim();
    let limit = limit.clamp(1, 50);
    let expression = build_match_query(trimmed);

    let tracks = tracks(
        pool,
        TrackQuery {
            text: Some(trimmed.to_string()),
            limit: limit.saturating_mul(2).min(MAX_PAGE_SIZE),
            sort: TrackSort::Relevance,
            ..TrackQuery::default()
        },
    )
    .await?;
    let albums = albums(pool, Some(trimmed), None, TrackSort::Relevance, limit, 0)
        .await?
        .items;
    let artists = artists(pool, Some(trimmed), limit, 0).await?.items;

    Ok(GlobalSearch {
        query: trimmed.to_string(),
        match_expression: expression,
        tracks,
        albums,
        artists,
        took_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
    })
}

/// Aggregations for the filter rail.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Facet {
    pub value: String,
    pub count: i64,
}

/// Facet payload: genres, formats and decades present in the library.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Facets {
    pub genres: Vec<Facet>,
    pub formats: Vec<Facet>,
    pub decades: Vec<Facet>,
}

/// Compute the filter-rail facets in three indexed group-bys.
pub async fn facets(pool: &SqlitePool) -> Result<Facets, PrismError> {
    let genres = sqlx::query(
        "SELECT genre AS value, COUNT(*) AS count FROM tracks \
         WHERE missing = 0 AND trim(genre) <> '' \
         GROUP BY genre COLLATE NOCASE ORDER BY count DESC, value COLLATE NOCASE ASC LIMIT 64",
    )
    .fetch_all(pool)
    .await?
    .iter()
    .map(|row| {
        Ok(Facet {
            value: row.try_get("value")?,
            count: row.try_get("count")?,
        })
    })
    .collect::<Result<Vec<_>, sqlx::Error>>()?;

    let formats = sqlx::query(
        "SELECT format AS value, COUNT(*) AS count FROM tracks \
         WHERE missing = 0 GROUP BY format COLLATE NOCASE \
         ORDER BY count DESC, value COLLATE NOCASE ASC LIMIT 32",
    )
    .fetch_all(pool)
    .await?
    .iter()
    .map(|row| {
        Ok(Facet {
            value: row.try_get("value")?,
            count: row.try_get("count")?,
        })
    })
    .collect::<Result<Vec<_>, sqlx::Error>>()?;

    let decades = sqlx::query(
        "SELECT (year / 10) * 10 AS value, COUNT(*) AS count FROM tracks \
         WHERE missing = 0 AND year IS NOT NULL GROUP BY value ORDER BY value ASC",
    )
    .fetch_all(pool)
    .await?
    .iter()
    .map(|row| {
        Ok(Facet {
            value: format!("{}s", row.try_get::<i64, _>("value")?),
            count: row.try_get("count")?,
        })
    })
    .collect::<Result<Vec<_>, sqlx::Error>>()?;

    Ok(Facets {
        genres,
        formats,
        decades,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn match_expression_quotes_and_prefixes_the_last_token() {
        assert_eq!(build_match_query("kid a"), Some("\"kid\" \"a\"".into()));
        assert_eq!(
            build_match_query("  radiohead  "),
            Some("\"radiohead\"*".into())
        );
        assert_eq!(build_match_query("beeth"), Some("\"beeth\"*".into()));
    }

    #[test]
    fn match_expression_neutralises_fts_operators() {
        // Without sanitising, `C++` is a syntax error and `NEAR(...)` is a
        // query operator that can force a full index scan.
        assert_eq!(build_match_query("C++"), Some("\"c\"".into()));
        assert_eq!(build_match_query("AC/DC"), Some("\"ac\" \"dc\"*".into()));
        assert_eq!(
            build_match_query("OR NEAR(title, hacked)"),
            Some("\"or\" \"near\" \"title\" \"hacked\"*".into())
        );
        assert_eq!(build_match_query("\"*\" - : ^"), None);
        assert_eq!(build_match_query("   "), None);
    }

    #[test]
    fn match_expression_is_bounded_and_diacritic_friendly() {
        let long = "one two three four five six seven eight nine ten eleven twelve";
        let expression = build_match_query(long).expect("expression");
        assert_eq!(expression.matches('"').count(), 20);
        assert_eq!(build_match_query("Beyoncé"), Some("\"beyoncé\"*".into()));
    }

    #[test]
    fn page_requests_are_clamped() {
        let query = TrackQuery {
            limit: 10_000,
            offset: 0,
            text: Some("   ".into()),
            ..TrackQuery::default()
        }
        .normalised();
        assert_eq!(query.limit, MAX_PAGE_SIZE);
        assert_eq!(query.text, None);
    }

    #[test]
    fn lossless_detection_is_case_insensitive() {
        assert!(is_lossless("FLAC"));
        assert!(is_lossless("dsf"));
        assert!(!is_lossless("mp3"));
        assert!(!is_lossless("opus"));
    }

    #[test]
    fn sort_fragments_are_constant_and_safe() {
        for sort in [
            TrackSort::Relevance,
            TrackSort::Title,
            TrackSort::Artist,
            TrackSort::Album,
            TrackSort::Year,
            TrackSort::Added,
            TrackSort::Duration,
            TrackSort::Bitrate,
        ] {
            let fragment = sort.order_by(true);
            assert!(!fragment.contains(';'));
            assert!(!fragment.contains("--"));
            assert!(fragment.contains("t."));
        }
        assert!(TrackSort::Relevance.order_by(true).contains("bm25"));
        assert!(!TrackSort::Relevance.order_by(false).contains("bm25"));
    }
}
