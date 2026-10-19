//! Headless smoke tests for the playback engine.
//!
//! These do not need a sound card: they exercise the supervisor thread, the
//! status snapshot and the device enumeration fallbacks, which is exactly the
//! code that used to be able to panic on a machine without audio hardware
//! (CI containers, remote sessions, freshly installed Linux boxes).

use std::time::{Duration, Instant};

use prism_lib::audio::engine::{Engine, EngineConfig, OutputMode, PlaybackState};

fn engine() -> std::sync::Arc<Engine> {
    Engine::new(EngineConfig::default()).expect("the supervisor thread must start")
}

#[test]
fn engine_reports_a_stopped_transport_before_the_first_track() {
    let engine = engine();
    let status = engine.status();
    assert_eq!(status.state, PlaybackState::Stopped);
    assert!(status.track.is_none());
    assert_eq!(status.position_frames, 0);
    assert_eq!(status.queue_len, 0);
    assert!(
        !status.bit_perfect,
        "nothing is playing, so nothing is bit-perfect"
    );
    engine.shutdown();
}

#[test]
fn device_enumeration_survives_a_machine_without_audio_hardware() {
    let engine = engine();
    // CI containers have no ALSA card and no PipeWire server. Enumeration must
    // answer with either an empty-but-valid list or a typed error, never a
    // panic and never a hang.
    let started = Instant::now();
    match engine.devices(OutputMode::Auto) {
        Ok(devices) => {
            for device in devices {
                assert!(!device.id.is_empty());
                assert!(!device.name.is_empty());
            }
        }
        Err(error) => {
            assert_eq!(error.code(), "audio_no_device", "{error}");
        }
    }
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "enumeration must not block the UI thread"
    );
    engine.shutdown();
}

#[test]
fn volume_is_clamped_to_the_supported_range() {
    let engine = engine();
    engine.set_volume_db(12.0);
    assert_eq!(engine.status().volume_db, 0.0);

    engine.set_volume_db(-80.0);
    assert_eq!(engine.status().volume_db, -60.0);

    engine.set_volume_db(-12.5);
    assert_eq!(engine.status().volume_db, -12.5);
    engine.shutdown();
}

#[test]
fn muting_is_reflected_in_the_status_snapshot() {
    let engine = engine();
    engine.set_muted(true);
    assert!(engine.status().muted);
    engine.set_muted(false);
    assert!(!engine.status().muted);
    engine.shutdown();
}

#[test]
fn stopping_an_idle_engine_is_a_no_op() {
    let engine = engine();
    engine.pause().expect("pausing an idle engine must succeed");
    engine.stop().expect("stopping an idle engine must succeed");
    assert_eq!(engine.status().state, PlaybackState::Stopped);
    engine.shutdown();
}
