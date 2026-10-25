//! Windows System Media Transport Controls (SMTC).
//!
//! The controls are bound to Prism's top-level HWND, not to a browser media
//! session. Native button callbacks run off the UI thread, so they only send
//! thread-safe engine commands or emit a queue command to the renderer.

use std::sync::Arc;
use std::time::{Duration, Instant};

use sqlx::SqlitePool;
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};
use windows::Foundation::{EventRegistrationToken, TimeSpan, TypedEventHandler, Uri};
use windows::Media::{
    MediaPlaybackStatus, MediaPlaybackType, PlaybackPositionChangeRequestedEventArgs,
    SystemMediaTransportControls, SystemMediaTransportControlsButton,
    SystemMediaTransportControlsButtonPressedEventArgs,
    SystemMediaTransportControlsTimelineProperties,
};
use windows::Storage::Streams::RandomAccessStreamReference;
use windows::Win32::Foundation::{HWND, RPC_E_CHANGED_MODE};
use windows::Win32::System::WinRT::{
    ISystemMediaTransportControlsInterop, RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize,
};
use windows::core::{HSTRING, factory};

use crate::audio::engine::{Engine, NowPlaying, PlaybackState};
use crate::commands::player::PlayerTick;
use crate::error::PrismError;
use crate::media::MEDIA_COMMAND_CHANNEL;

/// Process-lifetime owner for the native controls and their event registrations.
#[derive(Debug)]
pub struct MediaControls {
    controls: SystemMediaTransportControls,
    button_token: EventRegistrationToken,
    position_token: Option<EventRegistrationToken>,
    _winrt: WinRtApartment,
}

#[derive(Debug)]
struct WinRtApartment {
    initialized_here: bool,
}

impl Drop for WinRtApartment {
    fn drop(&mut self) {
        if self.initialized_here {
            // SAFETY: balances the successful RoInitialize on this thread.
            unsafe { RoUninitialize() };
        }
    }
}

impl MediaControls {
    /// Bind SMTC to the actual desktop window and subscribe native buttons.
    pub fn initialize(
        app: &AppHandle,
        window: &WebviewWindow,
        engine: Arc<Engine>,
    ) -> Result<Self, PrismError> {
        // SAFETY: called once on the main application thread. If Tauri already
        // initialized another COM apartment, WinRT is still usable and we do
        // not attempt to uninitialize that apartment later.
        let winrt = match unsafe { RoInitialize(RO_INIT_MULTITHREADED) } {
            Ok(()) => WinRtApartment {
                initialized_here: true,
            },
            Err(error) if error.code() == RPC_E_CHANGED_MODE => WinRtApartment {
                initialized_here: false,
            },
            Err(error) => return Err(media_error("cannot initialize WinRT", error)),
        };
        let native_window = window.hwnd()?;
        let hwnd = HWND(native_window.0);
        // SAFETY: `hwnd` is the live top-level window owned by this process;
        // the interop factory only borrows it for this call.
        let controls = unsafe {
            factory::<SystemMediaTransportControls, ISystemMediaTransportControlsInterop>()
                .and_then(|interop| interop.GetForWindow::<_, SystemMediaTransportControls>(hwnd))
        }
        .map_err(|error| media_error("cannot acquire system media controls", error))?;

        controls
            .SetIsEnabled(true)
            .and_then(|()| controls.SetIsPlayEnabled(true))
            .and_then(|()| controls.SetIsPauseEnabled(true))
            .and_then(|()| controls.SetIsStopEnabled(true))
            .and_then(|()| controls.SetIsPreviousEnabled(true))
            .and_then(|()| controls.SetIsNextEnabled(true))
            .map_err(|error| media_error("cannot enable system media buttons", error))?;

        let button_engine = Arc::clone(&engine);
        let button_app = app.clone();
        let button_handler = TypedEventHandler::<
            SystemMediaTransportControls,
            SystemMediaTransportControlsButtonPressedEventArgs,
        >::new(move |_sender, arguments| {
            let Some(arguments) = arguments else {
                return Ok(());
            };
            match arguments.Button()? {
                SystemMediaTransportControlsButton::Play => {
                    log_engine_result("play", button_engine.resume());
                }
                SystemMediaTransportControlsButton::Pause => {
                    log_engine_result("pause", button_engine.pause());
                }
                SystemMediaTransportControlsButton::Stop => {
                    log_engine_result("stop", button_engine.stop());
                }
                SystemMediaTransportControlsButton::Next => {
                    if let Err(error) = button_app.emit(MEDIA_COMMAND_CHANNEL, "next") {
                        tracing::warn!(%error, "cannot forward Windows next command");
                    }
                }
                SystemMediaTransportControlsButton::Previous => {
                    if let Err(error) = button_app.emit(MEDIA_COMMAND_CHANNEL, "previous") {
                        tracing::warn!(%error, "cannot forward Windows previous command");
                    }
                }
                _ => {}
            }
            Ok(())
        });
        let button_token = controls
            .ButtonPressed(&button_handler)
            .map_err(|error| media_error("cannot subscribe to system media buttons", error))?;

        let position_engine = engine;
        let position_handler = TypedEventHandler::<
            SystemMediaTransportControls,
            PlaybackPositionChangeRequestedEventArgs,
        >::new(move |_sender, arguments| {
            let Some(arguments) = arguments else {
                return Ok(());
            };
            let ticks = arguments.RequestedPlaybackPosition()?.Duration.max(0) as u64;
            log_engine_result("seek", position_engine.seek_ms(ticks / 10_000));
            Ok(())
        });
        let position_token = match controls.PlaybackPositionChangeRequested(&position_handler) {
            Ok(token) => Some(token),
            Err(error) => {
                // Position requests were added after the base SMTC contract.
                // Transport and metadata remain useful on an older endpoint.
                tracing::warn!(%error, "Windows seek integration is unavailable");
                None
            }
        };

        Ok(Self {
            controls,
            button_token,
            position_token,
            _winrt: winrt,
        })
    }

