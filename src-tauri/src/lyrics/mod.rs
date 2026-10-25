//! Lyrics: LRC parsing, resolution and caching.
//!
//! Prism treats lyrics as catalogue data, not as a network feature:
//!
//! 1. an embedded `USLT`/`LYRICS` frame in the file wins (it travels with the
//!    music and is what the artist shipped);
//! 2. otherwise a sidecar `.lrc` next to the track, matched case-insensitively;
//! 3. anything else is a miss, and a miss is cached as a miss only for the
//!    lifetime of the process — a user adding a `.lrc` file and pressing refresh
//!    must see it, so `lyrics_cache` never stores "nothing found".
//!
//! The parser accepts the dialects that exist in the wild rather than the one
//! in the specification: one to three millisecond digits, several timestamps on
//! a single line, metadata tags, `[offset:]` in both signs, BOM, CRLF and
//! timestamps that are not sorted.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use lofty::file::TaggedFileExt;
use lofty::tag::ItemKey;
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};

use crate::error::PrismError;

/// Longest accepted lyric body (a 20-minute prog epic fits in a few kilobytes;
/// anything larger is a mistake or an attack on the cache).
pub const MAX_LYRICS_BYTES: usize = 512 * 1024;
const LRCLIB_GET: &str = "https://lrclib.net/api/get";
const LRCLIB_SEARCH: &str = "https://lrclib.net/api/search";
const LRCLIB_TIMEOUT: Duration = Duration::from_secs(5);

/// The subset of LRCLIB's record contract consumed by Prism. Unknown fields
/// are intentionally ignored so the provider can add metadata without breaking
/// playback; every lyric body still passes through the strict local parser.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LrclibRecord {
    id: i64,
    track_name: String,
    artist_name: String,
    album_name: Option<String>,
    duration: Option<f64>,
    instrumental: Option<bool>,
    plain_lyrics: Option<String>,
    synced_lyrics: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExternalLyrics {
    body: String,
    source_url: String,
}

/// One timed line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricLine {
    /// Start of the line in milliseconds, relative to the track start.
    pub time_ms: i64,
    pub text: String,
}

/// A resolved lyric document, ready for the panel.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricsDocument {
    pub track_id: String,
    /// `embedded`, `sidecar`, `lrclib` or `none`.
    pub provider: String,
    /// True when `lines` carry usable timestamps.
    pub synced: bool,
    pub language: Option<String>,
    /// Global shift requested by the file, in milliseconds (already applied).
    pub offset_ms: i64,
    pub lines: Vec<LyricLine>,
    pub plain_text: String,
    pub source_path: Option<String>,
    pub fetched_at: Option<String>,
}

impl LyricsDocument {
    /// Empty answer. Never cached: the absence of lyrics is not a fact about
    /// the track, it is a fact about this moment.
    #[must_use]
    pub fn empty(track_id: &str) -> Self {
        Self {
            track_id: track_id.to_string(),
            provider: "none".to_string(),
            synced: false,
            language: None,
            offset_ms: 0,
            lines: Vec::new(),
            plain_text: String::new(),
            source_path: None,
            fetched_at: None,
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty() && self.plain_text.trim().is_empty()
    }
}

/// Result of parsing one LRC body.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ParsedLrc {
    pub metadata: BTreeMap<String, String>,
    pub offset_ms: i64,
    pub lines: Vec<LyricLine>,
}

impl ParsedLrc {
    /// True when at least one line carries a real timestamp.
    #[must_use]
    pub fn is_synced(&self) -> bool {
        self.lines.iter().any(|line| line.time_ms >= 0)
    }
}

