//! cpal-backed sink: WASAPI shared mode on Windows, the system default host
//! elsewhere, and ASIO when the `asio` feature is enabled.
//!
//! cpal hands the callback a mutable slice already in the device's native
//! sample type. The callback therefore does exactly three things: pop frames
//! from the lock-free ring, convert them, and wake the pump. No allocation,
//! no locking, no syscalls.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SizedSample};

use super::backend::{
    BackendKind, Negotiation, OutputDevice, OutputSink, RefillSignal, SinkDescriptor, common_rates,
    format_rank,
};
use crate::audio::AudioError;
use crate::audio::format::{AudioFormat, SampleFormat};
use crate::audio::ring::{RingStats, SampleConsumer};

/// Convert a cpal sample format into the container Prism reports.
///
/// Unsigned and 8-bit containers are rejected: no audiophile endpoint uses
/// them, and mapping them onto the signed enum would misreport the signal path.
fn to_prism(format: cpal::SampleFormat) -> Option<SampleFormat> {
    match format {
        cpal::SampleFormat::I16 => Some(SampleFormat::S16),
        cpal::SampleFormat::I32 => Some(SampleFormat::S32),
        cpal::SampleFormat::F32 => Some(SampleFormat::F32),
        cpal::SampleFormat::F64 => Some(SampleFormat::F64),
        _ => None,
    }
}

fn host_for(kind: BackendKind) -> Result<cpal::Host, AudioError> {
    match kind {
        BackendKind::WasapiShared => {
            #[cfg(windows)]
            {
                cpal::host_from_id(cpal::HostId::Wasapi)
                    .map_err(|error| AudioError::Device(format!("host unavailable: {error}")))
            }
            #[cfg(not(windows))]
            {
                Err(AudioError::Device(
                    "WASAPI is only available on Windows".into(),
                ))
            }
        }
        BackendKind::Asio => {
            #[cfg(all(windows, feature = "asio"))]
            {
                cpal::host_from_id(cpal::HostId::Asio)
                    .map_err(|error| AudioError::Device(format!("host unavailable: {error}")))
            }
            #[cfg(not(all(windows, feature = "asio")))]
            {
                Err(AudioError::Device(
                    "ASIO support is not compiled into this build (enable the `asio` feature)"
                        .into(),
                ))
            }
        }
        _ => Ok(cpal::default_host()),
    }
}

