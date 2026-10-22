//! Library scanner.
//!
//! ## Pipeline
//!
//! ```text
//! walkdir ──▶ fingerprints ──▶ per-file parse ──▶ batch upsert ──▶ missing sweep
//!  (I/O)        (DB read)       (CPU: lofty)        (WAL commit)       (SQL diff)
//! ```
//!
//! * **Walk** is single-threaded: `readdir` on a warm page cache saturates at
//!   ~250k entries/s, faster than the parser can consume, and one walker keeps
//!   progress reporting and cancellation trivial.
//! * **Parse** is CPU-bound (decompression + tag parsing). Each file is one
//!   task on the blocking pool, with at most `workers` in flight: a file that
//!   panics or hangs costs that file and nothing else. There is no chunk
//!   boundary to fail at, which is what used to stop an index at a fixed track
//!   count.
//! * **Write** happens on the tokio thread in batches of [`BATCH_ROWS`] inside
//!   one transaction each. `SQLite` serialises writers, so a second writer
//!   thread would only add contention.
//! * **Missing sweep** diffs the walked set against the indexed set inside
//!   `SQLite`, then flags absent files. Rows are never deleted: playlists,
//!   ratings and play history outlive a disconnected drive.
//!
//! A corrupt file never aborts a scan. It is recorded in `scan_errors` with a
//! stable code and the walk continues — a library of 100k FLACs almost always
//! contains one truncated download.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::Digest;
use sqlx::SqlitePool;
use walkdir::WalkDir;

use crate::audio::cue::CueSheet;
use crate::db::catalog::{
    AlbumMeta, BATCH_ROWS, CatalogWriter, CueSheetRecord, CueTrackInfo, FileFingerprint,
    TrackRecord, TrackSource, album_key, artist_key, cue_track_key, file_track_key, path_key,
    stable_id,
};
use crate::error::PrismError;
use crate::metadata::tags::{self, TrackTags};
use crate::metadata::{AUDIO_EXTENSIONS, clean, is_audio_file, parse_year, sort_key};

/// Wall-clock budget for a single file.
///
/// Tag readers are third-party parsers over arbitrary bytes: a truncated FLAC
/// or a file on a flaky mount can block inside the reader forever. A budget per
/// file means the scan loses one file, not the library — 30 s is roughly two
/// hundred times the slowest healthy file measured on a spinning disk.
const PARSE_FILE_BUDGET: Duration = Duration::from_secs(30);

/// Minimum interval between progress callbacks, in milliseconds.
const PROGRESS_INTERVAL_MS: u64 = 200;

/// What started the scan; stored in `scan_runs.trigger`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScanTrigger {
    Startup,
    Manual,
    Watch,
    Scheduled,
    Cli,
}

impl ScanTrigger {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Startup => "startup",
            Self::Manual => "manual",
            Self::Watch => "watch",
            Self::Scheduled => "scheduled",
            Self::Cli => "cli",
        }
    }
}

/// Phase of the pipeline, surfaced to the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScanPhase {
    Walking,
    Parsing,
    Writing,
    Finalising,
    Completed,
    Cancelled,
    Failed,
}

/// Throttled progress snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub run_id: String,
    pub phase: ScanPhase,
    pub files_seen: u64,
    pub files_indexed: u64,
    pub files_skipped: u64,
    pub files_failed: u64,
    pub bytes_read: u64,
    /// 0.0–1.0 while walking/parsing; `None` when the total is unknown.
    pub percent: Option<f32>,
    pub current_path: Option<String>,
}

/// Knobs for one scan.
#[derive(Debug, Clone)]
pub struct ScanConfig {
    /// Roots to walk. Empty means "read the enabled `library_roots`".
    pub roots: Vec<PathBuf>,
    /// Ignore the mtime/size index and re-read every file.
    pub full: bool,
    /// Delete albums/artists that no longer own a track (full scans only).
    pub prune: bool,
    /// Stop storing errors after this many (the counter keeps counting).
    pub max_errors: usize,
    /// Rows per transaction.
    pub batch_size: usize,
}

impl Default for ScanConfig {
    fn default() -> Self {
        Self {
            roots: Vec::new(),
            full: false,
            prune: false,
            max_errors: 200,
            batch_size: BATCH_ROWS,
        }
    }
}

/// Result of a finished scan.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanOutcome {
    pub run_id: String,
    pub status: String,
    pub files_seen: u64,
    pub files_indexed: u64,
    pub files_skipped: u64,
    pub files_failed: u64,
    pub tracks_missing: u64,
    pub tracks_pruned: u64,
    pub bytes_read: u64,
    pub duration_ms: u64,
}

/// A single per-file failure.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanErrorRecord {
    pub id: String,
    pub path: String,
    pub code: String,
    pub message: String,
}

/// Directory names never walked: version-control metadata, macOS resource
/// forks, Synology thumbnails and our own cache.
const SKIP_DIRECTORIES: &[&str] = &[
    ".git",
    ".svn",
    ".hg",
    "@eaDir",
    "#recycle",
    "lost+found",
    "$RECYCLE.BIN",
    "System Volume Information",
    ".Trash",
    ".Trashes",
];

/// One file discovered during the walk.
#[derive(Debug, Clone)]
struct WalkEntry {
    path: PathBuf,
    mtime: i64,
    size: i64,
}

/// A CUE sheet expanded into its sheet row, its virtual tracks and the image
/// paths it owns (which must be excluded from the standalone-file index).
struct ExpandedCue {
    sheet: CueSheetRecord,
    records: Vec<TrackRecord>,
}

/// Library scanner handle. Cheap to clone: all state is behind `Arc`.
#[derive(Debug, Clone)]
pub struct Scanner {
    pool: SqlitePool,
    cancel: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
    progress: Arc<parking_lot::RwLock<Option<ScanProgress>>>,
    files_done: Arc<AtomicU64>,
}

impl Scanner {
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            cancel: Arc::new(AtomicBool::new(false)),
            running: Arc::new(AtomicBool::new(false)),
            progress: Arc::new(parking_lot::RwLock::new(None)),
            files_done: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Ask a running scan to stop at the next chunk boundary.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    #[must_use]
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    #[must_use]
    pub fn progress(&self) -> Option<ScanProgress> {
        self.progress.read().clone()
    }