/// Parse an LRC document.
///
/// Timestamps are `[mm:ss]`, `[mm:ss.f]`, `[mm:ss.ff]` or `[mm:ss.fff]`. Every
/// leading bracketed timestamp on a line becomes its own lyric line (the
/// standard way to repeat a chorus), metadata tags are collected, `[offset:]`
/// shifts every timestamp, and the result is sorted — files in the wild are not.
#[must_use]
pub fn parse_lrc(input: &str) -> ParsedLrc {
    let body = input.strip_prefix('\u{feff}').unwrap_or(input);
    let mut parsed = ParsedLrc::default();
    let mut pending: Vec<LyricLine> = Vec::new();
    let mut offset_from_file: Option<i64> = None;

    for raw in body.lines() {
        let line = raw.trim_end_matches(['\r', '\n']);
        let mut rest = line.trim_start();
        let mut stamps: Vec<i64> = Vec::new();

        // Consume every leading `[...]` group. A tag that is not a timestamp is
        // metadata; keep consuming so `[ar:X][00:01.00]text` still works.
        loop {
            let Some(remainder) = rest.strip_prefix('[') else {
                break;
            };
            let Some(close) = remainder.find(']') else {
                break;
            };
            let token = &remainder[..close];
            rest = &remainder[close + 1..];

            if let Some(ms) = parse_timestamp(token) {
                stamps.push(ms);
            } else if let Some((key, value)) = token.split_once(':') {
                let key = key.trim().to_ascii_lowercase();
                let value = value.trim().to_string();
                if key == "offset" {
                    // `[offset:+500]` delays the lyrics by 500 ms; a negative
                    // value advances them (and clamps at zero, never before).
                    offset_from_file = value.parse::<i64>().ok();
                } else if !key.is_empty() && !value.is_empty() {
                    parsed.metadata.insert(key, value);
                }
            }
        }

        let text = rest.trim().to_string();
        if stamps.is_empty() {
            // A line with no timestamp is either blank spacing or an untimed
            // lyric; both are kept only if the file has no timestamps at all
            // (decided by the caller).
            if !text.is_empty() {
                pending.push(LyricLine { time_ms: -1, text });
            }
            continue;
        }
        for ms in stamps {
            pending.push(LyricLine {
                time_ms: ms,
                text: text.clone(),
            });
        }
    }

    parsed.offset_ms = offset_from_file.unwrap_or(0);
    if parsed.offset_ms != 0 {
        for line in &mut pending {
            if line.time_ms >= 0 {
                line.time_ms = (line.time_ms + parsed.offset_ms).max(0);
            }
        }
    }

    let any_synced = pending.iter().any(|line| line.time_ms >= 0);
    if any_synced {
        // Untimed lines in a timed file are noise (a stray header, a translator
        // credit). Keeping them would break the karaoke cursor.
        pending.retain(|line| line.time_ms >= 0);
        pending.sort_by(|a, b| a.time_ms.cmp(&b.time_ms).then_with(|| a.text.cmp(&b.text)));
        // Two identical timestamps with identical text are a duplicated tag.
        pending.dedup_by(|a, b| a.time_ms == b.time_ms && a.text == b.text);
    } else {
        // Unsynced file: timestamps carry no information, and the caller uses
        // `parse_plain` to keep the line order instead. Emitting `-1` rows here
        // would only give the karaoke cursor something invalid to match.
        pending.clear();
    }

    parsed.lines = pending;
    parsed
}

/// Parse `mm:ss`, `mm:ss.f`, `mm:ss.ff`, `mm:ss.fff` (also `mm:ss:ff`).
///
/// Returns `None` for anything else, including metadata tags such as `ar`.
fn parse_timestamp(token: &str) -> Option<i64> {
    let (minutes, rest) = token.split_once(':')?;
    let minutes: i64 = minutes.trim().parse().ok()?;
    if minutes < 0 {
        return None;
    }

    let (seconds, fraction) = match rest.split_once(['.', ':']) {
        Some((seconds, fraction)) => (seconds, Some(fraction)),
        None => (rest, None),
    };
    let seconds: i64 = seconds.trim().parse().ok()?;
    if !(0..60).contains(&seconds) {
        return None;
    }

    let millis = match fraction {
        None => 0,
        Some(value) => {
            let digits = value.trim();
            if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            // 1–3 digits only: `[00:01.5]` is 500 ms, `[00:01.500]` is 500 ms.
            if digits.len() > 3 {
                return None;
            }
            let padded = format!("{digits:0<3}");
            padded.parse::<i64>().ok()?
        }
    };

    Some(minutes * 60_000 + seconds * 1_000 + millis)
}

/// Turn an untimed lyric body into lines the panel can still follow.
#[must_use]
pub fn parse_plain(input: &str) -> Vec<LyricLine> {
    let body = input.strip_prefix('\u{feff}').unwrap_or(input);
    body.lines()
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty())
        .map(|line| LyricLine {
            time_ms: -1,
            text: line.to_string(),
        })
        .collect()
}

/// Where a resolved document came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LyricsSource {
    Embedded,
    Sidecar,
    Lrclib,
}

impl LyricsSource {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Embedded => "embedded",
            Self::Sidecar => "sidecar",
            Self::Lrclib => "lrclib",
        }
    }
}

