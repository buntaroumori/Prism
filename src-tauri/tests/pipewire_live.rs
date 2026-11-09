//! Opt-in test against an isolated PipeWire server with a disposable null sink.
#![cfg(target_os = "linux")]

use prism_lib::audio::decoder::TrackRequest;
use prism_lib::audio::engine::{Engine, EngineConfig, OutputMode, PlaybackState};
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

fn wav(path: &Path, rate: u32, hz: f32, bits: u16) {
    let frames = rate * 2;
    let block_align = 2 * bits / 8;
    let bytes = frames * u32::from(block_align);
    let mut out = Vec::with_capacity(bytes as usize + 44);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + bytes).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * u32::from(block_align)).to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&bits.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&bytes.to_le_bytes());
    for i in 0..frames {
        let value = ((std::f32::consts::TAU * hz * i as f32 / rate as f32).sin() * 4000.0) as i16;
        for _ in 0..2 {
            if bits == 24 {
                out.extend_from_slice(&(i32::from(value) << 8).to_le_bytes()[..3]);
            } else {
                out.extend_from_slice(&value.to_le_bytes());
            }
        }
    }
    std::fs::write(path, out).expect("write fixture");
}

#[test]
#[ignore = "requires an isolated PipeWire session with a null sink"]
fn mixed_rate_queue_reaches_all_tracks_through_real_pipewire() {
    let dir = tempfile::tempdir().expect("fixture directory");
    let tracks = [
        (44_100, 440.0, 16),
        (96_000, 550.0, 24),
        (192_000, 660.0, 24),
        (44_100, 770.0, 16),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (rate, hz, bits))| {
        let path = dir.path().join(format!("{index}.wav"));
        wav(&path, rate, hz, bits);
        TrackRequest {
            track_id: index.to_string(),
            title: index.to_string(),
            artist: "test".into(),
            album: "test".into(),
            path,
            start_ms: None,
            end_ms: None,
            replaygain_track_gain_db: None,
            replaygain_track_peak: None,
            replaygain_album_gain_db: None,
            replaygain_album_peak: None,
        }
    })
    .collect();
    let targets = prism_lib::audio::engine::backend::enumerate_devices(OutputMode::Shared)
        .expect("enumerate PipeWire sinks");
    if let Ok(target) = std::env::var("PRISM_PIPEWIRE_TEST_TARGET") {
        assert!(
            targets.iter().any(|device| device.id == target),
            "target sink missing: {targets:?}"
        );
        assert!(
            targets
                .iter()
                .all(|device| !device.id.contains("TestMicrophone")),
            "capture source leaked into output list"
        );
    }
    let config = EngineConfig {
        mode: OutputMode::Shared,
        device_id: Some(
            std::env::var("PRISM_PIPEWIRE_TEST_TARGET")
                .unwrap_or_else(|_| "pipewire:default".into()),
        ),
        ..EngineConfig::default()
    };
    let engine = Engine::new(config).expect("engine");
    let mut events = engine.subscribe();
    engine.play(tracks, 0).expect("play command");
    let mut seen = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        while let Ok(event) = events.try_recv() {
            if let prism_lib::audio::engine::EngineEvent::TrackChanged { index, .. } = event {
                if seen.last() != Some(&index) {
                    seen.push(index);
                }
            }
        }
        if seen.contains(&3) && engine.status().state == PlaybackState::Stopped {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        seen,
        vec![0, 1, 2, 3],
        "mixed-rate queue failed: {:?}",
        engine.status()
    );
    assert_eq!(engine.status().state, PlaybackState::Stopped);
}
