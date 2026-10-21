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

pub mod artwork;
pub mod audio;
pub mod commands;
pub mod db;
pub mod discord;
pub mod error;
pub mod logging;
pub mod lyrics;
pub mod metadata;
pub mod palette;
pub mod plugins;
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

            // --- Startup maintenance -----------------------------------------
            // Two jobs that must happen before the UI asks anything:
            //   1. A `scan_runs` row left in `running` by a crash or a forced
            //      quit is a lie the shell would keep polling forever. Stamp it
            //      failed so the ledger is honest.
            //   2. Every album without a cover gets one, in the background, so
            //      the grid paints artwork on first open instead of waiting for
            //      the listener to visit each record.
            if let Some(state) = app.try_state::<AppState>() {
                let scanner = state.scanner().clone();
                let pool = state.pool().clone();
                let artwork = std::sync::Arc::clone(state.artwork());
                tauri::async_runtime::spawn(async move {
                    if let Err(error) = scanner.recover_stale_runs().await {
                        tracing::warn!(%error, "cannot recover interrupted scans");
                    }
                    // Keyset sweep: every album is visited once, even when a
                    // page contains only records with no artwork anywhere.
                    // Resolved covers are attached as the sweep advances, so the
                    // grid paints them without the listener opening anything.
                    let mut cursor: Option<String> = None;
                    loop {
                        match commands::library::backfill_covers(
                            &pool,
                            &artwork,
                            32,
                            cursor.as_deref(),
                        )
                        .await
                        {
                            Ok(page) => {
                                if page.resolved > 0 {
                                    tracing::debug!(
                                        resolved = page.resolved,
                                        examined = page.examined,
                                        "cover sweep progress"
                                    );
                                }
                                match page.next {
                                    Some(next) => cursor = Some(next),
                                    None => break,
                                }
                            }
                            Err(error) => {
                                tracing::warn!(%error, "cover sweep stopped");
                                break;
                            }
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(120)).await;
                    }
                });
            }

            // --- Rich presence bridge -----------------------------------------
            // Presence follows the engine, not the React tree: minimising to
            // the tray keeps Discord accurate.
            if let Some(state) = app.try_state::<AppState>() {
                let mut events = state.engine().subscribe();
                let engine = std::sync::Arc::clone(state.engine());
                let discord = std::sync::Arc::clone(state.discord());
                tauri::async_runtime::spawn(async move {
                    let mut last: Option<(String, bool)> = None;
                    loop {
                        match events.recv().await {
                            Ok(audio::engine::EngineEvent::TrackChanged { .. })
                            | Ok(audio::engine::EngineEvent::StateChanged { .. }) => {
                                let status = engine.status();
                                let fingerprint = status.track.as_ref().map(|track| {
                                    (
                                        track.track_id.clone(),
                                        status.state == audio::engine::PlaybackState::Playing,
                                    )
                                });
                                if fingerprint != last {
                                    last = fingerprint;
                                    discord.sync(&status).await;
                                }
                            }
                            Ok(_) => {}
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                                tracing::warn!(skipped, "presence subscriber lagged");
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        }
                    }
                });
            }

            // --- Audio engine event bridge ------------------------------------
            // The supervisor pushes transport events through a tokio
            // broadcast channel; this task forwards them to the webview as
            // `player://event` so React never polls the engine.
            if let Some(state) = app.try_state::<AppState>() {
                let mut events = state.engine().subscribe();
                let handle = handle.clone();
                tauri::async_runtime::spawn(async move {
                    use tauri::Emitter;
                    loop {
                        match events.recv().await {
                            Ok(event) => {
                                if let Err(error) =
                                    handle.emit(commands::player::PLAYER_EVENT_CHANNEL, &event)
                                {
                                    tracing::warn!(error = %error, "cannot forward engine event");
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                                tracing::warn!(skipped, "engine event subscriber lagged");
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        }
                    }
                });
            }

            // --- Playhead ticker ----------------------------------------------
            // The supervisor owns the clock; the UI only needs a coarse
            // projection. Ticking here (not in React) means a background window
            // costs nothing: `emit` to a hidden webview is a no-op and the
            // payload is skipped entirely while the transport is stopped.
            if let Some(state) = app.try_state::<AppState>() {
                let engine = std::sync::Arc::clone(state.engine());
                let handle = handle.clone();
                tauri::async_runtime::spawn(async move {
                    use tauri::Emitter;
                    let mut interval = tokio::time::interval(std::time::Duration::from_millis(
                        commands::player::PLAYER_TICK_INTERVAL_MS,
                    ));
                    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                    loop {
                        interval.tick().await;
                        let status = engine.status();
                        if matches!(
                            status.state,
                            audio::engine::PlaybackState::Playing
                                | audio::engine::PlaybackState::Paused
                        ) {
                            let tick = commands::player::PlayerTick::from_status(&status);
                            if let Err(error) =
                                handle.emit(commands::player::PLAYER_TICK_CHANNEL, &tick)
                            {
                                tracing::debug!(%error, "cannot forward playhead tick");
                            }
                        }
                    }
                });
            }

            // --- Plugin bridge ----------------------------------------------
            // Two directions, one task each: playback events reach the plugins,
            // and plugin log lines reach the UI. Neither is allowed to block
            // the transport: dispatch is fuelled per call and failures disable
            // the offending plugin for the session.
            if let Some(state) = app.try_state::<AppState>() {
                let plugins = std::sync::Arc::clone(state.plugins());
                let mut events = state.engine().subscribe();
                tauri::async_runtime::spawn(async move {
                    loop {
                        match events.recv().await {
                            Ok(event) => {
                                let kind = match &event {
                                    audio::engine::EngineEvent::StateChanged { .. } => {
                                        "state_changed"
                                    }
                                    audio::engine::EngineEvent::TrackChanged { .. } => {
                                        "track_changed"
                                    }
                                    audio::engine::EngineEvent::DeviceChanged { .. } => {
                                        "device_changed"
                                    }
                                    audio::engine::EngineEvent::OutputError { .. } => {
                                        "output_error"
                                    }
                                    audio::engine::EngineEvent::Underrun { .. } => "underrun",
                                    audio::engine::EngineEvent::TrackSkipped { .. } => {
                                        "track_skipped"
                                    }
                                };
                                let payload =
                                    serde_json::to_value(&event).unwrap_or(serde_json::Value::Null);
                                plugins
                                    .dispatch(&plugins::PluginEvent::new(kind, payload))
                                    .await;
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                                tracing::warn!(skipped, "plugin subscriber lagged");
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        }
                    }
                });
            }
            let plugin_messages = app
                .try_state::<AppState>()
                .and_then(|state| state.take_plugin_messages());
            if let Some(mut messages) = plugin_messages {
                let handle = handle.clone();
                tauri::async_runtime::spawn(async move {
                    use tauri::Emitter;
                    while let Some(message) = messages.recv().await {
                        if let Err(error) = handle.emit(plugins::PLUGIN_MESSAGE_EVENT, &message) {
                            tracing::trace!(%error, "cannot forward plugin message");
                        }
                    }
                });
            }

            // --- Spectrum emitter -------------------------------------------
            // Frames are produced here, not in React: the webview receives a
            // 31-band snapshot and draws it. Nothing is emitted while the
            // visualiser panel is unmounted.
            if let Some(state) = app.try_state::<AppState>() {
                commands::visualiser::spawn_emitter(
                    handle.clone(),
                    std::sync::Arc::clone(state.engine().analyzer()),
                );
            }

            // The window is created hidden (`visible = false`) to avoid the
            // white flash before React paints the OLED background.
            if let Some(window) = app.get_webview_window("main") {
                window.show()?;
                window.set_focus()?;
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::library::get_library_stats,
            commands::library::library_scan_start,
            commands::library::library_scan_cancel,
            commands::library::library_scan_status,
            commands::library::library_scan_runs,
            commands::library::library_scan_errors,
            commands::library::library_roots,
            commands::library::library_add_root,
            commands::library::library_remove_root,
            commands::library::library_set_root_enabled,
            commands::library::library_search,
            commands::library::library_albums,
            commands::library::library_artists,
            commands::library::library_facets,
            commands::library::library_global_search,
            commands::library::library_home,
            commands::library::library_album_detail,
            commands::library::library_artist_detail,
            commands::library::library_note_play,
            commands::library::library_index_covers,
            commands::library::library_album_cover,
            commands::library::library_track_cover,
            commands::library::library_cover_palette,
            commands::discord::discord_status,
            commands::discord::discord_configure,
            commands::discord::discord_clear,
            commands::system::get_app_info,
            commands::system::get_runtime_health,
            commands::system::open_log_folder,
            commands::system::open_data_folder,
            commands::player::player_status,
            commands::player::player_play,
            commands::player::player_pause,
            commands::player::player_resume,
            commands::player::player_stop,
            commands::player::player_seek,
            commands::player::player_set_volume,
            commands::player::player_set_muted,
            commands::player::player_set_dsp,
            commands::player::player_set_output,
            commands::player::player_set_ram_play,
            commands::player::player_devices,
            commands::player::player_device_format,
            commands::player::player_event_channel,
            commands::player::player_tick_channel,
            commands::lyrics::lyrics_get,
            commands::lyrics::lyrics_clear,
            commands::visualiser::visualiser_set_enabled,
            commands::visualiser::visualiser_frame,
            commands::plugins::plugins_list,
            commands::plugins::plugins_reload,
            commands::plugins::plugins_set_enabled,
            commands::plugins::plugins_set_config,
            commands::plugins::plugins_open_folder,
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