    /// Enabled library roots as absolute paths.
    pub async fn roots(&self) -> Result<Vec<PathBuf>, PrismError> {
        let rows: Vec<String> =
            sqlx::query_scalar("SELECT path FROM library_roots WHERE enabled = 1 ORDER BY path")
                .fetch_all(&self.pool)
                .await?;
        Ok(rows.into_iter().map(PathBuf::from).collect())
    }

    /// Run a scan to completion.
    ///
    /// `on_progress` is called at most every [`PROGRESS_INTERVAL_MS`] and is
    /// always invoked at least once per phase transition, so the UI can never
    /// be left showing a stale phase.
    pub async fn scan<F>(
        &self,
        config: ScanConfig,
        trigger: ScanTrigger,
        on_progress: F,
    ) -> Result<ScanOutcome, PrismError>
    where
        F: Fn(ScanProgress) + Send + Sync,
    {
        if self
            .running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(PrismError::invalid("a library scan is already running"));
        }
        let guard = RunningGuard {
            flag: Arc::clone(&self.running),
            cancel: Arc::clone(&self.cancel),
        };
        self.cancel.store(false, Ordering::SeqCst);
        self.files_done.store(0, Ordering::SeqCst);
        let started = Instant::now();

        let result = self.scan_inner(config, trigger, &on_progress).await;
        drop(guard);

