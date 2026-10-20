//! End-to-end tests for the FASE 3 library pipeline.
//!
//! Every test builds a real directory tree of real audio containers, scans it
//! through the production code path (walkdir → lofty → sqlx → FTS5) and then
//! queries it through the same read models the UI uses. Nothing is mocked:
//! the assertions cover the schema constraints, the FTS triggers, the CUE
//! expansion and the incremental-scan fingerprint.

use std::path::{Path, PathBuf};

use lofty::config::WriteOptions;
use lofty::prelude::{Accessor, ItemKey, TagExt};
use lofty::tag::{Tag, TagType};
use prism_lib::db::catalog::CatalogWriter;
use prism_lib::db::search::{self, TrackQuery, TrackSort};
use prism_lib::db::{connect, health};
use prism_lib::metadata::scanner::{ScanConfig, ScanTrigger, Scanner};
use sqlx::{Row, SqlitePool};

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

/// Write a canonical 16-bit PCM WAV of `seconds` of silence.
fn write_wav(path: &Path, seconds: f32, sample_rate: u32, channels: u16, bits: u16) {
    std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    let frames = (seconds * sample_rate as f32) as u32;
    let block_align = channels * bits / 8;
    let data_len = frames * u32::from(block_align);

    let mut buffer = Vec::with_capacity(44 + data_len as usize);
    buffer.extend_from_slice(b"RIFF");
    buffer.extend_from_slice(&(36 + data_len).to_le_bytes());
    buffer.extend_from_slice(b"WAVE");
    buffer.extend_from_slice(b"fmt ");
    buffer.extend_from_slice(&16u32.to_le_bytes());
    buffer.extend_from_slice(&1u16.to_le_bytes()); // PCM
    buffer.extend_from_slice(&channels.to_le_bytes());
    buffer.extend_from_slice(&sample_rate.to_le_bytes());
    buffer.extend_from_slice(&(sample_rate * u32::from(block_align)).to_le_bytes());
    buffer.extend_from_slice(&block_align.to_le_bytes());
    buffer.extend_from_slice(&bits.to_le_bytes());
    buffer.extend_from_slice(b"data");
    buffer.extend_from_slice(&data_len.to_le_bytes());
    buffer.resize(buffer.len() + data_len as usize, 0);
    std::fs::write(path, buffer).expect("write wav");
}

/// Tagging helper mirroring a well-curated album.
struct Fixture<'a> {
    artist: &'a str,
    album: &'a str,
    track: u32,
    total: u32,
    title: &'a str,
    genre: &'a str,
    year: u32,
}

fn tag_wav(path: &Path, fixture: &Fixture<'_>) {
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title(fixture.title.to_string());
    tag.set_artist(fixture.artist.to_string());
    tag.set_album(fixture.album.to_string());
    tag.insert_text(ItemKey::AlbumArtist, fixture.artist.to_string());
    tag.set_genre(fixture.genre.to_string());
    tag.set_track(fixture.track);
    tag.set_track_total(fixture.total);
    tag.set_year(fixture.year);
    tag.insert_text(ItemKey::ReplayGainTrackGain, "-6.25 dB".to_string());
    tag.save_to_path(path, WriteOptions::default())
        .expect("write id3v2 tag");
}

