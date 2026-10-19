//! Output-backend contract shared by every driver stack Prism speaks.
//!
//! | Backend           | Platform | Underlying API                  | Bit-perfect |
//! |-------------------|----------|---------------------------------|-------------|
//! | `WasapiExclusive` | Windows  | `IAudioClient`, exclusive mode  | yes         |
//! | `WasapiShared`    | Windows  | cpal / WASAPI shared            | no          |
//! | `Asio`            | Windows  | cpal ASIO host (`asio` feature) | yes         |
//! | `AlsaHw`          | Linux    | `hw:` PCM, no dmix              | yes         |
//! | `PipeWire`        | Linux    | native `pw_stream`              | no          |
//! | `CpalShared`      | both     | cpal default host               | no          |
//!
//! A backend is split in two halves. [`negotiate`] answers "which format would
//! you give me for this source?" without touching the audio clock, and
//! [`start`] commits: it consumes the ring, spawns the real-time thread and
//! returns a [`OutputSink`] handle. The engine primes the ring in between, so
//! the DAC never starts on an empty buffer.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread::Thread;

use serde::{Deserialize, Serialize};

use crate::audio::AudioError;
use crate::audio::format::{AudioFormat, SampleFormat};
use crate::audio::ring::{RingStats, SampleConsumer, SampleProducer};

/// Which driver stack an endpoint belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BackendKind {
    /// Windows Audio Session API in exclusive mode, driven directly through COM.
    WasapiExclusive,
    /// Windows Audio Session API in shared mode (mixed by the OS).
    WasapiShared,
    /// Steinberg ASIO driver, only when the `asio` feature is enabled.
    Asio,
    /// Raw ALSA PCM (`hw:`), exclusive by construction.
    AlsaHw,
    /// Native PipeWire stream; the graph performs the mixing.
    PipeWire,
    /// cpal's default host: portable last resort.
    CpalShared,
}

impl BackendKind {
    /// Name shown in the device selector and in the status read-out.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::WasapiExclusive => "WASAPI Exclusive",
            Self::WasapiShared => "WASAPI Shared",
            Self::Asio => "ASIO",
            Self::AlsaHw => "ALSA Hardware",
            Self::PipeWire => "PipeWire",
            Self::CpalShared => "System Output",
        }
    }

    /// True when the endpoint owns the device and bypasses the OS mixer.
    #[must_use]
    pub const fn is_exclusive(self) -> bool {
        matches!(self, Self::WasapiExclusive | Self::Asio | Self::AlsaHw)
    }

    /// True when the engine may reopen the device at another sample rate.
    #[must_use]
    pub const fn follows_source_rate(self) -> bool {
        self.is_exclusive()
    }

    /// Stable identifier used by persisted settings.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::WasapiExclusive => "wasapi-exclusive",
            Self::WasapiShared => "wasapi-shared",
            Self::Asio => "asio",
            Self::AlsaHw => "alsa-hw",
            Self::PipeWire => "pipewire",
            Self::CpalShared => "system",
        }
    }
}

/// Endpoint family the engine should target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum OutputMode {
    /// Best exclusive endpoint, falling back to a mixed one.
    #[default]
    Auto,
    /// Force an exclusive endpoint (WASAPI Exclusive, ALSA `hw:`, ASIO).
    Exclusive,
    /// Force a mixed endpoint (WASAPI Shared, PipeWire, system default).
    Shared,
    /// Force the ASIO driver (Windows only).
    Asio,
}

impl OutputMode {
    /// Stable identifier for logs and settings round-trips.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Exclusive => "exclusive",
            Self::Shared => "shared",
            Self::Asio => "asio",
        }
    }
}

/// One output endpoint as reported to the UI.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputDevice {
    /// Opaque handle passed back to `player_set_output`.
    pub id: String,
    /// Endpoint name (WASAPI friendly name, ALSA card description, PW node).
    pub name: String,
    /// Driver stack that will be used.
    pub kind: BackendKind,
    /// True for the endpoint the OS considers default.
    pub is_default: bool,
    /// Maximum channel count the endpoint accepts.
    pub channels: u16,
    /// Sample rates the endpoint can clock, ascending.
    pub sample_rates: Vec<u32>,
    /// Sample formats the endpoint carries, widest first.
    pub sample_formats: Vec<SampleFormat>,
    /// True when choosing this endpoint bypasses the OS mixer.
    pub supports_exclusive: bool,
}