        // A scan that fails half-way must leave the ledger in a terminal state.
        // Otherwise `scan_runs` keeps a row with `status = 'running'`, the UI
        // polls it forever, and every later attempt reports "already running"
        // while nothing is running at all. The failure is recorded with the
        // error message so the next launch can explain it.
        if let Err(error) = &result {
            tracing::error!(%error, "library scan failed");
            if let Err(record) = self
                .record_failure(
                    &format!("{error}"),
                    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                )
                .await
            {
                tracing::warn!(%record, "cannot record the failed scan");
            }
            let failed = ScanProgress {
                run_id: String::new(),
                phase: ScanPhase::Failed,
                files_seen: 0,
                files_indexed: 0,
                files_skipped: 0,
                files_failed: 0,
                bytes_read: 0,
                percent: None,
                current_path: None,
            };
            on_progress(failed);
        }
        result
    }

    /// Stamp every stale `running` row as failed.
    ///
    /// Called once at startup: if the process dies mid-scan the row survives it,
    /// and a row that claims to be running is what makes the shell show a
    /// scanner that will never advance.
    pub async fn recover_stale_runs(&self) -> Result<u64, PrismError> {
        let affected = sqlx::query(
            "UPDATE scan_runs SET status = 'failed', \
                error_message = COALESCE(error_message, 'interrupted: the application exited mid-scan'), \
                finished_at = COALESCE(finished_at, strftime('%Y-%m-%dT%H:%M:%fZ','now')) \
             WHERE status = 'running'",
        )
        .execute(&self.pool)
        .await?
        .rows_affected();
        if affected > 0 {
            tracing::warn!(runs = affected, "recovered interrupted scans");
        }
        Ok(affected)
    }

    /// Record a run that failed outside `finish()`.
    async fn record_failure(&self, message: &str, elapsed_ms: u64) -> Result<(), PrismError> {
        sqlx::query(
            "UPDATE scan_runs SET status = 'failed', error_message = ?1, \
                finished_at = strftime('%Y-%m-%dT%H:%M:%fZ','now'), \
                duration_ms = ?2 \
             WHERE status = 'running'",
        )
        .bind(truncate(message, 1024))
        .bind(i64::try_from(elapsed_ms).unwrap_or(i64::MAX))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn scan_inner<F>(
        &self,
        config: ScanConfig,
        trigger: ScanTrigger,
        on_progress: &F,
    ) -> Result<ScanOutcome, PrismError>
    where
        F: Fn(ScanProgress) + Send + Sync,
    {
        let started = Instant::now();
        let run_id = stable_id("scan", &format!("{}", uuid::Uuid::new_v4()));

        // ---- Roots ---------------------------------------------------------
        let roots = if config.roots.is_empty() {
            self.roots().await?
        } else {
            config.roots.clone()
        };

        let mut root_strings: Vec<String> = Vec::new();
        let mut offline: Vec<(PathBuf, String)> = Vec::new();
        for root in &roots {
            match std::fs::metadata(root) {
                Ok(meta) if meta.is_dir() => {
                    root_strings.push(root.to_string_lossy().into_owned());
                }
                Ok(_) => offline.push((
                    root.clone(),
                    format!("{} is not a directory", root.display()),
                )),
                Err(error) => offline.push((root.clone(), error.to_string())),
            }
        }

        sqlx::query("INSERT INTO scan_runs (id, status, trigger) VALUES (?1, 'running', ?2)")
            .bind(&run_id)
            .bind(trigger.as_str())
            .execute(&self.pool)
            .await?;

        let mut progress = ScanProgress {
            run_id: run_id.clone(),
            phase: ScanPhase::Walking,
            files_seen: 0,
            files_indexed: 0,
            files_skipped: 0,
            files_failed: 0,
            bytes_read: 0,
            percent: Some(0.0),
            current_path: None,
        };
        self.publish(&progress, on_progress);

        let mut errors: Vec<ScanErrorRecord> = Vec::new();
        for (path, message) in &offline {
            if errors.len() < config.max_errors {
                errors.push(ScanErrorRecord {
                    id: stable_id("err", &format!("{run_id}:{path:?}")),
                    path: path.to_string_lossy().into_owned(),
                    code: "root_offline".to_string(),
                    message: message.clone(),
                });
            }
            tracing::warn!(root = %path.display(), %message, "library root unavailable");
        }

        if root_strings.is_empty() {
            let outcome = self
                .finish(
                    &run_id,
                    "completed",
                    &progress,
                    0,
                    0,
                    started.elapsed().as_millis() as u64,
                    Some("no readable library roots"),
                )
                .await?;
            tracing::warn!(run_id = %run_id, "scan finished without readable roots");
            let mut final_progress = progress;
            final_progress.phase = ScanPhase::Completed;
            self.publish(&final_progress, on_progress);
            return Ok(outcome);
        }

        // ---- Walk ----------------------------------------------------------
        let writer = CatalogWriter::new(self.pool.clone());
        let fingerprints: HashMap<String, FileFingerprint> = if config.full {
            HashMap::new()
        } else {
            writer
                .fingerprint_index(&root_strings)
                .await?
                .into_iter()
                .map(|fingerprint| (fingerprint.key.clone(), fingerprint))
                .collect()
        };

        let cue_digests = if config.full {
            HashMap::new()
        } else {
            writer.cue_digests().await?
        };

        let walked = walk_roots(&roots, &self.cancel, &mut progress, on_progress)?;
        progress.files_seen = walked.len() as u64;

        // A CUE sheet owns its image: the image must not also be indexed as a
        // standalone track, or every CUE album would show a duplicate row.
        let mut cue_images: HashSet<String> = HashSet::new();
        let mut cue_entries: Vec<WalkEntry> = Vec::new();
        let mut audio_entries: Vec<WalkEntry> = Vec::new();
        for entry in &walked {
            let extension = entry
                .path
                .extension()
                .and_then(|extension| extension.to_str())
                .map(|extension| extension.to_ascii_lowercase())
                .unwrap_or_default();
            if extension == "cue" {
                cue_entries.push(entry.clone());
            } else if is_audio_file(&entry.path) {
                audio_entries.push(entry.clone());
            }
        }
        for entry in &cue_entries {
            for image in referenced_images(entry) {
                cue_images.insert(path_key(&image.to_string_lossy()));
            }
        }
        audio_entries
            .retain(|entry| !cue_images.contains(&path_key(&entry.path.to_string_lossy())));
        let mut seen: Vec<String> = Vec::with_capacity(walked.len());
        // Sheets and their images exist by definition once walked; recording
        // them here keeps the missing sweep correct even for skipped sheets.
        for entry in &cue_entries {
            seen.push(path_key(&entry.path.to_string_lossy()));
        }
        seen.extend(cue_images.iter().cloned());

        // ---- Parse ---------------------------------------------------------
        progress.phase = ScanPhase::Parsing;
        progress.current_path = None;
        self.publish(&progress, on_progress);

        let cancelled = Arc::clone(&self.cancel);
        let fingerprints = Arc::new(fingerprints);
        let mut pending: Vec<TrackRecord> = Vec::with_capacity(config.batch_size);
        let mut cue_links: Vec<(String, String, String)> = Vec::new();
        let mut last_emit = Instant::now();
        let mut bytes: u64 = 0;

        // ---- CUE sheets ----------------------------------------------------
        // They are parsed first: every virtual track carries `cue_sheet_id`,
        // and the FK must be satisfiable inside the same transaction batch.
        for entry in &cue_entries {
            if cancelled.load(Ordering::SeqCst) {
                break;
            }
            match expand_cue(entry, &run_id, config.full, &cue_digests) {
                Ok(Some(expanded)) => {
                    bytes = bytes
                        .saturating_add(u64::try_from(expanded.sheet.content.len()).unwrap_or(0));
                    writer.upsert_cue_sheet(&expanded.sheet).await?;
                    if let Some(first) = expanded.records.first() {
                        cue_links.push((
                            expanded.sheet.id.clone(),
                            first.album_title.clone(),
                            first.album_artist_name.clone(),
                        ));
                    }
                    progress.files_indexed = progress
                        .files_indexed
                        .saturating_add(u64::try_from(expanded.records.len()).unwrap_or(0));
                    pending.extend(expanded.records);
                    if pending.len() >= config.batch_size {
                        self.flush(&writer, &mut pending).await?;
                        self.maybe_publish(&mut progress, on_progress, &mut last_emit, false);
                    }
                }
                Ok(None) => {
                    // Unchanged (or empty) sheet: its rows are already correct.
                    progress.files_skipped += 1;
                }
                Err(error) => {
                    progress.files_failed += 1;
                    if errors.len() < config.max_errors {
                        errors.push(error);
                    }
                }
            }
        }

        // ---- Audio files ---------------------------------------------------
        //
        // One task per file, at most `workers` in flight.
        //
        // The previous design parsed fixed chunks of 512 files inside a rayon
        // fan-out and wrapped the chunk in a single timeout. That has two
        // failure modes, and both were reported from the field: a single file
        // whose tag reader *panics* aborts the whole chunk (and therefore the
        // whole scan, always at the same file), and a single file whose reader
        // *blocks* costs the entire chunk up to the budget and then re-parses
        // all 512 files one by one. The library stopped at 1024 tracks — two
        // completed chunks — and never recovered.
        //
        // Per-file isolation removes the cliff: a panic is recorded against its
        // own file, a hang costs one file's budget, and the scan keeps going
        // for as many files as the library has.
        let workers = std::thread::available_parallelism()
            .map_or(2, std::num::NonZeroUsize::get)
            .clamp(2, 8);
        let mut cursor = 0usize;
        while cursor < audio_entries.len() {
            if cancelled.load(Ordering::SeqCst) {
                break;
            }
            let wave: Vec<WalkEntry> = audio_entries[cursor..]
                .iter()
                .take(workers)
                .cloned()
                .collect();
            cursor += wave.len();

            let mut handles = Vec::with_capacity(wave.len());
            for entry in wave {
                let run = run_id.clone();
                let index = Arc::clone(&fingerprints);
                let full = config.full;
                let label = entry.clone();
                handles.push((
                    entry,
                    tokio::task::spawn_blocking(move || {
                        parse_file_isolated(&label, &run, full, &index)
                    }),
                ));
            }

            // One deadline for the whole wave, not one per file: eight files
            // that all hang must cost one budget, not eight.
            let deadline = tokio::time::Instant::now() + PARSE_FILE_BUDGET;
            for (entry, handle) in handles {
                if cancelled.load(Ordering::SeqCst) {
                    break;
                }
                let outcome = match tokio::time::timeout_at(deadline, handle).await {
                    Ok(Ok(outcome)) => outcome,
                    // The task itself panicked outside `parse_file_isolated`'s
                    // own guard, or the runtime is shutting down.
                    Ok(Err(error)) => Err(ScanErrorRecord {
                        id: stable_id("err", &format!("{run_id}:join:{}", entry.path.display())),
                        path: entry.path.to_string_lossy().into_owned(),
                        code: "parser".to_string(),
                        message: format!("the tag parser failed: {error}"),
                    }),
                    // A blocking read cannot be interrupted. The file keeps its
                    // thread, the scan does not keep waiting for it.
                    Err(_elapsed) => Err(ScanErrorRecord {
                        id: stable_id("err", &format!("{run_id}:timeout:{}", entry.path.display())),
                        path: entry.path.to_string_lossy().into_owned(),
                        code: "parse_timeout".to_string(),
                        message: format!(
                            "reading tags took longer than {} s; the file was skipped",
                            PARSE_FILE_BUDGET.as_secs()
                        ),
                    }),
                };

                match outcome {
                    Ok(Some(record)) => {
                        bytes = bytes.saturating_add(u64::try_from(record.file_size).unwrap_or(0));
                        seen.push(path_key(&record.path));
                        pending.push(record);
                        if pending.len() >= config.batch_size {
                            self.flush(&writer, &mut pending).await?;
                        }
                    }
                    Ok(None) => {
                        progress.files_skipped += 1;
                        seen.push(path_key(&entry.path.to_string_lossy()));
                    }
                    Err(error) => {
                        progress.files_failed += 1;
                        if errors.len() < config.max_errors {
                            errors.push(error);
                        }
                    }
                }

                progress.current_path = Some(entry.path.to_string_lossy().into_owned());
                progress.bytes_read = bytes;
                self.maybe_publish(&mut progress, on_progress, &mut last_emit, false);
            }

            progress.files_indexed = self.files_done.load(Ordering::SeqCst);
            progress.bytes_read = bytes;
            self.maybe_publish(&mut progress, on_progress, &mut last_emit, false);
        }

        if !pending.is_empty() {
            self.flush(&writer, &mut pending).await?;
        }
        progress.files_indexed = self.files_done.load(Ordering::SeqCst);

        // Alben link for CUE sheets, now that their track rows exist.
        for (cue_id, album_title, album_artist) in &cue_links {
            writer
                .link_cue_album(cue_id, album_title, album_artist)
                .await?;
        }

        // ---- Finalise ------------------------------------------------------
        progress.phase = ScanPhase::Finalising;
        progress.files_indexed = self.files_done.load(Ordering::SeqCst);
        progress.bytes_read = bytes;
        self.publish(&progress, on_progress);

        let cancelled_now = cancelled.load(Ordering::SeqCst);
        let missing = if cancelled_now {
            0
        } else {
            writer.mark_missing(&root_strings, &seen).await?
        };

        if !cancelled_now && config.full {
            writer.reconcile_albums().await?;
        }

        let mut pruned = 0;
        if !cancelled_now && config.prune && config.full {
            let (albums, artists) = writer.prune_orphans().await?;
            pruned = albums + artists;
        }

        let status = if cancelled_now {
            "cancelled"
        } else {
            "completed"
        };
        let outcome = self
            .finish(
                &run_id,
                status,
                &progress,
                missing,
                pruned,
                u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                None,
            )
            .await?;

        for error in &errors {
            sqlx::query(
                "INSERT INTO scan_errors (id, run_id, path, error_code, message) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )
            .bind(&error.id)
            .bind(&run_id)
            .bind(&error.path)
            .bind(&error.code)
            .bind(truncate(&error.message, 1024))
            .execute(&self.pool)
            .await?;
        }

        progress.phase = if cancelled_now {
            ScanPhase::Cancelled
        } else {
            ScanPhase::Completed
        };
        progress.percent = Some(1.0);
        self.publish(&progress, on_progress);

        Ok(outcome)
    }

    // (the per-file parser used to live here; it is now `parse_file_isolated`)
    /// Write `pending` (draining it) and account for the rows.
    async fn flush(
        &self,
        writer: &CatalogWriter,
        pending: &mut Vec<TrackRecord>,
    ) -> Result<(), PrismError> {
        if pending.is_empty() {
            return Ok(());
        }
        let outcome = writer.write_batch(pending).await?;
        self.files_done
            .fetch_add(outcome.tracks_written as u64, Ordering::SeqCst);
        pending.clear();
        Ok(())
    }

    fn publish<F>(&self, progress: &ScanProgress, on_progress: &F)
    where
        F: Fn(ScanProgress) + Send + Sync,
    {
        *self.progress.write() = Some(progress.clone());
        on_progress(progress.clone());
    }

    fn maybe_publish<F>(
        &self,
        progress: &mut ScanProgress,
        on_progress: &F,
        last_emit: &mut Instant,
        force: bool,
    ) where
        F: Fn(ScanProgress) + Send + Sync,
    {
        if force || last_emit.elapsed().as_millis() >= u128::from(PROGRESS_INTERVAL_MS) {
            *last_emit = Instant::now();
            self.publish(progress, on_progress);
        }
    }

    /// Persist the final counters and return the outcome.
    async fn finish(
        &self,
        run_id: &str,
        status: &str,
        progress: &ScanProgress,
        missing: u64,
        pruned: u64,
        duration_ms: u64,
        note: Option<&str>,
    ) -> Result<ScanOutcome, PrismError> {
        sqlx::query(
            "UPDATE scan_runs SET status = ?2, files_seen = ?3, files_indexed = ?4, \
                files_skipped = ?5, files_failed = ?6, bytes_read = ?7, tracks_removed = ?8, \
                finished_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), duration_ms = ?9, \
                error_message = ?10 \
             WHERE id = ?1",
        )
        .bind(run_id)
        .bind(status)
        .bind(i64::try_from(progress.files_seen).unwrap_or(i64::MAX))
        .bind(i64::try_from(progress.files_indexed).unwrap_or(i64::MAX))
        .bind(i64::try_from(progress.files_skipped).unwrap_or(i64::MAX))
        .bind(i64::try_from(progress.files_failed).unwrap_or(i64::MAX))
        .bind(i64::try_from(progress.bytes_read).unwrap_or(i64::MAX))
        .bind(i64::try_from(missing).unwrap_or(i64::MAX))
        .bind(i64::try_from(duration_ms).unwrap_or(i64::MAX))
        .bind(note)
        .execute(&self.pool)
        .await?;

        tracing::info!(
            run_id,
            status,
            indexed = progress.files_indexed,
            skipped = progress.files_skipped,
            failed = progress.files_failed,
            missing,
            pruned,
            duration_ms,
            "library scan finished"
        );

        Ok(ScanOutcome {
            run_id: run_id.to_string(),
            status: status.to_string(),
            files_seen: progress.files_seen,
            files_indexed: progress.files_indexed,
            files_skipped: progress.files_skipped,
            files_failed: progress.files_failed,
            tracks_missing: missing,
            tracks_pruned: pruned,
            bytes_read: progress.bytes_read,
            duration_ms,
        })
    }
}

/// Clears the `running` flag and the cancel token even on early return.
struct RunningGuard {
    flag: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
}

impl Drop for RunningGuard {
    fn drop(&mut self) {
        self.flag.store(false, Ordering::SeqCst);
        self.cancel.store(false, Ordering::SeqCst);
    }
}

/// Parse one file, surviving anything the tag reader can do.
///
/// `lofty` walks arbitrary bytes from disk: a truncated FLAC, a mislabelled
/// container or a tag with a corrupt length can make it panic. A panic in a
/// worker must cost exactly one file. `catch_unwind` turns it into a recorded
/// error the UI can show, instead of unwinding into the scan task and taking
/// the whole library with it — which is how a single bad file used to stop an
/// index at a fixed track count, every time.
fn parse_file_isolated(
    entry: &WalkEntry,
    run_id: &str,
    full: bool,
    fingerprints: &HashMap<String, FileFingerprint>,
) -> Result<Option<TrackRecord>, ScanErrorRecord> {
    // No hook juggling: the panic hook is process-global, and swapping it from
    // several parsing threads at once would race. The default hook logs the
    // panic, which is useful; what matters here is that the unwind stops at
    // this file instead of tearing the scan down.
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        parse_file(entry, run_id, full, fingerprints)
    }));

    match outcome {
        Ok(result) => result,
        Err(payload) => {
            let reason = payload
                .downcast_ref::<&str>()
                .map(|text| (*text).to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown parser panic".to_string());
            let record = ScanErrorRecord {
                id: stable_id("err", &format!("{run_id}:panic:{}", entry.path.display())),
                path: entry.path.to_string_lossy().into_owned(),
                code: "parse_panic".to_string(),
                message: format!("the tag parser failed on this file: {reason}"),
            };
            tracing::warn!(path = %record.path, %reason, "tag parser panicked on a file");
            Err(record)
        }
    }
}

