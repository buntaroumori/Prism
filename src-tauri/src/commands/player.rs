//! Transport, device and DSP commands.
//!
//! Every command is a thin, non-blocking shell around [`crate::audio::engine::Engine`]:
//! the supervisor thread owns the device, so the IPC handler never waits on
//! audio I/O and the UI stays at 120 FPS even while a track is being opened.

use serde::Serialize;
use tauri::State;

use crate::audio::decoder::TrackRequest;
use crate::audio::dsp::DspSettings;
use crate::audio::engine::{EngineStatus, OutputDevice, OutputMode, PlaybackState};
use crate::audio::format::AudioFormat;
use crate::error::PrismError;
use crate::state::AppState;

/// Tauri event the frontend subscribes to for engine pushes.
pub const PLAYER_EVENT_CHANNEL: &str = "player://event";

/// Position update channel.
///
/// Deliberately separate from `player://event`: the playhead ticks at 8 Hz and
/// carries two numbers, while transport events are rare and large. Mixing them
/// would make every tick a full re-render trigger for whatever listens to the
/// heavy channel.
pub const PLAYER_TICK_CHANNEL: &str = "player://tick";

/// How often the playhead is pushed while a track is playing or paused.
pub const PLAYER_TICK_INTERVAL_MS: u64 = 125;

/// Minimal position projection for the player bar and the seek slider.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerTick {
    /// Position inside the current track, in seconds of the source rate.
    pub position_ms: u64,
    /// Track duration in milliseconds, when the container declares one.
    pub duration_ms: Option<u64>,
    /// True while the device clock is running.
    pub playing: bool,
}

impl PlayerTick {
    /// Project an engine snapshot onto the tick payload.
    #[must_use]
    pub fn from_status(status: &EngineStatus) -> Self {
        let rate = status
            .source_format
            .map_or(0, |format| u64::from(format.sample_rate));
        let to_ms = |frames: u64| {
            if rate == 0 {
                0
            } else {
                frames.saturating_mul(1_000) / rate
            }
        };
        Self {
            position_ms: to_ms(status.position_frames),
            duration_ms: status.duration_frames.map(to_ms),
            playing: matches!(
                status.state,
                PlaybackState::Playing | PlaybackState::Buffering
            ),
        }
    }
}

/// Current transport snapshot (state, track, position, formats, counters).
#[tauri::command]
pub fn player_status(state: State<'_, AppState>) -> EngineStatus {
    state.engine().status()
}

/// Replace the queue and start playing at `index`.
#[tauri::command]
pub fn player_play(
    state: State<'_, AppState>,
    tracks: Vec<TrackRequest>,
    index: usize,
) -> Result<(), PrismError> {
    if tracks.is_empty() {
        return Err(PrismError::InvalidInput("the queue is empty".into()));
    }
    if index >= tracks.len() {
        return Err(PrismError::InvalidInput(format!(
            "queue index {index} is outside the {} track queue",
            tracks.len()
        )));
    }
    state.engine().play(tracks, index)?;
    Ok(())
}

#[tauri::command]
pub fn player_pause(state: State<'_, AppState>) -> Result<(), PrismError> {
    state.engine().pause()?;
    Ok(())
}

#[tauri::command]
pub fn player_resume(state: State<'_, AppState>) -> Result<(), PrismError> {
    state.engine().resume()?;
    Ok(())
}

#[tauri::command]
pub fn player_stop(state: State<'_, AppState>) -> Result<(), PrismError> {
    state.engine().stop()?;
    Ok(())
}

/// Seek inside the current track, in frames of the source rate.
#[tauri::command]
pub fn player_seek(state: State<'_, AppState>, frame: u64) -> Result<(), PrismError> {
    state.engine().seek(frame)?;
    Ok(())
}

/// Software volume in dB, clamped to `-60..=0`.
///
/// At 0 dB, unmuted, with no DSP stage active and an exact device format the
/// engine reports `bitPerfect: true` and the DAC receives untouched samples.
#[tauri::command]
pub fn player_set_volume(state: State<'_, AppState>, volume_db: f32) -> Result<(), PrismError> {
    if !volume_db.is_finite() {
        return Err(PrismError::InvalidInput(
            "volume must be a finite number".into(),
        ));
    }
    state.engine().set_volume_db(volume_db);
    Ok(())
}

#[tauri::command]
pub fn player_set_muted(state: State<'_, AppState>, muted: bool) -> Result<(), PrismError> {
    state.engine().set_muted(muted);
    Ok(())
}

/// Apply EQ / ReplayGain / crossfeed settings.
///
/// `restart = false` rebuilds the chain in place (no audible gap); `true`
/// restarts the current track at the same position.
#[tauri::command]
pub fn player_set_dsp(
    state: State<'_, AppState>,
    settings: DspSettings,
    restart: bool,
) -> Result<(), PrismError> {
    state.engine().set_dsp(settings, restart)?;
    Ok(())
}

/// Switch output mode, device or buffer size.
///
/// Playback continues at the same position on the new endpoint.
#[tauri::command]
pub fn player_set_output(
    state: State<'_, AppState>,
    mode: OutputMode,
    device: Option<String>,
    buffer_frames: u32,
) -> Result<(), PrismError> {
    if buffer_frames != 0 && !(32..=8_192).contains(&buffer_frames) {
        return Err(PrismError::InvalidInput(format!(
            "buffer size {buffer_frames} is outside 32..=8192 frames"
        )));
    }
    state.engine().set_output(mode, device, buffer_frames)?;
    Ok(())
}

/// Toggle RAM Play (whole-track buffering for network or optical media).
#[tauri::command]
pub fn player_set_ram_play(state: State<'_, AppState>, enabled: bool) -> Result<(), PrismError> {
    state.engine().set_ram_play(enabled)?;
    Ok(())
}

/// Endpoints reachable through `mode`, best first.
#[tauri::command]
pub fn player_devices(
    state: State<'_, AppState>,
    mode: OutputMode,
) -> Result<Vec<OutputDevice>, PrismError> {
    Ok(state.engine().devices(mode)?)
}

/// The endpoint's live configuration, queried from the driver.
///
/// Settings shows this next to the device list so the number on screen is the
/// one the OS panel reports, not the container the engine happens to write.
/// `None` means the backend cannot answer without opening a stream (exclusive
/// endpoints), in which case the negotiated format in `player_status` is the
/// device truth.
#[tauri::command]
pub fn player_device_format(
    state: State<'_, AppState>,
    mode: OutputMode,
    device: Option<String>,
) -> Result<Option<AudioFormat>, PrismError> {
    let kinds = crate::audio::engine::backend::preferred_kinds(mode);
    for kind in kinds {
        if let Some(format) =
            crate::audio::engine::backend::live_device_format(kind, device.as_deref())
        {
            return Ok(Some(format));
        }
    }
    // Exclusive backends: the open stream already reports the device format.
    Ok(state.engine().status().device_format)
}

/// Name of the event channel the engine pushes to.
#[tauri::command]
#[must_use]
pub fn player_event_channel() -> &'static str {
    PLAYER_EVENT_CHANNEL
}

/// Name of the playhead tick channel.
#[tauri::command]
#[must_use]
pub fn player_tick_channel() -> &'static str {
    PLAYER_TICK_CHANNEL
}