/// Enumerate endpoints of one cpal host.
pub fn enumerate(kind: BackendKind) -> Result<Vec<OutputDevice>, AudioError> {
    let host = host_for(kind)?;
    let default_name = host
        .default_output_device()
        .and_then(|device| device.name().ok());
    let devices = host
        .output_devices()
        .map_err(|error| AudioError::Device(format!("cannot list devices: {error}")))?;

    let mut out = Vec::new();
    for device in devices {
        let Ok(name) = device.name() else { continue };
        let mut rates: Vec<u32> = Vec::new();
        let mut channels = 0u16;
        let mut formats: Vec<SampleFormat> = Vec::new();
        let mut max_rate = 0u32;

        if let Ok(configs) = device.supported_output_configs() {
            for config in configs {
                let Some(format) = to_prism(config.sample_format()) else {
                    continue;
                };
                let min = config.min_sample_rate().0;
                let max = config.max_sample_rate().0;
                max_rate = max_rate.max(max);
                channels = channels.max(config.channels());
                formats.push(format);
                // The discrete rates the endpoint advertises.
                for rate in common_rates(max) {
                    if rate >= min && !rates.contains(&rate) {
                        rates.push(rate);
                    }
                }
            }
        }

        if max_rate == 0 {
            // The endpoint offers no signed PCM configuration: not usable.
            continue;
        }

        formats.sort_by_key(|format| format_rank(*format));
        formats.dedup();
        rates.sort_unstable();
        rates.dedup();

        out.push(OutputDevice {
            id: format!("{}:{}", kind.id(), name),
            name: name.clone(),
            kind,
            is_default: default_name.as_deref() == Some(name.as_str()),
            channels: channels.max(1),
            sample_rates: rates,
            sample_formats: formats,
            supports_exclusive: kind.is_exclusive(),
        });
    }

    // Default endpoint first, then alphabetical: the selector never reshuffles.
    out.sort_by(|a, b| {
        b.is_default
            .cmp(&a.is_default)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(out)
}

fn find_device(
    kind: BackendKind,
    wanted: Option<&str>,
) -> Result<(cpal::Host, cpal::Device), AudioError> {
    let host = host_for(kind)?;
    if let Some(wanted) = wanted {
        let name = wanted.rsplit_once(':').map_or(wanted, |(_, name)| name);
        if let Ok(devices) = host.output_devices() {
            for device in devices {
                if device
                    .name()
                    .map(|candidate| candidate == name)
                    .unwrap_or(false)
                {
                    return Ok((host, device));
                }
            }
        }
    }
    let device = host.default_output_device().ok_or(AudioError::NoDevice)?;
    Ok((host, device))
}

/// The endpoint's live configuration, as the driver reports it *right now*.
///
/// This is `default_output_config()`: on WASAPI shared mode it is the mix
/// format the OS mixer is running (the one shown in the Sound panel), on
/// PipeWire it is the node's current format, on ALSA the card default. It is
/// deliberately not the format the engine opens with — reporting the latter as
/// "device format" is what made the old signal path claim 32/192 while the OS
/// said 24/192.
pub fn live_format(kind: BackendKind, device: Option<&str>) -> Option<AudioFormat> {
    let (_host, handle) = find_device(kind, device).ok()?;
    let default = handle.default_output_config().ok()?;
    Some(AudioFormat {
        sample_rate: default.sample_rate().0,
        channels: default.channels(),
        sample_format: to_prism(default.sample_format())?,
    })
}

/// Best configuration the endpoint can give for `desired`.
fn choose_config(
    device: &cpal::Device,
    desired: &AudioFormat,
) -> Result<(cpal::SupportedStreamConfig, SampleFormat), AudioError> {
    let ranges: Vec<(SampleFormat, u32, u32, u16)> = device
        .supported_output_configs()
        .map(|configs| {
            configs
                .filter_map(|config| {
                    Some((
                        to_prism(config.sample_format())?,
                        config.min_sample_rate().0,
                        config.max_sample_rate().0,
                        config.channels(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();

    // Exact match first: same rate, same container, same channel count.
    //
    // The channel count is not a detail. An endpoint that advertises 8 channels
    // and is opened with 8 will happily accept a stereo stream, and every frame
    // after the first is then interpreted as a different speaker — the single
    // most effective way to make a player sound broken. Prefer the config that
    // matches the source exactly and only widen when nothing else exists.
    for range in &ranges {
        if range.0 == desired.sample_format
            && (range.1..=range.2).contains(&desired.sample_rate)
            && range.3 == desired.channels
            && let Ok(configs) = device.supported_output_configs()
        {
            for config in configs {
                if to_prism(config.sample_format()) == Some(desired.sample_format) {
                    if let Some(selected) =
                        config.try_with_sample_rate(cpal::SampleRate(desired.sample_rate))
                    {
                        return Ok((selected, desired.sample_format));
                    }
                }
            }
        }
    }

    // Same rate, any container the endpoint carries, widest first.
    let mut at_rate: Vec<(u8, cpal::SupportedStreamConfigRange)> = Vec::new();
    if let Ok(configs) = device.supported_output_configs() {
        for config in configs {
            let Some(format) = to_prism(config.sample_format()) else {
                continue;
            };
            if config.min_sample_rate().0 <= desired.sample_rate
                && config.max_sample_rate().0 >= desired.sample_rate
            {
                at_rate.push((format_rank(format), config));
            }
        }
    }
    at_rate.sort_by_key(|(rank, _)| *rank);
    if let Some((_, range)) = at_rate.first() {
        if let Some(config) = range
            .clone()
            .try_with_sample_rate(cpal::SampleRate(desired.sample_rate))
        {
            let format = to_prism(config.sample_format()).unwrap_or(desired.sample_format);
            return Ok((config, format));
        }
    }

    // Nothing at that rate (shared mode locks the graph rate): use the default.
    let default = device
        .default_output_config()
        .map_err(|error| AudioError::UnsupportedByDevice(format!("{error}")))?;
    let format = to_prism(default.sample_format()).ok_or_else(|| {
        AudioError::UnsupportedByDevice(format!(
            "the endpoint carries {:?}, which Prism does not support",
            default.sample_format()
        ))
    })?;
    Ok((default, format))
}

/// Work out the format and timing without touching the audio clock.
pub fn negotiate(
    kind: BackendKind,
    device: Option<&str>,
    desired: &AudioFormat,
    buffer_frames: u32,
) -> Result<Negotiation, AudioError> {
    let (_host, handle) = find_device(kind, device)?;
    let name = handle
        .name()
        .unwrap_or_else(|_| "System output".to_string());
    let (config, sample_format) = choose_config(&handle, desired)?;
    let rate = config.sample_rate().0;

    let period = if buffer_frames == 0 {
        // 10 ms at the stream rate: the interactive default.
        (rate / 100).clamp(64, 2_048)
    } else {
        // cpal's `buffer_size` is a request per callback; keep it in a range
        // every host can honour.
        buffer_frames.clamp(64, 2_048)
    };

    let rates: Vec<u32> = {
        let mut rates = common_rates(rate.max(96_000));
        rates.push(rate);
        rates.sort_unstable();
        rates.dedup();
        rates
    };

    Ok(Negotiation {
        device: OutputDevice {
            id: format!("{}:{}", kind.id(), name),
            name,
            kind,
            is_default: true,
            channels: config.channels(),
            sample_rates: rates,
            sample_formats: vec![sample_format],
            supports_exclusive: kind.is_exclusive(),
        },
        kind,
        format: AudioFormat {
            sample_rate: rate,
            channels: config.channels(),
            sample_format,
        },
        period_frames: period,
        buffer_frames: period.saturating_mul(4),
        latency_frames: u64::from(period) * 2,
    })
}

/// Fill the device buffer from the ring, then wake the pump.
fn fill<T>(
    data: &mut [T],
    scratch: &mut [f32],
    consumer: &mut SampleConsumer,
    channels: usize,
    rate: u32,
    latency: &AtomicU64,
    signal: &RefillSignal,
    info: &cpal::OutputCallbackInfo,
) where
    T: Sample + FromSample<f32>,
{
    let timestamp = info.timestamp();
    if let Some(delay) = timestamp.playback.duration_since(&timestamp.callback) {
        let frames = (delay.as_secs_f64() * f64::from(rate)).round();
        latency.store(frames as u64, Ordering::Relaxed);
    }

    for block in data.chunks_mut(scratch.len()) {
        let frames = block.len() / channels;
        let window = &mut scratch[..frames * channels];
        consumer.pop_or_silence(window);
        for (target, source) in block.iter_mut().zip(window.iter()) {
            *target = T::from_sample(*source);
        }
    }
    signal.request();
}

/// cpal's `Stream` handle is not `Send` on every host, but it is safe to move:
/// `play`/`pause`/`stop` and `Drop` are all called from the thread that owns
/// the sink, and the audio thread is owned by cpal and joined on drop.
struct SendStream(cpal::Stream);

// SAFETY: see above. The handle is never shared: it lives in exactly one
// `CpalSink`, which the supervisor owns for its whole lifetime, so no two
// threads ever touch the native stream behind it.
#[allow(unsafe_code)]
unsafe impl Send for SendStream {}

struct CpalSink {
    descriptor: SinkDescriptor,
    stream: SendStream,
}

impl OutputSink for CpalSink {
    fn descriptor(&self) -> &SinkDescriptor {
        &self.descriptor
    }

    fn start(&mut self) -> Result<(), AudioError> {
        self.stream
            .0
            .play()
            .map_err(|error| AudioError::Device(format!("cannot start stream: {error}")))?;
        Ok(())
    }

    fn pause(&mut self) -> Result<(), AudioError> {
        self.stream
            .0
            .pause()
            .map_err(|error| AudioError::Device(format!("cannot pause stream: {error}")))?;
        Ok(())
    }

    fn resume(&mut self) -> Result<(), AudioError> {
        self.start()
    }

    fn stop(&mut self) {
        let _ = self.stream.0.pause();
    }
}

impl Drop for CpalSink {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Build the typed cpal stream the negotiated format needs.
#[allow(clippy::too_many_arguments)]
fn build_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut consumer: SampleConsumer,
    scratch_frames: usize,
    channels: usize,
    rate: u32,
    latency: Arc<AtomicU64>,
    signal: Arc<RefillSignal>,
    close: Arc<RefillSignal>,
) -> Result<cpal::Stream, cpal::BuildStreamError>
where
    T: Sample + FromSample<f32> + SizedSample,
{
    let mut scratch = vec![0.0f32; scratch_frames * channels];
    device.build_output_stream::<T, _, _>(
        config,
        move |data: &mut [T], info: &cpal::OutputCallbackInfo| {
            fill(
                data,
                &mut scratch,
                &mut consumer,
                channels,
                rate,
                &latency,
                &signal,
                info,
            );
        },
        move |stream_error| {
            // A shared-mode stream error is fatal: the host will not resume
            // it. Closing the refill channel tells the supervisor to rebuild
            // the endpoint instead of letting the ring starve forever.
            tracing::error!(error = %stream_error, "cpal stream failed");
            close.close();
        },
        None,
    )
}

/// Open the endpoint and spawn its callback.
pub fn start(
    negotiation: Negotiation,
    consumer: SampleConsumer,
    refill: Arc<RefillSignal>,
    stats: Arc<RingStats>,
) -> Result<Box<dyn OutputSink>, AudioError> {
    let kind = negotiation.kind;
    let (_host, device) = find_device(kind, Some(&negotiation.device.id))?;
    let name = device
        .name()
        .unwrap_or_else(|_| negotiation.device.name.clone());

    let format = negotiation.format;
    let channels = usize::from(format.channels);
    let config = cpal::StreamConfig {
        channels: format.channels,
        sample_rate: cpal::SampleRate(format.sample_rate),
        buffer_size: cpal::BufferSize::Fixed(negotiation.period_frames.max(32)),
    };

    // A shared endpoint may refuse a fixed buffer size; falling back to the
    // host default is the documented behaviour and keeps audio flowing.
    let config = if device
        .supported_output_configs()
        .ok()
        .and_then(|mut configs| {
            configs.find(|range| {
                to_prism(range.sample_format()) == Some(format.sample_format)
                    && range.min_sample_rate().0 <= format.sample_rate
                    && range.max_sample_rate().0 >= format.sample_rate
            })
        })
        .is_some_and(|range| match range.buffer_size() {
            cpal::SupportedBufferSize::Range { min, max } => {
                (*min..=*max).contains(&negotiation.period_frames)
            }
            cpal::SupportedBufferSize::Unknown => false,
        }) {
        config
    } else {
        cpal::StreamConfig {
            buffer_size: cpal::BufferSize::Default,
            ..config
        }
    };

    let scratch_frames = match config.buffer_size {
        cpal::BufferSize::Fixed(frames) => usize::try_from(frames).unwrap_or(2_048),
        cpal::BufferSize::Default => 2_048,
    }
    .max(2_048);

    let latency = Arc::new(AtomicU64::new(negotiation.latency_frames));
    let signal_cb = Arc::clone(&refill);
    let close_on_error = Arc::clone(&refill);

    let stream = match format.sample_format {
        SampleFormat::S16 => build_stream::<i16>(
            &device,
            &config,
            consumer,
            scratch_frames,
            channels,
            format.sample_rate,
            Arc::clone(&latency),
            Arc::clone(&signal_cb),
            Arc::clone(&close_on_error),
        ),
        SampleFormat::S24 | SampleFormat::S24In32 | SampleFormat::S32 => build_stream::<i32>(
            &device,
            &config,
            consumer,
            scratch_frames,
            channels,
            format.sample_rate,
            Arc::clone(&latency),
            Arc::clone(&signal_cb),
            Arc::clone(&close_on_error),
        ),
        SampleFormat::F32 => build_stream::<f32>(
            &device,
            &config,
            consumer,
            scratch_frames,
            channels,
            format.sample_rate,
            latency,
            signal_cb,
            close_on_error,
        ),
        SampleFormat::F64 => build_stream::<f64>(
            &device,
            &config,
            consumer,
            scratch_frames,
            channels,
            format.sample_rate,
            latency,
            signal_cb,
            close_on_error,
        ),
    }
    .map_err(|error| AudioError::Device(format!("cannot open {name}: {error}")))?;

    tracing::info!(
        device = %name,
        backend = kind.label(),
        format = ?format,
        period = negotiation.period_frames,
        "cpal stream opened"
    );

    Ok(Box::new(CpalSink {
        descriptor: SinkDescriptor {
            device: negotiation.device,
            format,
            period_frames: negotiation.period_frames,
            buffer_frames: negotiation.buffer_frames,
            latency_frames: negotiation.latency_frames,
            stats,
            refill,
        },
        stream: SendStream(stream),
    }))
}