/// Walk every root, skipping VCS/cache directories and hidden entries.
fn walk_roots<F>(
    roots: &[PathBuf],
    cancel: &Arc<AtomicBool>,
    progress: &mut ScanProgress,
    on_progress: &F,
) -> Result<Vec<WalkEntry>, PrismError>
where
    F: Fn(ScanProgress) + Send + Sync,
{
    let mut entries = Vec::new();
    let mut last_emit = Instant::now();

    for root in roots {
        let walker = WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|entry| {
                if !entry.file_type().is_dir() {
                    return true;
                }
                let Some(name) = entry.file_name().to_str() else {
                    return false;
                };
                if name.starts_with('.') && entry.depth() > 0 {
                    return false;
                }
                !SKIP_DIRECTORIES
                    .iter()
                    .any(|skip| skip.eq_ignore_ascii_case(name))
            });

        for item in walker {
            if cancel.load(Ordering::SeqCst) {
                return Ok(entries);
            }
            let entry = match item {
                Ok(entry) => entry,
                Err(error) => {
                    tracing::warn!(
                        path = %error.path().map(|path| path.display().to_string()).unwrap_or_default(),
                        %error,
                        "directory entry unreadable"
                    );
                    continue;
                }
            };
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            let extension = path
                .extension()
                .and_then(|extension| extension.to_str())
                .map(|extension| extension.to_ascii_lowercase());
            let Some(extension) = extension else {
                continue;
            };
            if extension != "cue" && !AUDIO_EXTENSIONS.contains(&extension.as_str()) {
                continue;
            }
            let metadata = match entry.metadata() {
                Ok(metadata) => metadata,
                Err(error) => {
                    tracing::debug!(path = %path.display(), %error, "stat failed during walk");
                    continue;
                }
            };
            entries.push(WalkEntry {
                path: path.to_path_buf(),
                mtime: mtime_seconds(metadata.modified().ok()),
                size: i64::try_from(metadata.len()).unwrap_or(i64::MAX),
            });
            progress.files_seen = entries.len() as u64;
            if last_emit.elapsed().as_millis() >= u128::from(PROGRESS_INTERVAL_MS) {
                last_emit = Instant::now();
                progress.current_path = Some(path.to_string_lossy().into_owned());
                on_progress(progress.clone());
            }
        }
    }

    Ok(entries)
}