/// Read an embedded lyric frame, if the container has one.
#[must_use]
pub fn embedded_lyrics(path: &Path) -> Option<String> {
    let tagged = lofty::read_from_path(path).ok()?;
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag())?;
    let text = tag.get_string(&ItemKey::Lyrics)?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Find a sidecar `.lrc` for a track.
///
/// The direct hit (`song.lrc`) is tried first; when that misses, the directory
/// is scanned for a case-insensitive match, because `.LRC` from a Windows ripper
/// is not a different format.
#[must_use]
pub fn sidecar_lyrics(path: &Path) -> Option<(PathBuf, String)> {
    let direct = path.with_extension("lrc");
    if let Ok(text) = std::fs::read_to_string(&direct) {
        return Some((direct, text));
    }

    let stem = path.file_stem()?.to_str()?.to_ascii_lowercase();
    let folder = path.parent()?;
    for entry in std::fs::read_dir(folder).ok()?.flatten() {
        let candidate = entry.path();
        if !candidate.is_file() {
            continue;
        }
        let matches_stem = candidate
            .file_stem()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.to_ascii_lowercase() == stem);
        let is_lrc = candidate
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("lrc"));
        if matches_stem
            && is_lrc
            && let Ok(text) = std::fs::read_to_string(&candidate)
        {
            return Some((candidate, text));
        }
    }
    None
}

/// Resolve a local miss against LRCLIB, first by exact metadata and then by a
/// tightly validated search. No API key is required and no request is made when
/// embedded or sidecar lyrics exist.
async fn fetch_lrclib(
    title: &str,
    artist: &str,
    album: &str,
    duration_ms: i64,
) -> Option<ExternalLyrics> {
    if title.trim().is_empty() || artist.trim().is_empty() {
        return None;
    }
    let client = reqwest::Client::builder()
        .user_agent(concat!("Prism/", env!("CARGO_PKG_VERSION")))
        .timeout(LRCLIB_TIMEOUT)
        .build()
        .map_err(|error| {
            tracing::debug!(%error, "cannot construct LRCLIB client");
            error
        })
        .ok()?;

    let mut exact = vec![
        ("track_name", title.to_string()),
        ("artist_name", artist.to_string()),
    ];
    if !album.trim().is_empty() {
        exact.push(("album_name", album.to_string()));
    }
    if duration_ms > 0 {
        exact.push(("duration", ((duration_ms + 500) / 1_000).to_string()));
    }

    let response = client
        .get(LRCLIB_GET)
        .query(&exact)
        .send()
        .await
        .map_err(|error| {
            tracing::debug!(%error, "LRCLIB exact lookup failed");
            error
        })
        .ok()?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return search_lrclib(&client, title, artist, album, duration_ms).await;
    }
    if !response.status().is_success() {
        tracing::debug!(status = %response.status(), "LRCLIB exact lookup rejected");
        return None;
    }
    match response.json::<LrclibRecord>().await {
        Ok(record) => external_from_record(&record),
        Err(error) => {
            tracing::debug!(%error, "LRCLIB returned an invalid exact record");
            None
        }
    }
}

async fn search_lrclib(
    client: &reqwest::Client,
    title: &str,
    artist: &str,
    album: &str,
    duration_ms: i64,
) -> Option<ExternalLyrics> {
    let response = client
        .get(LRCLIB_SEARCH)
        .query(&[("track_name", title), ("artist_name", artist)])
        .send()
        .await
        .map_err(|error| {
            tracing::debug!(%error, "LRCLIB search failed");
            error
        })
        .ok()?;
    if !response.status().is_success() {
        tracing::debug!(status = %response.status(), "LRCLIB search rejected");
        return None;
    }
    let mut records = match response.json::<Vec<LrclibRecord>>().await {
        Ok(records) => records,
        Err(error) => {
            tracing::debug!(%error, "LRCLIB returned an invalid search response");
            return None;
        }
    };
    records.retain(|record| {
        record.track_name.trim().to_lowercase() == title.trim().to_lowercase()
            && record.artist_name.trim().to_lowercase() == artist.trim().to_lowercase()
            && duration_matches(record.duration, duration_ms)
            && (record
                .synced_lyrics
                .as_deref()
                .is_some_and(|body| !body.trim().is_empty())
                || record
                    .plain_lyrics
                    .as_deref()
                    .is_some_and(|body| !body.trim().is_empty()))
    });
    records.sort_by(|left, right| {
        search_rank(left, album, duration_ms).cmp(&search_rank(right, album, duration_ms))
    });
    records.first().and_then(external_from_record)
}

