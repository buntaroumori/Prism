//! WASAPI exclusive-mode output via the raw COM interfaces.
//!
//! cpal only speaks shared mode (its WASAPI host always passes
//! `AUDCLNT_SHAREMODE_SHARED`), so the audiophile path is implemented here:
//! `IMMDeviceEnumerator` → `IMMDevice` → `IAudioClient` → `IAudioRenderClient`,
//! event-driven, in exclusive mode. Nothing between the file and the DAC
//! touches the samples — no APO, no mixer, no OS resampler.
//!
//! Container policy: WASAPI accepts 16-bit, 32-bit integer and 32/64-bit float.
//! Packed 24-bit (`S24`) is widened to a 32-bit container with
//! `wValidBitsPerSample = 24` and the sample left-justified, which is exactly
//! what the `f32 → S32` conversion produces (`value × 2³¹ = sample << 8`), so
//! 24-bit sources stay bit-exact.
//!
//! Everything in this module is `cfg(windows)`; all COM calls are wrapped in
//! `unsafe` blocks with the invariants they rely on.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::{
    AUDCLNT_SHAREMODE_EXCLUSIVE, AUDCLNT_STREAMFLAGS_EVENTCALLBACK, DEVICE_STATE_ACTIVE,
    IAudioClient, IAudioRenderClient, IMMDevice, IMMDeviceEnumerator, WAVEFORMATEX,
    WAVEFORMATEXTENSIBLE, eConsole, eRender,
};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize, STGM_READ,
};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY;
use windows::core::{GUID, PCWSTR, PWSTR};

use crate::audio::AudioError;
use crate::audio::engine::backend::{
    BackendKind, Negotiation, OutputDevice, OutputSink, RefillSignal, SinkDescriptor,
};
use crate::audio::format::{AudioFormat, SampleFormat, write_samples_f32};
use crate::audio::ring::{RingStats, SampleConsumer};

/// `KSDATAFORMAT_SUBTYPE_PCM` — `{00000001-0000-0010-8000-00AA00389B71}`.
const SUBTYPE_PCM: GUID = GUID::from_u128(0x0000_0001_0000_0010_8000_00aa_0038_9b71);
/// `KSDATAFORMAT_SUBTYPE_IEEE_FLOAT` — `{00000003-0000-0010-8000-00AA00389B71}`.
const SUBTYPE_IEEE_FLOAT: GUID = GUID::from_u128(0x0000_0003_0000_0010_8000_00aa_0038_9b71);
/// `PKEY_Device_FriendlyName`.
const PKEY_DEVICE_FRIENDLY_NAME: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0xa45c_254e_df1c_4efd_8020_67d1_46a8_50e0),
    pid: 14,
};
/// `WAVE_FORMAT_EXTENSIBLE`.
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;
/// `CLSID_MMDeviceEnumerator` — `{BCDE0395-E52F-467C-8E3D-C4579291692E}`.
const CLSID_MMDEVICE_ENUMERATOR: GUID = GUID::from_u128(0xBCDE_0395_E52F_467C_8E3D_C457_9291_692E);

const CANONICAL_RATES: [u32; 10] = [
    44_100, 48_000, 88_200, 96_000, 176_400, 192_000, 352_800, 384_000, 705_600, 768_000,
];

fn channel_mask(channels: u16) -> u32 {
    match channels {
        1 => 0x4,
        2 => 0x3,
        3 => 0x7,
        4 => 0x33,
        5 => 0x37,
        6 => 0x3F,
        7 => 0x13F,
        8 => 0x63F,
        _ => 0,
    }
}

/// One `WAVEFORMATEXTENSIBLE` plus the engine format it describes.
struct WaveFormat {
    extensible: WAVEFORMATEXTENSIBLE,
    format: AudioFormat,
}