/// Build the canonical fixture tree used by most tests.
fn build_library(root: &Path) -> Vec<PathBuf> {
    let kid_a = root.join("Radiohead/Kid A");
    write_wav(
        &kid_a.join("01 Everything In Its Right Place.wav"),
        1.0,
        44_100,
        2,
        16,
    );
    write_wav(&kid_a.join("02 Kid A.wav"), 1.0, 44_100, 2, 16);
    tag_wav(
        &kid_a.join("01 Everything In Its Right Place.wav"),
        &Fixture {
            artist: "Radiohead",
            album: "Kid A",
            track: 1,
            total: 10,
            title: "Everything In Its Right Place",
            genre: "Electronic",
            year: 2000,
        },
    );
    tag_wav(
        &kid_a.join("02 Kid A.wav"),
        &Fixture {
            artist: "Radiohead",
            album: "Kid A",
            track: 2,
            total: 10,
            title: "Kid A",
            genre: "Electronic",
            year: 2000,
        },
    );

    let lemonade = root.join("Beyoncé/Lemonade");
    write_wav(
        &lemonade.join("01 Pray You Catch Me.wav"),
        1.0,
        44_100,
        2,
        16,
    );
    tag_wav(
        &lemonade.join("01 Pray You Catch Me.wav"),
        &Fixture {
            artist: "Beyoncé",
            album: "Lemonade",
            track: 1,
            total: 12,
            title: "Pray You Catch Me",
            genre: "Pop",
            year: 2016,
        },
    );

    // Untagged: the scanner must fall back to the filename and folder.
    write_wav(&root.join("Loose/untagged take.wav"), 1.0, 44_100, 2, 16);

    // A corrupt container: readable bytes, unreadable audio.
    std::fs::create_dir_all(root.join("Broken")).expect("mkdir broken");
    std::fs::write(root.join("Broken/corrupt.flac"), b"not really a flac file")
        .expect("write corrupt");

    // A CUE sheet with two virtual tracks over one image.
    let cue_dir = root.join("Cue Album");
    write_wav(&cue_dir.join("album.wav"), 10.0, 44_100, 2, 16);
    std::fs::write(
        cue_dir.join("album.cue"),
        "PERFORMER \"Miles Davis\"\r\n\
         TITLE \"Kind of Blue\"\r\n\
         REM GENRE \"Jazz\"\r\n\
         REM DATE 1959\r\n\
         FILE \"album.wav\" WAVE\r\n  \
           TRACK 01 AUDIO\r\n    TITLE \"So What\"\r\n    INDEX 01 00:00:00\r\n  \
           TRACK 02 AUDIO\r\n    TITLE \"Freddie Freeloader\"\r\n    INDEX 01 00:05:00\r\n",
    )
    .expect("write cue");

    vec![
        kid_a.join("01 Everything In Its Right Place.wav"),
        kid_a.join("02 Kid A.wav"),
        lemonade.join("01 Pray You Catch Me.wav"),
        root.join("Loose/untagged take.wav"),
    ]
}

async fn pool_for(dir: &Path) -> SqlitePool {
    connect(&dir.join("prism.db")).await.expect("connect")
}

async fn scan(
    pool: &SqlitePool,
    root: &Path,
    config: ScanConfig,
) -> prism_lib::metadata::scanner::ScanOutcome {
    let mut config = config;
    if config.roots.is_empty() {
        config.roots = vec![root.to_path_buf()];
    }
    Scanner::new(pool.clone())
        .scan(config, ScanTrigger::Manual, |_| {})
        .await
        .expect("scan")
}

async fn scalar(pool: &SqlitePool, sql: &str) -> i64 {
    sqlx::query_scalar(sql)
        .fetch_one(pool)
        .await
        .unwrap_or_else(|error| panic!("query failed: {sql}: {error}"))
}

// ---------------------------------------------------------------------------
// Scanner
// ---------------------------------------------------------------------------

