//! Logging bootstrap.
//!
//! Two sinks are active at the same time:
//!
//!   * **stderr** — visible in `tauri dev`, CI and consoles. Coloured only when
//!     the stream is a TTY so redirected logs stay clean.
//!   * **daily-rotating file** under the platform application log directory,
//!     written through `tracing_appender`'s non-blocking channel so an audio
//!     callback can never stall on disk I/O.
//!
//! The file sink is attached during Tauri's `setup` hook, once the application
//! path resolver is available. The [`WorkerGuard`] returned by
//! [`attach_file_sink`] must live for the whole process; it flushes the channel
//! on shutdown. `AppState` owns it.

use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};
use tracing_appender::rolling;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use crate::error::PrismError;

/// Global handle to the asynchronous file sink. `None` until the Tauri setup
/// hook attaches it.
static FILE_SINK: OnceLock<Mutex<Option<NonBlocking>>> = OnceLock::new();

fn sink_slot() -> &'static Mutex<Option<NonBlocking>> {
    FILE_SINK.get_or_init(|| Mutex::new(None))
}

/// Guard that keeps the log worker alive for the process lifetime.
///
/// Stored as Tauri managed state; dropping it flushes pending lines.
pub struct LogSink {
    _guard: WorkerGuard,
}

// `WorkerGuard` deliberately does not implement Debug; the guard owns no
// inspectable state, so the implementation is explicit and non-leaking.
impl std::fmt::Debug for LogSink {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("LogSink").finish_non_exhaustive()
    }
}

/// Writer that mirrors every event to stderr and, when attached, to the
/// non-blocking file appender.
#[derive(Debug, Clone, Copy)]
struct Tee;

struct TeeStream {
    stderr: io::Stderr,
}

impl Write for TeeStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // stderr is unbuffered and cheap; failures must not break logging.
        let _ = self.stderr.write_all(buf);

        if let Ok(mut slot) = sink_slot().lock()
            && let Some(sink) = slot.as_mut()
        {
            // NonBlocking never blocks: it dispatches to its own worker and
            // drops lines under extreme backpressure instead of stalling.
            let _ = sink.write_all(buf);
        }

        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        let _ = self.stderr.flush();
        if let Ok(mut slot) = sink_slot().lock()
            && let Some(sink) = slot.as_mut()
        {
            let _ = sink.flush();
        }
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Tee {
    type Writer = TeeStream;

    fn make_writer(&'a self) -> Self::Writer {
        TeeStream {
            stderr: io::stderr(),
        }
    }
}

/// Install the global subscriber. Must be called once, before Tauri boots.
///
/// `PRISM_LOG` follows the standard `EnvFilter` syntax, e.g.
/// `PRISM_LOG="prism=debug,tauri=info,symphonia=warn"`.
pub fn init() -> Result<(), PrismError> {
    let default_filter = if cfg!(debug_assertions) {
        "prism=debug,tauri=info,wry=warn,sqlx=warn,info"
    } else {
        "prism=info,warn"
    };

    let filter =
        EnvFilter::try_from_env("PRISM_LOG").unwrap_or_else(|_| EnvFilter::new(default_filter));

    let ansi = io::stderr().is_terminal() && std::env::var_os("NO_COLOR").is_none();

    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_writer(Tee)
        .with_target(true)
        .with_ansi(ansi)
        .with_thread_ids(false)
        .with_file(false)
        .with_line_number(false);

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt_layer)
        .try_init()
        .map_err(|err| PrismError::Internal(format!("logging already initialised: {err}")))
}

/// Attach the rolling file sink. Called from Tauri's `setup` hook.
///
/// Creates the log directory if needed and emits one line per day per file
/// (`prism.log.YYYY-MM-DD`, UTC) with a 7-day retention hint enforced by the
/// platform-specific cleaner in [`purge_old_logs`].
pub fn attach_file_sink(log_dir: &Path) -> Result<LogSink, PrismError> {
    fs::create_dir_all(log_dir).map_err(|err| PrismError::io(log_dir, err))?;

    let appender = rolling::daily(log_dir, "prism.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(appender);

    {
        let mut slot = sink_slot()
            .lock()
            .map_err(|_| PrismError::Internal("log sink mutex poisoned".into()))?;
        *slot = Some(non_blocking);
    }

    // The guard must outlive every log line; it is moved into the returned
    // `LogSink` immediately so no temporary can drop it early.
    Ok(LogSink { _guard: guard })
}

/// Best-effort removal of log files older than `keep_days`.
///
/// Returns the number of removed files. Errors are reported but never fatal:
/// logging must not be able to prevent the player from starting.
pub fn purge_old_logs(log_dir: &Path, keep_days: u64) -> io::Result<usize> {
    let max_age = std::time::Duration::from_secs(keep_days.saturating_mul(86_400));
    let now = std::time::SystemTime::now();
    let mut removed = 0usize;

    for entry in fs::read_dir(log_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !entry.file_type()?.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.starts_with("prism.log") {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        let Ok(age) = now.duration_since(modified) else {
            continue;
        };
        if age > max_age && fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }

    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn purge_removes_only_old_prism_logs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let recent = dir.path().join("prism.log.2026-01-01");
        let unrelated = dir.path().join("notes.txt");
        fs::write(&recent, b"recent").expect("write");
        fs::write(&unrelated, b"keep").expect("write");
        std::thread::sleep(std::time::Duration::from_millis(20));

        // File modified "now": with keep_days = 0, everything older than 0s
        // qualifies, so the log file must go while unrelated files stay.
        let removed = purge_old_logs(dir.path(), 0).expect("purge");
        assert_eq!(removed, 1);
        assert!(!recent.exists());
        assert!(unrelated.exists());
    }
}