impl WaveFormat {
    /// Build the structure WASAPI needs for a given engine format.
    ///
    /// Returns `None` for formats WASAPI cannot represent (packed 24-bit is
    /// widened to 32-bit before calling this).
    fn build(format: &AudioFormat) -> Result<Self, AudioError> {
        let (bits, valid_bits, subtype) = match format.sample_format {
            SampleFormat::S16 => (16u16, 16u16, SUBTYPE_PCM),
            SampleFormat::S24 | SampleFormat::S24In32 | SampleFormat::S32 => (
                32,
                format.sample_format.display_bits().min(32) as u16,
                SUBTYPE_PCM,
            ),
            SampleFormat::F32 => (32, 32, SUBTYPE_IEEE_FLOAT),
            SampleFormat::F64 => (64, 64, SUBTYPE_IEEE_FLOAT),
        };
        let block_align = format.channels * (bits / 8);
        let mut extensible: WAVEFORMATEXTENSIBLE = unsafe { std::mem::zeroed() };
        extensible.Format.wFormatTag = WAVE_FORMAT_EXTENSIBLE;
        extensible.Format.nChannels = format.channels;
        extensible.Format.nSamplesPerSec = format.sample_rate;
        extensible.Format.wBitsPerSample = bits;
        extensible.Format.nBlockAlign = block_align;
        extensible.Format.nAvgBytesPerSec = format.sample_rate * u32::from(block_align);
        extensible.Format.cbSize = 22;
        extensible.Samples.wValidBitsPerSample = valid_bits;
        extensible.dwChannelMask = channel_mask(format.channels);
        extensible.SubFormat = subtype;

        let container = match format.sample_format {
            SampleFormat::S16 => SampleFormat::S16,
            SampleFormat::F32 => SampleFormat::F32,
            SampleFormat::F64 => SampleFormat::F64,
            _ => SampleFormat::S32,
        };
        Ok(Self {
            extensible,
            format: AudioFormat {
                sample_rate: format.sample_rate,
                channels: format.channels,
                sample_format: container,
            },
        })
    }

    fn as_ptr(&self) -> *const WAVEFORMATEX {
        std::ptr::addr_of!(self.extensible.Format)
    }
}

fn com_init() -> Result<(), AudioError> {
    // SAFETY: `None` means "no reserved pointer"; the call is idempotent per
    // thread and returns S_FALSE when COM is already initialised.
    let result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if result.is_err() {
        return Err(AudioError::Device(format!(
            "cannot initialise COM on the audio thread: {result:?}"
        )));
    }
    Ok(())
}

struct ComGuard;

impl ComGuard {
    fn new() -> Result<Self, AudioError> {
        com_init()?;
        Ok(Self)
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        // SAFETY: balances the successful `CoInitializeEx` in `new`.
        unsafe { CoUninitialize() };
    }
}

fn enumerator() -> Result<IMMDeviceEnumerator, AudioError> {
    // SAFETY: COM is initialised by the caller; the CLSID is the documented
    // MMDeviceEnumerator class and no aggregation/outer object is used.
    unsafe { CoCreateInstance(&CLSID_MMDEVICE_ENUMERATOR, None, CLSCTX_ALL) }.map_err(|error| {
        AudioError::Device(format!("cannot create the device enumerator: {error}"))
    })
}

fn friendly_name(device: &IMMDevice) -> Option<String> {
    // SAFETY: the device is alive; STGM_READ requests read-only access.
    let store = unsafe { device.OpenPropertyStore(STGM_READ) }.ok()?;
    // SAFETY: the key is a valid static PROPERTYKEY.
    let value = unsafe { store.GetValue(&PKEY_DEVICE_FRIENDLY_NAME) }.ok()?;
    let name = value.to_string();
    (!name.is_empty()).then_some(name)
}

fn endpoint_id(device: &IMMDevice) -> Option<String> {
    // SAFETY: the returned PWSTR is owned by COM; we copy it immediately.
    let raw: PWSTR = unsafe { device.GetId() }.ok()?;
    Some(unsafe { raw.to_string() }.ok()?)
}

fn default_device_id(enumerator: &IMMDeviceEnumerator) -> Option<String> {
    // SAFETY: enumerator is alive; eRender/eConsole is the documented pair for
    // the system default playback endpoint.
    let device = unsafe { enumerator.GetDefaultAudioEndpoint(eRender, eConsole) }.ok()?;
    endpoint_id(&device)
}

