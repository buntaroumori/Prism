//! CPU profile of the hot path: decode → DSP → (optional) resample.
//!
//! This example exists because "it feels fast" is not a measurement. It drives
//! the production decoding pipeline — the same `GaplessPlayer`, the same DSP
//! chain, the same Symphonia source — over a generated catalog, under a sampling
//! profiler, and writes an SVG flamegraph plus a throughput summary.
//!
//! ```text
//! cargo run --example profile_hot_path --release --features profiling
//! cargo run --example profile_hot_path --release --features profiling -- \
//!     --seconds 20 --tracks 24 --out target/profiles/decode.svg
//! ```
//!
//! Reading the output: width is time, not calls. A wide `symphonia` frame means
//! decoding dominates; a wide `rubato` frame means the resampler does. If neither
//! dominates and `memcpy`/`malloc` do, the bottleneck is the ring or the chunk
//! allocator, not the codec.
//!
//! Sampling needs symbol names. Release builds are stripped of debug info by
//! default, so function-level attribution works out of the box but line numbers
//! do not; add `debug = 1` to `[profile.release]` for line-level detail.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use prism_lib::audio::decoder::{GaplessPlayer, PlaybackConfig, TrackRequest};
use prism_lib::audio::dsp::DspSettings;
use prism_lib::audio::format::{AudioFormat, SampleFormat};

const SOURCE_FORMAT: AudioFormat = AudioFormat {
    sample_rate: 44_100,
    channels: 2,
    sample_format: SampleFormat::F32,
};

/// Frames per `next_chunk` call: the same 512-frame block the engine hands the
/// device, so the profile reflects production granularity.
const CHUNK_FRAMES: usize = 512;

fn main() -> Result<(), Box<dyn Error>> {
    let options = Options::parse(std::env::args().skip(1))?;

    println!(
        "Prism · CPU profile — {} tracks, {} s, {} Hz sampling",
        options.tracks, options.seconds, options.frequency
    );

    let directory = tempfile::tempdir()?;
    let catalog = build_catalog(directory.path(), options.tracks, options.track_seconds)?;
    println!(
        "catalog: {} WAV files, {:.1} MB",
        catalog.len(),
        total_bytes(&catalog)? as f64 / 1_048_576.0
    );

    let output_format = AudioFormat {
        sample_rate: options.output_rate,
        ..SOURCE_FORMAT
    };
    let config = PlaybackConfig {
        output_format,
        dsp: DspSettings::default(),
        ram_play: false,
        ram_limit_bytes: 0,
    };

    // Warm the OS page cache so the profile measures compute, not disk.
    let warmup_started = Instant::now();
    let warmup_frames = run_queue(&catalog, &config, None)?;
    println!(
        "warm-up: {warmup_frames} frames in {:.2} s",
        warmup_started.elapsed().as_secs_f64()
    );

    let guard = pprof::ProfilerGuardBuilder::default()
        .frequency(i32::try_from(options.frequency)?)
        .blocklist(&["libc", "libgcc", "pthread", "vdso"])
        .build()
        .map_err(|error| format!("cannot start the profiler: {error}"))?;

    let started = Instant::now();
    let deadline = started + Duration::from_secs(options.seconds);
    let mut frames: u64 = 0;
    let mut passes = 0_u32;
    while Instant::now() < deadline {
        frames += run_queue(&catalog, &config, Some(deadline))?;
        passes += 1;
        if frames == 0 {
            return Err("the decoder produced no frames".into());
        }
    }
    let elapsed = started.elapsed();

    let report = guard
        .report()
        .build()
        .map_err(|error| format!("cannot build the report: {error}"))?;

    if let Some(parent) = options.out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = std::fs::File::create(&options.out)?;
    report.flamegraph(file)?;

    let seconds = elapsed.as_secs_f64();
    let audio_seconds = (frames as f64) / f64::from(output_format.sample_rate);
    println!("\nresult");
    println!("{}", "─".repeat(58));
    println!("{:<26}{}", "passes", passes);
    println!("{:<26}{}", "wall clock", format!("{seconds:.2} s"));
    println!(
        "{:<26}{}",
        "samples produced",
        format!("{audio_seconds:.1} s")
    );
    println!(
        "{:<26}{}",
        "throughput",
        format!("{:.0}× real time", audio_seconds / seconds)
    );
    println!(
        "{:<26}{}",
        "CPU per 512-frame block",
        format!(
            "{:.3} ms",
            (seconds * 1_000.0) / (frames as f64 / CHUNK_FRAMES as f64).max(1.0)
        )
    );
    println!("{:<26}{}", "flamegraph", options.out.display());
    // A 512-frame block at 44.1 kHz is 11.6 ms of audio. The engine's budget is
    // a fraction of that: at 2 % CPU a block may cost 0.23 ms.
    println!(
        "\nA 512-frame block is 11.6 ms of audio at 44.1 kHz; the 2 % CPU target\n\
         therefore allows 0.23 ms per block. The figure above is what the decoder\n\
         actually spends, so any healthy margin there is headroom for the UI."
    );

    Ok(())
}

struct Options {
    seconds: u64,
    tracks: usize,
    track_seconds: f32,
    frequency: u32,
    output_rate: u32,
    out: PathBuf,
}

