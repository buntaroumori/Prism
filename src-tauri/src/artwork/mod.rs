//! Cover-art cache: one SHA-256 key per album, four providers, one row.
//!
//! ## The key
//!
//! `cover_cache.id` is the SHA-256 of `lower(casefold(artist)) + U+001F +
//! lower(casefold(album))`. Using the *tagged* identity (not the folder path)
//! means a re-organised library keeps its artwork, the same album on two discs
//! shares one cache entry, and Discord can look an asset up from nothing but the
//! two strings it receives from the audio engine.
//!
//! ## The cache
//!
//! * Successes live for [`SUCCESS_TTL_DAYS`] days and are re-validated by
//!   checking that the file still exists.
//! * Failures are cached for [`NEGATIVE_TTL_HOURS`] so a library of obscure
//!   bootlegs does not hammer iTunes on every scroll. A negative row has
//!   `provider = 'unknown'` and `remote_url = ''` (the schema's
//!   `cover_cache_has_asset` CHECK requires one non-null column).
//! * Images are normalised to JPEG, longest edge [`COVER_SIZE`] px, quality
//!   [`JPEG_QUALITY`]. A 12 MP gatefold scan becomes a 90 KB tile the UI can
//!   texture-upload without a decode hitch.
//!
//! ## Concurrency
//!
//! Two views can ask for the same album at once (grid + player bar). The
//! per-key async mutex collapses them into one fetch: the loser waits, then
//! hits the freshly written cache row.

pub mod providers;

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool};
use tokio::sync::Mutex;

use crate::error::PrismError;
use providers::{
    CoverProvider, ProviderImage, embedded, folder as folder_cover, select_itunes_result,
    select_musicbrainz_release,
};

/// Longest edge of a cached cover, in pixels.
pub const COVER_SIZE: u32 = 1_000;
/// JPEG quality of the cached cover.
pub const JPEG_QUALITY: u8 = 88;
/// Longest edge of the grid derivative, in pixels.
pub const THUMBNAIL_SIZE: u32 = 320;
/// JPEG quality of the grid derivative.
pub const THUMBNAIL_JPEG_QUALITY: u8 = 82;
/// How long a successful lookup is trusted.
pub const SUCCESS_TTL_DAYS: i64 = 90;
/// How long a failed lookup is remembered.
pub const NEGATIVE_TTL_HOURS: i64 = 24;

/// iTunes Search API endpoint.
const ITUNES_ENDPOINT: &str = "https://itunes.apple.com/search";
/// MusicBrainz web service endpoint.
const MUSICBRAINZ_ENDPOINT: &str = "https://musicbrainz.org/ws/2/release/";
/// Cover Art Archive front cover at 500 px.
const COVER_ART_ARCHIVE: &str = "https://coverartarchive.org/release";
/// MusicBrainz asks for at most one request per second per client.
const MUSICBRAINZ_INTERVAL: Duration = Duration::from_millis(1_100);

/// Where the artwork lookup may look for local files.
#[derive(Debug, Clone, Default)]
pub struct CoverHint {
    /// A track file of the album; used to read embedded pictures.
    pub track_path: Option<PathBuf>,
    /// The album folder; used to find `cover.jpg` & friends.
    pub folder: Option<PathBuf>,
}

/// Resolved cover, ready for the UI or for Discord.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverAsset {
    /// 64-char SHA-256, also the `cover_cache` primary key.
    pub key: String,
    pub provider: String,
    /// Absolute path of the cached JPEG (`asset://` convertible).
    pub local_path: Option<String>,
    /// Absolute path of the lightweight grid derivative.
    pub thumbnail_path: Option<String>,
    /// Provider URL; only this is usable for Discord rich presence.
    pub remote_url: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

impl CoverAsset {
    #[must_use]
    pub fn is_placeholder(&self) -> bool {
        self.provider == CoverProvider::Unknown.as_str()
    }
}

/// Artwork service: cache lookups, provider chain, image normalisation.
#[derive(Debug, Clone)]
pub struct ArtworkService {
    pool: SqlitePool,
    directory: PathBuf,
    http: reqwest::Client,
    musicbrainz_gate: Arc<Mutex<Instant>>,
    inflight: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    /// When true, network providers are skipped entirely (offline mode and
    /// tests). Local providers keep working.
    offline: bool,
}