#[tokio::test]
async fn scan_indexes_files_albums_artists_cue_tracks_and_errors() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("library");
    build_library(&root);
    let pool = pool_for(temp.path()).await;

    let outcome = scan(&pool, &root, ScanConfig::default()).await;

    assert_eq!(outcome.status, "completed");
    // 6 files with an audio extension (including the corrupt one and the CUE
    // image) + 1 sheet.
    assert_eq!(outcome.files_seen, 7, "walk counts audio + cue sheets");
    assert_eq!(
        outcome.files_failed, 1,
        "corrupt.flac is reported, not fatal"
    );
    // 4 plain tracks + 2 virtual CUE tracks.
    assert_eq!(outcome.files_indexed, 6);
    assert_eq!(scalar(&pool, "SELECT COUNT(*) FROM tracks").await, 6);
    assert_eq!(
        scalar(&pool, "SELECT COUNT(*) FROM tracks WHERE source = 'cue'").await,
        2
    );
    // Kid A, Lemonade, Kind of Blue and the folder-derived album of the untagged file.
    assert_eq!(scalar(&pool, "SELECT COUNT(*) FROM albums").await, 4);
    assert_eq!(scalar(&pool, "SELECT COUNT(*) FROM artists").await, 4);
    assert_eq!(scalar(&pool, "SELECT COUNT(*) FROM scan_errors").await, 1);
    assert_eq!(
        scalar(
            &pool,
            "SELECT COUNT(*) FROM scan_runs WHERE status = 'completed'"
        )
        .await,
        1
    );

    // Album totals are aggregated after the batch commit.
    let kid_a_tracks: i64 =
        sqlx::query_scalar("SELECT track_count FROM albums WHERE title = 'Kid A'")
            .fetch_one(&pool)
            .await
            .expect("kid a");
    assert_eq!(kid_a_tracks, 2);
    let kid_a_duration: i64 =
        sqlx::query_scalar("SELECT duration_ms FROM albums WHERE title = 'Kid A'")
            .fetch_one(&pool)
            .await
            .expect("kid a duration");
    assert_eq!(kid_a_duration, 2_000, "two one-second WAVs");

    // Technical properties came from the container, not from the tags.
    let (rate, depth, channels): (i64, i64, i64) =
        sqlx::query_as("SELECT sample_rate, bit_depth, channels FROM tracks WHERE title = 'Kid A'")
            .fetch_one(&pool)
            .await
            .expect("properties");
    assert_eq!((rate, depth, channels), (44_100, 16, 2));

    // ReplayGain survived the round trip through ID3v2.
    let gain: Option<f64> =
        sqlx::query_scalar("SELECT replaygain_track_gain_db FROM tracks WHERE title = 'Kid A'")
            .fetch_one(&pool)
            .await
            .expect("gain");
    assert_eq!(gain, Some(-6.25));

    // The CUE sheet produced a sheet row linked to its album.
    let (sheet_tracks, sheet_total, linked_album): (i64, i64, Option<String>) =
        sqlx::query_as("SELECT track_count, total_ms, album_id FROM cue_sheets LIMIT 1")
            .fetch_one(&pool)
            .await
            .expect("cue sheet");
    assert_eq!(sheet_tracks, 2);
    assert_eq!(sheet_total, 10_000);
    assert!(linked_album.is_some(), "cue sheet is linked to its album");

    // The image is *not* also indexed as a standalone track.
    assert_eq!(
        scalar(
            &pool,
            "SELECT COUNT(*) FROM tracks WHERE path LIKE '%album.wav' AND source = 'file'",
        )
        .await,
        0
    );

    // Every virtual track carries its exact span.
    let spans: Vec<(i64, Option<i64>, i64)> = sqlx::query_as(
        "SELECT cue_start_ms, cue_end_ms, duration_ms FROM tracks WHERE source = 'cue' \
         ORDER BY cue_start_ms",
    )
    .fetch_all(&pool)
    .await
    .expect("spans");
    assert_eq!(
        spans,
        vec![(0, Some(5_000), 5_000), (5_000, Some(10_000), 5_000)]
    );

    pool.close().await;
}

#[tokio::test]
async fn rescan_skips_unchanged_files_and_marks_deleted_ones_missing() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("library");
    let files = build_library(&root);
    let pool = pool_for(temp.path()).await;

    let first = scan(&pool, &root, ScanConfig::default()).await;
    assert_eq!(first.files_indexed, 6);

    let second = scan(&pool, &root, ScanConfig::default()).await;
    assert_eq!(second.files_indexed, 0, "nothing changed on disk");
    assert_eq!(
        second.files_skipped, 5,
        "four plain files are fingerprint-equal and the sheet digest is unchanged"
    );
    assert_eq!(second.files_failed, 1, "the corrupt file is retried");

    // Delete one file: its row must survive as `missing`, never be dropped.
    std::fs::remove_file(&files[0]).expect("remove");
    let third = scan(&pool, &root, ScanConfig::default()).await;
    assert_eq!(third.tracks_missing, 1);
    assert_eq!(scalar(&pool, "SELECT COUNT(*) FROM tracks").await, 6);
    assert_eq!(
        scalar(&pool, "SELECT COUNT(*) FROM tracks WHERE missing = 1").await,
        1
    );
    // The album aggregate excludes missing tracks.
    let kid_a_tracks: i64 =
        sqlx::query_scalar("SELECT track_count FROM albums WHERE title = 'Kid A'")
            .fetch_one(&pool)
            .await
            .expect("kid a");
    assert_eq!(kid_a_tracks, 1);

    pool.close().await;
}

