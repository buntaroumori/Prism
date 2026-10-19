//! Integration tests for the persistence layer.
//!
//! These run against a real `SQLite` file (never `:memory:`) so WAL, the
//! migration ledger and pragma behaviour are exercised exactly as in
//! production. Each test gets its own temporary directory.

use prism_lib::db::{connect, health};
use sqlx::SqlitePool;

/// Seed one artist / album / track plus a CUE virtual track sharing an image
/// path, mirroring what the scanner will insert in FASE 3.
async fn seed(pool: &SqlitePool) {
    sqlx::query(
        "INSERT INTO artists (id, name, sort_name, musicbrainz_artist_id) \
         VALUES ('artist-bjork', 'Björk', 'Bjork', '875c1f7e-4b1d-4c5b-9f6b-6e0f1d2c3a4b')",
    )
    .execute(pool)
    .await
    .expect("insert artist");

    sqlx::query(
        "INSERT INTO albums (id, title, sort_title, artist_id, album_artist_id, album_artist_name, year) \
         VALUES ('album-homogenic', 'Homogenic', 'Homogenic', 'artist-bjork', 'artist-bjork', 'Björk', 1997)",
    )
    .execute(pool)
    .await
    .expect("insert album");

    sqlx::query(
        "INSERT INTO cue_sheets (id, album_id, path, album_image, sha256, content) \
         VALUES ('cue-1', 'album-homogenic', '/music/Homogenic.cue', '/music/Homogenic.flac', \
                 '0000000000000000000000000000000000000000000000000000000000000000', \
                 'FILE \"Homogenic.flac\" WAVE')",
    )
    .execute(pool)
    .await
    .expect("insert cue sheet");

    sqlx::query(
        "INSERT INTO tracks (id, path, folder, filename, title, artist_id, artist_name, \
                             album_id, album_title, album_artist_id, album_artist_name, genre, year, \
                             track_no, format, bit_depth, sample_rate, channels, duration_ms, file_size) \
         VALUES ('track-1', '/music/Homogenic/01 Hunter.flac', '/music/Homogenic', '01 Hunter.flac', \
                 'Hunter', 'artist-bjork', 'Björk', 'album-homogenic', 'Homogenic', 'artist-bjork', \
                 'Björk', 'Electronic', 1997, 1, 'flac', 24, 96000, 2, 260000, 42000000)",
    )
    .execute(pool)
    .await
    .expect("insert track");

    for (id, start) in [("cue-track-1", 0_i64), ("cue-track-2", 260_000_i64)] {
        sqlx::query(
            "INSERT INTO tracks (id, path, folder, filename, source, title, artist_id, artist_name, \
                                 album_id, album_title, album_artist_id, album_artist_name, track_no, \
                                 format, duration_ms, cue_sheet_id, cue_start_ms, cue_end_ms) \
             VALUES (?, '/music/Homogenic.flac', '/music', 'Homogenic.flac', 'cue', ?, 'artist-bjork', \
                     'Björk', 'album-homogenic', 'Homogenic', 'artist-bjork', 'Björk', 2, 'flac', \
                     260000, 'cue-1', ?, ?)",
        )
        .bind(id)
        .bind(format!("Virtual {id}"))
        .bind(start)
        .bind(start + 260_000)
        .execute(pool)
        .await
        .expect("insert cue track");
    }
}

#[tokio::test]
async fn bootstraps_wal_database_with_embedded_migrations() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("prism.db");
    let pool = connect(&path).await.expect("connect");

    let health = health(&pool).await.expect("health");
    assert_eq!(health.journal_mode.to_lowercase(), "wal");
    assert_eq!(health.integrity, "ok");
    assert!(health.migration_version >= 1);
    assert!(health.applied_migrations >= 1);
    assert!(health.page_size > 0);

    // Foreign keys are enforced on every pooled connection.
    let fk: i64 = sqlx::query_scalar("PRAGMA foreign_keys;")
        .fetch_one(&pool)
        .await
        .expect("pragma");
    assert_eq!(fk, 1);

    pool.close().await;
    assert!(path.exists(), "database file must be created on disk");
}

