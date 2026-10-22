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
use prism_lib::db::search::{self, AlbumSort, TrackQuery, TrackSort};
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
    tag_wav_on_disc(path, fixture, 1);
}

fn tag_wav_on_disc(path: &Path, fixture: &Fixture<'_>, disc: u32) {
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_disk(disc);
    tag.set_disk_total(if disc > 1 { 2 } else { 1 });
    tag.set_title(fixture.title.to_string());
    tag.set_artist(fixture.artist.to_string());
    tag.set_album(fixture.album.to_string());
    tag.insert_text(ItemKey::AlbumArtist, fixture.artist.to_string());
    tag.set_genre(fixture.genre.to_string());
    tag.set_track(fixture.track);
    tag.set_track_total(fixture.total);
    tag.set_year(fixture.year);
    tag.insert_text(ItemKey::ReplayGainTrackGain, "-6.25 dB".to_string());
    // A recording id is not a file id: all fixtures intentionally share one,
    // as duplicate pressings and multi-disc editions do in real libraries.
    tag.insert_text(
        ItemKey::MusicBrainzTrackId,
        "0d4c1d3e-77e4-4f35-9f3d-1024shared".to_string(),
    );
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

/// The historical failure stopped after two 512-file parse chunks: progress sat
/// at exactly 1024 tracks forever because the third chunk waited on one hung
/// reader with no budget. This fixture is the smallest one that crosses that
/// boundary, and it fails if chunked parsing ever regains a hard ceiling.
#[tokio::test]
async fn scan_completes_past_the_old_1024_file_ceiling() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("library");
    let total: u32 = 1_100;
    for index in 0..total {
        let path = root.join(format!("Ceiling/Transmission/track-{index:04}.wav"));
        write_wav(&path, 0.02, 8_000, 1, 16);
        tag_wav(
            &path,
            &Fixture {
                artist: "Ceiling",
                album: "Transmission",
                track: (index % 12) + 1,
                total: 12,
                title: &format!("Track {index:04}"),
                genre: "Test",
                year: 2026,
            },
        );
    }
    let pool = pool_for(temp.path()).await;

    let outcome = scan(&pool, &root, ScanConfig::default()).await;

    assert_eq!(outcome.status, "completed");
    assert_eq!(outcome.files_seen, u64::from(total));
    assert_eq!(
        outcome.files_indexed,
        u64::from(total),
        "every file past the third chunk is indexed"
    );
    assert_eq!(
        scalar(&pool, "SELECT COUNT(*) FROM tracks WHERE missing = 0").await,
        i64::from(total)
    );
}

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
async fn cancelling_a_scan_releases_the_scanner_and_a_new_scan_starts() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("library");
    // Enough files that the scan is still parsing when the cancel lands.
    for index in 0..400u32 {
        let path = root.join(format!("Bulk/Record/track-{index:04}.wav"));
        write_wav(&path, 0.02, 8_000, 1, 16);
        tag_wav(
            &path,
            &Fixture {
                artist: "Bulk",
                album: "Record",
                track: (index % 9) + 1,
                total: 9,
                title: &format!("Track {index:04}"),
                genre: "Test",
                year: 2026,
            },
        );
    }
    let pool = pool_for(temp.path()).await;
    let scanner = Scanner::new(pool.clone());

    let running = scanner.clone();
    let spawned_root = root.clone();
    let handle = tokio::spawn(async move {
        running
            .scan(
                ScanConfig {
                    roots: vec![spawned_root],
                    ..ScanConfig::default()
                },
                ScanTrigger::Manual,
                |_| {},
            )
            .await
    });

    // Cancel as soon as the scanner admits it is parsing. Sleeping a fixed
    // interval made this test lie: on a fast machine the scan finished first and
    // the assertion below failed for the wrong reason.
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(20);
    while scanner.progress().is_none() && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
    assert!(
        scanner.is_running(),
        "the scan finished before it could be cancelled"
    );
    scanner.cancel();

    let outcome = tokio::time::timeout(std::time::Duration::from_secs(30), handle)
        .await
        .expect("a cancelled scan must return promptly")
        .expect("join")
        .expect("scan");
    assert_eq!(outcome.status, "cancelled");
    assert!(
        !scanner.is_running(),
        "the scanner must accept a new scan immediately after a cancel"
    );

    // The failure the user reported: after a cancel the next scan answered
    // "already running" until the process restarted.
    let second = scanner
        .scan(
            ScanConfig {
                roots: vec![root.clone()],
                ..ScanConfig::default()
            },
            ScanTrigger::Manual,
            |_| {},
        )
        .await
        .expect("a second scan must start");
    assert_eq!(second.status, "completed");
    assert!(!scanner.is_running());
}

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
    let albums = search::albums(&pool, None, None, AlbumSort::Title, 50, 0)
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
async fn the_album_grid_carries_the_artist_identifier() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("library");
    build_library(&root);
    let pool = pool_for(temp.path()).await;
    scan(&pool, &root, ScanConfig::default()).await;

    let page = search::albums(&pool, None, None, AlbumSort::Artist, 50, 0)
        .await
        .expect("albums");
    assert_eq!(page.total, 4);

    // Migration 0004 exists so a tile can link by identifier. A name is not an
    // identifier: two artists may share one. The untagged fixture has no album
    // artist at all and is therefore the one album without a link — every other
    // album must carry an id that resolves.
    let mut linked = 0;
    for album in &page.items {
        let Some(artist_id) = album.artist_id.clone() else {
            assert!(
                album.album_artist.is_empty() || album.album_artist == "Loose",
                "only the folder-derived album may lack an artist id, not {}",
                album.title
            );
            continue;
        };
        linked += 1;
        let known: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM artists WHERE id = ?1")
            .bind(&artist_id)
            .fetch_one(&pool)
            .await
            .expect("count artists");
        assert_eq!(known, 1, "{} points at a real artist", album.title);

        let filtered = search::albums(&pool, None, Some(&artist_id), AlbumSort::Artist, 50, 0)
            .await
            .expect("filtered");
        assert!(
            filtered
                .items
                .iter()
                .any(|candidate| candidate.id == album.id),
            "filtering by the artist id keeps {} in the grid",
            album.title
        );
    }
    assert!(linked >= 3, "the fixture links every tagged album");
}