    /// Mirror the engine into Windows without depending on a visible WebView.
    pub fn spawn_sync(&self, engine: Arc<Engine>, pool: SqlitePool) {
        let controls = self.controls.clone();
        tauri::async_runtime::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(125));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            let mut last_track: Option<String> = None;
            let mut last_state: Option<PlaybackState> = None;
            let mut last_position_ms = 0_u64;
            let mut last_timeline = Instant::now() - Duration::from_secs(2);

            loop {
                interval.tick().await;
                let status = engine.status();
                let tick = PlayerTick::from_status(&status);
                let track_id = status.track.as_ref().map(|track| track.track_id.clone());
                let track_changed = track_id != last_track;

                if track_changed {
                    let cover = match status.track.as_ref() {
                        Some(track) => cached_cover(&pool, &track.track_id).await,
                        None => None,
                    };
                    if let Err(error) =
                        update_metadata(&controls, status.track.as_ref(), cover.as_deref())
                    {
                        tracing::warn!(%error, "cannot update Windows music metadata");
                    }
                    last_track = track_id;
                    last_timeline = Instant::now() - Duration::from_secs(2);
                }

                if last_state != Some(status.state) {
                    if let Err(error) = controls.SetPlaybackStatus(playback_status(status.state)) {
                        tracing::warn!(%error, "cannot update Windows playback state");
                    }
                    last_state = Some(status.state);
                }

                let jumped = tick.position_ms.abs_diff(last_position_ms) > 1_500;
                let active = status.track.is_some() && status.state != PlaybackState::Stopped;
                if active
                    && (track_changed
                        || jumped
                        || last_timeline.elapsed() >= Duration::from_secs(1))
                {
                    if let Err(error) = update_timeline(&controls, &tick) {
                        tracing::debug!(%error, "cannot update Windows playback timeline");
                    }
                    last_timeline = Instant::now();
                }
                last_position_ms = tick.position_ms;
            }
        });
    }
}

impl Drop for MediaControls {
    fn drop(&mut self) {
        let _ = self.controls.RemoveButtonPressed(self.button_token);
        if let Some(token) = self.position_token {
            let _ = self.controls.RemovePlaybackPositionChangeRequested(token);
        }
        let _ = self.controls.SetIsEnabled(false);
    }
}

fn media_error(context: &str, error: windows::core::Error) -> PrismError {
    PrismError::Internal(format!("{context}: {error}"))
}

fn log_engine_result(operation: &str, result: Result<(), crate::audio::AudioError>) {
    if let Err(error) = result {
        tracing::warn!(%error, operation, "Windows media command was rejected");
    }
}

const fn playback_status(state: PlaybackState) -> MediaPlaybackStatus {
    match state {
        PlaybackState::Stopped => MediaPlaybackStatus::Stopped,
        PlaybackState::Buffering => MediaPlaybackStatus::Changing,
        PlaybackState::Playing => MediaPlaybackStatus::Playing,
        PlaybackState::Paused => MediaPlaybackStatus::Paused,
    }
}