/// `FILE` targets of a CUE sheet, resolved without fully parsing it.
///
/// This is intentionally cheaper and more forgiving than the real parser: the
/// only consumer is the exclusion set, and a malformed sheet is reported later
/// by [`parse_cue`] with a proper error.
fn referenced_images(entry: &WalkEntry) -> Vec<PathBuf> {
    let Ok(content) = std::fs::read_to_string(&entry.path) else {
        return Vec::new();
    };
    let base = entry.path.parent().unwrap_or_else(|| Path::new("."));
    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if !trimmed.to_ascii_uppercase().starts_with("FILE") {
                return None;
            }
            let rest = trimmed[4..].trim();
            let name = if let Some(quoted) = rest.strip_prefix('"') {
                quoted.split('"').next().unwrap_or("")
            } else {
                rest.split_whitespace().next().unwrap_or("")
            };
            if name.is_empty() {
                return None;
            }
            Some(base.join(name))
        })
        .collect()
}

/// Parse a single audio file, honouring the incremental fingerprint.
///
/// `Ok(None)` means "unchanged since the last scan": the row already in the
/// database is correct and does not need a write.
fn parse_file(
    entry: &WalkEntry,
    run_id: &str,
    full: bool,
    fingerprints: &HashMap<String, FileFingerprint>,
) -> Result<Option<TrackRecord>, ScanErrorRecord> {
    let key = path_key(&entry.path.to_string_lossy());
    if !full
        && let Some(fingerprint) = fingerprints.get(&key)
        && fingerprint.mtime == entry.mtime
        && fingerprint.size == entry.size
    {
        return Ok(None);
    }

    tags::read(&entry.path)
        .map(|tags| Some(record_from_tags(tags, &entry.path, entry.mtime, entry.size)))
        .map_err(|error| ScanErrorRecord {
            // Scoped by run: `scan_errors.id` is a primary key, and the same
            // broken file is reported again on every scan.
            id: stable_id("err", &format!("{run_id}:{key}:{}", error.code())),
            path: entry.path.to_string_lossy().into_owned(),
            code: error.code().to_string(),
            message: error.to_string(),
        })
}