impl Options {
    fn parse(arguments: impl Iterator<Item = String>) -> Result<Self, Box<dyn Error>> {
        let mut options = Self {
            seconds: 10,
            tracks: 8,
            track_seconds: 4.0,
            frequency: 997,
            output_rate: SOURCE_FORMAT.sample_rate,
            out: PathBuf::from("target/profiles/flamegraph.svg"),
        };
        let mut arguments = arguments;
        while let Some(flag) = arguments.next() {
            let mut value = || {
                arguments
                    .next()
                    .ok_or_else(|| format!("{flag} needs a value"))
            };
            match flag.as_str() {
                "--seconds" => options.seconds = value()?.parse()?,
                "--tracks" => options.tracks = value()?.parse()?,
                "--track-seconds" => options.track_seconds = value()?.parse()?,
                "--frequency" => options.frequency = value()?.parse()?,
                "--output-rate" => options.output_rate = value()?.parse()?,
                "--out" => options.out = PathBuf::from(value()?),
                "--help" | "-h" => {
                    println!(
                        "usage: profile_hot_path [--seconds N] [--tracks N] [--track-seconds S]\n\
                         \x20                    [--frequency HZ] [--output-rate HZ] [--out PATH]"
                    );
                    std::process::exit(0);
                }
                other => return Err(format!("unknown flag {other}").into()),
            }
        }
        if options.tracks == 0 || options.seconds == 0 {
            return Err("tracks and seconds must be positive".into());
        }
        Ok(options)
    }
}

/// Generate the catalog once: silence decodes as fast as music, and the point of
/// this profile is the codec and DSP, not the musical content.
fn build_catalog(
    directory: &Path,
    tracks: usize,
    seconds: f32,
) -> Result<Vec<TrackRequest>, Box<dyn Error>> {
    let mut requests = Vec::with_capacity(tracks);
    for index in 0..tracks {
        let path = directory.join(format!("track-{index:03}.wav"));
        write_wav(&path, seconds)?;
        requests.push(TrackRequest::file(path, format!("track-{index:03}")));
    }
    Ok(requests)
}

fn write_wav(path: &Path, seconds: f32) -> Result<(), Box<dyn Error>> {
    let frames = (seconds * SOURCE_FORMAT.sample_rate as f32) as u32;
    let block_align = u32::from(SOURCE_FORMAT.channels) * 2;
    let data_len = frames * block_align;

    let mut buffer = Vec::with_capacity(44 + data_len as usize);
    buffer.extend_from_slice(b"RIFF");
    buffer.extend_from_slice(&(36 + data_len).to_le_bytes());
    buffer.extend_from_slice(b"WAVE");
    buffer.extend_from_slice(b"fmt ");
    buffer.extend_from_slice(&16u32.to_le_bytes());
    buffer.extend_from_slice(&1u16.to_le_bytes());
    buffer.extend_from_slice(&SOURCE_FORMAT.channels.to_le_bytes());
    buffer.extend_from_slice(&SOURCE_FORMAT.sample_rate.to_le_bytes());
    buffer.extend_from_slice(&(SOURCE_FORMAT.sample_rate * block_align).to_le_bytes());
    buffer.extend_from_slice(&u16::try_from(block_align)?.to_le_bytes());
    buffer.extend_from_slice(&16u16.to_le_bytes());
    buffer.extend_from_slice(b"data");
    buffer.extend_from_slice(&data_len.to_le_bytes());

    // A quiet 440 Hz tone: non-silent input keeps every DSP stage in its
    // arithmetic path instead of short-circuiting on zeros.
    let step = std::f64::consts::TAU * 440.0 / f64::from(SOURCE_FORMAT.sample_rate);
    for frame in 0..frames {
        let value = (f64::from(frame) * step).sin();
        let sample = (value * 0.1 * f64::from(i16::MAX)) as i16;
        for _ in 0..SOURCE_FORMAT.channels {
            buffer.extend_from_slice(&sample.to_le_bytes());
        }
    }
    std::fs::write(path, buffer)?;
    Ok(())
}

fn total_bytes(requests: &[TrackRequest]) -> std::io::Result<u64> {
    let mut total = 0;
    for request in requests {
        total += std::fs::metadata(&request.path)?.len();
    }
    Ok(total)
}

/// Play `requests` end to end, or until `deadline` passes. Returns frames.
///
/// `black_box` keeps the optimiser from deleting the decode loop: without it a
/// release build is free to notice that nothing reads the samples.
fn run_queue(
    requests: &[TrackRequest],
    config: &PlaybackConfig,
    deadline: Option<Instant>,
) -> Result<u64, Box<dyn Error>> {
    let mut player = GaplessPlayer::new(requests.to_vec(), config.clone())?;
    let mut frames = 0_u64;
    let mut checksum = 0_u64;
    while !player.is_finished() {
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            break;
        }
        let chunk = player.next_chunk(CHUNK_FRAMES)?;
        if chunk.samples.is_empty() {
            break;
        }
        frames += (chunk.samples.len() / usize::from(SOURCE_FORMAT.channels)) as u64;
        checksum = checksum.wrapping_add(u64::from(chunk.samples[0].to_bits()));
    }
    std::hint::black_box(checksum);
    Ok(frames)
}