/// Active exclusive-mode endpoints.
pub fn enumerate() -> Result<Vec<OutputDevice>, AudioError> {
    let _com = ComGuard::new()?;
    let enumerator = enumerator()?;
    let default_id = default_device_id(&enumerator);

    // SAFETY: enumerator is alive; DEVICE_STATE_ACTIVE filters unplugged nodes.
    let collection = unsafe { enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) }
        .map_err(|error| AudioError::Device(format!("cannot enumerate endpoints: {error}")))?;
    // SAFETY: collection is alive.
    let count = unsafe { collection.GetCount() }
        .map_err(|error| AudioError::Device(format!("cannot count endpoints: {error}")))?;

    let mut devices = Vec::new();
    for index in 0..count {
        // SAFETY: index is bounded by GetCount.
        let Ok(device) = (unsafe { collection.Item(index) }) else {
            continue;
        };
        let Some(id) = endpoint_id(&device) else {
            continue;
        };
        let name = friendly_name(&device).unwrap_or_else(|| id.clone());
        devices.push(OutputDevice {
            id: format!("wasapi:{id}"),
            name,
            kind: BackendKind::WasapiExclusive,
            is_default: default_id.as_deref() == Some(id.as_str()),
            channels: 8,
            sample_rates: CANONICAL_RATES.to_vec(),
            sample_formats: vec![
                SampleFormat::S16,
                SampleFormat::S24,
                SampleFormat::S32,
                SampleFormat::F32,
            ],
            supports_exclusive: true,
        });
    }
    Ok(devices)
}

fn open_device(device_id: Option<&str>) -> Result<IMMDevice, AudioError> {
    let enumerator = enumerator()?;
    match device_id.map(|id| id.trim_start_matches("wasapi:")) {
        Some(id) if !id.is_empty() => {
            // SAFETY: enumerator is alive.
            let collection = unsafe { enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) }
                .map_err(|error| {
                    AudioError::Device(format!("cannot enumerate endpoints: {error}"))
                })?;
            // SAFETY: collection is alive.
            let count = unsafe { collection.GetCount() }
                .map_err(|error| AudioError::Device(format!("cannot count endpoints: {error}")))?;
            for index in 0..count {
                // SAFETY: index is bounded by GetCount.
                let Ok(candidate) = (unsafe { collection.Item(index) }) else {
                    continue;
                };
                if endpoint_id(&candidate).as_deref() == Some(id) {
                    return Ok(candidate);
                }
            }
            Err(AudioError::Device(format!(
                "Windows audio endpoint {id} is no longer present"
            )))
        }
        // SAFETY: eRender/eConsole is the documented default playback pair.
        _ => unsafe { enumerator.GetDefaultAudioEndpoint(eRender, eConsole) }
            .map_err(|error| AudioError::Device(format!("no default playback endpoint: {error}"))),
    }
}

fn activate(device: &IMMDevice) -> Result<IAudioClient, AudioError> {
    // SAFETY: CLSCTX_ALL lets COM pick the in-process server; no activation
    // parameters are needed for IAudioClient.
    unsafe { device.Activate::<IAudioClient>(CLSCTX_ALL, None) }
        .map_err(|error| AudioError::Device(format!("cannot activate IAudioClient: {error}")))
}

/// Exact-format probe, without opening a stream.
fn try_format(
    enumerator_device: &IMMDevice,
    format: &AudioFormat,
) -> Result<WaveFormat, AudioError> {
    let client = activate(enumerator_device)?;
    let wave = WaveFormat::build(format)?;
    // SAFETY: `wave` outlives the call and is a valid WAVEFORMATEXTENSIBLE;
    // exclusive mode is what we are probing.
    let result =
        unsafe { client.IsFormatSupported(AUDCLNT_SHAREMODE_EXCLUSIVE, wave.as_ptr(), None) };
    if result.0 != 0 {
        return Err(AudioError::UnsupportedByDevice(format!(
            "{} / {} Hz / {} ch is not accepted in exclusive mode (0x{:08X})",
            format.sample_format.label(),
            format.sample_rate,
            format.channels,
            result.0
        )));
    }
    Ok(wave)
}