/// Reject a different edit of the song. Unknown durations remain eligible but
/// sort after a close, known match.
fn duration_matches(candidate: Option<f64>, local_ms: i64) -> bool {
    if local_ms <= 0 {
        return true;
    }
    let Some(seconds) = candidate.filter(|value| value.is_finite() && *value > 0.0) else {
        return true;
    };
    ((seconds * 1_000.0) - local_ms as f64).abs() <= 5_000.0
}

fn search_rank(record: &LrclibRecord, album: &str, local_ms: i64) -> (u8, i64, u8) {
    let album_mismatch = if album.trim().is_empty()
        || record
            .album_name
            .as_deref()
            .is_some_and(|candidate| candidate.trim().to_lowercase() == album.trim().to_lowercase())
    {
        0
    } else {
        1
    };
    let distance = record.duration.map_or(i64::MAX / 4, |seconds| {
        if local_ms <= 0 || !seconds.is_finite() {
            i64::MAX / 4
        } else {
            ((seconds * 1_000.0) - local_ms as f64).abs() as i64
        }
    });
    let unsynced = u8::from(
        !record
            .synced_lyrics
            .as_deref()
            .is_some_and(|body| parse_lrc(body).is_synced()),
    );
    (album_mismatch, distance, unsynced)
}

fn external_from_record(record: &LrclibRecord) -> Option<ExternalLyrics> {
    let source_url = format!("https://lrclib.net/api/get/{}", record.id);
    if let Some(body) = record.synced_lyrics.as_deref().map(str::trim)
        && body.len() <= MAX_LYRICS_BYTES
        && parse_lrc(body).is_synced()
    {
        return Some(ExternalLyrics {
            body: body.to_string(),
            source_url,
        });
    }
    if record.instrumental == Some(true) {
        return None;
    }
    let body = record.plain_lyrics.as_deref().map(str::trim)?;
    if body.is_empty() || body.len() > MAX_LYRICS_BYTES {
        return None;
    }
    Some(ExternalLyrics {
        body: body.to_string(),
        source_url,
    })
}

/// Lyrics service: resolution order plus the `lyrics_cache` table.
#[derive(Debug, Clone)]
pub struct LyricsService {
    pool: SqlitePool,
}

impl LyricsService {
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Resolve lyrics for a track.
    ///
    /// `refresh = true` re-reads the file and rewrites the cache row; the normal
    /// path is a single indexed lookup, which is what the now-playing panel hits
    /// on every track change.
    pub async fn get(&self, track_id: &str, refresh: bool) -> Result<LyricsDocument, PrismError> {
        if !refresh && let Some(document) = self.cached(track_id).await? {
            return Ok(document);
        }

        let row = sqlx::query(
            "SELECT path, title, artist_name, album_artist_name, album_title, duration_ms \
             FROM tracks WHERE id = ?1",
        )
        .bind(track_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| PrismError::invalid("unknown track"))?;
        let path: String = row.try_get("path")?;
        let title: String = row.try_get("title")?;
        let track_artist: String = row.try_get("artist_name")?;
        let album_artist: String = row.try_get("album_artist_name")?;
        let album: String = row.try_get("album_title")?;
        let duration_ms: i64 = row.try_get("duration_ms")?;
        let artist = if track_artist.trim().is_empty() {
            album_artist
        } else {
            track_artist
        };
        let path = PathBuf::from(path);

        let (source, text, source_path) = if let Some(embedded) = embedded_lyrics(&path) {
            (LyricsSource::Embedded, embedded, None)
        } else if let Some((sidecar_path, text)) = sidecar_lyrics(&path) {
            (
                LyricsSource::Sidecar,
                text,
                Some(sidecar_path.to_string_lossy().into_owned()),
            )
        } else {
            let Some(external) = fetch_lrclib(&title, &artist, &album, duration_ms).await else {
                return Ok(LyricsDocument::empty(track_id));
            };
            (
                LyricsSource::Lrclib,
                external.body,
                Some(external.source_url),
            )
        };

        if text.len() > MAX_LYRICS_BYTES {
            return Err(PrismError::invalid(format!(
                "lyric body for '{title}' exceeds {MAX_LYRICS_BYTES} bytes"
            )));
        }

        let parsed = parse_lrc(&text);
        let synced = parsed.is_synced();
        let lines = if synced {
            parsed.lines.clone()
        } else {
            parse_plain(&text)
        };
        let plain = if synced {
            parsed
                .lines
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            lines
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        };

        let document = LyricsDocument {
            track_id: track_id.to_string(),
            provider: source.as_str().to_string(),
            synced,
            language: parsed.metadata.get("la").cloned(),
            offset_ms: parsed.offset_ms,
            lines,
            plain_text: plain,
            source_path,
            fetched_at: None,
        };

        self.store(track_id, source, &document, &text).await?;
        Ok(document)
    }

