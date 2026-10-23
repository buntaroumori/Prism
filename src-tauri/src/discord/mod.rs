//! Discord Rich Presence.
//!
//! The presence shown in Discord is derived from the audio engine snapshot, not
//! from whatever the React tree happens to be rendering: closing the window to
//! the tray keeps the activity accurate, and the UI never has to talk to the
//! RPC socket.
//!
//! ```text
//! EngineEvent ──▶ DiscordService::sync ──▶ Activity ──▶ DiscordRpc ──▶ socket
//!                                   └──▶ ArtworkService (cover URL)
//! ```
//!
//! Cover art is resolved asynchronously: the textual activity is published
//! immediately and the artwork follows as a second update once the cache or a
//! provider answers. Discord only accepts HTTPS URLs for external assets, so a
//! locally extracted cover still leaves the activity on the bundled `prism`
//! asset — the fallback is explicit, never a broken image.

pub mod rpc;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::artwork::ArtworkService;
use crate::audio::engine::{EngineStatus, PlaybackState};

pub use rpc::{Activity, DiscordRpc, DiscordState, DiscordStatus};

/// Settings key holding the persisted configuration JSON.
pub const SETTINGS_KEY: &str = "discord";

/// Environment variable read at boot, so a packaged build can be pointed at a
/// registered Discord application without shipping credentials.
pub const CLIENT_ID_ENV: &str = "PRISM_DISCORD_CLIENT_ID";

/// Bundled asset name for the "playing" badge.
pub const ASSET_PLAYING: &str = "prism";
/// Bundled asset name for the "paused" badge.
pub const ASSET_PAUSED: &str = "prism-paused";

/// User-editable mapping between an album's artwork identity and a public URL.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct ArtworkOverrideFile {
    version: u32,
    albums: BTreeMap<String, ArtworkOverride>,
}

impl Default for ArtworkOverrideFile {
    fn default() -> Self {
        Self {
            version: 1,
            albums: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ArtworkOverride {
    artist: String,
    album: String,
    url: Option<String>,
}

/// Serialises file access so concurrent track events cannot erase each other's
/// entries. Every write replaces the file atomically and preserves manual URLs.
#[derive(Debug)]
struct ArtworkOverrides {
    path: Option<PathBuf>,
    io: Mutex<()>,
}

impl ArtworkOverrides {
    fn new(path: Option<PathBuf>) -> Self {
        Self {
            path,
            io: Mutex::new(()),
        }
    }

    async fn read(&self, path: &Path) -> Result<ArtworkOverrideFile, crate::error::PrismError> {
        match tokio::fs::read_to_string(path).await {
            Ok(json) => serde_json::from_str(&json).map_err(|error| {
                crate::error::PrismError::InvalidInput(format!(
                    "cannot parse {}: {error}",
                    path.display()
                ))
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(ArtworkOverrideFile::default())
            }
            Err(error) => Err(crate::error::PrismError::io(path, error)),
        }
    }

    fn valid_url(value: &str) -> bool {
        value.len() <= 2_048
            && !value
                .chars()
                .any(|character| character.is_control() || character.is_whitespace())
            && reqwest::Url::parse(value).is_ok_and(|url| {
                url.scheme() == "https" && url.host_str().is_some_and(|host| !host.is_empty())
            })
    }

    async fn url(&self, artist: &str, album: &str) -> Option<String> {
        let path = self.path.as_ref()?;
        let _guard = self.io.lock().await;
        let file = match self.read(path).await {
            Ok(file) => file,
            Err(error) => {
                tracing::warn!(%error, "cannot read Discord artwork overrides");
                return None;
            }
        };
        file.albums
            .get(&ArtworkService::key(artist, album))
            .and_then(|entry| entry.url.as_deref())
            .map(str::trim)
            .filter(|url| Self::valid_url(url))
            .map(str::to_string)
    }

    async fn record_missing(
        &self,
        artist: &str,
        album: &str,
    ) -> Result<(), crate::error::PrismError> {
        let Some(path) = self.path.as_ref() else {
            return Ok(());
        };
        let _guard = self.io.lock().await;
        let mut file = self.read(path).await?;
        let key = ArtworkService::key(artist, album);
        if let Some(existing) = file.albums.get_mut(&key) {
            if existing.artist == artist && existing.album == album {
                return Ok(());
            }
            existing.artist = artist.to_string();
            existing.album = album.to_string();
        } else {
            file.albums.insert(
                key,
                ArtworkOverride {
                    artist: artist.to_string(),
                    album: album.to_string(),
                    url: None,
                },
            );
        }
        file.version = 1;
        let parent = path.parent().ok_or_else(|| {
            crate::error::PrismError::InvalidInput(format!(
                "invalid artwork override path: {}",
                path.display()
            ))
        })?;
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| crate::error::PrismError::io(parent, error))?;
        let temporary = tempfile::NamedTempFile::new_in(parent)
            .map_err(|error| crate::error::PrismError::io(parent, error))?;
        let json = serde_json::to_string_pretty(&file)
            .map_err(|error| crate::error::PrismError::Internal(error.to_string()))?;
        tokio::fs::write(temporary.path(), format!("{json}\n"))
            .await
            .map_err(|error| crate::error::PrismError::io(temporary.path(), error))?;
        temporary
            .persist(path)
            .map_err(|error| crate::error::PrismError::io(path, error.error))?;
        Ok(())
    }
}

/// User-facing configuration, persisted in the `settings` table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DiscordConfig {
    /// Master switch.
    pub enabled: bool,
    /// Discord application id (17–20 digits). Empty disables the feature.
    pub client_id: String,
    /// Publish the cover as `large_image` when an HTTPS URL is known.
    pub show_cover: bool,
    /// Publish elapsed/remaining timestamps.
    pub show_timestamps: bool,
    /// Publish the bit-perfect badge as `small_text`.
    pub show_quality: bool,
    /// Keep the current record visible while playback is paused. When false,
    /// pausing clears the activity without forgetting it; resuming restores
    /// the same track and its position immediately.
    pub show_paused: bool,
}

impl Default for DiscordConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            client_id: std::env::var(CLIENT_ID_ENV).unwrap_or_default(),
            show_cover: true,
            show_timestamps: true,
            show_quality: true,
            show_paused: true,
        }
    }
}

