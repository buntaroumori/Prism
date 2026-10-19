//! Native PipeWire output node.
//!
//! Prism connects to PipeWire as a first-class client instead of squatting on
//! the ALSA device: the graph handles routing, per-application volume and
//! device migration, and the stream survives default-device changes.
//!
//! This path is *not* bit-perfect by construction — the graph may convert or
//! remix — so the engine reports the negotiated format and the UI's Hi-Res
//! badge says "PipeWire" rather than "Bit-perfect". Use ALSA `hw:` or WASAPI
//! exclusive for that.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use pipewire as pw;
use pw::spa::param::ParamType;
use pw::spa::param::audio::{AudioFormat, AudioInfoRaw};
use pw::spa::pod::Value;
use pw::spa::pod::serialize::PodSerializer;
use pw::spa::utils::{Direction, SpaTypes};

use crate::audio::AudioError;
use crate::audio::engine::backend::{
    BackendKind, Negotiation, OutputDevice, OutputSink, RefillSignal, SinkDescriptor,
};
use crate::audio::format::{AudioFormat as EngineFormat, SampleFormat, write_samples_f32};
use crate::audio::ring::{RingStats, SampleConsumer};

const NODE_ID: &str = "pipewire:default";
const NODE_NAME: &str = "Prism Output";

fn socket_path() -> Option<std::path::PathBuf> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")?;
    Some(Path::new(&runtime).join("pipewire-0"))
}

/// True when a PipeWire daemon appears to be running for this user.
#[must_use]
pub fn is_available() -> bool {
    socket_path().is_some_and(|path| path.exists())
}

/// The PipeWire output node, when the server is reachable.
pub fn default_device() -> Result<Option<OutputDevice>, AudioError> {
    if !is_available() {
        return Ok(None);
    }
    Ok(Some(OutputDevice {
        id: NODE_ID.to_string(),
        name: NODE_NAME.to_string(),
        kind: BackendKind::PipeWire,
        is_default: false,
        channels: 8,
        sample_rates: vec![
            44_100, 48_000, 88_200, 96_000, 176_400, 192_000, 352_800, 384_000,
        ],
        sample_formats: vec![
            SampleFormat::S16,
            SampleFormat::S24In32,
            SampleFormat::S32,
            SampleFormat::F32,
        ],
        supports_exclusive: false,
    }))
}

fn spa_format(format: SampleFormat) -> AudioFormat {
    match format {
        SampleFormat::S16 => AudioFormat::S16LE,
        SampleFormat::S24 => AudioFormat::S24LE,
        SampleFormat::S24In32 | SampleFormat::S32 => AudioFormat::S32LE,
        SampleFormat::F32 => AudioFormat::F32LE,
        SampleFormat::F64 => AudioFormat::F64LE,
    }
}

/// PipeWire accepts the source format as-is; the graph decides what to do next.
pub fn negotiate(
    device_id: Option<&str>,
    desired: &EngineFormat,
    period_frames: u32,
) -> Result<Negotiation, AudioError> {
    if !is_available() {
        return Err(AudioError::Device(
            "no PipeWire session is running for this user".into(),
        ));
    }
    let device = default_device()?.ok_or(AudioError::NoDevice)?;
    if let Some(id) = device_id
        && id != NODE_ID
        && id != "pipewire"
    {
        return Err(AudioError::Device(format!(
            "unknown PipeWire endpoint {id:?}"
        )));
    }

    // PipeWire keeps 1024-frame quanta by default; asking for less than a
    // quantum just adds wake-ups without lowering latency.
    let period = period_frames.clamp(128, 1024);
    Ok(Negotiation {
        device,
        kind: BackendKind::PipeWire,
        format: *desired,
        period_frames: period,
        buffer_frames: period * 4,
        latency_frames: u64::from(period) * 2,
    })
}

#[derive(Debug)]
enum Control {
    Pause,
    Resume,
    Stop,
}

/// Control handle for the PipeWire main loop thread.
pub struct PipewireSink {
    control: Option<pw::channel::Sender<Control>>,
    worker: Option<std::thread::JoinHandle<()>>,
    descriptor: SinkDescriptor,
}