/// The format a backend will really deliver, plus its timing budget.
#[derive(Debug, Clone, PartialEq)]
pub struct Negotiation {
    /// Endpoint that will be opened.
    pub device: OutputDevice,
    /// Driver stack that will be used.
    pub kind: BackendKind,
    /// Exact format the device will run at.
    pub format: AudioFormat,
    /// Frames per device wake-up.
    pub period_frames: u32,
    /// Frames the engine keeps queued ahead of the device (ring capacity).
    pub buffer_frames: u32,
    /// Driver-reported end-to-end latency, in frames.
    pub latency_frames: u64,
}

impl Negotiation {
    /// True when the device runs at exactly the source format, which is the
    /// precondition for a bit-perfect path.
    #[must_use]
    pub fn is_exact(&self, source: &AudioFormat) -> bool {
        self.format == *source
    }
}

/// Static description of an open sink.
#[derive(Debug)]
pub struct SinkDescriptor {
    /// Endpoint the sink owns.
    pub device: OutputDevice,
    /// Format the device clock runs at.
    pub format: AudioFormat,
    /// Frames per device wake-up.
    pub period_frames: u32,
    /// Ring capacity in frames.
    pub buffer_frames: u32,
    /// Driver latency in frames.
    pub latency_frames: u64,
    /// Diagnostics counters shared with the engine.
    pub stats: Arc<RingStats>,
    /// Wake-up channel into the pump thread.
    pub refill: Arc<RefillSignal>,
}

/// Control handle for one open endpoint.
///
/// Implementations own their real-time thread and must close it on `stop`,
/// which also runs from `Drop`.
pub trait OutputSink: Send {
    /// Static description of the sink.
    fn descriptor(&self) -> &SinkDescriptor;

    /// Let the device clock run. Called after the ring has been primed.
    fn start(&mut self) -> Result<(), AudioError>;

    /// Stop consuming frames without closing the device.
    fn pause(&mut self) -> Result<(), AudioError>;

    /// Resume consumption after [`OutputSink::pause`].
    fn resume(&mut self) -> Result<(), AudioError>;

    /// Close the device and join its threads. Idempotent.
    fn stop(&mut self);
}

/// Everything [`open_sink`] produced.
pub struct OpenedSink {
    /// Control handle.
    pub sink: Box<dyn OutputSink>,
    /// Producer half of the ring the sink consumes.
    pub producer: SampleProducer,
    /// Diagnostics counters.
    pub stats: Arc<RingStats>,
    /// Wake-up channel: the pump thread must `attach()` before `start()`.
    pub refill: Arc<RefillSignal>,
    /// Format and timing that were agreed.
    pub negotiation: Negotiation,
}

impl std::fmt::Debug for OpenedSink {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OpenedSink")
            .field("negotiation", &self.negotiation)
            .field("occupied_frames", &self.producer.occupied_frames())
            .finish_non_exhaustive()
    }
}

/// Wake-up channel from the device thread back to the pump.
///
/// The audio callback cannot lock or block, so it only stores a flag and
/// unparks the pump thread. `Thread::unpark` allocates nothing and never
/// blocks, which is what makes it safe inside a real-time callback.
///
/// The same object carries the liveness flag: a driver that fails marks the
/// channel closed, and the supervisor turns that into a device-recovery path
/// instead of an endless underrun storm.
#[derive(Debug, Default)]
pub struct RefillSignal {
    thread: OnceLock<Thread>,
    requested: AtomicBool,
    open: AtomicBool,
}

impl RefillSignal {
    /// A fresh, open channel.
    #[must_use]
    pub fn new() -> Self {
        Self {
            thread: OnceLock::new(),
            requested: AtomicBool::new(false),
            open: AtomicBool::new(true),
        }
    }

    /// Register the pump thread. Must be called before the device starts.
    pub fn attach(&self) {
        let _ = self.thread.set(std::thread::current());
    }

    /// Ask the pump for more frames. Safe to call from an audio callback.
    pub fn request(&self) {
        self.requested.store(true, Ordering::Release);
        if let Some(thread) = self.thread.get() {
            thread.unpark();
        }
    }