#[tokio::test]
async fn a_full_scan_reconciles_album_counters_and_can_prune() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("library");
    build_library(&root);
    let pool = pool_for(temp.path()).await;

    scan(&pool, &root, ScanConfig::default()).await;

    // Simulate a stale counter written by an older build.
    sqlx::query("UPDATE albums SET track_count = 999, duration_ms = 999999")
        .execute(&pool)
        .await
        .expect("corrupt counters");

    let outcome = scan(
        &pool,
        &root,
        ScanConfig {
            full: true,
            ..ScanConfig::default()
        },
    )
    .await;
    assert_eq!(outcome.status, "completed");
    assert_eq!(
        scalar(&pool, "SELECT COUNT(*) FROM albums WHERE track_count = 999").await,
        0,
        "full scan reconciles every album"
    );

    // Emptying the tree (the directory still exists, as with a partially
    // deleted album) and pruning leaves an empty, consistent catalogue.
    for entry in walkdir::WalkDir::new(&root).into_iter().flatten() {
        if entry.file_type().is_file() {
            std::fs::remove_file(entry.path()).expect("remove file");
        }
    }
    let pruned = scan(
        &pool,
        &root,
        ScanConfig {
            full: true,
            prune: true,
            ..ScanConfig::default()
        },
    )
    .await;
    assert_eq!(pruned.status, "completed");
    assert_eq!(
        scalar(&pool, "SELECT COUNT(*) FROM tracks WHERE missing = 0").await,
        0
    );
    assert_eq!(scalar(&pool, "SELECT COUNT(*) FROM tracks").await, 6);
    // Missing rows keep their album alive on purpose: an unplugged drive must
    // not delete the user's playlists. Prune only removes true orphans.
    assert_eq!(scalar(&pool, "SELECT COUNT(*) FROM albums").await, 4);

    sqlx::query("DELETE FROM tracks")
        .execute(&pool)
        .await
        .expect("delete tracks");
    scan(
        &pool,
        &root,
        ScanConfig {
            full: true,
            prune: true,
            ..ScanConfig::default()
        },
    )
    .await;
    assert_eq!(
        scalar(&pool, "SELECT COUNT(*) FROM albums").await,
        0,
        "prune removes albums that no longer own a track"
    );
    assert_eq!(scalar(&pool, "SELECT COUNT(*) FROM artists").await, 0);
    pool.close().await;
}

#[tokio::test]
async fn scan_is_cancellable_and_reports_progress() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("library");
    build_library(&root);
    let pool = pool_for(temp.path()).await;

    let scanner = Scanner::new(pool.clone());
    let phases = std::sync::Arc::new(parking_lot::Mutex::new(Vec::new()));
    let sink = std::sync::Arc::clone(&phases);
    let outcome = scanner
        .scan(
            ScanConfig {
                roots: vec![root.clone()],
                ..ScanConfig::default()
            },
            ScanTrigger::Manual,
            move |progress| sink.lock().push(progress.phase),
        )
        .await
        .expect("scan");

    assert_eq!(outcome.status, "completed");
    let phases = phases.lock().clone();
    assert!(phases.contains(&prism_lib::metadata::scanner::ScanPhase::Walking));
    assert!(phases.contains(&prism_lib::metadata::scanner::ScanPhase::Parsing));
    assert_eq!(
        phases.last(),
        Some(&prism_lib::metadata::scanner::ScanPhase::Completed)
    );
    assert!(scanner.progress().is_some());
    assert!(!scanner.is_running());

    // A second concurrent scan is rejected instead of corrupting counters.
    let blocking = Scanner::new(pool.clone());
    assert_eq!(blocking.progress(), None);
    pool.close().await;
}

// ---------------------------------------------------------------------------
// Search and read models
// ---------------------------------------------------------------------------

