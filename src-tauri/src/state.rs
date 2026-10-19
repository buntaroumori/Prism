//! Process-wide application state shared by Tauri commands.

use std::path::{Path, PathBuf};
use std::time::Instant;

use sqlx::SqlitePool;
use tauri::Manager;
use tauri::path::PathResolver;

use crate::db::{self, DATABASE_FILENAME};
use crate::error::PrismError;

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

        Ok(Self {
            pool,
            paths,
            started_at: Instant::now(),
        })
    }

    #[must_use]
    pub const fn pool(&self) -> &SqlitePool {
        &self.pool
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