impl DiscordConfig {
    /// Load the persisted configuration, falling back to defaults (which read
    /// [`CLIENT_ID_ENV`]) when the row is missing or corrupt.
    pub async fn load(pool: &sqlx::SqlitePool) -> Self {
        let stored: Option<String> =
            sqlx::query_scalar("SELECT value FROM settings WHERE key = ?1")
                .bind(SETTINGS_KEY)
                .fetch_optional(pool)
                .await
                .ok()
                .flatten();
        match stored {
            Some(json) => serde_json::from_str(&json).unwrap_or_default(),
            None => Self::default(),
        }
    }

    /// Persist the configuration as JSON in the `settings` table.
    pub async fn save(&self, pool: &sqlx::SqlitePool) -> Result<(), crate::error::PrismError> {
        let json = serde_json::to_string(self)
            .map_err(|error| crate::error::PrismError::Internal(error.to_string()))?;
        sqlx::query(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, \
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')",
        )
        .bind(SETTINGS_KEY)
        .bind(json)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Validate the client id: Discord application ids are numeric snowflakes.
    pub fn validate(&self) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }
        if self.client_id.trim().is_empty() {
            return Err(format!(
                "rich presence is enabled but no Discord application id is configured \
                 (set {CLIENT_ID_ENV} or paste one in Settings)"
            ));
        }
        if !self
            .client_id
            .chars()
            .all(|character| character.is_ascii_digit())
        {
            return Err("the Discord application id must contain digits only".to_string());
        }
        if !(17..=20).contains(&self.client_id.trim().len()) {
            return Err("the Discord application id must be 17 to 20 digits long".to_string());
        }
        Ok(())
    }
}

/// Presence snapshot handed to Discord.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackPresence {
    pub track_id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub path: Option<String>,
    pub duration_ms: Option<i64>,
    pub position_ms: i64,
    pub playing: bool,
    pub bit_perfect: bool,
}