/// Probe the endpoint and report the exclusive stream it will accept.
pub fn negotiate(
    device_id: Option<&str>,
    desired: &AudioFormat,
    period_frames: u32,
) -> Result<Negotiation, AudioError> {
    let _com = ComGuard::new()?;
    let device = open_device(device_id)?;

    // Candidate order: the source format, then the same rate with wider
    // containers, then float. Packed 24-bit is always widened by `build`.
    let mut candidates = vec![*desired];
    if desired.sample_format != SampleFormat::S32 {
        candidates.push(AudioFormat {
            sample_format: SampleFormat::S32,
            ..*desired
        });
    }
    if desired.sample_format != SampleFormat::F32 {
        candidates.push(AudioFormat {
            sample_format: SampleFormat::F32,
            ..*desired
        });
    }

    let mut last_error = None;
    for candidate in candidates {
        match try_format(&device, &candidate) {
            Ok(wave) => {
                let device_info = describe(&device, device_id)?;
                let period = period_frames.clamp(64, 4096);
                return Ok(Negotiation {
                    device: device_info,
                    kind: BackendKind::WasapiExclusive,
                    format: wave.format,
                    period_frames: period,
                    buffer_frames: period,
                    // Exclusive event-driven streams run one period behind.
                    latency_frames: u64::from(period),
                });
            }
            Err(error) => last_error = Some(error),
        }
    }

    Err(last_error.unwrap_or_else(|| {
        AudioError::UnsupportedByDevice("no exclusive-mode format matched".into())
    }))
}

fn describe(device: &IMMDevice, requested: Option<&str>) -> Result<OutputDevice, AudioError> {
    let id = endpoint_id(device).ok_or_else(|| {
        AudioError::Device("the audio endpoint has no persistent identifier".into())
    })?;
    Ok(OutputDevice {
        id: format!("wasapi:{id}"),
        name: friendly_name(device).unwrap_or_else(|| id.clone()),
        kind: BackendKind::WasapiExclusive,
        is_default: requested.is_none(),
        channels: 8,
        sample_rates: CANONICAL_RATES.to_vec(),
        sample_formats: vec![
            SampleFormat::S16,
            SampleFormat::S24,
            SampleFormat::S32,
            SampleFormat::F32,
        ],
        supports_exclusive: true,
    })
}

#[derive(Debug)]
enum Control {
    Pause,
    Resume,
    Stop,
}

/// Control handle for the WASAPI render thread.
pub struct WasapiSink {
    control: Option<crossbeam_channel::Sender<Control>>,
    worker: Option<std::thread::JoinHandle<()>>,
    paused: Arc<AtomicBool>,
    descriptor: SinkDescriptor,
}

impl OutputSink for WasapiSink {
    fn descriptor(&self) -> &SinkDescriptor {
        &self.descriptor
    }

    fn start(&mut self) -> Result<(), AudioError> {
        self.paused.store(false, Ordering::Release);
        if let Some(control) = &self.control {
            let _ = control.send(Control::Resume);
        }
        Ok(())
    }

    fn pause(&mut self) -> Result<(), AudioError> {
        self.paused.store(true, Ordering::Release);
        if let Some(control) = &self.control {
            let _ = control.send(Control::Pause);
        }
        Ok(())
    }

    fn resume(&mut self) -> Result<(), AudioError> {
        self.paused.store(false, Ordering::Release);
        if let Some(control) = &self.control {
            let _ = control.send(Control::Resume);
        }
        Ok(())
    }