    /// Consume a pending request.
    pub fn take(&self) -> bool {
        self.requested.swap(false, Ordering::AcqRel)
    }

    /// Mark the sink as dead (device lost, driver crash, unrecoverable xrun).
    pub fn close(&self) {
        self.open.store(false, Ordering::Release);
        if let Some(thread) = self.thread.get() {
            thread.unpark();
        }
    }

    /// False once the sink reported a fatal error.
    pub fn is_open(&self) -> bool {
        self.open.load(Ordering::Acquire)
    }
}

/// Enumerate every endpoint reachable through `mode`, best first.
pub fn enumerate_devices(mode: OutputMode) -> Result<Vec<OutputDevice>, AudioError> {
    let mut devices = Vec::new();
    let mut last_error = None;

    for kind in candidates(mode, None) {
        match enumerate_kind(kind) {
            Ok(mut list) => devices.append(&mut list),
            Err(error) => last_error = Some(error),
        }
    }

    // Deduplicate by endpoint id, keeping the first (best) occurrence.
    let mut seen = std::collections::HashSet::new();
    devices.retain(|device| seen.insert(device.id.clone()));

    if devices.is_empty() {
        if let Some(error) = last_error {
            return Err(error);
        }
        return Err(AudioError::NoDevice);
    }
    Ok(devices)
}

fn enumerate_kind(kind: BackendKind) -> Result<Vec<OutputDevice>, AudioError> {
    match kind {
        BackendKind::WasapiExclusive => platform::wasapi_exclusive(),
        BackendKind::WasapiShared | BackendKind::CpalShared => platform::cpal(kind),
        BackendKind::Asio => platform::asio(),
        BackendKind::AlsaHw => platform::alsa(),
        BackendKind::PipeWire => platform::pipewire(),
    }
}

/// Open the best endpoint available for `mode` and start its clock.
///
/// Every candidate is tried in order; the first that negotiates and opens
/// wins. A failure in `Auto` therefore degrades gracefully from exclusive to
/// mixed output instead of leaving the user without sound.
pub fn open_sink(
    mode: OutputMode,
    device: Option<&str>,
    desired: &AudioFormat,
    buffer_frames: u32,
) -> Result<OpenedSink, AudioError> {
    let mut last_error = None;
    for kind in candidates(mode, device) {
        let negotiation = match negotiate(kind, device, desired, buffer_frames) {
            Ok(negotiation) => negotiation,
            Err(error) => {
                tracing::debug!(backend = kind.label(), error = %error, "backend declined the format");
                last_error = Some(error);
                continue;
            }
        };

        let capacity = negotiation.buffer_frames.max(negotiation.period_frames * 2) as usize;
        let (producer, consumer, stats) =
            crate::audio::ring::create_ring(capacity, usize::from(negotiation.format.channels));
        let refill = Arc::new(RefillSignal::new());
        let agreed = negotiation.clone();

        match start(
            kind,
            negotiation,
            consumer,
            Arc::clone(&refill),
            Arc::clone(&stats),
        ) {
            Ok(sink) => {
                return Ok(OpenedSink {
                    sink,
                    producer,
                    stats,
                    refill,
                    negotiation: agreed,
                });
            }
            Err(error) => {
                tracing::warn!(backend = kind.label(), error = %error, "backend failed to open");
                last_error = Some(error);
            }
        }
    }
    Err(last_error.unwrap_or(AudioError::NoDevice))
}

fn negotiate(
    kind: BackendKind,
    device: Option<&str>,
    desired: &AudioFormat,
    buffer_frames: u32,
) -> Result<Negotiation, AudioError> {
    match kind {
        BackendKind::WasapiExclusive => {
            platform::negotiate_wasapi_exclusive(device, desired, buffer_frames)
        }
        BackendKind::WasapiShared | BackendKind::CpalShared => {
            platform::negotiate_cpal(kind, device, desired, buffer_frames)
        }
        BackendKind::Asio => platform::negotiate_asio(device, desired, buffer_frames),
        BackendKind::AlsaHw => platform::negotiate_alsa(device, desired, buffer_frames),
        BackendKind::PipeWire => platform::negotiate_pipewire(device, desired, buffer_frames),
    }
}

