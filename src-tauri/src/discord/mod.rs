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

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::artwork::{ArtworkService, CoverHint};
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
}

impl Default for DiscordConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            client_id: std::env::var(CLIENT_ID_ENV).unwrap_or_default(),
            show_cover: true,
            show_timestamps: true,
            show_quality: true,
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
    inner: RwLock<Inner>,
    generation: AtomicU64,
}

impl DiscordService {
    #[must_use]
    pub fn new(artwork: Arc<ArtworkService>) -> Self {
        Self {
            artwork,
            inner: RwLock::new(Inner::default()),
            generation: AtomicU64::new(0),
        }
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

        // 1. Text goes out immediately, so a track change is visible before
        //    any artwork request has had time to hit the network.
        if let Some(activity) = self.activity_for(&config, presence.as_ref(), None) {
            self.push_if_changed(&rpc, activity);
        }

        let Some(presence) = presence else { return };
        if !config.show_cover || presence.album.trim().is_empty() {
            return;
        }

        // 2. Artwork resolves asynchronously and republishes when ready.
        let hint = CoverHint {
            track_path: presence.path.as_ref().map(std::path::PathBuf::from),
            folder: presence.path.as_ref().and_then(|path| {
                std::path::Path::new(path)
                    .parent()
                    .map(std::path::Path::to_path_buf)
            }),
        };
        let cover = self
            .artwork
            .ensure(&presence.artist, &presence.album, &hint)
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
                    .remote_url(&presence.artist, &presence.album)
                    .await
            }
        };
        if self.generation.load(Ordering::SeqCst) != generation {
            return;
        }
        let Some(url) = url else { return };

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

        let details = match (title.is_empty(), artist.is_empty()) {
            (false, false) => format!("{title} — {artist}"),
            (false, true) => title.to_string(),
            (true, false) => artist.to_string(),
            (true, true) => "Idle".to_string(),
        };
        let state = match (album.is_empty(), presence.playing) {
            (false, true) => album.to_string(),
            (false, false) => format!("{album} · Paused"),
            (true, true) => "Playing".to_string(),
            (true, false) => "Paused".to_string(),
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
            small_text: config
                .show_quality
                .then_some(if presence.bit_perfect {
                    "Bit-perfect"
                } else {
                    "Prism"
                })
                .map(ToString::to_string),
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
        assert_eq!(
            activity.details,
            "Everything In Its Right Place — Radiohead"
        );
        assert_eq!(activity.state, "Kid A");
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
        assert_eq!(activity.state, "Kid A · Paused");
        assert_eq!(activity.small_image.as_deref(), Some(ASSET_PAUSED));
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
    async fn empty_metadata_still_produces_a_valid_activity() {
        let service = test_service();
        let activity = service
            .activity_for(
                &DiscordConfig::default(),
                Some(&TrackPresence {
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
        assert_eq!(activity.details, "Idle");
        assert_eq!(activity.state, "Paused");
        // Discord requires at least two characters per field.
        assert!(activity.to_json()["details"].as_str().unwrap().len() >= 2);
    }
}
