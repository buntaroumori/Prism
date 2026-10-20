//! Spectrum visualiser bridge.
//!
//! The audio supervisor captures post-DSP blocks ([`crate::audio::analysis`]);
//! this module turns them into display frames and pushes them to the webview on
//! `visualiser://frame`. The React canvas only ever draws: no Web Audio graph, no
//! transform on the main thread, no decoding of the file a second time.
//!
//! Capture is opt-in. When no visualiser is mounted `set_enabled(false)` stops
//! both the sample copy in the audio thread and the emitter, so an idle player
//! pays nothing for a feature it is not showing.

use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Emitter, State};

use crate::audio::analysis::{Analyzer, BandSmoother, SpectrumFrame};
use crate::error::PrismError;
use crate::state::AppState;

/// Event carrying one [`SpectrumFrame`].
pub const VISUALISER_FRAME_EVENT: &str = "visualiser://frame";

/// Display refresh for the emitter.
///
/// 60 Hz is the most a webview will usefully accept over IPC. The canvas draws
/// on `requestAnimationFrame`, interpolating between frames, so the on-screen
/// motion runs at the display's rate (120 Hz included) while the IPC budget
/// stays at 60 events/s of ~450 bytes.
pub const VISUALISER_FPS: u64 = 60;

/// While silent, frames repeat at this lower rate: the bars still fall, but a
/// stopped transport does not cost a 60 Hz timer.
const SILENT_FPS: u64 = 20;

/// Enable or disable the spectrum tap.
///
/// Returns the state actually applied. The frontend calls this on mount and
/// unmount of the visualiser panel.
#[tauri::command]
pub async fn visualiser_set_enabled(
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<bool, PrismError> {
    state.engine().analyzer().set_enabled(enabled);
    tracing::debug!(enabled, "spectrum tap toggled");
    Ok(enabled)
}

/// One frame on demand, for clients that prefer polling to events.
///
/// Returns `None` until a full analysis window exists, which is the honest
/// answer while the transport is stopped.
#[tauri::command]
pub async fn visualiser_frame(
    state: State<'_, AppState>,
) -> Result<Option<SpectrumFrame>, PrismError> {
    let analyzer = state.engine().analyzer();
    let mut smoother = BandSmoother::default();
    Ok(analyzer.frame(&mut smoother))
}

/// Start the frame emitter. Called once from `setup`.
///
/// The task owns its smoother, so frames share one attack/release state for the
/// life of the process; a second subscriber would not perturb the first.
pub fn spawn_emitter(handle: AppHandle, analyzer: Arc<Analyzer>) {
    tauri::async_runtime::spawn(async move {
        let mut smoother = BandSmoother::default();
        let mut interval = tokio::time::interval(Duration::from_millis(1_000 / VISUALISER_FPS));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut silent_tick: u64 = 0;
        let silent_every = VISUALISER_FPS.div_ceil(SILENT_FPS).max(1);
        let mut was_enabled = false;

        loop {
            interval.tick().await;
            if !analyzer.is_enabled() {
                // Reset the shape so a resumed panel starts from silence rather
                // than from a stale snapshot of the previous session.
                if was_enabled {
                    let _ = smoother.decay();
                    was_enabled = false;
                }
                continue;
            }
            was_enabled = true;

            let frame = if analyzer.has_frames() {
                analyzer.frame(&mut smoother)
            } else {
                silent_tick = silent_tick.wrapping_add(1);
                (silent_tick % silent_every == 0).then(|| smoother.decay())
            };

            if let Some(frame) = frame
                && let Err(error) = handle.emit(VISUALISER_FRAME_EVENT, &frame)
            {
                tracing::trace!(%error, "cannot forward spectrum frame");
            }
        }
    });
}