    /// Drop the cached row so the next request re-reads the file.
    pub async fn clear(&self, track_id: &str) -> Result<bool, PrismError> {
        let affected = sqlx::query("DELETE FROM lyrics_cache WHERE track_id = ?1")
            .bind(track_id)
            .execute(&self.pool)
            .await?
            .rows_affected();
        Ok(affected > 0)
    }

    /// Cached document, if a row exists **and** the source file still exists.
    ///
    /// A sidecar deleted from disk must not keep serving stale lyrics: the row
    /// records its path precisely so this check is possible.
    async fn cached(&self, track_id: &str) -> Result<Option<LyricsDocument>, PrismError> {
        let row = sqlx::query(
            "SELECT provider, language, synced_lrc, plain_text, has_synced, fetched_at, source_url \
             FROM lyrics_cache WHERE track_id = ?1",
        )
        .bind(track_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };

        let provider: String = row.try_get("provider")?;
        if provider == "sidecar"
            && let Some(source_url) = row.try_get::<Option<String>, _>("source_url")?
            && !Path::new(&source_url).exists()
        {
            return Ok(None);
        }

        let synced_lrc: Option<String> = row.try_get("synced_lrc")?;
        let plain_text: Option<String> = row.try_get("plain_text")?;
        let has_synced: i64 = row.try_get("has_synced")?;

        let (lines, synced) = match (&synced_lrc, has_synced) {
            (Some(body), 1) => (parse_lrc(body).lines, true),
            _ => (
                plain_text.as_deref().map_or_else(Vec::new, parse_plain),
                false,
            ),
        };

        Ok(Some(LyricsDocument {
            track_id: track_id.to_string(),
            provider,
            synced,
            language: row.try_get("language")?,
            offset_ms: 0,
            lines,
            plain_text: plain_text.unwrap_or_default(),
            source_path: row.try_get("source_url")?,
            fetched_at: row.try_get("fetched_at")?,
        }))
    }

