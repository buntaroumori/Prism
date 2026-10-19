//! Throughput benchmark for the library pipeline.
//!
//! This test is `#[ignore]`d so it never slows down the normal gate, but it is
//! a real, reproducible measurement — the same production code path the
//! application runs (walkdir → lofty → sqlx → FTS5), over a real directory
//! tree of real audio containers, with real transactions.
//!
//! ```text
//! PRISM_BENCH_FILES=100000 \
//!   cargo test --release --test scanner_bench -- --ignored --nocapture
//! ```
//!
//! Environment:
//!   * `PRISM_BENCH_FILES` — number of files to generate (default 20 000).
//!   * `PRISM_BENCH_DIR`   — scratch directory (default `$TMPDIR/prism-bench`).
//!
//! The fixture layout is deliberately hostile to naive implementations: 500
//! tracks per artist folder, 20 tracks per album, mixed years and genres, and
//! a `cover.jpg` in every folder so the directory filter has to reject them.

use std::path::{Path, PathBuf};
use std::time::Instant;

use lofty::config::WriteOptions;
use lofty::prelude::{Accessor, ItemKey, TagExt};
use lofty::tag::{Tag, TagType};
use prism_lib::db::connect;
use prism_lib::db::search::{self, TrackQuery};
use prism_lib::metadata::scanner::{ScanConfig, ScanTrigger, Scanner};
use tempfile::TempDir;

const TRACKS_PER_ALBUM: usize = 20;

/// Write a canonical 16-bit PCM WAV of `seconds` of silence.
fn write_wav(path: &Path, seconds: f32, sample_rate: u32) {
    let frames = (seconds * sample_rate as f32) as u32;
    let block_align = 2u32; // mono, 16 bit
    let data_len = frames * block_align;

    let mut buffer = Vec::with_capacity(44 + data_len as usize);
    buffer.extend_from_slice(b"RIFF");
    buffer.extend_from_slice(&(36 + data_len).to_le_bytes());
    buffer.extend_from_slice(b"WAVE");
    buffer.extend_from_slice(b"fmt ");
    buffer.extend_from_slice(&16u32.to_le_bytes());
    buffer.extend_from_slice(&1u16.to_le_bytes());
    buffer.extend_from_slice(&1u16.to_le_bytes());
    buffer.extend_from_slice(&sample_rate.to_le_bytes());
    buffer.extend_from_slice(&(sample_rate * block_align).to_le_bytes());
    buffer.extend_from_slice(&(block_align as u16).to_le_bytes());
    buffer.extend_from_slice(&16u16.to_le_bytes());
    buffer.extend_from_slice(b"data");
    buffer.extend_from_slice(&data_len.to_le_bytes());
    buffer.resize(buffer.len() + data_len as usize, 0);

    std::fs::write(path, buffer).expect("write wav");
}

fn tag_file(path: &Path, artist: &str, album: &str, track: u32, total: u32, year: u32) {
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title(format!("Track {track:02}"));
    tag.set_artist(artist.to_string());
    tag.set_album(album.to_string());
    let genre = if year % 2 == 0 { "Electronic" } else { "Jazz" };
    tag.set_genre(genre.to_string());
    tag.insert_text(ItemKey::AlbumArtist, artist.to_string());
    tag.insert_text(ItemKey::TrackNumber, track.to_string());
    tag.insert_text(ItemKey::TrackTotal, total.to_string());
    tag.insert_text(ItemKey::Year, year.to_string());
    tag.save_to_path(path, WriteOptions::default())
        .expect("save tags");
}