fn start(
    kind: BackendKind,
    negotiation: Negotiation,
    consumer: SampleConsumer,
    refill: Arc<RefillSignal>,
    stats: Arc<RingStats>,
) -> Result<Box<dyn OutputSink>, AudioError> {
    match kind {
        BackendKind::WasapiExclusive => {
            platform::start_wasapi_exclusive(negotiation, consumer, refill, stats)
        }
        BackendKind::WasapiShared | BackendKind::CpalShared => {
            platform::start_cpal(negotiation, consumer, refill, stats)
        }
        BackendKind::Asio => platform::start_asio(negotiation, consumer, refill, stats),
        BackendKind::AlsaHw => platform::start_alsa(negotiation, consumer, refill, stats),
        BackendKind::PipeWire => platform::start_pipewire(negotiation, consumer, refill, stats),
    }
}

/// Backend preference order for a mode, before any endpoint is chosen.
#[must_use]
pub fn preferred_kinds(mode: OutputMode) -> Vec<BackendKind> {
    candidates(mode, None)
}

/// Backend preference order for a mode, honouring the endpoint the user chose.
fn candidates(mode: OutputMode, device: Option<&str>) -> Vec<BackendKind> {
    // An explicit endpoint pins the driver stack: choosing an `alsa-hw:` id
    // must never silently open PipeWire instead.
    if let Some(device) = device
        && let Some(kind) = kind_of(device)
    {
        return vec![kind];
    }

    let preferred = |kind: BackendKind| -> Vec<BackendKind> {
        if platform::is_available(kind) {
            vec![kind]
        } else {
            Vec::new()
        }
    };

    match mode {
        OutputMode::Exclusive => {
            let mut order = platform::exclusive_kinds();
            order.extend(preferred(BackendKind::CpalShared));
            order
        }
        OutputMode::Shared => {
            let mut order = platform::shared_kinds();
            order.extend(preferred(BackendKind::CpalShared));
            order
        }
        OutputMode::Asio => {
            let mut order = preferred(BackendKind::Asio);
            order.extend(platform::exclusive_kinds());
            order
        }
        OutputMode::Auto => {
            let mut order = platform::exclusive_kinds();
            order.extend(platform::shared_kinds());
            order.extend(preferred(BackendKind::CpalShared));
            order
        }
    }
}

/// Driver stack named by an endpoint id (`<backend-id>:<name>`).
#[must_use]
pub fn kind_of(device_id: &str) -> Option<BackendKind> {
    let prefix = device_id
        .split_once(':')
        .map_or(device_id, |(prefix, _)| prefix);
    [
        BackendKind::WasapiExclusive,
        BackendKind::WasapiShared,
        BackendKind::Asio,
        BackendKind::AlsaHw,
        BackendKind::PipeWire,
        BackendKind::CpalShared,
    ]
    .into_iter()
    .find(|kind| kind.id() == prefix)
}

/// Sample-format preference when the exact source container is unavailable.
///
/// Widest integer first: a 24-bit master must never be truncated to 16 bits
/// just because the device also accepts 16-bit.
#[must_use]
pub fn format_rank(format: SampleFormat) -> u8 {
    match format {
        SampleFormat::S32 => 0,
        SampleFormat::S24In32 => 1,
        SampleFormat::S24 => 2,
        SampleFormat::F32 => 3,
        SampleFormat::F64 => 4,
        SampleFormat::S16 => 5,
    }
}

/// Default sample rates probed from an endpoint that reports ranges only.
#[must_use]
pub fn common_rates(max: u32) -> Vec<u32> {
    [
        44_100, 48_000, 88_200, 96_000, 176_400, 192_000, 352_800, 384_000, 705_600, 768_000,
    ]
    .into_iter()
    .filter(|rate| *rate <= max)
    .collect()
}

#[cfg(windows)]
mod platform {
    use super::*;
    use crate::audio::engine::{cpal_out, wasapi_excl};

    pub(super) fn is_available(kind: BackendKind) -> bool {
        match kind {
            BackendKind::Asio => cfg!(feature = "asio"),
            _ => true,
        }
    }

    pub(super) fn exclusive_kinds() -> Vec<BackendKind> {
        vec![BackendKind::WasapiExclusive]
    }

