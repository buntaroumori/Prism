//! `SQLite` persistence layer.
//!
//! Design decisions:
//!   * **`WAL` journal** — readers never block the writer and the writer never
//!     blocks readers. Mandatory for a UI that scrolls 500k rows while a scan
//!     imports new files.
//!   * **One shared pool** (`max_connections = 6`). `SQLite` serialises writers
//!     internally; a wider pool only adds contention. Six connections cover
//!     the scanner writer + search + UI reads + Discord/lyrics fetchers.
//!   * **`mmap_size = 256 MiB`** — the OS page cache serves the hot B-tree
//!     pages without `read()` syscalls once the library is warm.
//!   * **`synchronous = NORMAL`** under `WAL` — durability is guaranteed against
//!     application crashes; only an OS-level power loss can drop the last
//!     commit batch, which is acceptable for a resumable library index.
//!   * **Foreign keys ON** for every pooled connection, set at connect time.
//!   * **Migrations embedded** with `sqlx::migrate!` so a packaged binary is
//!     self-contained and cannot start on a partially initialised schema.

use std::path::Path;
use std::time::Duration;

use serde::Serialize;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{ConnectOptions, SqlitePool};

use crate::error::PrismError;

pub mod catalog;
pub mod search;

/// File name of the library database inside the application data directory.
pub const DATABASE_FILENAME: &str = "prism.db";

/// Open (creating if needed) the Prism library database, apply pending
/// migrations and return a ready-to-use pool.
///
/// This is the single entry point used both by the application and by the
/// integration tests, which is why it takes a path instead of a Tauri handle.
pub async fn connect(database_path: &Path) -> Result<SqlitePool, PrismError> {
    if let Some(parent) = database_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|err| PrismError::io(parent, err))?;
    }

    let options = SqliteConnectOptions::new()
        .filename(database_path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5))
        .pragma("temp_store", "MEMORY")
        .pragma("cache_size", "-32768") // 32 MiB, negative = KiB
        .pragma("mmap_size", "268435456") // 256 MiB
        .pragma("wal_autocheckpoint", "1000") // pages (~4 MiB at 4 KiB pages)
        .pragma("journal_size_limit", "67108864") // 64 MiB; truncate WAL after checkpoint
        .pragma("analysis_limit", "400") // bounded ANALYZE introduced in 3.43
        .log_statements(tracing::log::LevelFilter::Debug);

    let pool = SqlitePoolOptions::new()
        .max_connections(6)
        .min_connections(1)
        .acquire_timeout(Duration::from_secs(5))
        .idle_timeout(Duration::from_secs(300))
        .max_lifetime(Duration::from_secs(1_800))
        .connect_with(options)
        .await?;

    migrate(&pool).await?;

    Ok(pool)
}

/// Apply all embedded migrations. Verified to be idempotent: sqlx tracks
/// applied versions in `_sqlx_migrations` and runs pending files in order.
pub async fn migrate(pool: &SqlitePool) -> Result<(), PrismError> {
    sqlx::migrate!("./migrations").run(pool).await?;

    // Repair the album → artist links of databases written before 0.1.2.
    // `ensure_albums` used to insert records without `album_artist_id`, so every
    // album in an existing library has a null link and no tile can reach its
    // artist. Idempotent, bounded by the number of albums, and executed once per
    // launch before anything reads the catalogue.
    sqlx::query(
        "UPDATE albums SET \
            album_artist_id = COALESCE(album_artist_id, \
                (SELECT ar.id FROM artists ar \
                  WHERE ar.name = albums.album_artist_name COLLATE NOCASE LIMIT 1)), \
            artist_id = COALESCE(artist_id, album_artist_id, \
                (SELECT ar.id FROM artists ar \
                  WHERE ar.name = albums.album_artist_name COLLATE NOCASE LIMIT 1)) \
         WHERE album_artist_id IS NULL AND album_artist_name <> ''",
    )
    .execute(pool)
    .await?;

    // A database that silently refused WAL would still work, but every
    // performance claim of the project would be a lie. Fail loudly instead.
    let mode: String = sqlx::query_scalar("PRAGMA journal_mode;")
        .fetch_one(pool)
        .await?;
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(PrismError::Install(format!(
            "SQLite refused WAL mode (reported '{mode}'); the database file may be on a \
             filesystem without shared-memory support"
        )));
    }

    Ok(())
}

/// Serialisable snapshot of database health, surfaced to the Settings panel and
/// to the diagnostics export.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseHealth {
    pub journal_mode: String,
    pub synchronous: i64,
    pub page_size: i64,
    pub page_count: i64,
    pub freelist_count: i64,
    pub size_bytes: i64,
    pub migration_version: i64,
    pub applied_migrations: i64,
    pub integrity: String,
    pub pool_size: u32,
    pub pool_idle: u32,
}

/// Collect health data with cheap pragmas plus one bounded `quick_check`.
///
/// `quick_check(1)` stops after the first inconsistency and never scans the
/// whole library, so it is safe to call from the UI on demand.
pub async fn health(pool: &SqlitePool) -> Result<DatabaseHealth, PrismError> {
    let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode;")
        .fetch_one(pool)
        .await?;
    let synchronous: i64 = sqlx::query_scalar("PRAGMA synchronous;")
        .fetch_one(pool)
        .await?;
    let page_size: i64 = sqlx::query_scalar("PRAGMA page_size;")
        .fetch_one(pool)
        .await?;
    let page_count: i64 = sqlx::query_scalar("PRAGMA page_count;")
        .fetch_one(pool)
        .await?;
    let freelist_count: i64 = sqlx::query_scalar("PRAGMA freelist_count;")
        .fetch_one(pool)
        .await?;
    let integrity: Option<String> = sqlx::query_scalar("PRAGMA quick_check(1);")
        .fetch_optional(pool)
        .await?;
    let migration_version: i64 =
        sqlx::query_scalar("SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations;")
            .fetch_one(pool)
            .await?;
    let applied_migrations: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations;")
        .fetch_one(pool)
        .await?;

    Ok(DatabaseHealth {
        journal_mode,
        synchronous,
        page_size,
        page_count,
        freelist_count,
        size_bytes: page_size.saturating_mul(page_count),
        migration_version,
        applied_migrations,
        integrity: integrity.unwrap_or_else(|| "unknown".to_string()),
        pool_size: pool.size(),
        pool_idle: u32::try_from(pool.num_idle()).unwrap_or(u32::MAX),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn creates_nested_database_directories() {
        // Smoke test for the path helper: connect() into a nested directory
        // must create it instead of failing.
        let dir = tempfile::tempdir().expect("tempdir");
        let nested = dir.path().join("deeply/nested/library.db");
        let pool = connect(&nested).await.expect("connect");
        assert!(nested.exists());
        pool.close().await;
    }
}
