//! Process-wide application state shared by Tauri commands.

use std::path::{Path, PathBuf};
use std::time::Instant;

use sqlx::SqlitePool;
use tauri::Manager;
use tauri::path::PathResolver;

use crate::artwork::ArtworkService;
use crate::audio::engine::{Engine, EngineConfig};
use crate::db::{self, DATABASE_FILENAME};
use crate::discord::{DiscordConfig, DiscordService};
use crate::error::PrismError;
use crate::metadata::Scanner;
use crate::palette::PaletteService;
use crate::plugins::PluginManager;

/// Canonical filesystem locations used by Prism.
///
/// All of them live under the platform application directories resolved by
/// Tauri from the bundle identifier (`io.prism.player`), which on Windows is
/// `%APPDATA%\io.prism.player` and on Linux follows the XDG base directory
/// specification (`~/.local/share/io.prism.player`, `~/.cache/...`, ...).
#[derive(Debug, Clone)]
pub struct AppPaths {
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub log_dir: PathBuf,
    pub database: PathBuf,
}

impl AppPaths {
    /// Resolve and create every directory Prism writes to.
    pub async fn resolve<R: tauri::Runtime>(
        resolver: &PathResolver<R>,
    ) -> Result<Self, PrismError> {
        let data_dir = resolver.app_data_dir()?;
        let cache_dir = resolver.app_cache_dir()?;
        let log_dir = resolver.app_log_dir()?;

        for dir in [&data_dir, &cache_dir, &log_dir] {
            tokio::fs::create_dir_all(dir)
                .await
                .map_err(|err| PrismError::io(dir, err))?;
        }

        Ok(Self {
            database: data_dir.join(DATABASE_FILENAME),
            data_dir,
            cache_dir,
            log_dir,
        })
    }

    /// Directory where extracted / downloaded cover art is stored.
    #[must_use]
    pub fn artwork_dir(&self) -> PathBuf {
        self.cache_dir.join("artwork")
    }

    /// Directory where waveform peaks and analysis caches are stored.
    #[must_use]
    pub fn analysis_dir(&self) -> PathBuf {
        self.cache_dir.join("analysis")
    }
}

/// Shared state managed by Tauri and injected into every command.
#[derive(Debug)]
pub struct AppState {
    pool: SqlitePool,
    paths: AppPaths,
    started_at: Instant,
    engine: std::sync::Arc<Engine>,
    scanner: Scanner,
    artwork: std::sync::Arc<ArtworkService>,
    discord: std::sync::Arc<DiscordService>,
    palette: std::sync::Arc<PaletteService>,
    plugins: std::sync::Arc<PluginManager>,
    plugin_messages: parking_lot::Mutex<
        Option<tokio::sync::mpsc::UnboundedReceiver<crate::plugins::PluginMessage>>,
    >,
}

impl AppState {
    /// Build the state: resolve paths, open the pool, apply migrations.
    ///
    /// Called once from Tauri's `setup` hook before the main window becomes
    /// visible, so the UI never renders against a half-initialised database.
    pub async fn initialize(app: &tauri::AppHandle) -> Result<Self, PrismError> {
        let paths = AppPaths::resolve(app.path()).await?;
        tracing::debug!(database = %paths.database.display(), "opening library database");

        let pool = db::connect(&paths.database).await?;

        // The audio engine starts lazily: its supervisor thread only opens a
        // device when the first track is queued, so boot stays under 300 ms.
        let engine = Engine::new(EngineConfig::default())?;
        tracing::debug!("audio engine supervisor started");

        // --- Library services --------------------------------------------
        let scanner = Scanner::new(pool.clone());
        let artwork = std::sync::Arc::new(ArtworkService::new(pool.clone(), paths.artwork_dir())?);
        let discord = std::sync::Arc::new(DiscordService::new(std::sync::Arc::clone(&artwork)));
        // Palettes are derived from the same files the artwork service writes,
        // so both share the directory and neither owns the other.
        let palette = std::sync::Arc::new(PaletteService::new(paths.artwork_dir()));

        // --- Extensibility -------------------------------------------------
        // Plugins are optional and never load-bearing: a broken plugin leaves
        // the player fully functional, and the manager records why in its
        // descriptor. The message stream is handed to the event bridge in
        // `lib.rs`, which forwards it to the webview.
        let (plugins, plugin_messages) = PluginManager::bootstrap(
            paths.data_dir.join("plugins"),
            pool.clone(),
            std::sync::Arc::clone(&engine),
        )
        .await?;

        // Rich presence is opt-in; a stored configuration is applied only when
        // it validates, so a broken client id cannot block startup.
        let discord_config = DiscordConfig::load(&pool).await;
        if discord_config.enabled {
            match discord.configure(discord_config) {
                Ok(status) => tracing::debug!(?status.state, "rich presence restored"),
                Err(error) => tracing::warn!(%error, "stored rich presence configuration rejected"),
            }
        }

        // Expired cover rows are swept in the background: a cache that is never
        // pruned grows without bound on a large library.
        let sweeper = std::sync::Arc::clone(&artwork);
        tokio::spawn(async move {
            match sweeper.sweep().await {
                Ok(0) => {}
                Ok(removed) => tracing::debug!(removed, "pruned expired cover cache rows"),
                Err(error) => tracing::warn!(%error, "cover cache sweep failed"),
            }
        });

        Ok(Self {
            pool,
            paths,
            started_at: Instant::now(),
            engine,
            scanner,
            artwork,
            discord,
            palette,
            plugins: std::sync::Arc::new(plugins),
            plugin_messages: parking_lot::Mutex::new(Some(plugin_messages)),
        })
    }

    #[must_use]
    pub const fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    #[must_use]
    pub fn engine(&self) -> &std::sync::Arc<Engine> {
        &self.engine
    }

    #[must_use]
    pub const fn scanner(&self) -> &Scanner {
        &self.scanner
    }

    #[must_use]
    pub fn artwork(&self) -> &std::sync::Arc<ArtworkService> {
        &self.artwork
    }

    #[must_use]
    pub fn discord(&self) -> &std::sync::Arc<DiscordService> {
        &self.discord
    }

    #[must_use]
    pub fn palette(&self) -> &std::sync::Arc<PaletteService> {
        &self.palette
    }

    #[must_use]
    pub fn plugins(&self) -> &std::sync::Arc<PluginManager> {
        &self.plugins
    }

    /// Take the plugin message stream exactly once, at boot.
    pub fn take_plugin_messages(
        &self,
    ) -> Option<tokio::sync::mpsc::UnboundedReceiver<crate::plugins::PluginMessage>> {
        self.plugin_messages.lock().take()
    }

    #[must_use]
    pub const fn paths(&self) -> &AppPaths {
        &self.paths
    }

    #[must_use]
    pub fn database_path(&self) -> &Path {
        &self.paths.database
    }

    /// Milliseconds since the state finished bootstrapping.
    #[must_use]
    pub fn uptime_ms(&self) -> u64 {
        u64::try_from(self.started_at.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
}