impl OutputSink for PipewireSink {
    fn descriptor(&self) -> &SinkDescriptor {
        &self.descriptor
    }

    fn start(&mut self) -> Result<(), AudioError> {
        if let Some(control) = &self.control {
            let _ = control.send(Control::Resume);
        }
        Ok(())
    }

    fn pause(&mut self) -> Result<(), AudioError> {
        if let Some(control) = &self.control {
            let _ = control.send(Control::Pause);
        }
        Ok(())
    }

    fn resume(&mut self) -> Result<(), AudioError> {
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

impl Drop for PipewireSink {
    fn drop(&mut self) {
        self.stop();
    }
}

struct StreamState {
    consumer: SampleConsumer,
    format: EngineFormat,
    scratch: Vec<f32>,
    bytes: Vec<u8>,
    refill: Arc<RefillSignal>,
    paused: Arc<AtomicBool>,
}

/// Open the node and start the main loop.
pub fn start(
    negotiation: Negotiation,
    consumer: SampleConsumer,
    refill: Arc<RefillSignal>,
    stats: Arc<RingStats>,
) -> Result<Box<dyn OutputSink>, AudioError> {
    let (control_tx, control_rx) = pw::channel::channel::<Control>();
    let (ready_tx, ready_rx) = crossbeam_channel::bounded::<Result<(), AudioError>>(1);

    let format = negotiation.format;
    let period = negotiation.period_frames;
    let worker_refill = Arc::clone(&refill);
    // Gated until `OutputSink::start` so the ring is primed first.
    let paused = Arc::new(AtomicBool::new(true));

    let worker = std::thread::Builder::new()
        .name("prism-pipewire".into())
        .spawn(move || {
            let result = run(
                format,
                period,
                consumer,
                worker_refill,
                paused,
                control_rx,
                &ready_tx,
            );
            if let Err(error) = result {
                tracing::error!(%error, "PipeWire stream failed");
            }
        })
        .map_err(|error| {
            AudioError::Device(format!("cannot spawn the PipeWire thread: {error}"))
        })?;

    match ready_rx.recv() {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            let _ = worker.join();
            return Err(error);
        }
        Err(_) => {
            let _ = worker.join();
            return Err(AudioError::Device(
                "the PipeWire thread exited before connecting the node".into(),
            ));
        }
    }

    Ok(Box::new(PipewireSink {
        control: Some(control_tx),
        worker: Some(worker),
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

fn run(
    format: EngineFormat,
    period: u32,
    consumer: SampleConsumer,
    refill: Arc<RefillSignal>,
    paused: Arc<AtomicBool>,
    control_rx: pw::channel::Receiver<Control>,
    ready: &crossbeam_channel::Sender<Result<(), AudioError>>,
) -> Result<(), AudioError> {
    pw::init();
    let main_loop = pw::main_loop::MainLoop::new(None)
        .map_err(|error| AudioError::Device(format!("cannot create the PipeWire loop: {error}")))?;
    let context = pw::context::Context::new(&main_loop).map_err(|error| {
        AudioError::Device(format!("cannot create the PipeWire context: {error}"))
    })?;
    let core = context
        .connect(None)
        .map_err(|error| AudioError::Device(format!("cannot connect to PipeWire: {error}")))?;

    let properties = pw::properties::properties! {
        *pw::keys::MEDIA_TYPE => "Audio",
        *pw::keys::MEDIA_CATEGORY => "Playback",
        *pw::keys::MEDIA_ROLE => "Music",
        *pw::keys::NODE_NAME => "prism-output",
        *pw::keys::NODE_DESCRIPTION => "Prism",
        *pw::keys::APP_NAME => "Prism",
    };
    let stream = pw::stream::Stream::new(&core, "prism-output", properties).map_err(|error| {
        AudioError::Device(format!("cannot create the PipeWire stream: {error}"))
    })?;

    let mut info = AudioInfoRaw::new();
    info.set_format(spa_format(format.sample_format));
    info.set_rate(format.sample_rate);
    info.set_channels(u32::from(format.channels));

    let mut serialized = PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &Value::Object(pw::spa::pod::Object {
            type_: SpaTypes::ObjectParamFormat.0,
            id: ParamType::EnumFormat.as_raw(),
            properties: info.into(),
        }),
    )
    .map_err(|error| AudioError::Device(format!("cannot build the PipeWire format pod: {error}")))?
    .0
    .into_inner();

    let Some(mut pod) = pw::spa::pod::Pod::from_bytes(&mut serialized) else {
        return Err(AudioError::Device(
            "the serialized PipeWire format pod is invalid".into(),
        ));
    };

    let state = StreamState {
        consumer,
        format,
        // PipeWire may hand us a buffer larger than the quantum; keep room
        // for eight periods so the RT callback never has to allocate.
        scratch: vec![0.0f32; period as usize * 8 * usize::from(format.channels)],
        bytes: Vec::with_capacity(period as usize * 8 * usize::from(format.channels) * 4),
        refill: Arc::clone(&refill),
        paused: Arc::clone(&paused),
    };

    let _listener = stream
        .add_local_listener_with_user_data(state)
        .state_changed({
            let error_refill = Arc::clone(&refill);
            move |_, _, _, new| {
                if let pw::stream::StreamState::Error(message) = new {
                    tracing::error!(%message, "PipeWire stream error");
                    error_refill.close();
                }
            }
        })
        .process(|stream, state| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let Some(data) = buffer.datas_mut().first_mut() else {
                return;
            };
            let frame_size = state.format.frame_size();

            // While paused the node keeps its clock and emits silence, so the
            // graph does not renegotiate the format on every transport stop.
            let written = if state.paused.load(Ordering::Relaxed) {
                let Some(slice) = data.data() else {
                    return;
                };
                let room = slice.len() / frame_size;
                slice.fill(0);
                room * frame_size
            } else {
                let Some(slice) = data.data() else {
                    return;
                };
                let room = slice.len() / frame_size;
                if room == 0 {
                    return;
                }
                let wanted = room * usize::from(state.format.channels);
                if wanted > state.scratch.len() {
                    // A buffer larger than the negotiated maximum: stay silent
                    // rather than allocating inside the RT callback.
                    slice.fill(0);
                    room * frame_size
                } else {
                    state.consumer.pop_or_silence(&mut state.scratch[..wanted]);
                    match write_samples_f32(
                        &state.scratch[..wanted],
                        state.format.sample_format,
                        &mut state.bytes,
                    ) {
                        Ok(()) => {
                            let produced = state.bytes.len().min(slice.len());
                            slice[..produced].copy_from_slice(&state.bytes[..produced]);
                            produced
                        }
                        Err(error) => {
                            tracing::error!(%error, "PipeWire sample conversion failed");
                            return;
                        }
                    }
                }
            };

            let chunk = data.chunk_mut();
            *chunk.offset_mut() = 0;
            *chunk.size_mut() = written as u32;
            *chunk.stride_mut() = frame_size as i32;
            if !state.paused.load(Ordering::Relaxed) {
                state.refill.request();
            }
        })
        .register()
        .map_err(|error| {
            AudioError::Device(format!("cannot register the PipeWire listener: {error}"))
        })?;

    stream
        .connect(
            Direction::Output,
            None,
            pw::stream::StreamFlags::AUTOCONNECT
                | pw::stream::StreamFlags::MAP_BUFFERS
                | pw::stream::StreamFlags::RT_PROCESS,
            &mut [&mut pod],
        )
        .map_err(|error| {
            AudioError::Device(format!("cannot connect the PipeWire node: {error}"))
        })?;

    let control_loop = main_loop.clone();
    let control_paused = Arc::clone(&paused);
    let _control = control_rx.attach(main_loop.loop_(), move |command| match command {
        Control::Pause => control_paused.store(true, Ordering::Release),
        Control::Resume => control_paused.store(false, Ordering::Release),
        Control::Stop => control_loop.quit(),
    });

    let _ = ready.send(Ok(()));
    main_loop.run();
    Ok(())
}