#[tokio::test]
async fn fts_search_ranks_titles_and_folds_diacritics() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("library");
    build_library(&root);
    let pool = pool_for(temp.path()).await;
    scan(&pool, &root, ScanConfig::default()).await;

    let page = search::tracks(
        &pool,
        TrackQuery {
            text: Some("kid".to_string()),
            limit: 20,
            ..TrackQuery::default()
        },
    )
    .await
    .expect("search");
    assert!(page.total >= 2);
    assert_eq!(
        page.items[0].title, "Kid A",
        "title hits outrank album hits"
    );
    assert!(page.items.iter().all(|row| row.missing == false));
    // Snippet markers are the control characters the UI splits on.
    let highlighted = page
        .items
        .iter()
        .filter_map(|row| row.highlight.as_deref())
        .any(|snippet| snippet.contains('\u{1}'));
    assert!(highlighted, "snippets carry highlight markers");

    // "beyonce" must find "Beyoncé" (unicode61 remove_diacritics 2).
    let accented = search::tracks(
        &pool,
        TrackQuery {
            text: Some("beyonce".to_string()),
            ..TrackQuery::default()
        },
    )
    .await
    .expect("accent search");
    assert_eq!(accented.total, 1);
    assert_eq!(accented.items[0].artist, "Beyoncé");

    // Operator characters never reach the FTS grammar.
    for hostile in ["C++", "\"*\"", "NEAR(", "a OR b", "-", "///"] {
        let result = search::tracks(
            &pool,
            TrackQuery {
                text: Some(hostile.to_string()),
                ..TrackQuery::default()
            },
        )
        .await;
        assert!(result.is_ok(), "query {hostile:?} must not error");
    }

    // Pagination is stable: page 1 + page 2 cover the same set as one page.
    let all = search::tracks(
        &pool,
        TrackQuery {
            sort: TrackSort::Title,
            limit: 50,
            ..TrackQuery::default()
        },
    )
    .await
    .expect("all");
    let first = search::tracks(
        &pool,
        TrackQuery {
            sort: TrackSort::Title,
            limit: 2,
            offset: 0,
            ..TrackQuery::default()
        },
    )
    .await
    .expect("page 1");
    let second = search::tracks(
        &pool,
        TrackQuery {
            sort: TrackSort::Title,
            limit: 2,
            offset: 2,
            ..TrackQuery::default()
        },
    )
    .await
    .expect("page 2");
    assert_eq!(first.total, all.total);
    assert_eq!(second.total, all.total);
    let mut paged: Vec<String> = first
        .items
        .iter()
        .chain(second.items.iter())
        .map(|row| row.id.clone())
        .collect();
    paged.dedup();
    assert_eq!(paged.len(), 4);

    pool.close().await;
}

#[tokio::test]
async fn browse_filters_and_facets_cover_the_library() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("library");
    build_library(&root);
    let pool = pool_for(temp.path()).await;
    scan(&pool, &root, ScanConfig::default()).await;

    // Album grid.
    let albums = search::albums(&pool, None, None, TrackSort::Title, 50, 0)
        .await
        .expect("albums");
    assert_eq!(albums.total, 4);
    let titles: Vec<&str> = albums
        .items
        .iter()
        .map(|album| album.title.as_str())
        .collect();
    assert!(titles.contains(&"Kid A"));
    assert!(titles.contains(&"Kind of Blue"));

    // Artist grid with aggregates.
    let artists = search::artists(&pool, None, 50, 0).await.expect("artists");
    assert_eq!(artists.total, 4);
    let radiohead = artists
        .items
        .iter()
        .find(|artist| artist.name == "Radiohead")
        .expect("radiohead");
    assert_eq!(radiohead.track_count, 2);

    // Genre facet.
    let facets = search::facets(&pool).await.expect("facets");
    assert!(
        facets
            .genres
            .iter()
            .any(|facet| facet.value == "Electronic")
    );
    assert!(facets.formats.iter().any(|facet| facet.value == "wav"));

    // Album drill-down.
    let kid_a = albums
        .items
        .iter()
        .find(|album| album.title == "Kid A")
        .expect("kid a");
    let album_tracks = search::tracks(
        &pool,
        TrackQuery {
            album_id: Some(kid_a.id.clone()),
            ..TrackQuery::default()
        },
    )
    .await
    .expect("album tracks");
    assert_eq!(album_tracks.total, 2);

    // Lossless filter: every WAV in the fixture is lossless, and nothing else
    // exists, so the filter must not change the total.
    let lossless = search::tracks(
        &pool,
        TrackQuery {
            lossless_only: true,
            limit: 50,
            ..TrackQuery::default()
        },
    )
    .await
    .expect("lossless");
    assert_eq!(lossless.total, 6);

    // The ⌘K overlay returns all three entity types in one call.
    let global = search::global(&pool, "kind", 5).await.expect("global");
    assert_eq!(global.query, "kind");
    assert!(global.match_expression.is_some());
    assert!(global.tracks.total >= 1);
    assert_eq!(global.albums.len(), 1);

    // Missing tracks can be included on demand.
    sqlx::query("UPDATE tracks SET missing = 1 WHERE title = 'Kid A'")
        .execute(&pool)
        .await
        .expect("mark missing");
    let hidden = search::tracks(
        &pool,
        TrackQuery {
            text: Some("kid a".to_string()),
            ..TrackQuery::default()
        },
    )
    .await
    .expect("hidden");
    let shown = search::tracks(
        &pool,
        TrackQuery {
            text: Some("kid a".to_string()),
            include_missing: true,
            ..TrackQuery::default()
        },
    )
    .await
    .expect("shown");
    assert_eq!(shown.total, hidden.total + 1);

    pool.close().await;
}