impl ArtworkService {
    /// Build the service and create the cache directory.
    pub fn new(pool: SqlitePool, cache_dir: impl AsRef<Path>) -> Result<Self, PrismError> {
        let directory = cache_dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&directory).map_err(|error| PrismError::io(&directory, error))?;
        let http = reqwest::Client::builder()
            .user_agent(concat!(
                "Prism/",
                env!("CARGO_PKG_VERSION"),
                " (+https://github.com/prism-audio/prism)"
            ))
            .timeout(Duration::from_secs(12))
            .connect_timeout(Duration::from_secs(6))
            .build()
            .map_err(|error| PrismError::Internal(format!("http client: {error}")))?;
        Ok(Self {
            pool,
            directory,
            http,
            musicbrainz_gate: Arc::new(Mutex::new(Instant::now() - MUSICBRAINZ_INTERVAL)),
            inflight: Arc::new(Mutex::new(HashMap::new())),
            offline: false,
        })
    }

    /// Disable every network provider, keeping embedded and folder artwork.
    #[must_use]
    pub fn with_offline(mut self, offline: bool) -> Self {
        self.offline = offline;
        self
    }

    /// Directory where normalised covers are stored.
    #[must_use]
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// Cache key for an album identity.
    #[must_use]
    pub fn key(artist: &str, album: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(artist.trim().to_lowercase().as_bytes());
        hasher.update([0x1f]);
        hasher.update(album.trim().to_lowercase().as_bytes());
        hex::encode(hasher.finalize())
    }

    /// Resolve artwork for `(artist, album)`, fetching it when needed.
    ///
    /// Never fails: a total provider outage yields a placeholder asset that the
    /// UI renders with the album's palette, exactly like a missing cover in a
    /// physical collection. Errors are logged and cached.
    pub async fn ensure(&self, artist: &str, album: &str, hint: &CoverHint) -> CoverAsset {
        let key = Self::key(artist, album);

        if let Some(asset) = self.cached(&key).await {
            return asset;
        }

        // Collapse concurrent lookups for the same album.
        let gate = {
            let mut inflight = self.inflight.lock().await;
            Arc::clone(
                inflight
                    .entry(key.clone())
                    .or_insert_with(|| Arc::new(Mutex::new(()))),
            )
        };
        let _guard = gate.lock().await;

        // Another task may have filled the cache while we waited.
        if let Some(asset) = self.cached(&key).await {
            self.inflight.lock().await.remove(&key);
            return asset;
        }

        let resolved = self.fetch(artist, album, hint).await;
        let asset = match resolved {
            Ok(image) => self.store(&key, artist, album, image).await,
            Err(error) => {
                tracing::debug!(artist, album, %error, "cover art unavailable");
                self.store_negative(&key, artist, album, &error.to_string())
                    .await
            }
        };
        self.inflight.lock().await.remove(&key);
        asset
    }

    /// Canonical album identity for artwork lookups. Track artists remain in
    /// the activity text, while all songs from one compilation share this key.
    pub async fn identity_for_track(&self, track_id: &str) -> Option<(String, String)> {
        let row = sqlx::query(
            "SELECT artist_name, album_artist_name, album_title FROM tracks WHERE id = ?1",
        )
        .bind(track_id)
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten()?;
        let track_artist: String = row.try_get("artist_name").ok()?;
        let album_artist: String = row.try_get("album_artist_name").ok()?;
        let album: String = row.try_get("album_title").ok()?;
        let artist = if album_artist.trim().is_empty() {
            track_artist
        } else {
            album_artist
        };
        Some((artist, album))
    }

    /// Resolve the best local hint for a track's *album*, not just its folder.
    ///
    /// Multi-disc records commonly keep artwork beside disc one while the
    /// current file is under `CD2/`. Using the first available file of the
    /// album keeps embedded/folder artwork and the Discord cover consistent.
    pub async fn hint_for_track(&self, track_id: &str, current_path: Option<PathBuf>) -> CoverHint {
        let first: Option<String> = sqlx::query_scalar(
            "SELECT first.path FROM tracks AS current \
             JOIN tracks AS first ON first.album_id = current.album_id \
             WHERE current.id = ?1 AND first.missing = 0 \
             ORDER BY first.disc_no ASC, first.track_no ASC, first.path ASC LIMIT 1",
        )
        .bind(track_id)
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();
        let track_path = first.map(PathBuf::from).or(current_path);
        let folder = track_path
            .as_ref()
            .and_then(|path| path.parent().map(Path::to_path_buf));
        CoverHint { track_path, folder }
    }

    /// An HTTPS URL for `(artist, album)`, for surfaces that cannot read a file.
    ///
    /// Discord Rich Presence is the reason this exists: `large_image` accepts a
    /// public URL or an asset key uploaded to the application, never a local
    /// path. Embedded and folder artwork — the two providers that actually fire
    /// for a normal library — carry no URL, so a presence built only from the
    /// cached asset silently lost its cover. This asks the catalogue providers
    /// for a *URL* (no download, no cache write) and returns the first match.
    ///
    /// Returns `None` when offline or when nothing matches, which the caller
    /// treats as "no artwork", not as an error.
    pub async fn remote_url(&self, artist: &str, album: &str) -> Option<String> {
        if self.offline || artist.trim().is_empty() || album.trim().is_empty() {
            return None;
        }
        // A cached network cover already knows its URL: trust it first.
        if let Some(asset) = self.cached(&Self::key(artist, album)).await
            && let Some(url) = asset.remote_url
            && url.starts_with("https://")
        {
            return Some(url);
        }

        let term = format!("{artist} {album}");
        let response = self
            .http
            .get(ITUNES_ENDPOINT)
            .query(&[
                ("term", term.as_str()),
                ("entity", "album"),
                ("limit", "10"),
                ("media", "music"),
            ])
            .send()
            .await
            .ok()?
            .error_for_status()
            .ok()?;
        let json: serde_json::Value = response.json().await.ok()?;
        let (url, _collection) = select_itunes_result(&json, artist, album)?;
        url.starts_with("https://").then_some(url)
    }

    /// Read a fresh success row from the cache, upgrading pre-thumbnail rows
    /// from their existing master instead of repeating the provider lookup.
    async fn cached(&self, key: &str) -> Option<CoverAsset> {
        let row: Option<(
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<i64>,
            Option<i64>,
            i64,
        )> = sqlx::query_as(
            "SELECT provider, remote_url, local_path, thumbnail_path, width, height, \
                        CASE WHEN expires_at IS NULL \
                                  OR expires_at > strftime('%Y-%m-%dT%H:%M:%fZ','now') \
                             THEN 1 ELSE 0 END AS fresh \
                 FROM cover_cache WHERE id = ?1",
        )
        .bind(key)
        .fetch_optional(&self.pool)
        .await
        .ok()?;
        let (provider, remote_url, local_path, thumbnail_path, width, height, fresh) = row?;

        if provider == CoverProvider::Unknown.as_str() || fresh == 0 {
            return None;
        }
        if let Some(path) = &local_path
            && !Path::new(path).exists()
        {
            return None;
        }

        let mut thumbnail_path = thumbnail_path.filter(|path| Path::new(path).exists());
        if thumbnail_path.is_none()
            && let Some(master) = local_path.as_deref()
            && let Ok(path) = self.thumbnail_from_master(key, Path::new(master))
        {
            thumbnail_path = Some(path.to_string_lossy().into_owned());
            if let Err(error) = sqlx::query(
                "UPDATE cover_cache SET thumbnail_path = ?2, \
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?1",
            )
            .bind(key)
            .bind(thumbnail_path.as_deref())
            .execute(&self.pool)
            .await
            {
                tracing::warn!(%error, "cannot persist cover thumbnail path");
            }
        }

        Some(CoverAsset {
            key: key.to_string(),
            provider,
            local_path,
            thumbnail_path,
            remote_url,
            width: width.map(|value| value as u32),
            height: height.map(|value| value as u32),
        })
    }

    /// Run the provider chain.
    async fn fetch(
        &self,
        artist: &str,
        album: &str,
        hint: &CoverHint,
    ) -> Result<ProviderImage, PrismError> {
        if let Some(path) = &hint.track_path
            && let Some(image) = embedded(path)
        {
            return Ok(image);
        }
        if let Some(folder) = &hint.folder
            && let Some(image) = folder_cover(folder)
        {
            return Ok(image);
        }
        if self.offline {
            return Err(PrismError::Internal(
                "network artwork providers are disabled".to_string(),
            ));
        }
        if let Some(image) = self.itunes(artist, album).await {
            return Ok(image);
        }
        if let Some(image) = self.cover_art_archive(artist, album).await {
            return Ok(image);
        }
        Err(PrismError::Internal(format!(
            "no artwork found for {artist} — {album}"
        )))
    }

    /// iTunes Search API. The only provider that works without an exact album
    /// id, which is why it is tried first when no local art exists.
    async fn itunes(&self, artist: &str, album: &str) -> Option<ProviderImage> {
        let term = format!("{artist} {album}");
        let response = self
            .http
            .get(ITUNES_ENDPOINT)
            .query(&[
                ("term", term.as_str()),
                ("entity", "album"),
                ("limit", "10"),
            ])
            .send()
            .await
            .ok()?
            .error_for_status()
            .ok()?;
        let json: serde_json::Value = response.json().await.ok()?;
        let (url, _collection) = select_itunes_result(&json, artist, album)?;
        let bytes = self.download(&url).await?;
        Some(ProviderImage {
            provider: CoverProvider::Itunes,
            bytes,
            remote_url: Some(url),
        })
    }

    /// MusicBrainz release search → Cover Art Archive front image.
    async fn cover_art_archive(&self, artist: &str, album: &str) -> Option<ProviderImage> {
        {
            // Serialise every caller through the shared gate: MusicBrainz bans
            // clients that exceed one request per second.
            let mut last = self.musicbrainz_gate.lock().await;
            let elapsed = last.elapsed();
            if elapsed < MUSICBRAINZ_INTERVAL {
                tokio::time::sleep(MUSICBRAINZ_INTERVAL - elapsed).await;
            }
            *last = Instant::now();
        }

        let query = format!("release:\"{album}\" AND artist:\"{artist}\"");
        let response = self
            .http
            .get(MUSICBRAINZ_ENDPOINT)
            .query(&[("query", query.as_str()), ("fmt", "json"), ("limit", "5")])
            .send()
            .await
            .ok()?
            .error_for_status()
            .ok()?;
        let json: serde_json::Value = response.json().await.ok()?;
        let release_id = select_musicbrainz_release(&json, album)?;

        let url = format!("{COVER_ART_ARCHIVE}/{release_id}/front-500");
        let bytes = self.download(&url).await?;
        Some(ProviderImage {
            provider: CoverProvider::CoverArtArchive,
            bytes,
            remote_url: Some(url),
        })
    }

    async fn download(&self, url: &str) -> Option<Vec<u8>> {
        let response = self
            .http
            .get(url)
            .send()
            .await
            .ok()?
            .error_for_status()
            .ok()?;
        let bytes = response.bytes().await.ok()?.to_vec();
        if bytes.len() < 512 || bytes.len() > 20 * 1024 * 1024 {
            return None;
        }
        Some(bytes)
    }

    /// Normalise, write and index a provider image.
    async fn store(
        &self,
        key: &str,
        artist: &str,
        album: &str,
        image: ProviderImage,
    ) -> CoverAsset {
        match self.normalise_and_write(key, &image.bytes) {
            Ok((path, thumbnail_path, width, height)) => {
                let path_string = path.to_string_lossy().into_owned();
                let thumbnail_string = thumbnail_path
                    .as_ref()
                    .map(|path| path.to_string_lossy().into_owned());
                let expires = format!("+{} days", SUCCESS_TTL_DAYS);
                let result = sqlx::query(
                    "INSERT INTO cover_cache (\
                        id, artist_key, album_key, hash, provider, remote_url, local_path, \
                        thumbnail_path, width, height, attempts, last_error, fetched_at, expires_at\
                     ) VALUES (\
                        ?1, ?2, ?3, ?1, ?4, ?5, ?6, ?7, ?8, ?9, 1, NULL, \
                        strftime('%Y-%m-%dT%H:%M:%fZ','now'), \
                        strftime('%Y-%m-%dT%H:%M:%fZ','now', ?10)\
                     ) ON CONFLICT(id) DO UPDATE SET \
                        provider = excluded.provider, remote_url = excluded.remote_url, \
                        local_path = excluded.local_path, thumbnail_path = excluded.thumbnail_path, \
                        width = excluded.width, \
                        height = excluded.height, attempts = cover_cache.attempts + 1, \
                        last_error = NULL, fetched_at = excluded.fetched_at, \
                        expires_at = excluded.expires_at, \
                        updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')",
                )
                .bind(key)
                .bind(artist.trim().to_lowercase())
                .bind(album.trim().to_lowercase())
                .bind(image.provider.as_str())
                .bind(image.remote_url.as_deref())
                .bind(&path_string)
                .bind(thumbnail_string.as_deref())
                .bind(i64::from(width))
                .bind(i64::from(height))
                .bind(expires)
                .execute(&self.pool)
                .await;

                if let Err(error) = result {
                    tracing::warn!(%error, "cannot persist cover cache row");
                }

                CoverAsset {
                    key: key.to_string(),
                    provider: image.provider.as_str().to_string(),
                    local_path: Some(path_string),
                    thumbnail_path: thumbnail_string,
                    remote_url: image.remote_url,
                    width: Some(width),
                    height: Some(height),
                }
            }
            Err(error) => {
                tracing::warn!(%error, "cannot normalise cover art");
                self.store_negative(key, artist, album, &error.to_string())
                    .await
            }
        }
    }

    /// Decode any supported image and write a bounded JPEG.
    fn normalise_and_write(
        &self,
        key: &str,
        bytes: &[u8],
    ) -> Result<(PathBuf, Option<PathBuf>, u32, u32), PrismError> {
        let decoded = image::load_from_memory(bytes)
            .map_err(|error| PrismError::Internal(format!("decode cover: {error}")))?;
        let decoded = if decoded.width() > COVER_SIZE || decoded.height() > COVER_SIZE {
            decoded.thumbnail(COVER_SIZE, COVER_SIZE)
        } else {
            decoded
        };
        let width = decoded.width();
        let height = decoded.height();

        // The grid never needs to decode the 1000 px master. A failure to make
        // this derivative does not discard otherwise usable full-size artwork.
        let thumbnail_path = match self.write_thumbnail(key, &decoded) {
            Ok(path) => Some(path),
            Err(error) => {
                tracing::warn!(%error, "cannot write cover thumbnail");
                None
            }
        };
        let rgb = decoded.to_rgb8();
        let encoded = encode_jpeg(&image::DynamicImage::ImageRgb8(rgb), JPEG_QUALITY)?;

        let path = self.directory.join(format!("{key}.jpg"));
        write_atomic(&path, &encoded)?;
        Ok((path, thumbnail_path, width, height))
    }

    /// Create (or recreate) the grid derivative from an already normalised master.
    fn thumbnail_from_master(&self, key: &str, master: &Path) -> Result<PathBuf, PrismError> {
        let decoded = image::open(master)
            .map_err(|error| PrismError::Internal(format!("decode cached cover: {error}")))?;
        self.write_thumbnail(key, &decoded)
    }

    fn write_thumbnail(
        &self,
        key: &str,
        decoded: &image::DynamicImage,
    ) -> Result<PathBuf, PrismError> {
        let thumbnail = if decoded.width() > THUMBNAIL_SIZE || decoded.height() > THUMBNAIL_SIZE {
            decoded.thumbnail(THUMBNAIL_SIZE, THUMBNAIL_SIZE)
        } else {
            decoded.clone()
        };
        let encoded = encode_jpeg(&thumbnail, THUMBNAIL_JPEG_QUALITY)?;
        let path = self.directory.join(format!("{key}.320.jpg"));
        write_atomic(&path, &encoded)?;
        Ok(path)
    }

    /// Remember a miss so the UI never blocks on the same lookup twice a day.
    async fn store_negative(
        &self,
        key: &str,
        artist: &str,
        album: &str,
        error: &str,
    ) -> CoverAsset {
        let result = sqlx::query(
            "INSERT INTO cover_cache (\
                id, artist_key, album_key, hash, provider, remote_url, local_path, \
                attempts, last_error, fetched_at, expires_at\
             ) VALUES (\
                ?1, ?2, ?3, ?1, 'unknown', '', NULL, 1, ?4, \
                strftime('%Y-%m-%dT%H:%M:%fZ','now'), \
                strftime('%Y-%m-%dT%H:%M:%fZ','now', ?5)\
             ) ON CONFLICT(id) DO UPDATE SET \
                provider = 'unknown', remote_url = '', local_path = NULL, thumbnail_path = NULL, \
                attempts = cover_cache.attempts + 1, last_error = excluded.last_error, \
                expires_at = excluded.expires_at, \
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')",
        )
        .bind(key)
        .bind(artist.trim().to_lowercase())
        .bind(album.trim().to_lowercase())
        .bind(truncate(error, 512))
        .bind(format!("+{NEGATIVE_TTL_HOURS} hours"))
        .execute(&self.pool)
        .await;
        if let Err(error) = result {
            tracing::warn!(%error, "cannot persist negative cover cache row");
        }

        CoverAsset {
            key: key.to_string(),
            provider: CoverProvider::Unknown.as_str().to_string(),
            local_path: None,
            thumbnail_path: None,
            remote_url: None,
            width: None,
            height: None,
        }
    }

    /// Delete cache rows whose TTL elapsed, plus their files.
    pub async fn sweep(&self) -> Result<u64, PrismError> {
        let expired: Vec<(String, Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT id, local_path, thumbnail_path FROM cover_cache \
             WHERE expires_at IS NOT NULL AND expires_at < strftime('%Y-%m-%dT%H:%M:%fZ','now')",
        )
        .fetch_all(&self.pool)
        .await?;
        let mut removed = 0;
        for (id, local_path, thumbnail_path) in &expired {
            for path in [local_path, thumbnail_path].into_iter().flatten() {
                if Path::new(path).exists() {
                    let _ = std::fs::remove_file(path);
                }
            }
            removed += sqlx::query("DELETE FROM cover_cache WHERE id = ?1")
                .bind(id)
                .execute(&self.pool)
                .await?
                .rows_affected();
        }
        Ok(removed)
    }
}