#[tokio::test]
async fn fts5_survives_cascades_and_matches_with_diacritics() {
    let dir = tempfile::tempdir().expect("tempdir");
    let pool = connect(&dir.path().join("prism.db"))
        .await
        .expect("connect");
    seed(&pool).await;

    let hits: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM tracks_fts WHERE tracks_fts MATCH 'hunter'")
            .fetch_one(&pool)
            .await
            .expect("fts query");
    assert_eq!(hits, 1);

    let folded: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM tracks_fts WHERE tracks_fts MATCH 'bjork'")
            .fetch_one(&pool)
            .await
            .expect("diacritic query");
    assert_eq!(
        folded, 3,
        "all three tracks carry the artist and must match"
    );

    // Note: `unicode61 remove_diacritics 2` folds accents (ö→o, ó→o) but deliberately
    // does not transliterate distinct letters such as ð (eth), so the assertion uses a
    // plain token from the new name instead of the surname.
    sqlx::query("UPDATE artists SET name = 'Björk Reissued' WHERE id = 'artist-bjork'")
        .execute(&pool)
        .await
        .expect("rename artist");

    let renamed: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM tracks_fts WHERE tracks_fts MATCH '\"reissued\"'")
            .fetch_one(&pool)
            .await
            .expect("renamed query");
    assert_eq!(renamed, 3, "rename cascade must keep FTS coherent");

    // External-content integrity must still hold after the cascade.
    sqlx::query("INSERT INTO tracks_fts (tracks_fts) VALUES ('integrity-check')")
        .execute(&pool)
        .await
        .expect("fts integrity check");

    pool.close().await;
}

#[tokio::test]
async fn rejects_duplicate_paths_case_insensitively() {
    let dir = tempfile::tempdir().expect("tempdir");
    let pool = connect(&dir.path().join("prism.db"))
        .await
        .expect("connect");
    seed(&pool).await;

    // Same separator style, different case: the NOCASE expression index must
    // fold it into the seeded row.
    let duplicate_case = sqlx::query(
        "INSERT INTO tracks (id, path, title, format, duration_ms) \
         VALUES ('track-dup-case', '/MUSIC/HOMOGENIC/01 HUNTER.FLAC', 'Dup', 'flac', 1)",
    )
    .execute(&pool)
    .await;
    assert!(
        duplicate_case.is_err(),
        "case-folded duplicate must be rejected"
    );

    // Windows-style separators are folded by the same expression index.
    let duplicate_separators = sqlx::query(
        "INSERT INTO tracks (id, path, title, format, duration_ms) \
         VALUES ('track-dup-sep', '/music\\Homogenic\\01 Hunter.flac', 'Dup', 'flac', 1)",
    )
    .execute(&pool)
    .await;
    assert!(
        duplicate_separators.is_err(),
        "separator-folded duplicate must be rejected"
    );

    let duplicate_exact = sqlx::query(
        "INSERT INTO tracks (id, path, title, format, duration_ms) \
         VALUES ('track-dup-exact', '/music/Homogenic/01 Hunter.flac', 'Dup', 'flac', 1)",
    )
    .execute(&pool)
    .await;
    assert!(duplicate_exact.is_err(), "exact duplicate must be rejected");

    pool.close().await;
}

#[tokio::test]
async fn foreign_keys_are_enforced() {
    let dir = tempfile::tempdir().expect("tempdir");
    let pool = connect(&dir.path().join("prism.db"))
        .await
        .expect("connect");

    let result = sqlx::query(
        "INSERT INTO tracks (id, path, title, format, duration_ms, album_id) \
         VALUES ('bad-track', '/music/x.flac', 'X', 'flac', 1000, 'does-not-exist')",
    )
    .execute(&pool)
    .await;

    assert!(result.is_err(), "dangling album reference must fail");

    pool.close().await;
}

#[tokio::test]
async fn library_views_are_aggregate_and_empty_safe() {
    let dir = tempfile::tempdir().expect("tempdir");
    let pool = connect(&dir.path().join("prism.db"))
        .await
        .expect("connect");

    // Empty library: every aggregate must be zero, never NULL.
    let (tracks, bytes): (i64, i64) =
        sqlx::query_as("SELECT track_count, total_bytes FROM v_library_stats")
            .fetch_one(&pool)
            .await
            .expect("empty stats");
    assert_eq!((tracks, bytes), (0, 0));

    seed(&pool).await;

    let (tracks, albums, artists, cue): (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT track_count, album_count, artist_count, cue_track_count FROM v_library_stats",
    )
    .fetch_one(&pool)
    .await
    .expect("stats");
    assert_eq!(tracks, 3);
    assert_eq!(albums, 1);
    assert_eq!(artists, 1);
    assert_eq!(cue, 2);

    let details: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM v_track_details")
        .fetch_one(&pool)
        .await
        .expect("details view");
    assert_eq!(details, 3);

    let album_tracks: i64 = sqlx::query_scalar(
        "SELECT available_tracks FROM v_album_summary WHERE id = 'album-homogenic'",
    )
    .fetch_one(&pool)
    .await
    .expect("album summary");
    assert_eq!(
        album_tracks, 1,
        "only physical files count as available audio"
    );

    pool.close().await;
}
