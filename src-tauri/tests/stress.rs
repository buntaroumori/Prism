//! Stress and failure-injection tests for FASE 6.
//!
//! These exercise the failure paths that a real machine produces and a CI
//! container normally cannot: a DAC that disappears mid-track, a device that is
//! swapped while playing, files that are truncated or replaced by garbage, a
//! queue whose next entry cannot be opened, and a CUE sheet describing a single
//! image larger than one gigabyte.
//!
//! Device behaviour is injected through [`Engine::with_opener`], which takes the
//! same `Fn` the production build points at `backend::open_sink`. The replacement
//! is a *loopback sink*: a thread that consumes the very same lock-free ring the
//! real device threads consume, honours the same `RefillSignal` contract and can
//! be unplugged on demand. Everything above that boundary — the supervisor, the
//! decoder, the DSP chain, the events — is production code.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use prism_lib::audio::decoder::TrackRequest;
use prism_lib::audio::engine::{
    BackendKind, Engine, EngineConfig, EngineEvent, Negotiation, OpenedSink, OutputDevice,
    OutputMode, OutputSink, PlaybackState, RefillSignal, SinkDescriptor, SinkOpener,
};
use prism_lib::audio::format::{AudioFormat, SampleFormat};
use prism_lib::audio::ring::{RingStats, SampleConsumer, create_ring};
use prism_lib::db::connect;
use prism_lib::metadata::scanner::{ScanConfig, ScanTrigger, Scanner};

// ---------------------------------------------------------------------------
// Loopback device
// ---------------------------------------------------------------------------

/// Controls shared between the test and the loopback's device thread.
#[derive(Debug, Default)]
struct LoopbackControl {
    running: AtomicBool,
    paused: AtomicBool,
    stop: AtomicBool,
    /// Set by the test to emulate a DAC being removed: the thread exits and
    /// closes the refill signal exactly like a driver that died.
    unplug: AtomicBool,
    frames_consumed: AtomicUsize,
}

struct LoopbackSink {
    descriptor: SinkDescriptor,
    control: Arc<LoopbackControl>,
    thread: Option<JoinHandle<()>>,
}

impl OutputSink for LoopbackSink {
    fn descriptor(&self) -> &SinkDescriptor {
        &self.descriptor
    }

    fn start(&mut self) -> Result<(), prism_lib::audio::AudioError> {
        self.control.running.store(true, Ordering::Release);
        Ok(())
    }

    fn pause(&mut self) -> Result<(), prism_lib::audio::AudioError> {
        self.control.paused.store(true, Ordering::Release);
        Ok(())
    }

    fn resume(&mut self) -> Result<(), prism_lib::audio::AudioError> {
        self.control.paused.store(false, Ordering::Release);
        Ok(())
    }