    pub(super) fn shared_kinds() -> Vec<BackendKind> {
        vec![BackendKind::WasapiShared]
    }

    pub(super) fn wasapi_exclusive() -> Result<Vec<OutputDevice>, AudioError> {
        wasapi_excl::enumerate()
    }

    pub(super) fn cpal(kind: BackendKind) -> Result<Vec<OutputDevice>, AudioError> {
        cpal_out::enumerate(kind)
    }

    pub(super) fn asio() -> Result<Vec<OutputDevice>, AudioError> {
        cpal_out::enumerate(BackendKind::Asio)
    }

    pub(super) fn alsa() -> Result<Vec<OutputDevice>, AudioError> {
        Err(AudioError::Device("ALSA is only available on Linux".into()))
    }

    pub(super) fn pipewire() -> Result<Vec<OutputDevice>, AudioError> {
        Err(AudioError::Device(
            "PipeWire is only available on Linux".into(),
        ))
    }

    pub(super) fn negotiate_wasapi_exclusive(
        device: Option<&str>,
        desired: &AudioFormat,
        buffer_frames: u32,
    ) -> Result<Negotiation, AudioError> {
        wasapi_excl::negotiate(device, desired, buffer_frames)
    }

    pub(super) fn negotiate_cpal(
        kind: BackendKind,
        device: Option<&str>,
        desired: &AudioFormat,
        buffer_frames: u32,
    ) -> Result<Negotiation, AudioError> {
        cpal_out::negotiate(kind, device, desired, buffer_frames)
    }

    pub(super) fn negotiate_asio(
        device: Option<&str>,
        desired: &AudioFormat,
        buffer_frames: u32,
    ) -> Result<Negotiation, AudioError> {
        cpal_out::negotiate(BackendKind::Asio, device, desired, buffer_frames)
    }

    pub(super) fn negotiate_alsa(
        _device: Option<&str>,
        _desired: &AudioFormat,
        _buffer_frames: u32,
    ) -> Result<Negotiation, AudioError> {
        Err(AudioError::Device("ALSA is only available on Linux".into()))
    }

    pub(super) fn negotiate_pipewire(
        _device: Option<&str>,
        _desired: &AudioFormat,
        _buffer_frames: u32,
    ) -> Result<Negotiation, AudioError> {
        Err(AudioError::Device(
            "PipeWire is only available on Linux".into(),
        ))
    }

    pub(super) fn start_wasapi_exclusive(
        negotiation: Negotiation,
        consumer: SampleConsumer,
        refill: Arc<RefillSignal>,
        stats: Arc<RingStats>,
    ) -> Result<Box<dyn OutputSink>, AudioError> {
        wasapi_excl::start(negotiation, consumer, refill, stats)
    }

    pub(super) fn start_cpal(
        negotiation: Negotiation,
        consumer: SampleConsumer,
        refill: Arc<RefillSignal>,
        stats: Arc<RingStats>,
    ) -> Result<Box<dyn OutputSink>, AudioError> {
        cpal_out::start(negotiation, consumer, refill, stats)
    }

    pub(super) fn start_asio(
        negotiation: Negotiation,
        consumer: SampleConsumer,
        refill: Arc<RefillSignal>,
        stats: Arc<RingStats>,
    ) -> Result<Box<dyn OutputSink>, AudioError> {
        cpal_out::start(negotiation, consumer, refill, stats)
    }

    pub(super) fn start_alsa(
        _negotiation: Negotiation,
        _consumer: SampleConsumer,
        _refill: Arc<RefillSignal>,
        _stats: Arc<RingStats>,
    ) -> Result<Box<dyn OutputSink>, AudioError> {
        Err(AudioError::Device("ALSA is only available on Linux".into()))
    }

