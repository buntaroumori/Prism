//! Prism — library backend and Tauri shell.
//!
//! FASE 1 scope:
//!   * bootstraps tracing (console + rolling file),
//!   * opens the `WAL`-mode `SQLite` library and applies embedded migrations,
//!   * exposes the system health command surface,
//!   * enforces a single running instance that re-focuses its window.
//!
//! Audio (FASE 2), metadata/scanner/DB domain queries (FASE 3), Discord RPC
//! (FASE 3), plugin runtime (FASE 5) and the React shell (FASE 4) plug into the
//! modules declared here.

pub mod commands;
pub mod db;
pub mod error;
pub mod logging;
pub mod state;

use tauri::Manager;

use crate::state::AppState;

/// Number of days of rotating logs kept on disk.
const LOG_RETENTION_DAYS: u64 = 14;

/// Application entry point called from `main.rs`.
///
/// Any fatal startup error is printed to stderr and terminates with a non-zero
/// exit code: a player that boots without a database must never present a
/// broken UI.
pub fn run() {
    if let Err(error) = run_application() {
        eprintln!("[prism] fatal error: {error:#}");
        std::process::exit(1);
    }
}

fn run_application() -> Result<(), Box<dyn std::error::Error>> {
    logging::init()?;

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        os = std::env::consts::OS,
        arch = std::env::consts::ARCH,
        profile = if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        "starting Prism"
    );

    tauri::Builder::default()
        // Single-instance MUST be the first plugin registered. A second launch
        // forwards its context to the running instance and exits.
        .plugin(tauri_plugin_single_instance::init(|app, argv, cwd| {
            tracing::info!(argv = ?argv, cwd = %cwd, "second instance requested focus");
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let handle = app.handle().clone();

            // --- Rolling file logging (paths resolved only now) ---------------
            let log_dir = app.path().app_log_dir()?;
            std::fs::create_dir_all(&log_dir)?;
            let sink = logging::attach_file_sink(&log_dir)?;
            // Keep the appender's WorkerGuard alive for the whole process.
            app.manage(std::sync::Mutex::new(sink));

            match logging::purge_old_logs(&log_dir, LOG_RETENTION_DAYS) {
                Ok(removed) if removed > 0 => {
                    tracing::debug!(removed, "pruned expired log files");
                }
                Ok(_) => {}
                Err(err) => tracing::warn!(error = %err, "log retention sweep failed"),
            }

            // --- Database bootstrap -------------------------------------------
            let state = tauri::async_runtime::block_on(AppState::initialize(&handle))?;
            tracing::info!(
                database = %state.database_path().display(),
                "library database ready"
            );
            app.manage(state);

            // The window is created hidden (`visible: false`) to avoid the
            // white flash before React paints the OLED background.
            if let Some(window) = app.get_webview_window("main") {
                window.show()?;
                window.set_focus()?;
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::library::get_library_stats,
            commands::system::get_app_info,
            commands::system::get_runtime_health,
            commands::system::open_log_folder,
            commands::system::open_data_folder,
        ])
        .run(tauri::generate_context!())?;

    tracing::info!("event loop terminated");
    Ok(())
}

/// Convert an unexpected panic payload into a readable message.
///
/// Used by command wrappers when `catch_unwind` is applicable; the audio
/// callbacks added in FASE 2 rely on it to survive a panicking plugin without
/// poisoning the IPC layer.
pub fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload.downcast_ref::<&str>().map_or_else(
        || {
            payload
                .downcast_ref::<String>()
                .map_or_else(|| "unknown panic payload".to_string(), Clone::clone)
        },
        |message| (*message).to_string(),
    )
}

// Re-export the structured error for integration tests and future crates.
pub use error::PrismError as Error;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panic_message_extracts_str_payloads() {
        let payload = std::panic::catch_unwind(|| panic!("boom")).unwrap_err();
        assert_eq!(panic_message(payload.as_ref()), "boom");
    }

    #[test]
    fn panic_message_handles_unknown_payloads() {
        let payload: Box<dyn std::any::Any + Send> = Box::new(42_u32);
        assert_eq!(panic_message(payload.as_ref()), "unknown panic payload");
    }

    #[test]
    fn error_is_publicly_reexported() {
        let err: Error = error::PrismError::invalid("x");
        assert_eq!(err.code(), "invalid_input");
    }
}