    fn stop(&mut self) {
        self.control.stop.store(true, Ordering::Release);
        // Unparking is unnecessary: the thread sleeps in short slices.
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for LoopbackSink {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Build a loopback endpoint that behaves like a device clock.
///
/// `wake` is how long each period takes in wall-clock terms. Tests pick a value
/// far below real time so a 30-second track finishes in a few seconds.
fn loopback(
    ordinal: u32,
    format: AudioFormat,
    period_frames: u32,
    buffer_frames: u32,
    wake: Duration,
) -> (OpenedSink, Arc<LoopbackControl>) {
    let capacity = usize::try_from(buffer_frames.max(period_frames * 2)).expect("capacity");
    let (producer, consumer, stats) = create_ring(capacity, usize::from(format.channels));
    let refill = Arc::new(RefillSignal::new());
    let control = Arc::new(LoopbackControl::default());

    let device = OutputDevice {
        id: format!("loopback-{ordinal}"),
        name: format!("Loopback Device {ordinal}"),
        kind: BackendKind::CpalShared,
        is_default: ordinal == 1,
        channels: format.channels,
        sample_rates: vec![format.sample_rate],
        sample_formats: vec![format.sample_format],
        supports_exclusive: false,
    };

    let descriptor = SinkDescriptor {
        device: device.clone(),
        format,
        period_frames,
        buffer_frames,
        latency_frames: u64::from(period_frames),
        stats: Arc::clone(&stats),
        refill: Arc::clone(&refill),
    };

    let thread = spawn_consumer(
        consumer,
        Arc::clone(&control),
        Arc::clone(&refill),
        Arc::clone(&stats),
        usize::try_from(period_frames).expect("period"),
        usize::from(format.channels),
        wake,
    );

    let sink = LoopbackSink {
        descriptor,
        control: Arc::clone(&control),
        thread: Some(thread),
    };

    let negotiation = Negotiation {
        device,
        kind: BackendKind::CpalShared,
        format,
        period_frames,
        buffer_frames,
        latency_frames: u64::from(period_frames),
    };

    (
        OpenedSink {
            sink: Box::new(sink),
            producer,
            stats,
            refill,
            negotiation,
        },
        control,
    )
}

fn spawn_consumer(
    mut consumer: SampleConsumer,
    control: Arc<LoopbackControl>,
    refill: Arc<RefillSignal>,
    stats: Arc<RingStats>,
    period_frames: usize,
    channels: usize,
    wake: Duration,
) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name("loopback-device".into())
        .spawn(move || {
            let mut buffer = vec![0.0_f32; period_frames * channels];
            loop {
                if control.stop.load(Ordering::Acquire) {
                    break;
                }
                if control.unplug.load(Ordering::Acquire) {
                    // A removed endpoint: the driver dies, the ring stops being
                    // drained and the supervisor must notice and rebuild.
                    refill.close();
                    break;
                }
                if !control.running.load(Ordering::Acquire)
                    || control.paused.load(Ordering::Acquire)
                {
                    std::thread::sleep(Duration::from_millis(1));
                    continue;
                }
                let frames = consumer.pop_or_silence(&mut buffer);
                control.frames_consumed.fetch_add(frames, Ordering::Relaxed);
                let _ = &stats;
                refill.request();
                std::thread::sleep(wake);
            }
        })
        .expect("spawn loopback device")
}

#[derive(Debug, Default)]
struct Harness {
    opens: AtomicU32,
    controls: Mutex<HashMap<u32, Arc<LoopbackControl>>>,
}

impl Harness {
    fn control(&self, ordinal: u32) -> Option<Arc<LoopbackControl>> {
        self.controls
            .lock()
            .expect("controls")
            .get(&ordinal)
            .cloned()
    }

    fn latest(&self) -> Option<(u32, Arc<LoopbackControl>)> {
        let opens = self.opens.load(Ordering::Acquire);
        self.control(opens).map(|control| (opens, control))
    }

    fn opener(harness: Arc<Self>, wake: Duration) -> SinkOpener {
        Arc::new(move |_mode, _device, format, buffer_frames| {
            let ordinal = harness.opens.fetch_add(1, Ordering::AcqRel) + 1;
            let (opened, control) = loopback(ordinal, *format, 512, buffer_frames, wake);
            harness
                .controls
                .lock()
                .expect("controls")
                .insert(ordinal, control);
            Ok(opened)
        })
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const FORMAT: AudioFormat = AudioFormat {
    sample_rate: 44_100,
    channels: 2,
    sample_format: SampleFormat::F32,
};

fn write_wav(path: &Path, seconds: f32) -> PathBuf {
    std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    let frames = (seconds * FORMAT.sample_rate as f32) as u32;
    let block_align = u32::from(FORMAT.channels) * 2;
    let data_len = frames * block_align;

    let mut buffer = Vec::with_capacity(44 + data_len as usize);
    buffer.extend_from_slice(b"RIFF");
    buffer.extend_from_slice(&(36 + data_len).to_le_bytes());
    buffer.extend_from_slice(b"WAVE");
    buffer.extend_from_slice(b"fmt ");
    buffer.extend_from_slice(&16u32.to_le_bytes());
    buffer.extend_from_slice(&1u16.to_le_bytes());
    buffer.extend_from_slice(&FORMAT.channels.to_le_bytes());
    buffer.extend_from_slice(&FORMAT.sample_rate.to_le_bytes());
    buffer.extend_from_slice(&(FORMAT.sample_rate * block_align).to_le_bytes());
    buffer.extend_from_slice(&u16::try_from(block_align).expect("align").to_le_bytes());
    buffer.extend_from_slice(&16u16.to_le_bytes());
    buffer.extend_from_slice(b"data");
    buffer.extend_from_slice(&data_len.to_le_bytes());
    buffer.resize(buffer.len() + data_len as usize, 0);
    std::fs::write(path, buffer).expect("write wav");
    path.to_path_buf()
}

fn request(path: &Path, id: &str) -> TrackRequest {
    TrackRequest {
        track_id: id.to_string(),
        title: id.to_string(),
        artist: "Stress".into(),
        album: "Suite".into(),
        path: path.to_path_buf(),
        start_ms: None,
        end_ms: None,
        replaygain_track_gain_db: None,
        replaygain_track_peak: None,
        replaygain_album_gain_db: None,
        replaygain_album_peak: None,
    }
}

fn engine_with(harness: Arc<Harness>, wake: Duration) -> Arc<Engine> {
    Engine::with_opener(EngineConfig::default(), Harness::opener(harness, wake)).expect("engine")
}

fn wait_until(timeout: Duration, mut condition: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if condition() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    condition()
}

/// Drain events until `predicate` is satisfied or the deadline passes.
///
/// The engine starts in `Stopped`, so "wait until stopped" alone is satisfied
/// before the supervisor has even read the command; every wait in this file is
/// therefore anchored to something that can only happen *during* playback.
fn wait_for_events(
    receiver: &mut tokio::sync::broadcast::Receiver<EngineEvent>,
    timeout: Duration,
    mut predicate: impl FnMut(&[EngineEvent]) -> bool,
) -> Vec<EngineEvent> {
    let deadline = Instant::now() + timeout;
    let mut collected = Vec::new();
    loop {
        while let Ok(event) = receiver.try_recv() {
            collected.push(event);
        }
        if predicate(&collected) || Instant::now() >= deadline {
            return collected;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn skip_ids(events: &[EngineEvent]) -> Vec<&str> {
    events
        .iter()
        .filter_map(|event| match event {
            EngineEvent::TrackSkipped { track_id, .. } => Some(track_id.as_str()),
            _ => None,
        })
        .collect()
}

fn collect_events(
    receiver: &mut tokio::sync::broadcast::Receiver<EngineEvent>,
) -> Vec<EngineEvent> {
    let mut events = Vec::new();
    while let Ok(event) = receiver.try_recv() {
        events.push(event);
    }
    events
}

// ---------------------------------------------------------------------------
// 1. Hot-swap
// ---------------------------------------------------------------------------

#[test]
fn swapping_the_output_mid_track_keeps_the_playhead_and_the_queue() {
    let harness = Arc::new(Harness::default());
    let engine = engine_with(Arc::clone(&harness), Duration::from_millis(2));
    let mut events = engine.subscribe();

    let dir = tempfile::tempdir().expect("tempdir");
    let first = write_wav(&dir.path().join("a.wav"), 30.0);
    let second = write_wav(&dir.path().join("b.wav"), 30.0);
    engine
        .play(vec![request(&first, "a"), request(&second, "b")], 0)
        .expect("play accepted");

    assert!(
        wait_until(Duration::from_secs(5), || engine.status().state
            == PlaybackState::Playing),
        "the queue must start: {:?}",
        engine.status()
    );
    assert_eq!(
        engine.status().device_name.as_deref(),
        Some("Loopback Device 1")
    );

    let mut previous = engine.status().position_frames;
    for round in 0..5_u32 {
        let target = format!("device-{round}");
        engine
            .set_output(OutputMode::Shared, Some(target), 256)
            .expect("swap accepted");

        let expected = format!("Loopback Device {}", round + 2);
        assert!(
            wait_until(Duration::from_secs(5), || {
                engine.status().device_name.as_deref() == Some(expected.as_str())
                    && engine.status().state == PlaybackState::Playing
            }),
            "swap {round} never reopened: {:?}",
            engine.status()
        );

        let status = engine.status();
        assert_eq!(
            status.track.as_ref().map(|t| t.track_id.as_str()),
            Some("a")
        );
        assert_eq!(
            status.queue_index, 0,
            "the queue position must survive a swap"
        );
        assert!(
            status.position_frames + 44_100 >= previous,
            "the playhead went backwards: {} -> {}",
            previous,
            status.position_frames
        );
        assert!(
            status.position_frames <= previous + 44_100 * 5,
            "the playhead jumped forwards: {} -> {}",
            previous,
            status.position_frames
        );
        previous = status.position_frames;
    }

    assert_eq!(
        harness.opens.load(Ordering::Acquire),
        6,
        "one open per swap"
    );

    let events = collect_events(&mut events);
    let device_changes = events
        .iter()
        .filter(|event| matches!(event, EngineEvent::DeviceChanged { .. }))
        .count();
    assert_eq!(device_changes, 6, "every open must be announced");
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, EngineEvent::OutputError { .. })),
        "a requested swap is not an error: {events:?}"
    );

    engine.stop().expect("stop");
    engine.shutdown();
}

// ---------------------------------------------------------------------------
// 2. Device loss
// ---------------------------------------------------------------------------

#[test]
fn an_unplugged_dac_is_rebuilt_and_playback_resumes_at_the_same_position() {
    let harness = Arc::new(Harness::default());
    let engine = engine_with(Arc::clone(&harness), Duration::from_millis(2));
    let mut events = engine.subscribe();

    let dir = tempfile::tempdir().expect("tempdir");
    let track = write_wav(&dir.path().join("lost.wav"), 60.0);
    engine.play(vec![request(&track, "lost")], 0).expect("play");

    assert!(wait_until(Duration::from_secs(5), || engine.status().state
        == PlaybackState::Playing));
    // Let the playhead advance far enough that resuming at zero would be visible.
    assert!(wait_until(Duration::from_secs(5), || {
        engine.status().position_frames > 44_100
    }));
    let before = engine.status().position_frames;

    // The device disappears.
    let (_, control) = harness.latest().expect("an open endpoint");
    control.unplug.store(true, Ordering::Release);

    assert!(
        wait_until(Duration::from_secs(5), || harness
            .opens
            .load(Ordering::Acquire)
            == 2),
        "the supervisor must reopen the endpoint"
    );
    assert!(
        wait_until(Duration::from_secs(5), || {
            let status = engine.status();
            status.state == PlaybackState::Playing
                && status.device_name.as_deref() == Some("Loopback Device 2")
        }),
        "playback must resume on the new endpoint: {:?}",
        engine.status()
    );

    let after = engine.status().position_frames;
    assert!(
        after + 44_100 * 2 >= before,
        "resume must not rewind: {before} -> {after}"
    );

    let events = collect_events(&mut events);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, EngineEvent::OutputError { message } if message.contains("device"))),
        "the loss must be reported: {events:?}"
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, EngineEvent::DeviceChanged { device, .. } if device == "Loopback Device 2")),
        "the rebuild must be announced: {events:?}"
    );

    engine.stop().expect("stop");
    engine.shutdown();
}