/// Build `files` tagged WAVs in `root`, returning the number of albums.
fn generate(root: &Path, files: usize) -> usize {
    let albums = files.div_ceil(TRACKS_PER_ALBUM).max(1);
    for index in 0..files {
        let album = index / TRACKS_PER_ALBUM;
        let track = (index % TRACKS_PER_ALBUM) as u32 + 1;
        let artist = format!("Artist {:03}", album / 25);
        let album_name = format!("Album {album:05}");
        let year = 1960 + (album % 60) as u32;

        let directory = root.join(&artist).join(&album_name);
        std::fs::create_dir_all(&directory).expect("mkdir");
        let path = directory.join(format!("{track:02} - Track.flac.wav"));
        write_wav(&path, 0.05, 8_000);
        tag_file(
            &path,
            &artist,
            &album_name,
            track,
            TRACKS_PER_ALBUM as u32,
            year,
        );

        if index % TRACKS_PER_ALBUM == 0 {
            // A folder cover: the walker must classify and skip it.
            std::fs::write(
                directory.join("cover.jpg"),
                b"\xFF\xD8\xFF\xE0not-a-real-jpeg",
            )
            .expect("write cover");
        }
    }
    albums
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "benchmark: run explicitly with --release --ignored --nocapture"]
async fn library_throughput() {
    let files: usize = std::env::var("PRISM_BENCH_FILES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(20_000);
    let scratch: PathBuf = std::env::var("PRISM_BENCH_DIR")
        .map_or_else(|_| std::env::temp_dir().join("prism-bench"), PathBuf::from);

    if scratch.exists() {
        std::fs::remove_dir_all(&scratch).expect("clean scratch");
    }
    std::fs::create_dir_all(&scratch).expect("mkdir scratch");

    let started = Instant::now();
    let albums = generate(&scratch.join("library"), files);
    let generation = started.elapsed();

    let database: TempDir = tempfile::tempdir().expect("tempdir");
    let pool = connect(&database.path().join("bench.db"))
        .await
        .expect("db");

    sqlx::query("INSERT INTO library_roots (id, path, enabled) VALUES ('bench', ?1, 1)")
        .bind(scratch.join("library").to_string_lossy().into_owned())
        .execute(&pool)
        .await
        .expect("root");

    let scanner = Scanner::new(pool.clone());

    // ---- First scan: cold index -------------------------------------------
    let started = Instant::now();
    let first = scanner
        .scan(
            ScanConfig {
                full: true,
                ..ScanConfig::default()
            },
            ScanTrigger::Manual,
            |_| {},
        )
        .await
        .expect("scan");
    let cold = started.elapsed();

    assert_eq!(first.files_indexed as usize, files, "every file indexed");
    assert_eq!(first.files_failed, 0, "no parse failures");

    // ---- Second scan: incremental -----------------------------------------
    let started = Instant::now();
    let second = scanner
        .scan(ScanConfig::default(), ScanTrigger::Manual, |_| {})
        .await
        .expect("rescan");
    let warm = started.elapsed();
    assert_eq!(second.files_indexed, 0, "nothing re-indexed");
    assert_eq!(second.files_skipped as usize, files, "all files skipped");

    // ---- Query latency -----------------------------------------------------
    let started = Instant::now();
    let page = search::tracks(
        &pool,
        TrackQuery {
            text: Some("track 07".into()),
            limit: 100,
            ..TrackQuery::default()
        },
    )
    .await
    .expect("search");
    let search_time = started.elapsed();

    let per_file = cold.as_secs_f64() * 1_000.0 / files as f64;
    println!("\nPrism library throughput");
    println!("  files            {files}");
    println!("  albums           {albums}");
    println!("  fixture built    {:.2} s", generation.as_secs_f64());
    println!(
        "  cold scan        {:.2} s  ({per_file:.3} ms/file)",
        cold.as_secs_f64()
    );
    println!(
        "  incremental scan {:.2} s  ({:.0} files/s)",
        warm.as_secs_f64(),
        files as f64 / warm.as_secs_f64()
    );
    println!(
        "  FTS query        {:.2} ms  ({} hits of {})",
        search_time.as_secs_f64() * 1_000.0,
        page.items.len(),
        page.total
    );
    println!(
        "  projected 100k   {:.2} s cold / {:.2} s incremental\n",
        cold.as_secs_f64() * 100_000.0 / files as f64,
        warm.as_secs_f64() * 100_000.0 / files as f64
    );
}