/// Expand a CUE sheet into one sheet row plus one row per virtual track.
///
/// Returns `Ok(None)` when the sheet is empty (no `TRACK` entries); that is a
/// user error worth reporting once, not an error per second.
fn expand_cue(
    entry: &WalkEntry,
    run_id: &str,
    full: bool,
    digests: &HashMap<String, String>,
) -> Result<Option<ExpandedCue>, ScanErrorRecord> {
    let key = path_key(&entry.path.to_string_lossy());
    let fail = |code: &str, message: String| ScanErrorRecord {
        id: stable_id("err", &format!("{run_id}:{key}")),
        path: entry.path.to_string_lossy().into_owned(),
        code: code.to_string(),
        message,
    };

    let content =
        std::fs::read_to_string(&entry.path).map_err(|error| fail("io", error.to_string()))?;
    let digest = hex::encode(sha2::Sha256::digest(content.as_bytes()));
    if !full && digests.get(&key).is_some_and(|stored| stored == &digest) {
        return Ok(None);
    }
    let base = entry
        .path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();

    let mut sheet = CueSheet::parse(&content, &entry.path, &base)
        .map_err(|error| fail("cue", error.to_string()))?;
    if sheet.track_count() == 0 {
        return Ok(None);
    }

    // Durations and technical properties come from the referenced image. A
    // missing image leaves the last track open-ended instead of failing the
    // whole sheet — a NAS that is still mounting must not wipe the album.
    let mut images: HashMap<PathBuf, Option<TrackTags>> = HashMap::new();
    for file in &sheet.files {
        images.insert(
            file.resolved_path.clone(),
            tags::read(&file.resolved_path).ok(),
        );
    }
    let durations: HashMap<PathBuf, u64> = images
        .iter()
        .map(|(path, tags)| {
            (
                path.clone(),
                tags.as_ref()
                    .map_or(0, |tags| u64::try_from(tags.duration_ms).unwrap_or(0)),
            )
        })
        .collect();
    sheet.resolve_boundaries(|path| {
        durations
            .get(path)
            .copied()
            .filter(|duration| *duration > 0)
    });

    let cue_id = stable_id("cs", &key);
    let sheet_title = sheet
        .title
        .as_deref()
        .map(clean)
        .filter(|title| !title.is_empty());
    let folder_name = base
        .file_name()
        .and_then(|name| name.to_str())
        .map(clean)
        .filter(|name| !name.is_empty());
    let album_title = sheet_title
        .clone()
        .or(folder_name)
        .unwrap_or_else(|| "Unknown Album".to_string());
    let sheet_performer = sheet
        .performer
        .as_deref()
        .map(clean)
        .filter(|performer| !performer.is_empty());

    let mut records = Vec::new();
    let mut infos = Vec::new();
    let mut total_ms: i64 = 0;
    let mut image_paths: Vec<String> = Vec::new();

    for file in &sheet.files {
        let image_tags = images.get(&file.resolved_path).and_then(Clone::clone);
        image_paths.push(file.resolved_path.to_string_lossy().into_owned());
        let album_artist = image_tags
            .as_ref()
            .map(|tags| {
                if tags.album_artist.trim().is_empty() {
                    tags.artist.clone()
                } else {
                    tags.album_artist.clone()
                }
            })
            .filter(|artist| !artist.trim().is_empty())
            .or_else(|| sheet_performer.clone())
            .unwrap_or_else(|| "Unknown Artist".to_string());

        for track in &file.tracks {
            let title = track
                .title
                .as_deref()
                .map(clean)
                .filter(|title| !title.is_empty())
                .unwrap_or_else(|| format!("Track {:02}", track.number));
            let performer = track
                .performer
                .as_deref()
                .map(clean)
                .filter(|performer| !performer.is_empty())
                .or_else(|| sheet_performer.clone())
                .or_else(|| image_tags.as_ref().map(|tags| tags.artist.clone()))
                .filter(|performer| !performer.is_empty())
                .unwrap_or_else(|| "Unknown Artist".to_string());

            let start_ms = i64::try_from(track.start_ms).unwrap_or(i64::MAX);
            let end_ms = track
                .end_ms
                .map(|end| i64::try_from(end).unwrap_or(i64::MAX));
            let duration = end_ms.map_or(0, |end| end.saturating_sub(start_ms)).max(0);
            total_ms = total_ms.saturating_add(duration);

            let track_id = stable_id(
                "tr",
                &cue_track_key(&entry.path.to_string_lossy(), start_ms),
            );
            let image = image_tags.as_ref();
            records.push(TrackRecord {
                id: track_id.clone(),
                path: entry.path.to_string_lossy().into_owned(),
                folder: base.to_string_lossy().into_owned(),
                filename: entry
                    .path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                source: TrackSource::Cue,
                title: truncate(&title, 2048),
                title_sort: sort_key(&title),
                artist_name: truncate(&performer, 512),
                album_title: truncate(&album_title, 1024),
                album_artist_name: truncate(&album_artist, 512),
                composer: track
                    .songwriter
                    .as_deref()
                    .map(clean)
                    .or_else(|| sheet.songwriter.as_deref().map(clean))
                    .unwrap_or_default(),
                genre: sheet.genre.as_deref().map(clean).unwrap_or_default(),
                comment: sheet.comment.as_deref().map(clean).unwrap_or_default(),
                year: sheet.date.as_deref().and_then(parse_year),
                track_no: Some(i64::from(track.number)),
                disc_no: Some(1),
                track_total: Some(i64::try_from(file.tracks.len()).unwrap_or(0)),
                disc_total: Some(1),
                format: file
                    .resolved_path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .map(|extension| extension.to_ascii_lowercase())
                    .unwrap_or_else(|| "wav".to_string()),
                container: image.and_then(|tags| tags.container.clone()),
                codec_profile: image.and_then(|tags| tags.codec_profile.clone()),
                bit_depth: image.and_then(|tags| tags.bit_depth),
                sample_rate: image.and_then(|tags| tags.sample_rate),
                channels: image.and_then(|tags| tags.channels),
                bitrate_kbps: image.and_then(|tags| tags.bitrate_kbps),
                dsd_rate: image.and_then(|tags| tags.dsd_rate),
                duration_ms: duration,
                file_size: image.map_or(0, |tags| tags.file_size),
                replaygain_track_gain_db: None,
                replaygain_track_peak: None,
                replaygain_album_gain_db: image.and_then(|tags| tags.replaygain_album_gain_db),
                replaygain_album_peak: image.and_then(|tags| tags.replaygain_album_peak),
                musicbrainz_track_id: None,
                musicbrainz_release_track_id: None,
                isrc: track.isrc.clone(),
                file_mtime: entry.mtime,
                cue_sheet_id: Some(cue_id.clone()),
                cue_start_ms: Some(start_ms),
                cue_end_ms: end_ms.filter(|end| *end > start_ms),
                has_embedded_art: image.is_some_and(|tags| tags.has_embedded_art),
                extra_tags: "{}".to_string(),
                album: album_meta_from(&image_tags),
            });

            infos.push(CueTrackInfo {
                number: track.number,
                title,
                performer,
                start_ms,
                end_ms,
                duration_ms: duration,
                track_id: Some(track_id),
            });
        }
    }

    let first_image = image_paths.first().cloned();
    let sheet_record = CueSheetRecord {
        id: cue_id,
        path: entry.path.to_string_lossy().into_owned(),
        album_image: first_image
            .clone()
            .unwrap_or_else(|| entry.path.to_string_lossy().into_owned()),
        sha256: digest,
        catalog: sheet.catalog.as_deref().map(clean),
        performer: sheet_performer,
        title: sheet_title,
        songwriter: sheet.songwriter.as_deref().map(clean),
        genre: sheet.genre.as_deref().map(clean),
        date: sheet.date.as_deref().map(clean),
        disc_id: sheet.disc_id.as_deref().map(clean),
        comment: sheet.comment.as_deref().map(clean),
        track_count: i64::try_from(infos.len()).unwrap_or(0),
        total_ms,
        content,
        tracks: infos,
    };

    Ok(Some(ExpandedCue {
        sheet: sheet_record,
        records,
    }))
}