#[tokio::test]
async fn removing_a_root_purges_its_tracks_albums_and_artists() {
    let temp = tempfile::tempdir().expect("tempdir");
    let keep = temp.path().join("keep");
    let drop = temp.path().join("drop");

    let kept = keep.join("Kept Artist/A Record");
    write_wav(&kept.join("01 Kept.wav"), 0.05, 8_000, 1, 16);
    tag_wav(
        &kept.join("01 Kept.wav"),
        &Fixture {
            artist: "Kept Artist",
            album: "A Record",
            track: 1,
            total: 1,
            title: "Kept",
            genre: "Test",
            year: 2026,
        },
    );

    // Same album *title* as the kept record, different artist: the purge works
    // on paths, so a title collision must not cost the surviving album a row.
    let dropped = drop.join("Dropped Artist/A Record");
    for track in 1..=2u32 {
        let name = format!("{track:02} Gone.wav");
        write_wav(&dropped.join(&name), 0.05, 8_000, 1, 16);
        tag_wav(
            &dropped.join(&name),
            &Fixture {
                artist: "Dropped Artist",
                album: "A Record",
                track,
                total: 2,
                title: &format!("Gone {track}"),
                genre: "Test",
                year: 2026,
            },
        );
    }

    let pool = pool_for(temp.path()).await;
    let config = ScanConfig {
        roots: vec![keep.clone(), drop.clone()],
        ..ScanConfig::default()
    };
    scan(&pool, &keep, config).await;
    assert_eq!(scalar(&pool, "SELECT COUNT(*) FROM tracks").await, 3);
    assert_eq!(scalar(&pool, "SELECT COUNT(*) FROM albums").await, 2);

    let writer = CatalogWriter::new(pool.clone());
    let outcome = writer
        .purge_root(&drop.to_string_lossy())
        .await
        .expect("purge");
    assert_eq!(outcome.tracks, 2, "both dropped tracks are deleted");
    assert_eq!(outcome.albums, 1, "only the orphaned record goes");

    assert_eq!(scalar(&pool, "SELECT COUNT(*) FROM tracks").await, 1);
    assert_eq!(
        scalar(
            &pool,
            "SELECT COUNT(*) FROM tracks WHERE path LIKE '%keep%'"
        )
        .await,
        1
    );
    assert_eq!(scalar(&pool, "SELECT COUNT(*) FROM albums").await, 1);
    assert_eq!(
        scalar(
            &pool,
            "SELECT COUNT(*) FROM albums WHERE album_artist_name = 'Dropped Artist'"
        )
        .await,
        0
    );
    assert_eq!(
        scalar(
            &pool,
            "SELECT COUNT(*) FROM artists WHERE name = 'Dropped Artist'"
        )
        .await,
        0,
        "the artist row follows its last track"
    );
    assert_eq!(
        scalar(
            &pool,
            "SELECT COUNT(*) FROM artists WHERE name = 'Kept Artist'"
        )
        .await,
        1
    );
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

// ---------------------------------------------------------------------------
// 0.1.4 — duplicate recording ids, playlists, collections and multi-disc art
// ---------------------------------------------------------------------------

#[tokio::test]
async fn playlists_keep_the_users_order_and_never_duplicate_a_track() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("library");
    build_library(&root);
    let pool = pool_for(temp.path()).await;
    scan(&pool, &root, ScanConfig::default()).await;

    let tracks = search::tracks(
        &pool,
        TrackQuery {
            sort: TrackSort::Title,
            limit: 20,
            ..TrackQuery::default()
        },
    )
    .await
    .expect("tracks")
    .items;
    assert!(tracks.len() >= 2);
    let first = tracks[0].id.clone();
    let second = tracks[1].id.clone();

    let playlist = prism_lib::commands::playlists::create_in(&pool, "Evening".into())
        .await
        .expect("create playlist");
    assert!(
        prism_lib::commands::playlists::create_in(&pool, " evening ".into())
            .await
            .is_err(),
        "names are unique without regard to case or surrounding space"
    );

    let updated = prism_lib::commands::playlists::add_tracks_in(
        &pool,
        playlist.id.clone(),
        vec![second.clone(), first.clone(), second.clone()],
    )
    .await
    .expect("add tracks");
    assert_eq!(updated.track_count, 2, "the repeated id is a no-op");

    let page = search::playlist_tracks(&pool, &playlist.id, 50, 0)
        .await
        .expect("playlist page");
    let ids: Vec<String> = page.items.into_iter().map(|track| track.id).collect();
    assert_eq!(ids, vec![second, first], "explicit order is preserved");
}