fn update_metadata(
    controls: &SystemMediaTransportControls,
    track: Option<&NowPlaying>,
    cover_path: Option<&str>,
) -> windows::core::Result<()> {
    let updater = controls.DisplayUpdater()?;
    updater.ClearAll()?;
    let Some(track) = track else {
        return updater.Update();
    };
    updater.SetType(MediaPlaybackType::Music)?;
    updater.SetAppMediaId(&HSTRING::from(track.track_id.as_str()))?;
    let music = updater.MusicProperties()?;
    music.SetTitle(&HSTRING::from(track.title.as_str()))?;
    music.SetArtist(&HSTRING::from(track.artist.as_str()))?;
    music.SetAlbumArtist(&HSTRING::from(track.artist.as_str()))?;
    music.SetAlbumTitle(&HSTRING::from(track.album.as_str()))?;

    if let Some(path) = cover_path.filter(|path| std::path::Path::new(path).is_file())
        && let Some(uri) = url::Url::from_file_path(path)
            .ok()
            .and_then(|url| Uri::CreateUri(&HSTRING::from(url.as_str())).ok())
        && let Ok(reference) = RandomAccessStreamReference::CreateFromUri(&uri)
        && let Err(error) = updater.SetThumbnail(&reference)
    {
        tracing::debug!(%error, "cannot attach Windows music artwork");
    }
    updater.Update()
}

fn update_timeline(
    controls: &SystemMediaTransportControls,
    tick: &PlayerTick,
) -> windows::core::Result<()> {
    let Some(duration_ms) = tick.duration_ms.filter(|duration| *duration > 0) else {
        return Ok(());
    };
    let timeline = SystemMediaTransportControlsTimelineProperties::new()?;
    let start = time_span(0);
    let end = time_span(duration_ms);
    timeline.SetStartTime(start)?;
    timeline.SetMinSeekTime(start)?;
    timeline.SetEndTime(end)?;
    timeline.SetMaxSeekTime(end)?;
    timeline.SetPosition(time_span(tick.position_ms.min(duration_ms)))?;
    controls.UpdateTimelineProperties(&timeline)
}

const fn time_span(milliseconds: u64) -> TimeSpan {
    let ticks = milliseconds.saturating_mul(10_000);
    if ticks > i64::MAX as u64 {
        TimeSpan { Duration: i64::MAX }
    } else {
        TimeSpan {
            Duration: ticks as i64,
        }
    }
}

async fn cached_cover(pool: &SqlitePool, track_id: &str) -> Option<String> {
    sqlx::query_scalar::<_, Option<String>>(
        "SELECT COALESCE(c.thumbnail_path, c.local_path) \
         FROM tracks t \
         LEFT JOIN albums a ON a.id = t.album_id \
         LEFT JOIN cover_cache c ON c.id = a.cover_id \
         WHERE t.id = ?1",
    )
    .bind(track_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| {
        tracing::debug!(%error, "cannot read Windows artwork path");
        error
    })
    .ok()
    .flatten()
    .flatten()
}

/// Retrieve the managed controls and start their mirror exactly once.
pub fn spawn(app: &AppHandle) {
    let Some(state) = app.try_state::<crate::state::AppState>() else {
        return;
    };
    let engine = Arc::clone(state.engine());
    let pool = state.pool().clone();
    drop(state);
    if let Some(media) = app.try_state::<MediaControls>() {
        media.spawn_sync(engine, pool);
    }
}

/// Exercise the pure native projections during Windows CI without hardware.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_playback_states_match_the_engine() {
        assert_eq!(
            playback_status(PlaybackState::Playing),
            MediaPlaybackStatus::Playing
        );
        assert_eq!(
            playback_status(PlaybackState::Paused),
            MediaPlaybackStatus::Paused
        );
        assert_eq!(
            playback_status(PlaybackState::Buffering),
            MediaPlaybackStatus::Changing
        );
        assert_eq!(
            playback_status(PlaybackState::Stopped),
            MediaPlaybackStatus::Stopped
        );
    }

    #[test]
    fn native_time_spans_are_bounded_hundred_nanosecond_ticks() {
        assert_eq!(time_span(1_234).Duration, 12_340_000);
        assert_eq!(time_span(u64::MAX).Duration, i64::MAX);
    }

    #[test]
    fn an_empty_status_has_no_playhead_panic() {
        let tick = PlayerTick::from_status(&crate::audio::engine::EngineStatus::default());
        assert_eq!(tick.position_ms, 0);
        assert_eq!(tick.duration_ms, None);
    }
}