    async fn store(
        &self,
        track_id: &str,
        source: LyricsSource,
        document: &LyricsDocument,
        raw: &str,
    ) -> Result<(), PrismError> {
        let id = format!("ly:{track_id}");
        sqlx::query(
            "INSERT INTO lyrics_cache \
                (id, track_id, provider, source_url, language, plain_text, synced_lrc, has_synced) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
             ON CONFLICT(track_id) DO UPDATE SET \
                provider = excluded.provider, source_url = excluded.source_url, \
                language = excluded.language, plain_text = excluded.plain_text, \
                synced_lrc = excluded.synced_lrc, has_synced = excluded.has_synced, \
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
        )
        .bind(&id)
        .bind(track_id)
        .bind(source.as_str())
        .bind(&document.source_path)
        .bind(&document.language)
        .bind(&document.plain_text)
        .bind(if document.synced {
            Some(raw.to_string())
        } else {
            None
        })
        .bind(i64::from(document.synced))
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_common_timestamp_dialects() {
        let input = "[00:01.00]one\n[00:02.5]two\n[00:03.250]three\n[01:04]four\n";
        let parsed = parse_lrc(input);
        let times: Vec<i64> = parsed.lines.iter().map(|line| line.time_ms).collect();
        assert_eq!(times, vec![1_000, 2_500, 3_250, 64_000]);
        assert!(parsed.is_synced());
    }

    #[test]
    fn repeated_timestamps_become_repeated_lines() {
        let input = "[00:10.00][01:20.00]chorus\n[00:30.00]verse\n";
        let parsed = parse_lrc(input);
        assert_eq!(parsed.lines.len(), 3);
        assert_eq!(parsed.lines[0].text, "chorus");
        assert_eq!(parsed.lines[0].time_ms, 10_000);
        assert_eq!(parsed.lines[2].time_ms, 80_000);
    }

    #[test]
    fn metadata_and_offset_are_understood() {
        let input = "[ar:Radiohead]\n[ti:Kid A]\n[offset:+500]\n[00:01.00]hello\n";
        let parsed = parse_lrc(input);
        assert_eq!(
            parsed.metadata.get("ar").map(String::as_str),
            Some("Radiohead")
        );
        assert_eq!(parsed.offset_ms, 500);
        // A positive offset delays the lyrics: 1 s becomes 1.5 s.
        assert_eq!(parsed.lines[0].time_ms, 1_500);
    }

    #[test]
    fn a_negative_offset_advances_and_never_goes_below_zero() {
        let parsed = parse_lrc("[offset:-2500]\n[00:01.00]early\n[00:05.00]later\n");
        assert_eq!(parsed.lines[0].time_ms, 0);
        assert_eq!(parsed.lines[1].time_ms, 2_500);
    }

    #[test]
    fn unsorted_files_are_sorted_and_duplicates_removed() {
        let parsed = parse_lrc("[00:30.00]b\n[00:10.00]a\n[00:10.00]a\n");
        let texts: Vec<&str> = parsed.lines.iter().map(|line| line.text.as_str()).collect();
        assert_eq!(texts, vec!["a", "b"]);
    }

    #[test]
    fn crlf_bom_and_untimed_noise_are_handled() {
        let input = "\u{feff}[00:01.00]first\r\nstray line\r\n[00:02.00]second\r\n";
        let parsed = parse_lrc(input);
        assert_eq!(
            parsed.lines.len(),
            2,
            "untimed lines must not break the cursor"
        );
        assert_eq!(parsed.lines[0].text, "first");
        assert_eq!(parsed.lines[1].text, "second");
    }

    #[test]
    fn a_file_without_timestamps_falls_back_to_plain_lines() {
        let parsed = parse_lrc("just a poem\nwith two lines\n");
        assert!(!parsed.is_synced());
        assert!(
            parsed.lines.is_empty(),
            "untimed lines are not emitted as timed"
        );

        let plain = parse_plain("just a poem\n\nwith two lines\n");
        assert_eq!(plain.len(), 2);
        assert_eq!(plain[0].time_ms, -1);
        assert_eq!(plain[1].text, "with two lines");
    }

    #[test]
    fn malformed_timestamps_are_metadata_not_lyrics() {
        for token in ["99:99", "mm:ss", "-01:00", "00:60.00", "00:01.1234"] {
            assert!(
                parse_timestamp(token).is_none(),
                "{token} must not parse as a timestamp"
            );
        }
        assert_eq!(parse_timestamp("02:03"), Some(123_000));
        assert_eq!(parse_timestamp("02:03.4"), Some(123_400));
        assert_eq!(
            parse_timestamp("02:03:45"),
            Some(123_450),
            "frames-style ms"
        );
    }

    #[test]
    fn a_document_reports_emptiness() {
        assert!(LyricsDocument::empty("t").is_empty());
        assert_eq!(LyricsDocument::empty("t").provider, "none");
    }

    fn lrclib_record() -> LrclibRecord {
        LrclibRecord {
            id: 42,
            track_name: "Song".into(),
            artist_name: "Artist".into(),
            album_name: Some("Album".into()),
            duration: Some(180.0),
            instrumental: Some(false),
            plain_lyrics: Some("plain words".into()),
            synced_lyrics: Some("[00:01.00]timed words".into()),
        }
    }

    #[test]
    fn external_records_prefer_real_synchronized_lyrics() {
        let resolved = external_from_record(&lrclib_record()).expect("synchronized lyrics");
        assert_eq!(resolved.body, "[00:01.00]timed words");
        assert!(resolved.source_url.ends_with("/42"));

        let mut malformed = lrclib_record();
        malformed.synced_lyrics = Some("not timed".into());
        assert_eq!(
            external_from_record(&malformed)
                .expect("plain fallback")
                .body,
            "plain words"
        );
    }

    #[test]
    fn external_matching_rejects_a_different_duration() {
        assert!(duration_matches(Some(180.0), 180_000));
        assert!(duration_matches(Some(184.9), 180_000));
        assert!(!duration_matches(Some(185.1), 180_000));
        assert!(duration_matches(None, 180_000));
        assert!(duration_matches(Some(999.0), 0));
    }

    #[test]
    fn instrumental_records_do_not_fabricate_a_lyric_sheet() {
        let mut record = lrclib_record();
        record.synced_lyrics = None;
        record.plain_lyrics = None;
        record.instrumental = Some(true);
        assert!(external_from_record(&record).is_none());
    }
}