    fn stop(&mut self) {
        if let Some(control) = &self.control {
            let _ = control.send(Control::Stop);
        }
        self.control = None;
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for WasapiSink {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Open the exclusive stream and start the render thread.
pub fn start(
    negotiation: Negotiation,
    consumer: SampleConsumer,
    refill: Arc<RefillSignal>,
    stats: Arc<RingStats>,
) -> Result<Box<dyn OutputSink>, AudioError> {
    let (control_tx, control_rx) = crossbeam_channel::bounded::<Control>(8);
    let (ready_tx, ready_rx) = crossbeam_channel::bounded::<Result<(), AudioError>>(1);
    let paused = Arc::new(AtomicBool::new(false));

    let device_id = negotiation.device.id.clone();
    let format = negotiation.format;
    let period = negotiation.period_frames;
    let worker_refill = Arc::clone(&refill);
    let worker_paused = Arc::clone(&paused);

    let worker = std::thread::Builder::new()
        .name("prism-wasapi".into())
        .spawn(move || {
            let result = run(
                &device_id,
                format,
                period,
                consumer,
                Arc::clone(&worker_refill),
                worker_paused,
                control_rx,
                &ready_tx,
            );
            if let Err(error) = result {
                tracing::error!(%error, "WASAPI stream failed");
            }
            worker_refill.close();
        })
        .map_err(|error| AudioError::Device(format!("cannot spawn the WASAPI thread: {error}")))?;

    match ready_rx.recv() {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            let _ = worker.join();
            return Err(error);
        }
        Err(_) => {
            let _ = worker.join();
            return Err(AudioError::Device(
                "the WASAPI thread exited before opening the endpoint".into(),
            ));
        }
    }

    Ok(Box::new(WasapiSink {
        control: Some(control_tx),
        worker: Some(worker),
        paused,
        descriptor: SinkDescriptor {
            device: negotiation.device,
            format,
            period_frames: period,
            buffer_frames: negotiation.buffer_frames,
            latency_frames: negotiation.latency_frames,
            stats,
            refill,
        },
    }))
}

struct EventHandle(HANDLE);

impl Drop for EventHandle {
    fn drop(&mut self) {
        // SAFETY: the handle was created by CreateEventW and is owned here.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run(
    device_id: &str,
    format: AudioFormat,
    period: u32,
    mut consumer: SampleConsumer,
    refill: Arc<RefillSignal>,
    paused: Arc<AtomicBool>,
    control: crossbeam_channel::Receiver<Control>,
    ready: &crossbeam_channel::Sender<Result<(), AudioError>>,
) -> Result<(), AudioError> {
    let _com = ComGuard::new()?;
    let device = open_device(Some(device_id))?;
    let client = activate(&device)?;
    let wave = try_format(&device, &format)?;
    let stream_format = wave.format;

    // Exclusive mode: the period is dictated by the endpoint, and
    // `hnsPeriodicity` must be zero.
    let buffer_hns = frames_to_hns(period.max(64) as u64, stream_format.sample_rate);
    // SAFETY: `wave` outlives the call; event-driven exclusive streams require
    // a zero periodicity and no session GUID.
    unsafe {
        client
            .Initialize(
                AUDCLNT_SHAREMODE_EXCLUSIVE,
                AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                buffer_hns,
                0,
                wave.as_ptr(),
                None,
            )
            .map_err(|error| {
                AudioError::Device(format!("cannot initialise the exclusive stream: {error}"))
            })?;
    }

    // SAFETY: the stream is initialised.
    let buffer_frames = unsafe { client.GetBufferSize() }.map_err(|error| {
        AudioError::Device(format!("cannot read the endpoint buffer size: {error}"))
    })?;
    let render: IAudioRenderClient = unsafe { client.GetService() }.map_err(|error| {
        AudioError::Device(format!("cannot obtain IAudioRenderClient: {error}"))
    })?;

    // SAFETY: manual-reset = false, initially unsignalled, unnamed.
    let event = EventHandle(
        unsafe { CreateEventW(None, false, false, PCWSTR::null()) }.map_err(|error| {
            AudioError::Device(format!("cannot create the render event: {error}"))
        })?,
    );
    // SAFETY: the handle stays alive until after the client is dropped.
    unsafe { client.SetEventHandle(event.0) }
        .map_err(|error| AudioError::Device(format!("cannot bind the render event: {error}")))?;

    // Left stopped: the supervisor primes the ring and then calls
    // `OutputSink::start`, whose Resume lands in the loop below.
    let _ = ready.send(Ok(()));

    let channels = usize::from(stream_format.channels);
    let frame_size = stream_format.frame_size();
    let mut scratch = vec![0.0f32; buffer_frames as usize * channels];
    let mut bytes = Vec::with_capacity(buffer_frames as usize * frame_size);
    // Gated until `OutputSink::start` so the ring is primed first.
    let mut playing = false;

    let outcome = loop {
        // Poll the control channel between buffer periods; the timeout doubles
        // as the wait bound so a stalled endpoint cannot wedge the thread.
        match control.recv_timeout(std::time::Duration::from_millis(0)) {
            Ok(Control::Stop) | Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                // `Stop` ends the stream on the spot, so the frames still
                // queued in the endpoint — up to a whole period, audible as a
                // click — would be discarded. Let them play out first.
                if playing {
                    drain(&client);
                }
                break Ok(());
            }
            Ok(Control::Pause) => {
                playing = false;
                // SAFETY: the stream is running.
                let _ = unsafe { client.Stop() };
            }
            Ok(Control::Resume) => {
                playing = true;
                // SAFETY: the stream was stopped by the pause branch.
                let _ = unsafe { client.Start() };
            }
            // No command pending: keep feeding the endpoint.
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
        }

        if !playing || paused.load(Ordering::Acquire) {
            std::thread::sleep(std::time::Duration::from_millis(2));
            continue;
        }

        // SAFETY: `event` is a valid handle owned by this thread.
        let wait = unsafe { WaitForSingleObject(event.0, 10) };
        if wait != WAIT_OBJECT_0 {
            continue;
        }

        // An event-driven exclusive stream signals a *period*, not a whole
        // buffer: everything still queued must not be overwritten, or the tail
        // of the previous period is replaced by new audio before it is heard.
        // Writing `buffer_frames` on every wake-up did exactly that, and it is
        // the reason exclusive mode on Windows sounded like a broken radio.
        // SAFETY: the stream is initialised and running.
        let padding = unsafe { client.GetCurrentPadding() }.unwrap_or(0);
        let free = buffer_frames.saturating_sub(padding);
        if free == 0 {
            continue;
        }

        let window = &mut scratch[..free as usize * channels];
        consumer.pop_or_silence(window);
        if let Err(error) = write_samples_f32(window, stream_format.sample_format, &mut bytes) {
            break Err(error);
        }
        if bytes.len() != free as usize * frame_size {
            break Err(AudioError::Device(
                "the WASAPI buffer size changed underneath the writer".into(),
            ));
        }

        // SAFETY: `free` never exceeds the buffer size reported by
        // GetBufferSize, and the slice covers exactly those frames.
        let data = unsafe { render.GetBuffer(free) }
            .map_err(|error| AudioError::Device(format!("cannot map the render buffer: {error}")));
        let data = match data {
            Ok(data) => data,
            Err(error) => break Err(error),
        };
        // SAFETY: WASAPI hands us a writable region of `free × frame_size`.
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), data, bytes.len());
        }
        // SAFETY: matching the GetBuffer call above.
        if let Err(error) = unsafe { render.ReleaseBuffer(free, 0) } {
            break Err(AudioError::Device(format!(
                "cannot release the render buffer: {error}"
            )));
        }
        refill.request();
    };

    // SAFETY: stopping an initialised stream; ignoring the result because the
    // device may already be gone.
    unsafe {
        let _ = client.Stop();
    }
    outcome
}

/// Let the frames already queued in the endpoint play before stopping.
///
/// WASAPI has no drain call for exclusive streams: `Stop` truncates the buffer
/// immediately. Polling `GetCurrentPadding` until it reaches zero keeps the
/// last milliseconds of the file — and the end of every record — intact; the
/// deadline is a guard so a stalled endpoint can never wedge the writer.
fn drain(client: &IAudioClient) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
    while std::time::Instant::now() < deadline {
        // SAFETY: the stream is initialised; a query cannot invalidate it.
        match unsafe { client.GetCurrentPadding() } {
            Ok(0) | Err(_) => break,
            Ok(_) => std::thread::sleep(std::time::Duration::from_millis(2)),
        }
    }
}

fn frames_to_hns(frames: u64, rate: u32) -> i64 {
    if rate == 0 {
        return 0;
    }
    // 100-ns reference time units, rounded up so the buffer is never shorter
    // than a period.
    let hns = (frames.saturating_mul(10_000_000) + u64::from(rate) - 1) / u64::from(rate);
    i64::try_from(hns).unwrap_or(i64::MAX)
}