    pub(super) fn start_pipewire(
        _negotiation: Negotiation,
        _consumer: SampleConsumer,
        _refill: Arc<RefillSignal>,
        _stats: Arc<RingStats>,
    ) -> Result<Box<dyn OutputSink>, AudioError> {
        Err(AudioError::Device(
            "PipeWire is only available on Linux".into(),
        ))
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use crate::audio::engine::{alsa_out, cpal_out, pipewire_out};

    pub(super) fn is_available(kind: BackendKind) -> bool {
        match kind {
            BackendKind::PipeWire => pipewire_out::is_available(),
            _ => true,
        }
    }

    pub(super) fn exclusive_kinds() -> Vec<BackendKind> {
        vec![BackendKind::AlsaHw]
    }

    pub(super) fn shared_kinds() -> Vec<BackendKind> {
        if pipewire_out::is_available() {
            vec![BackendKind::PipeWire, BackendKind::CpalShared]
        } else {
            vec![BackendKind::CpalShared]
        }
    }

    pub(super) fn alsa() -> Result<Vec<OutputDevice>, AudioError> {
        alsa_out::enumerate()
    }

    pub(super) fn pipewire() -> Result<Vec<OutputDevice>, AudioError> {
        pipewire_out::default_device().map(|device| device.into_iter().collect())
    }

    pub(super) fn cpal(kind: BackendKind) -> Result<Vec<OutputDevice>, AudioError> {
        cpal_out::enumerate(kind)
    }

    pub(super) fn wasapi_exclusive() -> Result<Vec<OutputDevice>, AudioError> {
        Err(AudioError::Device(
            "WASAPI is only available on Windows".into(),
        ))
    }

    pub(super) fn asio() -> Result<Vec<OutputDevice>, AudioError> {
        Err(AudioError::Device(
            "ASIO is only available on Windows".into(),
        ))
    }

    pub(super) fn negotiate_alsa(
        device: Option<&str>,
        desired: &AudioFormat,
        buffer_frames: u32,
    ) -> Result<Negotiation, AudioError> {
        alsa_out::negotiate(device, desired, buffer_frames)
    }

    pub(super) fn negotiate_pipewire(
        device: Option<&str>,
        desired: &AudioFormat,
        buffer_frames: u32,
    ) -> Result<Negotiation, AudioError> {
        pipewire_out::negotiate(device, desired, buffer_frames)
    }

    pub(super) fn negotiate_cpal(
        kind: BackendKind,
        device: Option<&str>,
        desired: &AudioFormat,
        buffer_frames: u32,
    ) -> Result<Negotiation, AudioError> {
        cpal_out::negotiate(kind, device, desired, buffer_frames)
    }

    pub(super) fn negotiate_wasapi_exclusive(
        _device: Option<&str>,
        _desired: &AudioFormat,
        _buffer_frames: u32,
    ) -> Result<Negotiation, AudioError> {
        Err(AudioError::Device(
            "WASAPI is only available on Windows".into(),
        ))
    }

    pub(super) fn negotiate_asio(
        _device: Option<&str>,
        _desired: &AudioFormat,
        _buffer_frames: u32,
    ) -> Result<Negotiation, AudioError> {
        Err(AudioError::Device(
            "ASIO is only available on Windows".into(),
        ))
    }

    pub(super) fn start_alsa(
        negotiation: Negotiation,
        consumer: SampleConsumer,
        refill: Arc<RefillSignal>,
        stats: Arc<RingStats>,
    ) -> Result<Box<dyn OutputSink>, AudioError> {
        alsa_out::start(negotiation, consumer, refill, stats)
    }

    pub(super) fn start_pipewire(
        negotiation: Negotiation,
        consumer: SampleConsumer,
        refill: Arc<RefillSignal>,
        stats: Arc<RingStats>,
    ) -> Result<Box<dyn OutputSink>, AudioError> {
        pipewire_out::start(negotiation, consumer, refill, stats)
    }

    pub(super) fn start_cpal(
        negotiation: Negotiation,
        consumer: SampleConsumer,
        refill: Arc<RefillSignal>,
        stats: Arc<RingStats>,
    ) -> Result<Box<dyn OutputSink>, AudioError> {
        cpal_out::start(negotiation, consumer, refill, stats)
    }

    pub(super) fn start_wasapi_exclusive(
        _negotiation: Negotiation,
        _consumer: SampleConsumer,
        _refill: Arc<RefillSignal>,
        _stats: Arc<RingStats>,
    ) -> Result<Box<dyn OutputSink>, AudioError> {
        Err(AudioError::Device(
            "WASAPI is only available on Windows".into(),
        ))
    }

    pub(super) fn start_asio(
        _negotiation: Negotiation,
        _consumer: SampleConsumer,
        _refill: Arc<RefillSignal>,
        _stats: Arc<RingStats>,
    ) -> Result<Box<dyn OutputSink>, AudioError> {
        Err(AudioError::Device(
            "ASIO is only available on Windows".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_ids_round_trip_to_their_backend() {
        assert_eq!(
            kind_of("alsa-hw:hw:CARD=PCH,DEV=0"),
            Some(BackendKind::AlsaHw)
        );
        assert_eq!(
            kind_of("wasapi-exclusive:Speakers"),
            Some(BackendKind::WasapiExclusive)
        );
        assert_eq!(kind_of("pipewire:default"), Some(BackendKind::PipeWire));
        assert_eq!(kind_of("asio:Focusrite USB ASIO"), Some(BackendKind::Asio));
        assert_eq!(kind_of("system:Default"), Some(BackendKind::CpalShared));
        assert_eq!(kind_of("nonsense"), None);
        // A device name containing colons must still resolve.
        assert_eq!(
            kind_of("alsa-hw:hw:CARD=Pro,DEV=1"),
            Some(BackendKind::AlsaHw)
        );
    }

    #[test]
    fn exclusive_backends_are_the_only_ones_following_the_source_clock() {
        for kind in [
            BackendKind::WasapiExclusive,
            BackendKind::Asio,
            BackendKind::AlsaHw,
        ] {
            assert!(kind.is_exclusive());
            assert!(kind.follows_source_rate());
        }
        for kind in [
            BackendKind::WasapiShared,
            BackendKind::PipeWire,
            BackendKind::CpalShared,
        ] {
            assert!(!kind.is_exclusive());
            assert!(!kind.follows_source_rate());
        }
    }

    #[test]
    fn refill_signal_is_edge_triggered_and_close_is_sticky() {
        let signal = RefillSignal::new();
        assert!(!signal.take(), "a fresh channel has nothing pending");

        signal.request();
        signal.request();
        assert!(signal.take(), "requests coalesce into one wake-up");
        assert!(!signal.take(), "the request was consumed exactly once");

        assert!(signal.is_open());
        signal.close();
        assert!(!signal.is_open(), "a fatal driver error is permanent");
    }

    #[test]
    fn format_rank_prefers_the_widest_integer_container() {
        let mut formats = vec![
            SampleFormat::S16,
            SampleFormat::F32,
            SampleFormat::S32,
            SampleFormat::S24In32,
        ];
        formats.sort_by_key(|format| format_rank(*format));
        assert_eq!(
            formats,
            vec![
                SampleFormat::S32,
                SampleFormat::S24In32,
                SampleFormat::F32,
                SampleFormat::S16
            ]
        );
    }

    #[test]
    fn negotiation_is_exact_only_on_a_full_format_match() {
        let device = OutputDevice {
            id: "alsa-hw:hw:0".into(),
            name: "Test DAC".into(),
            kind: BackendKind::AlsaHw,
            is_default: false,
            channels: 2,
            sample_rates: vec![44_100, 48_000],
            sample_formats: vec![SampleFormat::S24In32],
            supports_exclusive: true,
        };
        let negotiation = Negotiation {
            device,
            kind: BackendKind::AlsaHw,
            format: AudioFormat {
                sample_rate: 44_100,
                channels: 2,
                sample_format: SampleFormat::S24In32,
            },
            period_frames: 512,
            buffer_frames: 2_048,
            latency_frames: 1_024,
        };

        assert!(negotiation.is_exact(&AudioFormat {
            sample_rate: 44_100,
            channels: 2,
            sample_format: SampleFormat::S24In32,
        }));
        assert!(!negotiation.is_exact(&AudioFormat {
            sample_rate: 48_000,
            ..negotiation.format
        }));
        assert!(!negotiation.is_exact(&AudioFormat {
            sample_format: SampleFormat::S16,
            ..negotiation.format
        }));
    }

    #[test]
    fn common_rates_never_exceed_the_endpoint_ceiling() {
        let rates = common_rates(96_000);
        assert!(rates.contains(&44_100));
        assert!(rates.contains(&96_000));
        assert!(!rates.contains(&176_400));
        assert_eq!(common_rates(0), Vec::<u32>::new());
    }
}