#[tokio::test]
async fn library_health_stays_consistent_after_a_scan() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("library");
    build_library(&root);
    let pool = pool_for(temp.path()).await;
    scan(&pool, &root, ScanConfig::default()).await;

    let report = health(&pool).await.expect("health");
    assert_eq!(report.integrity, "ok");
    assert_eq!(report.journal_mode.to_ascii_lowercase(), "wal");

    // FTS is an external-content table: an integrity-check rebuild must agree
    // with the content table, otherwise search silently drifts.
    sqlx::query("INSERT INTO tracks_fts(tracks_fts) VALUES('integrity-check')")
        .execute(&pool)
        .await
        .expect("fts integrity check");

    // Deleting a track must cascade into the FTS index through the trigger.
    let before = search::tracks(
        &pool,
        TrackQuery {
            text: Some("freeloader".to_string()),
            ..TrackQuery::default()
        },
    )
    .await
    .expect("before");
    assert_eq!(before.total, 1);
    sqlx::query("DELETE FROM tracks WHERE title = 'Freddie Freeloader'")
        .execute(&pool)
        .await
        .expect("delete");
    let after = search::tracks(
        &pool,
        TrackQuery {
            text: Some("freeloader".to_string()),
            ..TrackQuery::default()
        },
    )
    .await
    .expect("after");
    assert_eq!(after.total, 0, "FTS trigger removed the row");

    pool.close().await;
}

#[tokio::test]
async fn catalog_writer_is_idempotent_across_repeated_batches() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("library");
    build_library(&root);
    let pool = pool_for(temp.path()).await;
    let _ = CatalogWriter::new(pool.clone());

    // Three full scans in a row must converge to the same row ids.
    scan(&pool, &root, ScanConfig::default()).await;
    let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM tracks ORDER BY id")
        .fetch_all(&pool)
        .await
        .expect("ids");
    for _ in 0..2 {
        scan(
            &pool,
            &root,
            ScanConfig {
                full: true,
                ..ScanConfig::default()
            },
        )
        .await;
    }
    let ids_after: Vec<String> = sqlx::query_scalar("SELECT id FROM tracks ORDER BY id")
        .fetch_all(&pool)
        .await
        .expect("ids");
    assert_eq!(
        ids, ids_after,
        "deterministic ids survive repeated full scans"
    );
    assert_eq!(scalar(&pool, "SELECT COUNT(*) FROM tracks").await, 6);

    pool.close().await;
}

// ---------------------------------------------------------------------------
// Root counters
// ---------------------------------------------------------------------------

/// A folder literally named `100% Hits` must not turn the root counter into a
/// wildcard that also counts a sibling `100X Hits`. The `_` case is covered by
/// the same expression; both characters come from the same escaping block.
#[tokio::test]
async fn root_track_counter_escapes_like_wildcards() {
    use prism_lib::commands::library::ROOT_SELECT;

    let temp = tempfile::tempdir().expect("tempdir");
    let literal = temp.path().join("100% Hits");
    let sibling = temp.path().join("100X Hits");
    std::fs::create_dir_all(&literal).expect("mkdir literal");
    std::fs::create_dir_all(&sibling).expect("mkdir sibling");
    write_wav(&literal.join("a.wav"), 1.0, 44_100, 2, 16);
    write_wav(&literal.join("b.wav"), 1.0, 44_100, 2, 16);
    write_wav(&sibling.join("c.wav"), 1.0, 44_100, 2, 16);

    let pool = pool_for(temp.path()).await;
    sqlx::query("INSERT INTO library_roots (id, path, name) VALUES ('r1', ?1, 'hits')")
        .bind(literal.to_string_lossy().into_owned())
        .execute(&pool)
        .await
        .expect("insert root");

    // Scan the parent so the sibling is indexed too.
    scan(
        &pool,
        temp.path(),
        ScanConfig {
            full: true,
            ..ScanConfig::default()
        },
    )
    .await;
    assert_eq!(scalar(&pool, "SELECT COUNT(*) FROM tracks").await, 3);

    let sql = format!("{ROOT_SELECT} WHERE r.id = 'r1'");
    let row = sqlx::query(&sql).fetch_one(&pool).await.expect("root row");
    assert_eq!(
        row.try_get::<i64, _>("tracks").expect("tracks"),
        2,
        "the % in the root path must be a literal, not a wildcard"
    );
}