/// Convert parsed tags into a row, applying the fallbacks the schema requires
/// (non-empty title, non-null album, clamped ranges).
fn record_from_tags(tags: TrackTags, path: &Path, mtime: i64, size: i64) -> TrackRecord {
    let folder = path
        .parent()
        .map(|parent| parent.to_string_lossy().into_owned())
        .unwrap_or_default();
    let filename = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();

    let artist_name = if tags.artist.trim().is_empty() {
        "Unknown Artist".to_string()
    } else {
        tags.artist.clone()
    };
    let album_artist_name = if tags.album_artist.trim().is_empty() {
        artist_name.clone()
    } else {
        tags.album_artist.clone()
    };
    let album_title = if tags.album.trim().is_empty() {
        Path::new(&folder)
            .file_name()
            .and_then(|name| name.to_str())
            .map(clean)
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "Unknown Album".to_string())
    } else {
        tags.album.clone()
    };
    let format = if tags.format.is_empty() {
        "audio".to_string()
    } else {
        tags.format.clone()
    };
    let extra_tags = serde_json::to_string(&tags.extra).unwrap_or_else(|_| "{}".to_string());
    let year = tags.year.filter(|year| (1000..=2999).contains(year));

    TrackRecord {
        id: stable_id("tr", &file_track_key(&path.to_string_lossy())),
        path: path.to_string_lossy().into_owned(),
        folder,
        filename,
        source: TrackSource::File,
        title: truncate(
            if tags.title.trim().is_empty() {
                "Untitled"
            } else {
                &tags.title
            },
            2048,
        ),
        title_sort: tags.title_sort.clone(),
        artist_name: truncate(&artist_name, 512),
        album_title: truncate(&album_title, 1024),
        album_artist_name: truncate(&album_artist_name, 512),
        composer: tags.composer.clone(),
        genre: tags.genre.clone(),
        comment: tags.comment.clone(),
        year,
        track_no: tags.track_no.filter(|value| (0..=9999).contains(value)),
        disc_no: tags.disc_no.filter(|value| (0..=999).contains(value)),
        track_total: tags.track_total.filter(|value| (0..=9999).contains(value)),
        disc_total: tags.disc_total.filter(|value| (0..=999).contains(value)),
        format,
        container: tags.container.clone(),
        codec_profile: tags.codec_profile.clone(),
        bit_depth: tags.bit_depth.filter(|value| (1..=64).contains(value)),
        sample_rate: tags
            .sample_rate
            .filter(|value| (8_000..=1_536_000).contains(value)),
        channels: tags.channels.filter(|value| (1..=16).contains(value)),
        bitrate_kbps: tags.bitrate_kbps.filter(|value| *value >= 0),
        dsd_rate: tags.dsd_rate,
        duration_ms: tags.duration_ms.max(0),
        file_size: size.max(0),
        replaygain_track_gain_db: tags.replaygain_track_gain_db,
        replaygain_track_peak: tags.replaygain_track_peak,
        replaygain_album_gain_db: tags.replaygain_album_gain_db,
        replaygain_album_peak: tags.replaygain_album_peak,
        musicbrainz_track_id: tags.musicbrainz_track_id.clone(),
        musicbrainz_release_track_id: tags.musicbrainz_release_track_id.clone(),
        isrc: tags.isrc.clone(),
        file_mtime: mtime,
        cue_sheet_id: None,
        cue_start_ms: None,
        cue_end_ms: None,
        has_embedded_art: tags.has_embedded_art,
        extra_tags,
        album: AlbumMeta {
            year,
            original_year: tags
                .original_year
                .filter(|year| (1000..=2999).contains(year)),
            release_date: tags.release_date.clone(),
            release_type: release_type(&tags),
            is_compilation: tags.is_compilation,
            label: tags.label.clone(),
            catalog_number: tags.catalog_number.clone(),
            barcode: tags.barcode.clone(),
            musicbrainz_release_id: tags.musicbrainz_release_id.clone(),
            musicbrainz_release_group_id: tags.musicbrainz_release_group_id.clone(),
        },
    }
}

