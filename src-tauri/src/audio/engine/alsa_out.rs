//! Raw ALSA sink (`hw:` / `plughw:`), exclusive by construction.
//!
//! The device clock is the write call: `snd_pcm_writei` blocks until the
//! kernel ring has room, so a dedicated writer thread paces itself without
//! timers and without drift. Only containers ALSA can carry natively are
//! accepted; there is no hidden conversion stage and therefore no resampler
//! between the DSP chain and the DAC.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use alsa::pcm::{Access, Format, HwParams, IO, PCM, State};
use alsa::{Direction, ValueOr};

use super::backend::{
    BackendKind, Negotiation, OutputDevice, OutputSink, RefillSignal, SinkDescriptor, common_rates,
    format_rank,
};
use crate::audio::AudioError;
use crate::audio::format::{AudioFormat, SampleFormat};
use crate::audio::ring::{RingStats, SampleConsumer};

const ID_PREFIX: &str = "alsa-hw:";

/// Wire format negotiated with the device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wire {
    S16,
    /// Signed 24-bit left-justified inside a 32-bit word (`S24_LE`).
    S24,
    S32,
    F32,
    F64,
}

impl Wire {
    const ALL: [Self; 5] = [Self::S32, Self::S24, Self::S16, Self::F32, Self::F64];

    fn alsa(self) -> Format {
        match self {
            Self::S16 => Format::S16LE,
            Self::S24 => Format::S24LE,
            Self::S32 => Format::S32LE,
            Self::F32 => Format::FloatLE,
            Self::F64 => Format::Float64LE,
        }
    }

    fn reporting(self) -> SampleFormat {
        match self {
            Self::S16 => SampleFormat::S16,
            Self::S24 => SampleFormat::S24In32,
            Self::S32 => SampleFormat::S32,
            Self::F32 => SampleFormat::F32,
            Self::F64 => SampleFormat::F64,
        }
    }
}

impl SampleFormat {
    /// The ALSA container that carries this width losslessly.
    fn wire(self) -> Wire {
        match self {
            Self::S16 => Wire::S16,
            Self::S24 | Self::S24In32 => Wire::S24,
            Self::S32 => Wire::S32,
            Self::F32 => Wire::F32,
            Self::F64 => Wire::F64,
        }
    }
}

fn alsa_error(context: &str, error: alsa::Error) -> AudioError {
    AudioError::Device(format!("{context}: {error}"))
}

/// Capabilities of one PCM, read without starting the clock.
struct Caps {
    channels: u16,
    min_rate: u32,
    max_rate: u32,
    formats: Vec<SampleFormat>,
}

fn probe(name: &str) -> Option<Caps> {
    let pcm = PCM::new(name, Direction::Playback, true).ok()?;
    let hwp = HwParams::any(&pcm).ok()?;
    let channels = u16::try_from(hwp.get_channels_max().unwrap_or(2)).unwrap_or(u16::MAX);
    let min_rate = hwp.get_rate_min().unwrap_or(8_000);
    let max_rate = hwp.get_rate_max().unwrap_or(192_000);
    let mut formats: Vec<SampleFormat> = Wire::ALL
        .iter()
        .filter(|wire| hwp.test_format(wire.alsa()).is_ok())
        .map(|wire| wire.reporting())
        .collect();
    formats.sort_by_key(|format| format_rank(*format));
    formats.dedup();
    Some(Caps {
        channels: channels.max(1),
        min_rate,
        max_rate: max_rate.max(min_rate),
        formats,
    })
}