impl TrackPresence {
    /// Build from an engine snapshot; `None` when nothing is loaded.
    #[must_use]
    pub fn from_engine(status: &EngineStatus) -> Option<Self> {
        let track = status.track.as_ref()?;
        let sample_rate = status.source_format.map(|format| format.sample_rate);
        let to_ms = |frames: u64| -> i64 {
            let rate = sample_rate.filter(|rate| *rate > 0).unwrap_or(44_100);
            i64::try_from(frames.saturating_mul(1_000) / u64::from(rate)).unwrap_or(i64::MAX)
        };
        Some(Self {
            track_id: track.track_id.clone(),
            title: track.title.clone(),
            artist: track.artist.clone(),
            album: track.album.clone(),
            path: Some(track.path.clone()),
            duration_ms: status.duration_frames.map(to_ms).filter(|ms| *ms > 0),
            position_ms: to_ms(status.position_frames),
            playing: status.state == PlaybackState::Playing,
            bit_perfect: status.bit_perfect,
        })
    }
}

#[derive(Debug, Default)]
struct Inner {
    config: DiscordConfig,
    rpc: Option<Arc<DiscordRpc>>,
    last: Option<TrackPresence>,
    last_activity: Option<Activity>,
}

/// Application-wide Rich Presence service.
#[derive(Debug)]
pub struct DiscordService {
    artwork: Arc<ArtworkService>,
    overrides: ArtworkOverrides,
    inner: RwLock<Inner>,
    generation: AtomicU64,
}

impl DiscordService {
    #[must_use]
    pub fn new(artwork: Arc<ArtworkService>) -> Self {
        Self {
            artwork,
            overrides: ArtworkOverrides::new(None),
            inner: RwLock::new(Inner::default()),
            generation: AtomicU64::new(0),
        }
    }

    /// Enable the local, user-editable album URL mapping used by production.
    #[must_use]
    pub fn with_artwork_overrides(mut self, path: PathBuf) -> Self {
        self.overrides = ArtworkOverrides::new(Some(path));
        self
    }

    /// Apply a configuration: (re)starts or stops the RPC worker.
    ///
    /// Returns the resulting status. Validation errors leave the previous
    /// configuration untouched.
    pub fn configure(&self, config: DiscordConfig) -> Result<DiscordStatus, String> {
        config.validate()?;

        let mut inner = self.inner.write();
        // Stop the previous worker before replacing it so two sockets can
        // never publish conflicting activities.
        if let Some(rpc) = inner.rpc.take() {
            rpc.stop();
        }

        inner.config = config.clone();
        inner.last_activity = None;

        if config.enabled {
            let rpc = Arc::new(DiscordRpc::spawn(config.client_id.clone()));
            let status = rpc.status();
            inner.rpc = Some(rpc);
            tracing::info!(client_id = %config.client_id, "discord rich presence enabled");
            Ok(status)
        } else {
            tracing::info!("discord rich presence disabled");
            Ok(DiscordStatus::disabled(config.client_id))
        }
    }

    /// Current state of the connection plus the active configuration.
    #[must_use]
    pub fn status(&self) -> (DiscordConfig, DiscordStatus) {
        let inner = self.inner.read();
        let status = inner.rpc.as_ref().map_or_else(
            || DiscordStatus::disabled(inner.config.client_id.clone()),
            |rpc| rpc.status(),
        );
        (inner.config.clone(), status)
    }

    async fn artwork_identity(&self, presence: &TrackPresence) -> (String, String) {
        self.artwork
            .identity_for_track(&presence.track_id)
            .await
            .unwrap_or_else(|| (presence.artist.clone(), presence.album.clone()))
    }

    /// Push an engine snapshot to Discord.
    ///
    /// Cheap to call on every engine event: it renders an [`Activity`] and
    /// returns without touching the network when nothing visible changed.
    pub async fn sync(&self, status: &EngineStatus) {
        let presence = TrackPresence::from_engine(status);
        self.publish(presence).await;
    }