fn album_meta_from(tags: &Option<TrackTags>) -> AlbumMeta {
    let Some(tags) = tags else {
        return AlbumMeta::default();
    };
    AlbumMeta {
        year: tags.year.filter(|year| (1000..=2999).contains(year)),
        original_year: tags
            .original_year
            .filter(|year| (1000..=2999).contains(year)),
        release_date: tags.release_date.clone(),
        release_type: release_type(tags),
        is_compilation: tags.is_compilation,
        label: tags.label.clone(),
        catalog_number: tags.catalog_number.clone(),
        barcode: tags.barcode.clone(),
        musicbrainz_release_id: tags.musicbrainz_release_id.clone(),
        musicbrainz_release_group_id: tags.musicbrainz_release_group_id.clone(),
    }
}

/// Heuristic release type from track/disc counts, used until MusicBrainz
/// resolves the release group.
fn release_type(tags: &TrackTags) -> Option<String> {
    if tags.is_compilation {
        return Some("compilation".to_string());
    }
    match tags.track_total {
        Some(tracks) if tracks <= 3 => Some("single".to_string()),
        Some(tracks) if tracks <= 6 => Some("ep".to_string()),
        _ => Some("album".to_string()),
    }
}

fn mtime_seconds(value: Option<SystemTime>) -> i64 {
    value
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |duration| {
            i64::try_from(duration.as_secs()).unwrap_or(i64::MAX)
        })
}

fn truncate(value: &str, max: usize) -> String {
    let trimmed = value.trim();
    if trimmed.chars().count() <= max {
        return trimmed.to_string();
    }
    trimmed.chars().take(max).collect()
}

/// Convenience used by tests and by the CLI: scan with no progress sink.
pub async fn scan_quiet(
    scanner: &Scanner,
    config: ScanConfig,
    trigger: ScanTrigger,
) -> Result<ScanOutcome, PrismError> {
    scanner.scan(config, trigger, |_| {}).await
}

/// True when two tag-derived identities would land on the same album row.
#[must_use]
pub fn same_album(first: &TrackRecord, second: &TrackRecord) -> bool {
    album_key(&first.album_title, &first.album_artist_name)
        == album_key(&second.album_title, &second.album_artist_name)
}

/// Artist identity used by the scanner when grouping.
#[must_use]
pub fn same_artist(first: &str, second: &str) -> bool {
    artist_key(first) == artist_key(second)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::tags::TrackTags;

    fn empty_tags() -> TrackTags {
        TrackTags {
            title: String::new(),
            title_sort: String::new(),
            artist: String::new(),
            artist_sort: String::new(),
            album: String::new(),
            album_sort: String::new(),
            album_artist: String::new(),
            album_artist_sort: String::new(),
            composer: String::new(),
            genre: String::new(),
            comment: String::new(),
            year: None,
            original_year: None,
            release_date: None,
            track_no: None,
            track_total: None,
            disc_no: None,
            disc_total: None,
            format: "flac".to_string(),
            container: Some("FLAC".to_string()),
            codec_profile: None,
            bit_depth: Some(24),
            sample_rate: Some(96_000),
            channels: Some(2),
            bitrate_kbps: Some(2_000),
            dsd_rate: None,
            duration_ms: 180_000,
            file_size: 1_000,
            replaygain_track_gain_db: None,
            replaygain_track_peak: None,
            replaygain_album_gain_db: None,
            replaygain_album_peak: None,
            musicbrainz_track_id: None,
            musicbrainz_release_id: None,
            musicbrainz_release_track_id: None,
            musicbrainz_artist_id: None,
            musicbrainz_release_group_id: None,
            isrc: None,
            label: None,
            catalog_number: None,
            barcode: None,
            has_embedded_art: false,
            is_compilation: false,
            extra: serde_json::Map::new(),
        }
    }

    #[test]
    fn untagged_files_get_the_folder_as_album() {
        let record = record_from_tags(
            empty_tags(),
            Path::new("/music/Boards of Canada/Music Has the Right to Children/01.flac"),
            1_700_000_000,
            42,
        );
        assert_eq!(record.title, "Untitled");
        assert_eq!(record.artist_name, "Unknown Artist");
        assert_eq!(record.album_artist_name, "Unknown Artist");
        assert_eq!(record.album_title, "Music Has the Right to Children");
        assert_eq!(record.source, TrackSource::File);
        assert_eq!(record.duration_ms, 180_000);
        assert_eq!(record.album.year, None);
    }

    #[test]
    fn identity_is_deterministic_for_the_same_path() {
        let first = record_from_tags(empty_tags(), Path::new("/music/a.flac"), 1, 1);
        let second = record_from_tags(empty_tags(), Path::new("/music/a.flac"), 9, 99);
        assert_eq!(first.id, second.id);
        assert_eq!(first.id, stable_id("tr", "/music/a.flac|file|-1"));
        let other = record_from_tags(empty_tags(), Path::new("/music/b.flac"), 1, 1);
        assert_ne!(first.id, other.id);
    }

    #[test]
    fn out_of_range_values_are_dropped_before_they_reach_the_schema() {
        let mut tags = empty_tags();
        tags.year = Some(12_345);
        tags.sample_rate = Some(4_000);
        tags.bit_depth = Some(99);
        tags.disc_no = Some(1_000);
        tags.track_no = Some(-3);
        let record = record_from_tags(tags, Path::new("/music/x.flac"), 0, 0);
        assert_eq!(record.year, None);
        assert_eq!(record.sample_rate, None);
        assert_eq!(record.bit_depth, None);
        assert_eq!(record.disc_no, None);
        assert_eq!(record.track_no, None);
    }

    #[test]
    fn release_type_is_inferred_from_the_track_count() {
        let mut tags = empty_tags();
        tags.track_total = Some(2);
        assert_eq!(release_type(&tags).as_deref(), Some("single"));
        tags.track_total = Some(5);
        assert_eq!(release_type(&tags).as_deref(), Some("ep"));
        tags.track_total = Some(11);
        assert_eq!(release_type(&tags).as_deref(), Some("album"));
        tags.is_compilation = true;
        assert_eq!(release_type(&tags).as_deref(), Some("compilation"));
    }

    #[test]
    fn mtime_conversion_is_monotonic_and_saturating() {
        assert_eq!(mtime_seconds(None), 0);
        assert!(mtime_seconds(Some(SystemTime::now())) > 1_600_000_000);
    }
}