/// Playback PCMs exposed by the kernel.
pub fn enumerate() -> Result<Vec<OutputDevice>, AudioError> {
    let hints = alsa::device_name::HintIter::new_str(None, "pcm")
        .map_err(|error| alsa_error("hints", error))?;

    let mut devices = Vec::new();
    for hint in hints {
        if hint.direction != Some(Direction::Playback) {
            continue;
        }
        let Some(name) = hint.name else { continue };
        let exclusive = name.starts_with("hw:");
        if !exclusive && name != "default" && !name.starts_with("plughw:") {
            continue;
        }
        if exclusive && !name.contains("CARD=") && !name.contains(',') {
            // "hw" without a card selector cannot be opened.
            continue;
        }

        let description = hint
            .desc
            .as_deref()
            .map(|desc| desc.lines().next().unwrap_or(desc).trim().to_string())
            .filter(|desc| !desc.is_empty())
            .unwrap_or_else(|| name.clone());

        let caps = probe(&name);
        let (channels, min_rate, max_rate, formats) =
            caps.as_ref()
                .map_or((2, 8_000, 192_000, vec![SampleFormat::S32]), |caps| {
                    (
                        caps.channels,
                        caps.min_rate,
                        caps.max_rate,
                        caps.formats.clone(),
                    )
                });

        devices.push(OutputDevice {
            id: format!("{ID_PREFIX}{name}"),
            name: format!("{description} [{name}]"),
            kind: BackendKind::AlsaHw,
            is_default: name == "default",
            channels,
            sample_rates: common_rates(max_rate)
                .into_iter()
                .filter(|rate| *rate >= min_rate)
                .collect(),
            sample_formats: formats,
            supports_exclusive: exclusive,
        });
    }

    // Hardware endpoints first: `default` goes through dmix/PulseAudio and is
    // not bit-perfect even though it works everywhere.
    devices.sort_by(|a, b| {
        b.supports_exclusive
            .cmp(&a.supports_exclusive)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    if devices.is_empty() {
        return Err(AudioError::NoDevice);
    }
    Ok(devices)
}

fn pcm_name(device: Option<&str>) -> String {
    device
        .and_then(|id| id.strip_prefix(ID_PREFIX).or(Some(id)))
        .unwrap_or("default")
        .to_string()
}

/// Decide the wire container the writer will use.
fn pick_wire(caps: &Caps, desired: SampleFormat) -> Option<Wire> {
    let preferred = desired.wire();
    std::iter::once(preferred)
        .chain(Wire::ALL.iter().copied().filter(|wire| *wire != preferred))
        .find(|wire| caps.formats.contains(&wire.reporting()))
}

/// Work out the exact format the writer will open, without starting it.
pub fn negotiate(
    device: Option<&str>,
    desired: &AudioFormat,
    buffer_frames: u32,
) -> Result<Negotiation, AudioError> {
    let name = pcm_name(device);
    let caps = probe(&name).ok_or_else(|| {
        AudioError::Device(format!(
            "cannot query ALSA PCM {name}: device is busy or absent"
        ))
    })?;

    let wire = pick_wire(&caps, desired.sample_format).ok_or_else(|| {
        AudioError::UnsupportedByDevice(format!("{name} carries no PCM format Prism supports"))
    })?;

    if !(caps.min_rate..=caps.max_rate).contains(&desired.sample_rate) {
        return Err(AudioError::UnsupportedByDevice(format!(
            "{name} clocks {}..{} Hz, the track needs {} Hz",
            caps.min_rate, caps.max_rate, desired.sample_rate
        )));
    }

    let channels = desired.channels.max(2).min(caps.channels.max(1));

    let period = if buffer_frames == 0 {
        (desired.sample_rate / 100).clamp(64, 2_048)
    } else {
        buffer_frames.clamp(32, 8_192)
    };

    Ok(Negotiation {
        device: OutputDevice {
            id: format!("{ID_PREFIX}{name}"),
            name: name.clone(),
            kind: BackendKind::AlsaHw,
            is_default: name == "default",
            channels,
            sample_rates: common_rates(caps.max_rate)
                .into_iter()
                .filter(|rate| *rate >= caps.min_rate)
                .collect(),
            sample_formats: caps.formats,
            supports_exclusive: name.starts_with("hw:"),
        },
        kind: BackendKind::AlsaHw,
        format: AudioFormat {
            sample_rate: desired.sample_rate,
            channels,
            sample_format: wire.reporting(),
        },
        period_frames: period,
        buffer_frames: period.saturating_mul(4),
        latency_frames: u64::from(period) * 2,
    })
}

#[derive(Debug, Default)]
struct Control {
    running: AtomicBool,
    paused: AtomicBool,
    stop: AtomicBool,
}

struct AlsaSink {
    descriptor: SinkDescriptor,
    control: Arc<Control>,
    thread: Option<JoinHandle<()>>,
}

impl OutputSink for AlsaSink {
    fn descriptor(&self) -> &SinkDescriptor {
        &self.descriptor
    }

    fn start(&mut self) -> Result<(), AudioError> {
        self.control.running.store(true, Ordering::Release);
        Ok(())
    }

    fn pause(&mut self) -> Result<(), AudioError> {
        self.control.paused.store(true, Ordering::Release);
        Ok(())
    }

    fn resume(&mut self) -> Result<(), AudioError> {
        self.control.running.store(true, Ordering::Release);
        self.control.paused.store(false, Ordering::Release);
        Ok(())
    }

    fn stop(&mut self) {
        self.control.stop.store(true, Ordering::Release);
        self.control.paused.store(false, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for AlsaSink {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Open the PCM and spawn its writer thread.
pub fn start(
    negotiation: Negotiation,
    consumer: SampleConsumer,
    refill: Arc<RefillSignal>,
    stats: Arc<RingStats>,
) -> Result<Box<dyn OutputSink>, AudioError> {
    let name = pcm_name(Some(&negotiation.device.id));
    let format = negotiation.format;
    let period = negotiation.period_frames as i64;

    let pcm = PCM::new(&name, Direction::Playback, false).map_err(|e| alsa_error("open", e))?;
    let wire = configure(&pcm, format, period)?;

    let control = Arc::new(Control {
        running: AtomicBool::new(false),
        paused: AtomicBool::new(false),
        stop: AtomicBool::new(false),
    });
    let worker_control = Arc::clone(&control);
    let worker_refill = Arc::clone(&refill);
    let close_refill = Arc::clone(&refill);
    let period_frames = usize::try_from(period).unwrap_or(512);

    let thread = std::thread::Builder::new()
        .name("prism-alsa".into())
        .spawn(move || {
            if let Err(error) = run(
                &pcm,
                wire,
                usize::from(format.channels),
                period_frames,
                consumer,
                worker_refill,
                &worker_control,
            ) {
                tracing::error!(error = %error, "ALSA writer stopped");
            }
            // A dead writer means the supervisor must rebuild the endpoint.
            close_refill.close();
            worker_control.stop.store(true, Ordering::Release);
        })
        .map_err(|error| AudioError::Device(format!("cannot spawn ALSA thread: {error}")))?;

    tracing::info!(
        device = %name,
        format = ?format,
        period_frames,
        "ALSA hardware stream opened"
    );

    Ok(Box::new(AlsaSink {
        descriptor: SinkDescriptor {
            device: negotiation.device,
            format,
            period_frames: negotiation.period_frames,
            buffer_frames: negotiation.buffer_frames,
            latency_frames: negotiation.latency_frames,
            stats,
            refill,
        },
        control,
        thread: Some(thread),
    }))
}

/// Apply the hardware parameters the negotiation promised.
fn configure(pcm: &PCM, format: AudioFormat, period: i64) -> Result<Wire, AudioError> {
    let wire = format.sample_format.wire();
    let hwp = HwParams::any(pcm).map_err(|e| alsa_error("hw_params", e))?;
    hwp.set_access(Access::RWInterleaved)
        .map_err(|e| alsa_error("access", e))?;
    hwp.set_format(wire.alsa())
        .map_err(|e| alsa_error("format", e))?;
    hwp.set_channels(u32::from(format.channels))
        .map_err(|e| alsa_error("channels", e))?;
    let rate = hwp
        .set_rate_near(format.sample_rate, ValueOr::Nearest)
        .map_err(|e| alsa_error("rate", e))?;
    if rate != format.sample_rate {
        return Err(AudioError::UnsupportedByDevice(format!(
            "the PCM clocked {rate} Hz instead of {} Hz",
            format.sample_rate
        )));
    }
    let period = hwp
        .set_period_size_near(period.max(16), ValueOr::Nearest)
        .map_err(|e| alsa_error("period size", e))?;
    hwp.set_buffer_size_near(period.saturating_mul(4))
        .map_err(|e| alsa_error("buffer size", e))?;
    pcm.hw_params(&hwp)
        .map_err(|e| alsa_error("hw_params apply", e))?;

    // Start on the first period and wake the writer whenever one period of
    // room is available.
    if let Ok(swp) = pcm.sw_params_current() {
        let _ = swp.set_start_threshold(period);
        let _ = swp.set_avail_min(period);
        let _ = pcm.sw_params(&swp);
    }
    Ok(wire)
}

fn run(
    pcm: &PCM,
    wire: Wire,
    channels: usize,
    period_frames: usize,
    mut consumer: SampleConsumer,
    refill: Arc<RefillSignal>,
    control: &Control,
) -> Result<(), AudioError> {
    let mut scratch = vec![0.0f32; period_frames * channels];

    'playback: while !control.stop.load(Ordering::Acquire) {
        // The engine primes the ring before flipping `running`; nothing is
        // written to the PCM until then, so the clock never starts empty.
        if !control.running.load(Ordering::Acquire) {
            std::thread::sleep(Duration::from_millis(2));
            continue 'playback;
        }
        if control.paused.load(Ordering::Acquire) {
            let _ = pcm.pause(true);
            std::thread::sleep(Duration::from_millis(5));
            continue 'playback;
        }
        let _ = pcm.pause(false);

        consumer.pop_or_silence(&mut scratch);

        match write_period(pcm, wire, &scratch, channels) {
            Ok(()) => {}
            Err(Recovery::XRun) => {
                // XRUN or suspend: re-arm the PCM and drop this block. The
                // ring's underrun counter already recorded the discontinuity.
                pcm.prepare()
                    .map_err(|e| alsa_error("prepare after xrun", e))?;
                continue 'playback;
            }
            Err(Recovery::Fatal(error)) => {
                refill.close();
                return Err(error);
            }
        }

        refill.request();
    }

    let _ = pcm.drain();
    Ok(())
}

/// How a failed period write should be handled.
enum Recovery {
    /// The PCM needs `prepare()` before the next write (xrun, suspend).
    XRun,
    /// Unrecoverable: the device is gone.
    Fatal(AudioError),
}

fn write_period(pcm: &PCM, wire: Wire, samples: &[f32], channels: usize) -> Result<(), Recovery> {
    let result = match wire {
        Wire::S16 => {
            let io = pcm.io_i16().map_err(io_error)?;
            let mut buffer = vec![0i16; samples.len()];
            for (target, source) in buffer.iter_mut().zip(samples) {
                *target = (fsample(*source) * 32_767.0).round() as i16;
            }
            write_all(&io, &buffer, channels)
        }
        Wire::S24 => {
            let io = pcm.io_i32().map_err(io_error)?;
            let mut buffer = vec![0i32; samples.len()];
            for (target, source) in buffer.iter_mut().zip(samples) {
                *target = (fsample(*source) * 8_388_607.0).round() as i32;
            }
            write_all(&io, &buffer, channels)
        }
        Wire::S32 => {
            let io = pcm.io_i32().map_err(io_error)?;
            let mut buffer = vec![0i32; samples.len()];
            for (target, source) in buffer.iter_mut().zip(samples) {
                *target = (fsample(*source) * 2_147_483_647.0).round() as i32;
            }
            write_all(&io, &buffer, channels)
        }
        Wire::F32 => {
            let io = pcm.io_f32().map_err(io_error)?;
            write_all(&io, samples, channels)
        }
        Wire::F64 => {
            let io = pcm.io_f64().map_err(io_error)?;
            let buffer: Vec<f64> = samples.iter().map(|s| f64::from(fsample(*s))).collect();
            write_all(&io, &buffer, channels)
        }
    };

    match result {
        Ok(()) => Ok(()),
        Err(error) => match pcm.state() {
            State::XRun | State::Suspended => Err(Recovery::XRun),
            State::Disconnected => Err(Recovery::Fatal(AudioError::Device(
                "the ALSA device was disconnected".into(),
            ))),
            state => Err(Recovery::Fatal(AudioError::Device(format!(
                "ALSA write failed in state {state:?}: {error}"
            )))),
        },
    }
}

fn io_error(error: alsa::Error) -> Recovery {
    Recovery::Fatal(AudioError::Device(format!("ALSA IO setup failed: {error}")))
}

fn fsample(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

/// Write a whole interleaved buffer, tolerating short writes.
///
/// `writei` returns *frames*, so the retry offset advances in frames and is
/// converted back to an interleaved slice with the channel count.
fn write_all<S: alsa::pcm::IoFormat>(
    io: &IO<'_, S>,
    buffer: &[S],
    channels: usize,
) -> Result<(), alsa::Error> {
    let total_frames = buffer.len() / channels;
    let mut written = 0usize;
    while written < total_frames {
        let frames = io.writei(&buffer[written * channels..])?;
        if frames == 0 {
            break;
        }
        written += frames;
    }
    Ok(())
}