    /// Publish a prepared presence (used by [`Self::sync`] and by tests).
    pub async fn publish(&self, presence: Option<TrackPresence>) {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;

        let (config, rpc) = {
            let mut inner = self.inner.write();
            let config = inner.config.clone();
            let Some(rpc) = inner.rpc.clone() else {
                inner.last = presence;
                return;
            };
            if presence.is_none() {
                rpc.clear();
                inner.last = None;
                inner.last_activity = None;
                return;
            }
            inner.last = presence.clone();
            (config, rpc)
        };

        // Hiding a paused record is a presentation choice, not a stop command.
        // Keep `inner.last` and the RPC worker alive so resume can publish the
        // very same record without waiting for another track change.
        if presence.as_ref().is_some_and(|track| !track.playing) && !config.show_paused {
            rpc.clear();
            self.inner.write().last_activity = None;
            return;
        }

        // A manual URL is local configuration, so it is even faster than a
        // provider lookup and can be included in the first activity update.
        let artwork_identity = match presence.as_ref() {
            Some(track) if config.show_cover => Some(self.artwork_identity(track).await),
            _ => None,
        };
        let manual_url = match artwork_identity.as_ref() {
            Some((artist, album)) if !album.trim().is_empty() => {
                self.overrides.url(artist, album).await
            }
            _ => None,
        };
        if self.generation.load(Ordering::SeqCst) != generation {
            return;
        }

        // 1. Text goes out immediately, so a track change is visible before
        //    any artwork request has had time to hit the network.
        if let Some(activity) = self.activity_for(&config, presence.as_ref(), manual_url.clone()) {
            self.push_if_changed(&rpc, activity);
        }
        if manual_url.is_some() {
            return;
        }

        let Some(presence) = presence else { return };
        let Some((artwork_artist, artwork_album)) = artwork_identity else {
            return;
        };
        if !config.show_cover || artwork_album.trim().is_empty() {
            return;
        }

        // 2. Artwork resolves asynchronously and republishes when ready.
        // A disc-two folder often has no cover of its own. Resolve the first
        // file of the album instead of caching a false negative for the whole
        // record merely because the current track lives in `CD2/`.
        let hint = self
            .artwork
            .hint_for_track(
                &presence.track_id,
                presence.path.as_ref().map(PathBuf::from),
            )
            .await;
        let cover = self
            .artwork
            .ensure(&artwork_artist, &artwork_album, &hint)
            .await;

        // A newer track superseded this lookup while it was in flight.
        if self.generation.load(Ordering::SeqCst) != generation {
            return;
        }

        // Local artwork has no URL, and Discord cannot display a file. When the
        // cover came from an embedded tag or the album folder — the normal case
        // for a private library — ask the providers for a public URL of the same
        // record. Without this the presence showed text and a generic asset for
        // every track a listener actually owned.
        let url = match cover.remote_url {
            Some(url) if url.starts_with("https://") => Some(url),
            _ => {
                self.artwork
                    .remote_url(&artwork_artist, &artwork_album)
                    .await
            }
        };
        if self.generation.load(Ordering::SeqCst) != generation {
            return;
        }
        let Some(url) = url else {
            if let Err(error) = self
                .overrides
                .record_missing(&artwork_artist, &artwork_album)
                .await
            {
                tracing::warn!(%error, "cannot record missing Discord artwork");
            }
            return;
        };

        if let Some(activity) = self.activity_for(&config, Some(&presence), Some(url)) {
            self.push_if_changed(&rpc, activity);
        }
    }

    /// Re-read only the local URL mapping while a track remains loaded. This
    /// lets a manual edit appear without a track change and never hits a remote
    /// artwork provider.
    pub async fn refresh_artwork_override(&self) {
        let generation = self.generation.load(Ordering::SeqCst);
        let (config, rpc, presence) = {
            let inner = self.inner.read();
            (inner.config.clone(), inner.rpc.clone(), inner.last.clone())
        };
        let (Some(rpc), Some(presence)) = (rpc, presence) else {
            return;
        };
        if !config.show_cover || presence.album.trim().is_empty() {
            return;
        }
        let (artist, album) = self.artwork_identity(&presence).await;
        let Some(url) = self.overrides.url(&artist, &album).await else {
            return;
        };
        if self.generation.load(Ordering::SeqCst) != generation {
            return;
        }
        if let Some(activity) = self.activity_for(&config, Some(&presence), Some(url)) {
            self.push_if_changed(&rpc, activity);
        }
    }

    /// Send an activity only when it differs from the last one published.
    fn push_if_changed(&self, rpc: &DiscordRpc, activity: Activity) {
        let changed = {
            let mut inner = self.inner.write();
            if inner.last_activity.as_ref() == Some(&activity) {
                false
            } else {
                inner.last_activity = Some(activity.clone());
                true
            }
        };
        if changed {
            rpc.set_activity(activity);
        }
    }

    /// Stop the worker and clear the activity.
    pub fn shutdown(&self) {
        let mut inner = self.inner.write();
        if let Some(rpc) = inner.rpc.take() {
            rpc.clear();
            rpc.stop();
        }
        inner.last = None;
        inner.last_activity = None;
    }