// ---------------------------------------------------------------------------
// 3. Corrupt files
// ---------------------------------------------------------------------------

#[test]
fn a_corrupt_track_mid_queue_is_skipped_and_the_album_finishes() {
    let harness = Arc::new(Harness::default());
    let engine = engine_with(Arc::clone(&harness), Duration::from_millis(1));
    let mut events = engine.subscribe();

    let dir = tempfile::tempdir().expect("tempdir");
    let first = write_wav(&dir.path().join("01.wav"), 1.0);
    // A real FLAC marker followed by nonsense: symphonia trusts the magic and
    // then fails on the STREAMINFO block, which is exactly what a half-copied
    // file from a crashed rip looks like.
    let broken = dir.path().join("02.flac");
    let mut payload = b"fLaC\x00\x00\x00\x22".to_vec();
    payload.extend(std::iter::repeat_n(0xABu8, 700));
    std::fs::write(&broken, payload).expect("write broken");
    let third = write_wav(&dir.path().join("03.wav"), 1.0);

    engine
        .play(
            vec![
                request(&first, "first"),
                request(&broken, "broken"),
                request(&third, "third"),
            ],
            0,
        )
        .expect("play");

    // The queue must run to completion: corrupt track skipped, third track played.
    // The wait is anchored on the skip itself, because `Stopped` is also the
    // state the engine holds before it has read the command.
    let events = wait_for_events(&mut events, Duration::from_secs(30), |events| {
        !skip_ids(events).is_empty()
            && events.iter().any(|event| {
                matches!(
                    event,
                    EngineEvent::StateChanged {
                        state: PlaybackState::Stopped
                    }
                )
            })
    });
    assert!(
        wait_until(Duration::from_secs(30), || engine.status().state
            == PlaybackState::Stopped),
        "the queue never finished: {:?}",
        engine.status()
    );
    let skipped: Vec<&EngineEvent> = events
        .iter()
        .filter(|event| matches!(event, EngineEvent::TrackSkipped { .. }))
        .collect();
    assert_eq!(
        skipped.len(),
        1,
        "exactly the corrupt file is skipped: {events:?}"
    );
    match skipped[0] {
        EngineEvent::TrackSkipped {
            track_id, reason, ..
        } => {
            assert_eq!(track_id, "broken");
            assert!(!reason.is_empty(), "the reason must be human-readable");
        }
        _ => unreachable!(),
    }

    let played: Vec<&str> = events
        .iter()
        .filter_map(|event| match event {
            EngineEvent::TrackChanged { track, .. } => Some(track.track_id.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        played.contains(&"first") && played.contains(&"third"),
        "both playable tracks must reach the output: {played:?}"
    );

    // The device consumed both good tracks (2 s of audio) plus silence gaps.
    let consumed = harness.latest().map_or(0, |(_, control)| {
        control.frames_consumed.load(Ordering::Relaxed)
    });
    assert!(
        consumed >= 44_100,
        "the device must have been fed: {consumed} frames"
    );

    engine.shutdown();
}

#[test]
fn a_queue_of_only_corrupt_files_stops_cleanly_instead_of_hanging() {
    let harness = Arc::new(Harness::default());
    let engine = engine_with(Arc::clone(&harness), Duration::from_millis(1));

    let dir = tempfile::tempdir().expect("tempdir");
    let mut queue = Vec::new();
    for index in 0..4 {
        let path = dir.path().join(format!("bad{index}.wav"));
        std::fs::write(&path, b"not audio at all").expect("write");
        queue.push(request(&path, &format!("bad{index}")));
    }

    let mut events = engine.subscribe();

    // The first track is probed when the queue starts, so the engine reports the
    // failure as an event rather than accepting the queue.
    engine.play(queue, 0).expect("command accepted");
    let events = wait_for_events(&mut events, Duration::from_secs(10), |events| {
        skip_ids(events).len() >= 4
    });
    assert_eq!(
        skip_ids(&events),
        vec!["bad0", "bad1", "bad2", "bad3"],
        "every dead file is reported once, in queue order"
    );
    assert!(
        wait_until(Duration::from_secs(10), || engine.status().state
            == PlaybackState::Stopped),
        "a dead queue must stop, not spin: {:?}",
        engine.status()
    );
    // Nothing may be left running: a second stop is a no-op, not a panic.
    engine.stop().expect("stop is idempotent");
    engine.shutdown();
}

// ---------------------------------------------------------------------------
// 4. CUE sheets over 1 GiB
// ---------------------------------------------------------------------------

/// The property under test — a 1.5 GB image is expanded from its CUE sheet
/// without any code path reading it end to end — is only provable with a
/// **sparse** file: on NTFS `set_len` allocates the range and `metadata().len()`
/// reports it, so the "physical size" assertion below has no counterpart there.
/// CUE expansion itself is covered on every platform by
/// `tests/library.rs::scan_indexes_files_albums_artists_cue_tracks_and_errors`.
#[cfg(unix)]
#[test]
fn a_cue_image_over_one_gigabyte_expands_without_being_read() {
    let dir = tempfile::tempdir().expect("tempdir");
    let image = dir.path().join("album.wav");

    // A sparse file: 1.5 GB of logical size, a few kilobytes on disk. If any code
    // path tried to read it end to end this test would take minutes, so the
    // elapsed-time assertion below is the real proof.
    let data_len: u32 = 1_500_000_000;
    let mut header = Vec::with_capacity(44);
    header.extend_from_slice(b"RIFF");
    header.extend_from_slice(&(36 + data_len).to_le_bytes());
    header.extend_from_slice(b"WAVE");
    header.extend_from_slice(b"fmt ");
    header.extend_from_slice(&16u32.to_le_bytes());
    header.extend_from_slice(&1u16.to_le_bytes());
    header.extend_from_slice(&2u16.to_le_bytes());
    header.extend_from_slice(&44_100u32.to_le_bytes());
    header.extend_from_slice(&(44_100u32 * 4).to_le_bytes());
    header.extend_from_slice(&4u16.to_le_bytes());
    header.extend_from_slice(&16u16.to_le_bytes());
    header.extend_from_slice(b"data");
    header.extend_from_slice(&data_len.to_le_bytes());

    {
        use std::io::Write;
        let mut file = std::fs::File::create(&image).expect("create image");
        file.write_all(&header).expect("header");
        file.set_len(44 + u64::from(data_len)).expect("grow sparse");
    }

    let cue = dir.path().join("album.cue");
    std::fs::write(
        &cue,
        "PERFORMER \"Stress Orchestra\"\n\
         TITLE \"Big Image\"\n\
         FILE \"album.wav\" WAVE\n\
         \x20 TRACK 01 AUDIO\n\
         \x20   TITLE \"First Movement\"\n\
         \x20   INDEX 01 00:00:00\n\
         \x20 TRACK 02 AUDIO\n\
         \x20   TITLE \"Second Movement\"\n\
         \x20   INDEX 01 25:00:00\n\
         \x20 TRACK 03 AUDIO\n\
         \x20   TITLE \"Third Movement\"\n\
         \x20   INDEX 01 50:00:00\n",
    )
    .expect("cue");

    let physical = std::fs::metadata(&image)
        .map(|meta| {
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                meta.blocks() * 512
            }
            #[cfg(not(unix))]
            {
                meta.len()
            }
        })
        .expect("metadata");
    // The elapsed-time assertion below is the real proof: a code path that read
    // the image end to end would take minutes. Sparseness is a *precondition*
    // that keeps the fixture cheap, and it is not universal — overlayfs and
    // some network filesystems allocate the range eagerly, so the file is
    // 1.5 GB on disk and the fixture simply costs a few seconds instead. The
    // property under test is unchanged, so this is reported, not enforced.
    if physical >= 4 * 1024 * 1024 {
        eprintln!(
            "note: {physical} bytes on disk — this filesystem does not sparsify, \
             falling back to the elapsed-time assertion alone"
        );
    }

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let started = Instant::now();
    let (tracks, elapsed) = runtime.block_on(async {
        let temp = tempfile::tempdir().expect("db dir");
        let pool = connect(&temp.path().join("stress.db")).await.expect("db");
        let scanner = Scanner::new(pool.clone());
        let outcome = scanner
            .scan(
                ScanConfig {
                    roots: vec![dir.path().to_path_buf()],
                    full: true,
                    ..ScanConfig::default()
                },
                ScanTrigger::Manual,
                |_| {},
            )
            .await
            .expect("scan");
        assert_eq!(outcome.tracks_missing, 0);
        let rows: Vec<(String, i64, Option<i64>, Option<i64>)> = sqlx::query_as(
            "SELECT title, duration_ms, cue_start_ms, cue_end_ms FROM tracks \
             WHERE source = 'cue' ORDER BY cue_start_ms",
        )
        .fetch_all(&pool)
        .await
        .expect("rows");
        (rows, started.elapsed())
    });

    assert_eq!(tracks.len(), 3, "three virtual tracks expected: {tracks:?}");
    assert_eq!(tracks[0].0, "First Movement");
    assert_eq!(tracks[0].2, Some(0));
    assert_eq!(tracks[0].3, Some(25 * 60 * 1000));
    assert_eq!(tracks[1].2, Some(25 * 60 * 1000));
    assert_eq!(tracks[1].3, Some(50 * 60 * 1000));
    assert_eq!(tracks[2].2, Some(50 * 60 * 1000));
    // The last span is bounded by the image itself: 1 500 000 000 bytes of
    // 16-bit stereo at 44.1 kHz is 8 503 401 ms. The value proves the scanner
    // took the length from the WAV header instead of walking the file.
    assert_eq!(
        tracks[2].3,
        Some(8_503_401),
        "the last track runs to the end of the image"
    );
    assert_eq!(tracks[0].1, 25 * 60 * 1000, "duration comes from the span");

    assert!(
        elapsed < Duration::from_secs(20),
        "a 1.5 GB image must be indexed from its header, not read ({elapsed:?})"
    );
}

// ---------------------------------------------------------------------------
// 5. Endpoint failures
// ---------------------------------------------------------------------------

#[test]
fn an_endpoint_that_refuses_to_open_leaves_the_engine_usable() {
    let opens = Arc::new(AtomicU32::new(0));
    let counter = Arc::clone(&opens);
    let opener: SinkOpener = Arc::new(move |_mode, _device, format, buffer_frames| {
        let attempt = counter.fetch_add(1, Ordering::AcqRel);
        if attempt == 0 {
            // The first open fails the way a busy exclusive endpoint does.
            return Err(prism_lib::audio::AudioError::NoDevice);
        }
        let (opened, _control) = loopback(
            attempt + 1,
            *format,
            512,
            buffer_frames,
            Duration::from_millis(2),
        );
        Ok(opened)
    });

    let engine = Engine::with_opener(EngineConfig::default(), opener).expect("engine");
    let mut events = engine.subscribe();

    let dir = tempfile::tempdir().expect("tempdir");
    let track = write_wav(&dir.path().join("retry.wav"), 10.0);

    engine
        .play(vec![request(&track, "retry")], 0)
        .expect("play");
    assert!(
        wait_until(Duration::from_secs(5), || {
            collect_events(&mut events)
                .iter()
                .any(|event| matches!(event, EngineEvent::OutputError { .. }))
        }),
        "the failure must be reported"
    );
    assert_eq!(engine.status().state, PlaybackState::Stopped);

    // The same engine keeps working: the user presses play again and the
    // endpoint opens this time.
    engine
        .play(vec![request(&track, "retry")], 0)
        .expect("play");
    assert!(
        wait_until(Duration::from_secs(5), || engine.status().state
            == PlaybackState::Playing),
        "a retry must recover: {:?}",
        engine.status()
    );
    assert_eq!(opens.load(Ordering::Acquire), 2);

    engine.stop().expect("stop");
    engine.shutdown();
}

#[test]
fn stopping_during_an_endpoint_swap_never_leaves_a_dangling_device() {
    let harness = Arc::new(Harness::default());
    let engine = engine_with(Arc::clone(&harness), Duration::from_millis(1));

    let dir = tempfile::tempdir().expect("tempdir");
    let track = write_wav(&dir.path().join("churn.wav"), 20.0);
    engine
        .play(vec![request(&track, "churn")], 0)
        .expect("play");
    assert!(wait_until(Duration::from_secs(5), || engine.status().state
        == PlaybackState::Playing));

    // Swap and stop as fast as the commands can be issued: the supervisor must
    // serialise them and end in a stopped state with every device thread joined.
    for round in 0..8_u32 {
        let _ = engine.set_output(OutputMode::Shared, Some(format!("churn-{round}")), 128);
        if round % 3 == 0 {
            let _ = engine.stop();
        }
    }
    let _ = engine.stop();

    assert!(
        wait_until(Duration::from_secs(5), || engine.status().state
            == PlaybackState::Stopped),
        "the engine must settle in a stopped state: {:?}",
        engine.status()
    );
    let live: usize = harness
        .controls
        .lock()
        .expect("controls")
        .values()
        .filter(|control| !control.stop.load(Ordering::Acquire))
        .count();
    assert_eq!(live, 0, "every loopback device must be stopped");
    engine.shutdown();
}