fn encode_jpeg(image: &image::DynamicImage, quality: u8) -> Result<Vec<u8>, PrismError> {
    let mut encoded = Vec::with_capacity(64 * 1024);
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut encoded, quality);
    encoder
        .encode_image(image)
        .map_err(|error| PrismError::Internal(format!("encode cover: {error}")))?;
    Ok(encoded)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), PrismError> {
    let parent = path
        .parent()
        .ok_or_else(|| PrismError::Internal("artwork cache path has no parent".to_string()))?;
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(|error| PrismError::io(parent, error))?;
    temporary
        .write_all(bytes)
        .map_err(|error| PrismError::io(temporary.path(), error))?;
    temporary
        .persist(path)
        .map_err(|error| PrismError::io(path, error.error))?;
    Ok(())
}

fn truncate(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_sha256_of_the_normalised_identity() {
        let key = ArtworkService::key("Radiohead", "Kid A");
        assert_eq!(key.len(), 64);
        assert!(key.chars().all(|character| character.is_ascii_hexdigit()));
        assert_eq!(key, ArtworkService::key(" radiohead ", "kid a"));
        assert_ne!(key, ArtworkService::key("Radiohead", "Amnesiac"));
        assert_ne!(key, ArtworkService::key("Portishead", "Kid A"));
    }

    #[test]
    fn the_separator_prevents_identity_collisions() {
        // Concatenating without a separator would make these two collide.
        assert_ne!(
            ArtworkService::key("AB", "C"),
            ArtworkService::key("A", "BC")
        );
    }

    #[tokio::test]
    async fn thumbnails_are_bounded_jpegs_and_can_be_replaced() {
        let directory = tempfile::tempdir().expect("temporary artwork cache");
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("in-memory database");
        let service = ArtworkService::new(pool.clone(), directory.path()).expect("artwork service");
        let source = image::DynamicImage::new_rgb8(1_600, 900);

        let path = service
            .write_thumbnail("thumbnail-test", &source)
            .expect("write thumbnail");
        service
            .write_thumbnail("thumbnail-test", &source)
            .expect("replace thumbnail");
        let decoded = image::open(&path).expect("decode thumbnail");

        assert_eq!(decoded.width(), 320);
        assert_eq!(decoded.height(), 180);
        assert_eq!(
            path.extension().and_then(|value| value.to_str()),
            Some("jpg")
        );
        pool.close().await;
    }
}