    fn activity_for(
        &self,
        config: &DiscordConfig,
        presence: Option<&TrackPresence>,
        cover_url: Option<String>,
    ) -> Option<Activity> {
        let presence = presence?;
        let title = presence.title.trim();
        let artist = presence.artist.trim();
        let album = presence.album.trim();

        // Discord exposes two activity lines plus the cover caption. Use each
        // field once: song, artist, album. The old renderer put the album in
        // both `state` and `large_text`, which is why it appeared twice.
        let details = if !title.is_empty() {
            title.to_string()
        } else if !artist.is_empty() {
            artist.to_string()
        } else {
            "Unknown track".to_string()
        };
        let state = if !artist.is_empty() {
            artist.to_string()
        } else if !album.is_empty() {
            album.to_string()
        } else if presence.playing {
            "Playing".to_string()
        } else {
            "Paused".to_string()
        };

        let now_ms = i64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_millis())
                .unwrap_or(0),
        )
        .unwrap_or(0);
        let (start_timestamp, end_timestamp) = if config.show_timestamps && presence.playing {
            let start = now_ms - presence.position_ms.max(0);
            let end = presence
                .duration_ms
                .map(|duration| start + duration.max(0))
                .filter(|end| *end > now_ms);
            (Some(start), end)
        } else {
            (None, None)
        };

        Some(Activity {
            details,
            state,
            start_timestamp,
            end_timestamp,
            large_image: cover_url,
            large_text: (!album.is_empty()).then(|| album.to_string()),
            small_image: Some(if presence.playing {
                ASSET_PLAYING.to_string()
            } else {
                ASSET_PAUSED.to_string()
            }),
            small_text: if !presence.playing {
                Some("Paused".to_string())
            } else {
                config
                    .show_quality
                    .then_some(if presence.bit_perfect {
                        "Bit-perfect"
                    } else {
                        "Prism"
                    })
                    .map(ToString::to_string)
            },
        })
    }
}