#[tokio::test]
async fn home_see_all_queries_match_their_shelves_and_rediscover_reacts_to_plays() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("library");
    build_library(&root);
    let pool = pool_for(temp.path()).await;
    scan(&pool, &root, ScanConfig::default()).await;

    let home = search::home(&pool, 24).await.expect("home");
    let fresh = search::albums_filtered(
        &pool,
        None,
        None,
        None,
        Some(search::AlbumCollection::RecentReleases),
        None,
        50,
        0,
    )
    .await
    .expect("fresh collection");
    let fresh_ids: Vec<String> = fresh.items.iter().map(|album| album.id.clone()).collect();
    let rail_ids: Vec<String> = home
        .recent_releases
        .iter()
        .map(|album| album.id.clone())
        .collect();
    assert_eq!(
        fresh_ids, rail_ids,
        "See all and the shelf share one predicate"
    );
    assert!(
        fresh.items.iter().all(|album| album.year == Some(2016)),
        "Fresh releases means the newest year, not a twelve-year window"
    );

    let electronic =
        search::albums_filtered(&pool, None, None, Some("Electronic"), None, None, 50, 0)
            .await
            .expect("genre collection");
    let genre = home
        .genres
        .iter()
        .find(|genre| genre.name == "Electronic")
        .expect("Electronic genre shelf");
    assert_eq!(genre.albums.len(), electronic.items.len());

    let played_id = electronic.items[0].id.clone();
    let track_id: String = sqlx::query_scalar("SELECT id FROM tracks WHERE album_id = ?1 LIMIT 1")
        .bind(&played_id)
        .fetch_one(&pool)
        .await
        .expect("played track");
    CatalogWriter::new(pool.clone())
        .note_play(&track_id, 1_000, false)
        .await
        .expect("note play");
    let after = search::home(&pool, 24).await.expect("home after play");
    assert!(
        !after.rediscover.iter().any(|album| album.id == played_id),
        "a record that was just heard must leave Rediscover"
    );
    assert!(
        after
            .recently_played
            .iter()
            .any(|album| album.id == played_id),
        "the same play enters Recently played"
    );
}

#[tokio::test]
async fn multi_disc_albums_stay_ordered_and_artwork_hints_use_disc_one() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("library");
    let first_path = root.join("Artist/Box Set/CD1/01 Opening.wav");
    let second_path = root.join("Artist/Box Set/CD2/01 Closing.wav");
    for (path, track, title, disc) in [
        (&first_path, 1, "Opening", 1),
        (&second_path, 2, "Closing", 2),
    ] {
        write_wav(path, 0.1, 44_100, 2, 16);
        tag_wav_on_disc(
            path,
            &Fixture {
                artist: "Artist",
                album: "Box Set",
                track,
                total: 2,
                title,
                genre: "Jazz",
                year: 2024,
            },
            disc,
        );
    }
    let pool = pool_for(temp.path()).await;
    scan(&pool, &root, ScanConfig::default()).await;

    let albums = search::albums(&pool, None, None, AlbumSort::Title, 10, 0)
        .await
        .expect("albums");
    let album = albums
        .items
        .iter()
        .find(|album| album.title == "Box Set")
        .expect("box set");
    assert_eq!(album.disc_count, 2);
    assert_eq!(album.track_count, 2);
    let tracks = search::tracks(
        &pool,
        TrackQuery {
            album_id: Some(album.id.clone()),
            sort: TrackSort::Album,
            limit: 10,
            ..TrackQuery::default()
        },
    )
    .await
    .expect("ordered tracks")
    .items;
    let discs: Vec<Option<i64>> = tracks.iter().map(|track| track.disc_no).collect();
    assert_eq!(discs, vec![Some(1), Some(2)]);

    let artwork = prism_lib::artwork::ArtworkService::new(pool.clone(), temp.path().join("covers"))
        .expect("artwork service");
    let hint = artwork
        .hint_for_track(&tracks[1].id, Some(PathBuf::from(&tracks[1].path)))
        .await;
    assert_eq!(
        hint.track_path,
        Some(first_path),
        "a CD2 track must not make the album miss its CD1 artwork"
    );
}