impl Drop for DiscordService {
    fn drop(&mut self) {
        // Best effort: clear the activity and close the socket when the app
        // tears down, so Discord never shows a stale track after quitting.
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Service whose artwork never resolves; these tests only cover rendering.
    fn test_service() -> DiscordService {
        let directory =
            std::env::temp_dir().join(format!("prism-discord-{}", uuid::Uuid::new_v4()));
        let pool = sqlx::SqlitePool::connect_lazy("sqlite::memory:").expect("lazy pool");
        DiscordService::new(Arc::new(
            ArtworkService::new(pool, &directory).expect("artwork service"),
        ))
    }

    fn presence() -> TrackPresence {
        TrackPresence {
            track_id: "track-1".into(),
            title: "Everything In Its Right Place".into(),
            artist: "Radiohead".into(),
            album: "Kid A".into(),
            path: None,
            duration_ms: Some(240_000),
            position_ms: 60_000,
            playing: true,
            bit_perfect: true,
        }
    }

    #[test]
    fn client_id_validation_matches_discord_snowflakes() {
        let mut config = DiscordConfig {
            enabled: true,
            client_id: "123456789012345678".into(),
            ..DiscordConfig::default()
        };
        assert!(config.validate().is_ok());
        config.client_id = "12345".into();
        assert!(config.validate().is_err());
        config.client_id = "abcdefghijklmnopq".into();
        assert!(config.validate().is_err());
        // A disabled configuration never requires an id.
        config.enabled = false;
        config.client_id.clear();
        assert!(config.validate().is_ok());
    }

    #[tokio::test]
    async fn activity_renders_track_album_and_timestamps() {
        let service = test_service();
        let activity = service
            .activity_for(&DiscordConfig::default(), Some(&presence()), None)
            .expect("activity");
        assert_eq!(activity.details, "Everything In Its Right Place");
        assert_eq!(activity.state, "Radiohead");
        assert_eq!(activity.large_text.as_deref(), Some("Kid A"));
        let start = activity.start_timestamp.expect("start");
        let end = activity.end_timestamp.expect("end");
        assert_eq!(end - start, 240_000);
        assert_eq!(activity.small_text.as_deref(), Some("Bit-perfect"));
    }

    #[tokio::test]
    async fn paused_tracks_have_no_countdown() {
        let service = test_service();
        let mut paused = presence();
        paused.playing = false;
        let activity = service
            .activity_for(&DiscordConfig::default(), Some(&paused), None)
            .expect("activity");
        assert!(activity.start_timestamp.is_none());
        assert_eq!(activity.state, "Radiohead");
        assert_eq!(activity.small_image.as_deref(), Some(ASSET_PAUSED));
        assert_eq!(activity.small_text.as_deref(), Some("Paused"));
    }

    #[tokio::test]
    async fn hidden_pause_is_forgotten_visually_but_not_for_resume() {
        let service = test_service();
        service
            .configure(DiscordConfig {
                enabled: true,
                client_id: "123456789012345678".into(),
                show_cover: false,
                show_paused: false,
                ..DiscordConfig::default()
            })
            .expect("configure");
        let mut track = presence();
        track.playing = false;
        service.publish(Some(track.clone())).await;
        assert_eq!(service.inner.read().last_activity, None);
        assert_eq!(service.inner.read().last.as_ref(), Some(&track));

        track.playing = true;
        service.publish(Some(track)).await;
        assert!(service.inner.read().last_activity.is_some());
        service.shutdown();
    }

    #[tokio::test]
    async fn a_resolved_cover_is_published_as_an_external_asset() {
        let service = test_service();
        let url = "https://is1-ssl.mzstatic.com/image/thumb/600x600bb.jpg";
        let activity = service
            .activity_for(
                &DiscordConfig::default(),
                Some(&presence()),
                Some(url.to_string()),
            )
            .expect("activity");
        let json = activity.to_json();
        // Discord renders external artwork only from `assets.large_image`, and
        // only when the value is the full HTTPS URL — this is the wire format
        // that the presence has to carry for the cover to appear at all.
        assert_eq!(json["assets"]["large_image"], serde_json::json!(url));
        assert_eq!(json["assets"]["large_text"], serde_json::json!("Kid A"));
    }

    #[tokio::test]
    async fn artwork_overrides_are_created_read_and_preserved() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("discord-artwork.json");
        let overrides = ArtworkOverrides::new(Some(path.clone()));
        overrides
            .record_missing("DJ Example", "Unsigned Mixtape")
            .await
            .expect("record missing");
        overrides
            .record_missing("DJ Example", "Unsigned Mixtape")
            .await
            .expect("idempotent record");

        let key = ArtworkService::key("DJ Example", "Unsigned Mixtape");
        let mut file = overrides.read(&path).await.expect("read overrides");
        assert_eq!(file.albums.len(), 1);
        assert_eq!(
            file.albums.get(&key).and_then(|entry| entry.url.as_deref()),
            None
        );
        file.albums.get_mut(&key).expect("entry").url =
            Some("https://images.example.com/mixtape.jpg".into());
        tokio::fs::write(&path, serde_json::to_string_pretty(&file).expect("json"))
            .await
            .expect("manual edit");
        assert_eq!(
            overrides
                .url("DJ Example", "Unsigned Mixtape")
                .await
                .as_deref(),
            Some("https://images.example.com/mixtape.jpg")
        );
        overrides
            .record_missing("DJ Example", "Unsigned Mixtape")
            .await
            .expect("preserve manual url");
        assert!(
            overrides
                .url("DJ Example", "Unsigned Mixtape")
                .await
                .is_some()
        );

        file.albums.get_mut(&key).expect("entry").url =
            Some("http://insecure.example.com/mixtape.jpg".into());
        tokio::fs::write(&path, serde_json::to_string(&file).expect("json"))
            .await
            .expect("manual insecure edit");
        assert_eq!(overrides.url("DJ Example", "Unsigned Mixtape").await, None);
    }

    #[tokio::test]
    async fn empty_metadata_still_produces_a_valid_activity() {
        let service = test_service();
        let activity = service
            .activity_for(
                &DiscordConfig::default(),
                Some(&TrackPresence {
                    track_id: String::new(),
                    title: String::new(),
                    artist: String::new(),
                    album: String::new(),
                    path: None,
                    duration_ms: None,
                    position_ms: 0,
                    playing: false,
                    bit_perfect: false,
                }),
                None,
            )
            .expect("activity");
        assert_eq!(activity.details, "Unknown track");
        assert_eq!(activity.state, "Paused");
        // Discord requires at least two characters per field.
        assert!(activity.to_json()["details"].as_str().unwrap().len() >= 2);
    }
}
