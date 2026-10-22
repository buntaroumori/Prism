//! Playback supervisor: transport state, ring feeding, device lifecycle.
//!
//! One thread owns the [`GaplessPlayer`], the output sink and the playhead.
//! It is woken by device callbacks (through [`RefillSignal`]) and by control
//! commands (through an `unpark` from [`Engine`]), so it sleeps instead of
//! polling whenever there is nothing to do.
//!
//! Responsibilities:
//!
//! * compile a [`PlaybackConfig`] from the probed source format and the
//!   negotiated device format, then prime the ring *before* starting the
//!   device clock;
//! * ramp the volume per block so no change ever clicks;
//! * report a sample-accurate playhead, including tracks that started in the
//!   middle of a pushed block;
//! * renegotiate the device when an exclusive stream meets a track with a
//!   different native format (bit-perfect playback of mixed-rate queues);
//! * recover from device loss (unplugged DAC, driver reset) and follow the
//!   system default device when it changes.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, TryRecvError, unbounded};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::audio::AudioError;
use crate::audio::analysis::Analyzer;
use crate::audio::decoder::{GaplessPlayer, PlaybackConfig, TrackRequest, probe_format};
use crate::audio::dsp::DspSettings;
use crate::audio::engine::backend::{
    self, BackendKind, OpenedSink, OutputDevice, OutputMode, RatePolicy,
};
use crate::audio::format::AudioFormat;

/// Transport state as seen by the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PlaybackState {
    Stopped,
    Buffering,
    Playing,
    Paused,
}

/// Metadata for the track currently loaded in the engine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NowPlaying {
    pub track_id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub path: String,
    pub duration_frames: Option<u64>,
    pub source_format: Option<AudioFormat>,
}

impl From<&TrackRequest> for NowPlaying {
    fn from(request: &TrackRequest) -> Self {
        Self {
            track_id: request.track_id.clone(),
            title: request.title.clone(),
            artist: request.artist.clone(),
            album: request.album.clone(),
            path: request.path.display().to_string(),
            duration_frames: None,
            source_format: None,
        }
    }
}

/// Full engine snapshot, polled by the UI and pushed on every change.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineStatus {
    pub state: PlaybackState,
    pub track: Option<NowPlaying>,
    pub position_frames: u64,
    pub duration_frames: Option<u64>,
    pub source_format: Option<AudioFormat>,
    pub output_format: Option<AudioFormat>,
    pub backend: Option<BackendKind>,
    pub device_name: Option<String>,
    /// The endpoint's *own* configuration, queried from the driver at open
    /// time. Distinct from `output_format`, which is the container this engine
    /// writes: on Windows shared mode they differ by design (the OS mix is
    /// float32, the panel may say 24-bit) and conflating them is exactly how a
    /// player ends up reporting a format the DAC never sees.
    pub device_format: Option<AudioFormat>,
    /// No DSP stage, no volume attenuation and an exact format match.
    pub bit_perfect: bool,
    /// The samples are DSD-over-PCM and must not be touched.
    pub dop: bool,
    pub volume_db: f32,
    pub muted: bool,
    pub latency_frames: u64,
    pub underruns: u64,
    pub overruns: u64,
    pub queue_index: usize,
    pub queue_len: usize,
}

impl Default for EngineStatus {
    fn default() -> Self {
        Self {
            state: PlaybackState::Stopped,
            track: None,
            position_frames: 0,
            duration_frames: None,
            source_format: None,
            output_format: None,
            backend: None,
            device_name: None,
            device_format: None,
            bit_perfect: false,
            dop: false,
            volume_db: 0.0,
            muted: false,
            latency_frames: 0,
            underruns: 0,
            overruns: 0,
            queue_index: 0,
            queue_len: 0,
        }
    }
}

/// Engine configuration persisted in the settings table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineConfig {
    pub mode: OutputMode,
    /// Whether the endpoint follows the record's rate or stays pinned to its own.
    pub rate_policy: RatePolicy,
    pub device_id: Option<String>,
    /// Frames per device period. 512 at 48 kHz ≈ 10.7 ms.
    pub buffer_frames: u32,
    pub dsp: DspSettings,
    pub ram_play: bool,
    pub ram_limit_bytes: u64,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            // Standard behaviour: share the endpoint with every other
            // application and let the OS mixer keep its clock. Exclusive mode
            // stays available through `player_set_output` but is never the
            // default: taking a device hostage is not a sane first impression.
            mode: OutputMode::Shared,
            rate_policy: RatePolicy::MatchSource,
            device_id: None,
            buffer_frames: 512,
            dsp: DspSettings::default(),
            ram_play: false,
            ram_limit_bytes: 512 * 1024 * 1024,
        }
    }
}

/// Events emitted to the frontend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum EngineEvent {
    StateChanged {
        state: PlaybackState,
    },
    TrackChanged {
        track: NowPlaying,
        index: usize,
    },
    DeviceChanged {
        device: String,
        backend: BackendKind,
        format: AudioFormat,
        bit_perfect: bool,
    },
    OutputError {
        message: String,
    },
    Underrun {
        total: u64,
    },
    /// A queued file could not be opened and was skipped.
    ///
    /// Playback continues with the next track; the UI explains which file and
    /// why, because a silently missing track is worse than a visible gap.
    TrackSkipped {
        track_id: String,
        title: String,
        artist: String,
        reason: String,
    },
}

enum Command {
    Play {
        tracks: Vec<TrackRequest>,
        index: usize,
    },
    Pause,
    Resume,
    Stop,
    Seek {
        frame: u64,
    },
    /// Seek by wall-clock position.
    ///
    /// The renderer knows what the listener sees on the clock, and nothing
    /// about frame counting; converting here uses the *playhead's own* format,
    /// which is the only rate that is certainly correct. A frontend-side
    /// conversion with a guessed rate is how a seek lands tens of seconds away
    /// on a 192 kHz file.
    SeekMs {
        position_ms: u64,
    },
    SetDsp {
        dsp: DspSettings,
        restart: bool,
    },
    SetOutput {
        mode: OutputMode,
        device: Option<String>,
        buffer_frames: u32,
    },
    SetRatePolicy {
        policy: RatePolicy,
    },
    SetRamPlay {
        enabled: bool,
    },
    Shutdown,
}

struct Boundary {
    /// Global pushed-frame offset where the new track starts.
    at: u64,
    request: TrackRequest,
    duration: Option<u64>,
    format: AudioFormat,
}

struct Playhead {
    anchor: u64,
    base: u64,
    request: Option<TrackRequest>,
    duration: Option<u64>,
    format: Option<AudioFormat>,
    pending: Option<Boundary>,
}

impl Playhead {
    fn new() -> Self {
        Self {
            anchor: 0,
            base: 0,
            request: None,
            duration: None,
            format: None,
            pending: None,
        }
    }

    fn reset(&mut self, request: &TrackRequest, format: AudioFormat, anchor: u64) {
        self.anchor = anchor;
        self.base = 0;
        self.request = Some(request.clone());
        self.format = Some(format);
        self.duration = None;
        self.pending = None;
    }
}

/// Opens an output endpoint.
///
/// Production always uses [`backend::open_sink`]. Tests inject a loopback sink so
/// device loss and hot-swap can be reproduced on a machine with no sound card —
/// the same code path a real DAC takes, with the device replaced by a thread that
/// consumes the ring and reports liveness.
pub type SinkOpener = Arc<
    dyn Fn(OutputMode, Option<&str>, &AudioFormat, u32) -> Result<OpenedSink, AudioError>
        + Send
        + Sync,
>;

/// Handle used by the Tauri layer. Cheap to clone through an `Arc`.
pub struct Engine {
    commands: Sender<Command>,
    status: Arc<RwLock<EngineStatus>>,
    volume_db: Arc<AtomicU32>,
    muted: Arc<AtomicBool>,
    events: broadcast::Sender<EngineEvent>,
    analyzer: Arc<Analyzer>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Engine")
            .field("status", &*self.status.read())
            .finish_non_exhaustive()
    }
}

impl Engine {
    /// Start the supervisor thread with the given configuration.
    pub fn new(config: EngineConfig) -> Result<Arc<Self>, AudioError> {
        Self::with_opener(
            config,
            Arc::new(|mode, device, format, buffer| {
                backend::open_sink(mode, device, format, buffer)
            }),
        )
    }

    /// Start the supervisor with an explicit endpoint opener.
    ///
    /// Used by the stress suite to drive the device lifecycle (open, hot-swap,
    /// loss, recovery) without hardware. Everything else is the production code.
    pub fn with_opener(config: EngineConfig, opener: SinkOpener) -> Result<Arc<Self>, AudioError> {
        let (commands, receiver) = unbounded::<Command>();
        let status = Arc::new(RwLock::new(EngineStatus::default()));
        let volume_db = Arc::new(AtomicU32::new(
            config.dsp.volume_db.clamp(-60.0, 0.0).to_bits(),
        ));
        let muted = Arc::new(AtomicBool::new(false));
        let (events, _) = broadcast::channel::<EngineEvent>(256);
        let analyzer = Arc::new(Analyzer::new());

        let supervisor = Supervisor {
            commands: receiver,
            status: Arc::clone(&status),
            volume_db: Arc::clone(&volume_db),
            muted: Arc::clone(&muted),
            events: events.clone(),
            analyzer: Arc::clone(&analyzer),
            opener,
            config,
            player: None,
            output: None,
            queue: Vec::new(),
            index: 0,
            state: PlaybackState::Stopped,
            playhead: Playhead::new(),
            pushed: 0,
            gain: 0.0,
            last_publish: Instant::now() - Duration::from_secs(1),
            negotiated_exclusive: false,
            follows_source_rate: false,
            device_format: None,
            carry: Vec::new(),
            carry_boundary: None,
            carry_finished: false,
        };

        let worker = std::thread::Builder::new()
            .name("prism-audio-supervisor".into())
            .spawn(move || supervisor.run())
            .map_err(|error| {
                AudioError::Device(format!("cannot spawn the audio supervisor: {error}"))
            })?;

        Ok(Arc::new(Self {
            commands,
            status,
            volume_db,
            muted,
            events,
            analyzer,
            worker: Some(worker),
        }))
    }

    /// Subscribe to engine events.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<EngineEvent> {
        self.events.subscribe()
    }

    /// Spectrum tap. The visualiser panel enables it when it mounts.
    #[must_use]
    pub fn analyzer(&self) -> &Arc<Analyzer> {
        &self.analyzer
    }

    #[must_use]
    pub fn status(&self) -> EngineStatus {
        self.status.read().clone()
    }

    pub fn play(&self, tracks: Vec<TrackRequest>, index: usize) -> Result<(), AudioError> {
        if tracks.is_empty() {
            return Err(AudioError::EmptyQueue);
        }
        self.send(Command::Play { tracks, index })
    }

    pub fn pause(&self) -> Result<(), AudioError> {
        self.send(Command::Pause)
    }

    pub fn resume(&self) -> Result<(), AudioError> {
        self.send(Command::Resume)
    }

    pub fn stop(&self) -> Result<(), AudioError> {
        self.send(Command::Stop)
    }

    pub fn seek(&self, frame: u64) -> Result<(), AudioError> {
        self.send(Command::Seek { frame })
    }

    /// Seek to a position in milliseconds of the loaded track.
    pub fn seek_ms(&self, position_ms: u64) -> Result<(), AudioError> {
        self.send(Command::SeekMs { position_ms })
    }

    /// Set the software volume. Values are clamped to `-60..=0 dB`; anything
    /// above unity would need a limiter and would break bit-perfect output.
    pub fn set_volume_db(&self, db: f32) {
        let clamped = if db.is_finite() {
            db.clamp(-60.0, 0.0)
        } else {
            0.0
        };
        self.volume_db.store(clamped.to_bits(), Ordering::Relaxed);
        self.status.write().volume_db = clamped;
        self.wake();
    }

    pub fn set_muted(&self, muted: bool) {
        self.muted.store(muted, Ordering::Relaxed);
        self.status.write().muted = muted;
        self.wake();
    }

    /// Change EQ / ReplayGain / crossfeed. When `restart` is false the chain is
    /// rebuilt in place and the change lands within one buffer.
    pub fn set_dsp(&self, dsp: DspSettings, restart: bool) -> Result<(), AudioError> {
        self.send(Command::SetDsp { dsp, restart })
    }

    pub fn set_output(
        &self,
        mode: OutputMode,
        device: Option<String>,
        buffer_frames: u32,
    ) -> Result<(), AudioError> {
        self.send(Command::SetOutput {
            mode,
            device,
            buffer_frames,
        })
    }

    /// Change whether the endpoint follows the record's sample rate.
    ///
    /// Takes effect immediately: the device is reopened at the current position,
    /// which the supervisor guarantees to survive.
    pub fn set_rate_policy(&self, policy: RatePolicy) -> Result<(), AudioError> {
        self.send(Command::SetRatePolicy { policy })
    }

    pub fn set_ram_play(&self, enabled: bool) -> Result<(), AudioError> {
        self.send(Command::SetRamPlay { enabled })
    }

    /// Enumerate endpoints for the current mode.
    pub fn devices(&self, mode: OutputMode) -> Result<Vec<OutputDevice>, AudioError> {
        backend::enumerate_devices(mode)
    }

    pub fn shutdown(&self) {
        let _ = self.send(Command::Shutdown);
    }

    fn send(&self, command: Command) -> Result<(), AudioError> {
        self.commands
            .send(command)
            .map_err(|_| AudioError::NotRunning)?;
        self.wake();
        Ok(())
    }

    fn wake(&self) {
        if let Some(worker) = &self.worker {
            worker.thread().unpark();
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
        if let Some(worker) = self.worker.take() {
            worker.thread().unpark();
            let _ = worker.join();
        }
    }
}

struct Supervisor {
    commands: Receiver<Command>,
    status: Arc<RwLock<EngineStatus>>,
    volume_db: Arc<AtomicU32>,
    muted: Arc<AtomicBool>,
    events: broadcast::Sender<EngineEvent>,
    analyzer: Arc<Analyzer>,
    opener: SinkOpener,
    config: EngineConfig,
    player: Option<GaplessPlayer>,
    output: Option<OpenedSink>,
    queue: Vec<TrackRequest>,
    index: usize,
    state: PlaybackState,
    playhead: Playhead,
    /// Frames pushed into the ring since playback started.
    pushed: u64,
    /// Current volume gain (ramped towards the target every block).
    gain: f32,
    last_publish: Instant,
    /// True when the open sink is an exclusive backend, which is what enables
    /// per-track rate renegotiation.
    negotiated_exclusive: bool,
    /// True when the endpoint granted the *source* rate, which is what lets the
    /// engine follow the record from track to track without asking the driver
    /// the same question — and failing it — at every boundary.
    follows_source_rate: bool,
    /// Endpoint configuration as reported by the driver for the open sink.
    device_format: Option<AudioFormat>,
    /// Decoded frames the ring could not accept yet, interleaved.
    ///
    /// A resampler emits a whole input chunk at once — thousands of output
    /// frames — while the ring only promises the free space it reported a
    /// moment earlier. Pushing "as much as fits" and discarding the rest is
    /// precisely what turned every sample-rate conversion into a machine-gun
    /// crackle: the discarded tail is a discontinuity in the middle of a
    /// waveform. Whatever does not fit waits here and is offered again.
    carry: Vec<f32>,
    /// Offset inside [`Self::carry`] where the next track begins, in frames.
    carry_boundary: Option<u64>,
    /// The queue ended inside [`Self::carry`].
    carry_finished: bool,
}

impl Supervisor {
    fn run(mut self) {
        loop {
            let mut shutdown = false;
            loop {
                match self.commands.try_recv() {
                    Ok(Command::Shutdown) => {
                        shutdown = true;
                        break;
                    }
                    Ok(command) => {
                        if let Err(error) = self.handle(command) {
                            tracing::error!(%error, "audio command failed");
                            self.emit(EngineEvent::OutputError {
                                message: error.to_string(),
                            });
                            self.fail();
                        }
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        shutdown = true;
                        break;
                    }
                }
            }
            if shutdown {
                self.teardown();
                return;
            }

            if self.state == PlaybackState::Playing
                && let Err(error) = self.pump()
            {
                tracing::error!(%error, "playback pump failed");
                self.emit(EngineEvent::OutputError {
                    message: error.to_string(),
                });
                self.recover();
            }

            self.publish(false);

            // Sleep until a device callback, a command or (as a watchdog)
            // 100 ms. The device callback wakes the thread once per period, so
            // the playhead is republished every 40 ms and the scrubber advances
            // smoothly; the watchdog only matters if the signal is ever missed.
            std::thread::park_timeout(Duration::from_millis(100));
        }
    }

    fn handle(&mut self, command: Command) -> Result<(), AudioError> {
        match command {
            Command::Play { tracks, index } => {
                self.queue = tracks;
                self.index = index.min(self.queue.len().saturating_sub(1));
                self.start_queue(self.index, true)
            }
            Command::Pause => {
                if self.state == PlaybackState::Playing {
                    if let Some(output) = &mut self.output {
                        output.sink.pause()?;
                    }
                    self.set_state(PlaybackState::Paused);
                }
                Ok(())
            }
            Command::Resume => {
                if self.state == PlaybackState::Paused {
                    if let Some(output) = &mut self.output {
                        output.sink.resume()?;
                    }
                    self.set_state(PlaybackState::Playing);
                }
                Ok(())
            }
            Command::Stop => {
                self.teardown();
                Ok(())
            }
            Command::Seek { frame } => self.seek(frame),
            Command::SeekMs { position_ms } => {
                let frame = self
                    .playhead
                    .format
                    .map_or(0, |format| format.ms_to_frames(position_ms));
                self.seek(frame)
            }
            Command::SetDsp { dsp, restart } => {
                self.config.dsp = dsp;
                if !restart {
                    // Hot-swap the chain: no restart, no gap. The samples
                    // already in the ring keep the old curve for one buffer.
                    let settings = self.playback_dsp();
                    let format = self.output.as_ref().map(|output| output.negotiation.format);
                    if let (Some(player), Some(format)) = (self.player.as_mut(), format)
                        && player.update_dsp(&settings, &format).is_ok()
                    {
                        return Ok(());
                    }
                }
                if self.state != PlaybackState::Stopped {
                    let position = self.position_frames();
                    let index = self.index;
                    self.start_queue(index, false)?;
                    self.seek(position)?;
                }
                Ok(())
            }
            Command::SetOutput {
                mode,
                device,
                buffer_frames,
            } => {
                self.config.mode = mode;
                self.config.device_id = device;
                if buffer_frames > 0 {
                    self.config.buffer_frames = buffer_frames;
                }
                if self.state != PlaybackState::Stopped {
                    let position = self.position_frames();
                    let index = self.index;
                    let was_paused = self.state == PlaybackState::Paused;
                    self.start_queue(index, false)?;
                    self.seek(position)?;
                    if was_paused {
                        if let Some(output) = &mut self.output {
                            let _ = output.sink.pause();
                        }
                        self.set_state(PlaybackState::Paused);
                    }
                }
                Ok(())
            }
            Command::SetRamPlay { enabled } => {
                self.config.ram_play = enabled;
                Ok(())
            }
            Command::SetRatePolicy { policy } => {
                if self.config.rate_policy != policy {
                    self.config.rate_policy = policy;
                    if self.state != PlaybackState::Stopped {
                        let position = self.position_frames();
                        let index = self.index;
                        let was_paused = self.state == PlaybackState::Paused;
                        self.start_queue(index, false)?;
                        self.seek(position)?;
                        if was_paused {
                            if let Some(output) = &mut self.output {
                                let _ = output.sink.pause();
                            }
                            self.set_state(PlaybackState::Paused);
                        }
                    }
                }
                Ok(())
            }
            Command::Shutdown => Ok(()),
        }
    }

    /// Open the device and the player for `queue[index..]`.
    fn start_queue(&mut self, index: usize, reset_position: bool) -> Result<(), AudioError> {
        self.teardown_playback();
        if self.queue.is_empty() {
            return Ok(());
        }
        self.index = index.min(self.queue.len() - 1);
        self.set_state(PlaybackState::Buffering);

        // A track that cannot be opened must not take the queue down with it:
        // walk forward, report each dead file, and start on the first playable
        // one. Without this, an album whose first rip failed would refuse to
        // play at all. The remaining tracks are handled by the decoder, which
        // skips them mid-stream without dropping the device.
        let source_format = loop {
            let candidate = self.queue[self.index].clone();
            match probe_format(&candidate) {
                Ok(format) => break format,
                Err(error) => {
                    tracing::warn!(
                        track = %candidate.track_id,
                        path = %candidate.path.display(),
                        %error,
                        "skipping unplayable track at the head of the queue"
                    );
                    self.emit(EngineEvent::TrackSkipped {
                        track_id: candidate.track_id.clone(),
                        title: candidate.title.clone(),
                        artist: candidate.artist.clone(),
                        reason: error.to_string(),
                    });
                    if self.index + 1 >= self.queue.len() {
                        // Nothing between here and the end of the queue plays.
                        self.teardown();
                        return Ok(());
                    }
                    self.index += 1;
                }
            }
        };
        let first = self.queue[self.index].clone();
        let desired = self.choose_format(&source_format);

        let opened = (self.opener)(
            self.config.mode,
            self.config.device_id.as_deref(),
            &desired,
            self.config.buffer_frames,
        )?;

        self.negotiated_exclusive = opened.negotiation.kind.is_exclusive();
        self.follows_source_rate =
            opened.negotiation.format.sample_rate == source_format.sample_rate;
        // Ask the driver what the endpoint is actually configured to do right
        // now. This is the number the user compares against the OS panel, and
        // the only honest source for it.
        self.device_format =
            backend::live_device_format(opened.negotiation.kind, self.config.device_id.as_deref());
        // The device threads unpark this one through their refill signal.
        opened.refill.attach();
        tracing::info!(
            device = %opened.negotiation.device.name,
            backend = opened.negotiation.kind.label(),
            rate = opened.negotiation.format.sample_rate,
            channels = opened.negotiation.format.channels,
            bits = opened.negotiation.format.sample_format.label(),
            "output stream opened"
        );

        let playback = PlaybackConfig {
            output_format: opened.negotiation.format,
            dsp: self.playback_dsp(),
            ram_play: self.config.ram_play,
            ram_limit_bytes: self.config.ram_limit_bytes,
        };
        let player = GaplessPlayer::new(self.queue[self.index..].to_vec(), playback)?;

        self.pushed = 0;
        self.gain = 0.0;
        if reset_position {
            self.playhead.reset(&first, source_format, 0);
            self.playhead.duration = player.current_total();
            self.emit(EngineEvent::TrackChanged {
                track: NowPlaying::from(&first),
                index: self.index,
            });
        }
        self.player = Some(player);
        self.output = Some(opened);

        self.prime()?;
        let mut event = None;
        if let Some(output) = &mut self.output {
            output.sink.start()?;
            event = Some(EngineEvent::DeviceChanged {
                device: output.negotiation.device.name.clone(),
                backend: output.negotiation.kind,
                format: output.negotiation.format,
                bit_perfect: false,
            });
        }
        if let Some(event) = event {
            let bit_perfect = self.is_bit_perfect();
            self.emit(match event {
                EngineEvent::DeviceChanged {
                    device,
                    backend,
                    format,
                    ..
                } => EngineEvent::DeviceChanged {
                    device,
                    backend,
                    format,
                    bit_perfect,
                },
                other => other,
            });
        }
        self.set_state(PlaybackState::Playing);
        self.publish(true);
        Ok(())
    }

    /// The format we ask the backend for.
    ///
    /// Exclusive backends honour it exactly (that is what bit-perfect means);
    /// shared backends are allowed to answer with their mix format, which the
    /// returned [`backend::Negotiation`] reports back.
    /// The format to ask the endpoint for.
    ///
    /// Shared mode is not ours to choose: the endpoint runs at whatever rate and
    /// container the OS panel says, and everything else is converted by Windows
    /// on the way in. Asking cpal for the *source* rate here is what made the
    /// Windows build sound broken — the stream was opened at 44.1/16 into a
    /// 24/192 mix graph, and the OS converter mangled every sample while the
    /// quality badge still reported the endpoint's 32/192. So: query the live
    /// default configuration and ask for exactly that, then let the DSP chain
    /// resample the file into it.
    ///
    /// Exclusive mode is the opposite: the stream owns the DAC, so we ask for
    /// the source's native format and let the negotiation ladder fall back only
    /// when the endpoint refuses it.
    fn choose_format(&self, source: &AudioFormat) -> AudioFormat {
        if self.config.mode == OutputMode::Exclusive {
            return *source;
        }
        if self.config.rate_policy == RatePolicy::MatchSource {
            // Hand the record's own format to the backend. It answers with the
            // closest configuration it can grant: on an endpoint free to run at
            // the source rate that is an exact match and nothing resamples; on
            // one pinned by the OS mixer it is the mix format, and the decoder
            // converts — which is the same path as before, only requested
            // honestly instead of assumed.
            return *source;
        }
        self.live_shared_format().unwrap_or(*source)
    }

    /// The endpoint's own default configuration, asked of the driver at
    /// runtime.
    ///
    /// A pinned endpoint answers for itself; otherwise the first shared backend
    /// the platform would open is asked. `None` means the backend does its own
    /// negotiation (PipeWire) or the device vanished, and the caller keeps the
    /// source format.
    fn live_shared_format(&self) -> Option<AudioFormat> {
        let device = self.config.device_id.as_deref();
        if let Some(kind) = device.and_then(backend::kind_of) {
            return backend::live_device_format(kind, device);
        }
        // Ask the backend that will actually take the stream. Querying a
        // different one and handing its format to the first would be fiction:
        // a PipeWire node negotiates its own rate with the graph, and forcing
        // ALSA's default onto it is how a working Linux install starts
        // resampling twice.
        let first = *backend::preferred_kinds(OutputMode::Shared).first()?;
        backend::live_device_format(first, None)
    }

    /// DSP settings as handed to the decoder: volume is applied by the pump so
    /// the slider never rebuilds a chain, and therefore never glitches.
    fn playback_dsp(&self) -> DspSettings {
        DspSettings {
            volume_db: 0.0,
            ..self.config.dsp.clone()
        }
    }

    /// Fill the ring before the device clock starts.
    fn prime(&mut self) -> Result<(), AudioError> {
        for _ in 0..8 {
            if !self.fill_once()? {
                break;
            }
        }
        Ok(())
    }

    /// Produce up to one period of audio and hand it to the ring.
    ///
    /// Returns `false` when there is nothing to do right now: the ring is full,
    /// the device has not made room, or the queue has been played to its end.
    ///
    /// Blocks are never truncated to fit. The decoder answers in whole
    /// resampler chunks, which are several times a device period whenever the
    /// file rate differs from the endpoint rate, so the ring is frequently
    /// smaller than the block in hand. The remainder is kept in `carry` and
    /// pushed first on the next call — the alternative, dropping it, is a click
    /// per block, and it is exactly what made 44.1 kHz material sound broken on
    /// a 192 kHz endpoint while 192 kHz files sounded fine.
    fn fill_once(&mut self) -> Result<bool, AudioError> {
        let Some((period, channels, free)) = self.output.as_ref().map(|output| {
            (
                output.negotiation.period_frames as usize,
                usize::from(output.negotiation.format.channels),
                output.producer.free_frames(),
            )
        }) else {
            return Ok(false);
        };

        // 1. Anything left over from the previous block goes first, in order.
        if !self.carry.is_empty() {
            if free == 0 {
                return Ok(false);
            }
            let mut block = std::mem::take(&mut self.carry);
            let boundary = self.carry_boundary.take();
            let finished = self.carry_finished;
            let before = self.pushed;
            let (accepted, rest) = self.push_block(&mut block, channels, free);
            if let Some(offset) = boundary {
                if offset <= accepted {
                    self.schedule_boundary(before + offset);
                } else {
                    self.carry_boundary = Some(offset - accepted);
                }
            }
            self.carry = rest;
            self.carry_finished = finished;
            return Ok(true);
        }

        if free < period {
            return Ok(false);
        }

        let Some(player) = self.player.as_mut() else {
            return Ok(false);
        };
        let chunk = player.next_chunk(period)?;
        if chunk.samples.is_empty() {
            return Ok(false);
        }
        let mut samples = chunk.samples;
        let boundary = chunk.boundary_offset_frames;

        // A track boundary that also changes the native format forces a device
        // renegotiation: keep the old track's tail, drop the new track's head
        // (≤ one period) and reopen the device on the new clock.
        if chunk.started.is_some() && self.should_renegotiate() {
            if let Some(offset) = boundary {
                samples.truncate(offset as usize * channels);
            }
            let (_, rest) = self.push_block(&mut samples, channels, free);
            debug_assert!(rest.is_empty(), "renegotiation keeps at most one period");
            self.carry.clear();
            self.carry_boundary = None;
            self.carry_finished = false;
            self.index = (self.index + 1).min(self.queue.len().saturating_sub(1));
            self.renegotiate()?;
            return Ok(false);
        }

        let before = self.pushed;
        let (accepted, rest) = self.push_block(&mut samples, channels, free);
        if let Some(offset) = boundary {
            if offset <= accepted {
                // Clamp to what was really written so an overrun cannot move
                // the boundary past the end of the block.
                self.schedule_boundary(before + offset);
            } else {
                self.carry_boundary = Some(offset - accepted);
            }
        }

        let finished =
            chunk.ended.is_some() && self.player.as_ref().is_some_and(GaplessPlayer::is_finished);
        self.carry = rest;
        self.carry_finished = finished;
        if finished && self.carry.is_empty() {
            return Ok(false);
        }
        Ok(true)
    }

    /// Apply the volume ramp and hand `samples` to the ring, up to
    /// `capacity_frames`.
    ///
    /// Returns the frames accepted and — when the block did not fit — the
    /// remainder as an owned interleaved buffer, ready to be carried.
    fn push_block(
        &mut self,
        samples: &mut [f32],
        channels: usize,
        capacity_frames: usize,
    ) -> (u64, Vec<f32>) {
        if channels == 0 || samples.is_empty() {
            return (0, Vec::new());
        }
        let target = self.target_gain();
        apply_gain(samples, channels, &mut self.gain, target);

        // Spectrum tap: post-DSP, post-gain, pre-ring. A no-op unless the
        // visualiser panel has asked for frames.
        if self.analyzer.is_enabled()
            && let Some(output) = self.output.as_ref()
        {
            self.analyzer
                .push(samples, channels, output.negotiation.format.sample_rate);
        }

        let Some(output) = self.output.as_mut() else {
            return (0, Vec::new());
        };
        let frames = samples.len() / channels;
        let take = frames.min(capacity_frames);
        let written = output.producer.push_partial(&samples[..take * channels]);
        self.pushed = self.pushed.saturating_add(written as u64);
        if written < take {
            output.stats.overruns.fetch_add(1, Ordering::Relaxed);
            tracing::warn!(
                dropped = take - written,
                "the ring reported free space it could not honour"
            );
        }
        let rest = if written < frames {
            samples[written * channels..].to_vec()
        } else {
            Vec::new()
        };
        (written as u64, rest)
    }

    fn target_gain(&self) -> f32 {
        if self.muted.load(Ordering::Relaxed) {
            return 0.0;
        }
        let db = f32::from_bits(self.volume_db.load(Ordering::Relaxed));
        10f32.powf(db / 20.0).clamp(0.0, 1.0)
    }

    fn schedule_boundary(&mut self, at: u64) {
        let Some(player) = self.player.as_ref() else {
            return;
        };
        let Some(request) = player.current_request().cloned() else {
            return;
        };
        let format = player.current_format().unwrap_or(AudioFormat {
            sample_rate: 44_100,
            channels: 2,
            sample_format: crate::audio::format::SampleFormat::F32,
        });
        self.playhead.pending = Some(Boundary {
            at,
            request,
            duration: player.current_total(),
            format,
        });
    }

    fn should_renegotiate(&self) -> bool {
        let Some(player) = self.player.as_ref() else {
            return false;
        };
        let (Some(next), Some(output)) = (player.current_format(), self.output.as_ref()) else {
            return false;
        };
        let current = output.negotiation.format;
        if self.negotiated_exclusive {
            // An exclusive stream *is* the record's format: anything that
            // differs has to be reopened or it would be resampled.
            return next.sample_rate != current.sample_rate
                || next.channels != current.channels
                || next.sample_format != current.sample_format;
        }
        // In shared mode only the rate is worth reopening for, and only when
        // this endpoint has already proved it can follow one. Without that
        // proof (Windows shared mode is pinned to the system mix format) every
        // boundary would reopen the device and be granted the same format,
        // which turns a gapless album into a stutter.
        self.config.rate_policy == RatePolicy::MatchSource
            && self.follows_source_rate
            && next.sample_rate != current.sample_rate
    }

    fn renegotiate(&mut self) -> Result<(), AudioError> {
        let index = self.index;
        self.set_state(PlaybackState::Buffering);
        // `start_queue` tears down the old sink and rebuilds the player from
        // the new track, which is exactly the boundary we just crossed.
        self.start_queue(index, true)
    }

    fn pump(&mut self) -> Result<(), AudioError> {
        if self
            .output
            .as_ref()
            .is_some_and(|output| !output.refill.is_open())
        {
            return Err(AudioError::Device(
                "the output device reported an unrecoverable error".into(),
            ));
        }

        for _ in 0..4 {
            if !self.fill_once()? {
                break;
            }
        }

        self.promote_if_audible();
        self.drain_skips();

        // Everything has been decoded *and* played: stop cleanly instead of
        // letting the ring starve into an underrun storm.
        if self.player.as_ref().is_some_and(GaplessPlayer::is_finished)
            && self.backlog() == 0
            && self.carry.is_empty()
        {
            self.teardown();
            return Ok(());
        }
        self.check_underruns();
        Ok(())
    }

    /// Move the playhead onto the pending track once its first frame is
    /// actually audible.
    fn promote_if_audible(&mut self) {
        let Some(pending) = self.playhead.pending.take() else {
            return;
        };
        if self.pushed.saturating_sub(self.backlog()) >= pending.at {
            self.playhead.anchor = pending.at;
            self.playhead.base = 0;
            self.playhead.format = Some(pending.format);
            self.playhead.duration = pending.duration;
            self.playhead.request = Some(pending.request.clone());
            self.emit(EngineEvent::TrackChanged {
                track: NowPlaying {
                    duration_frames: pending.duration,
                    source_format: Some(pending.format),
                    ..NowPlaying::from(&pending.request)
                },
                index: self.index,
            });
            self.publish(true);
        } else {
            self.playhead.pending = Some(pending);
        }
    }

    /// Forward unplayable-track reports from the decoder to the UI.
    fn drain_skips(&mut self) {
        let Some(player) = self.player.as_mut() else {
            return;
        };
        for skip in player.take_skipped() {
            self.emit(EngineEvent::TrackSkipped {
                track_id: skip.track_id,
                title: skip.title,
                artist: skip.artist,
                reason: skip.reason,
            });
        }
    }

    fn check_underruns(&mut self) {
        let Some(output) = &self.output else {
            return;
        };
        let total = output.stats.underruns.load(Ordering::Relaxed);
        if total > 0 && total.is_multiple_of(50) {
            self.emit(EngineEvent::Underrun { total });
        }
    }

    fn backlog(&self) -> u64 {
        self.output
            .as_ref()
            .map_or(0, |output| output.producer.occupied_frames() as u64)
    }

    /// Playhead in **source** frames.
    ///
    /// `pushed` and `backlog` count frames as they reach the device — that is,
    /// after the DSP chain, at the negotiated output rate. Seek offsets and
    /// durations are in source frames. Whenever the two rates differ the
    /// elapsed delta has to be rescaled, or the counter runs at the ratio:
    /// a 44.1 kHz file on a 192 kHz endpoint advanced 4.35× too fast and the
    /// slider hit the end of the track in the first ten seconds. This is the
    /// single bug behind "the scrubber does not advance correctly".
    fn position_frames(&self) -> u64 {
        let audible = self.pushed.saturating_sub(self.backlog());
        let elapsed_output = audible.saturating_sub(self.playhead.anchor);
        let output_rate = self
            .output
            .as_ref()
            .map_or(0, |output| output.negotiation.format.sample_rate);
        let source_rate = self.playhead.format.map_or(0, |format| format.sample_rate);
        let elapsed_source = if output_rate == 0 || source_rate == 0 || output_rate == source_rate {
            elapsed_output
        } else {
            // Multiply first, divide second: 64-bit headroom is ample for a
            // whole track at 768 kHz.
            elapsed_output.saturating_mul(u64::from(source_rate)) / u64::from(output_rate)
        };
        self.playhead.base.saturating_add(elapsed_source)
    }

    fn seek(&mut self, frame: u64) -> Result<(), AudioError> {
        let (Some(player), Some(output)) = (self.player.as_mut(), self.output.as_mut()) else {
            return Err(AudioError::NotRunning);
        };
        let was_paused = self.state == PlaybackState::Paused;
        output.sink.pause()?;
        output.producer.clear();
        player.seek(frame)?;

        self.pushed = 0;
        self.gain = 0.0;
        self.playhead.anchor = 0;
        self.playhead.base = frame;
        self.playhead.pending = None;

        self.prime()?;
        // The device keeps running; the short pre-roll is silence.
        if !was_paused {
            if let Some(output) = &mut self.output {
                output.sink.resume()?;
            }
            self.set_state(PlaybackState::Playing);
        }
        self.publish(true);
        Ok(())
    }

    fn is_bit_perfect(&self) -> bool {
        let Some(output) = &self.output else {
            return false;
        };
        if !output.negotiation.kind.is_exclusive() {
            return false;
        }
        if self.muted.load(Ordering::Relaxed) {
            return false;
        }
        if f32::from_bits(self.volume_db.load(Ordering::Relaxed)).abs() > f32::EPSILON {
            return false;
        }
        let Some(source) = self.playhead.format else {
            return false;
        };
        // DoP is bit-perfect as long as the carrier reaches the DAC at its own
        // rate: the "DSP" for DSD is the marker embedding itself.
        if self
            .player
            .as_ref()
            .is_some_and(GaplessPlayer::current_is_dop)
        {
            return source.sample_rate == output.negotiation.format.sample_rate;
        }
        self.config
            .dsp
            .is_bit_perfect(&source, &output.negotiation.format)
    }

    fn recover(&mut self) {
        let position = self.position_frames();
        let index = self.index;
        tracing::warn!(position, index, "recovering the output device");
        if let Err(error) = self.start_queue(index, false) {
            tracing::error!(%error, "device recovery failed");
            self.teardown_playback();
            self.set_state(PlaybackState::Stopped);
            self.emit(EngineEvent::OutputError {
                message: format!("output lost: {error}"),
            });
            return;
        }
        if let Err(error) = self.seek(position) {
            tracing::error!(%error, "cannot restore the playhead after recovery");
        }
    }

    fn fail(&mut self) {
        self.teardown_playback();
        self.set_state(PlaybackState::Stopped);
    }

    /// Stop the sink and the decoder but keep the queue and the playhead.
    fn teardown_playback(&mut self) {
        self.carry.clear();
        self.carry_boundary = None;
        self.carry_finished = false;
        if let Some(mut output) = self.output.take() {
            output.refill.close();
            output.sink.stop();
        }
        if let Some(mut player) = self.player.take() {
            player.shutdown();
        }
        self.gain = 0.0;
    }

    fn teardown(&mut self) {
        self.teardown_playback();
        self.queue.clear();
        self.index = 0;
        self.playhead = Playhead::new();
        self.pushed = 0;
        self.set_state(PlaybackState::Stopped);
        self.publish(true);
    }

    fn set_state(&mut self, state: PlaybackState) {
        if self.state == state {
            return;
        }
        self.state = state;
        self.emit(EngineEvent::StateChanged { state });
        self.publish(true);
    }

    fn publish(&mut self, force: bool) {
        let now = Instant::now();
        if !force && now.duration_since(self.last_publish) < Duration::from_millis(40) {
            return;
        }
        self.last_publish = now;

        let (backend, device_name, output_format, latency, underruns, overruns) = match &self.output
        {
            Some(output) => (
                Some(output.negotiation.kind),
                Some(output.negotiation.device.name.clone()),
                Some(output.negotiation.format),
                output.negotiation.latency_frames,
                output.stats.underruns.load(Ordering::Relaxed),
                output.stats.overruns.load(Ordering::Relaxed),
            ),
            None => (None, None, None, 0, 0, 0),
        };

        // The length of the first track is not known when the queue starts —
        // the player has not opened the file yet — so `Playhead::reset` leaves
        // it empty and nothing ever filled it in. Every snapshot therefore said
        // "length unknown" for the whole of the first track, and the bar, which
        // needs a maximum to be draggable, stayed disabled at 0:00.
        let duration = self.current_duration();
        let track = self.playhead.request.as_ref().map(|request| NowPlaying {
            duration_frames: duration,
            source_format: self.playhead.format,
            ..NowPlaying::from(request)
        });

        let mut status = self.status.write();
        status.state = self.state;
        status.track = track;
        status.position_frames = self.position_frames();
        status.duration_frames = duration;
        status.source_format = self.playhead.format;
        status.output_format = output_format;
        status.device_format = self.device_format;
        status.backend = backend;
        status.device_name = device_name;
        status.dop = self
            .player
            .as_ref()
            .is_some_and(GaplessPlayer::current_is_dop);
        status.bit_perfect = self.is_bit_perfect();
        status.volume_db = f32::from_bits(self.volume_db.load(Ordering::Relaxed));
        status.muted = self.muted.load(Ordering::Relaxed);
        status.latency_frames = latency;
        status.underruns = underruns;
        status.overruns = overruns;
        status.queue_index = self.index;
        status.queue_len = self.queue.len();
    }

    /// Length of the track whose playhead is live, in source frames.
    ///
    /// Read from the playhead when it knows, and otherwise asked of the player
    /// once and cached. While a boundary is pending the player has already
    /// moved on to the *next* file, so its answer belongs to that track and
    /// must not be borrowed by this one.
    fn current_duration(&mut self) -> Option<u64> {
        if self.playhead.duration.is_none() && self.playhead.pending.is_none() {
            self.playhead.duration = self.player.as_ref().and_then(GaplessPlayer::current_total);
        }
        self.playhead.duration
    }

    fn emit(&self, event: EngineEvent) {
        let _ = self.events.send(event);
    }
}

/// Linear gain ramp across one block, applied per frame so the two channels of
/// a stereo pair never drift apart.
fn apply_gain(samples: &mut [f32], channels: usize, current: &mut f32, target: f32) {
    if channels == 0 {
        return;
    }
    if (*current - target).abs() <= f32::EPSILON {
        *current = target;
        if (target - 1.0).abs() > f32::EPSILON {
            for sample in samples.iter_mut() {
                *sample *= target;
            }
        }
        return;
    }

    let frames = samples.len() / channels;
    if frames == 0 {
        return;
    }
    let step = (target - *current) / frames as f32;
    let mut gain = *current;
    for frame in samples.chunks_exact_mut(channels) {
        gain += step;
        for sample in frame {
            *sample *= gain;
        }
    }
    *current = target;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gain_ramp_reaches_the_target() {
        let mut samples = vec![1.0f32; 8];
        let mut current = 0.0f32;
        apply_gain(&mut samples, 2, &mut current, 1.0);
        assert!((current - 1.0).abs() < f32::EPSILON);
        assert!((samples[7] - 1.0).abs() < 1e-6);
        assert!(samples[0] > 0.0 && samples[0] < samples[7]);
    }

    #[test]
    fn gain_is_skipped_at_unity() {
        let mut samples = vec![0.5f32, -0.5];
        let mut current = 1.0f32;
        apply_gain(&mut samples, 2, &mut current, 1.0);
        assert_eq!(samples, vec![0.5, -0.5]);
    }

    #[test]
    fn mute_ramps_to_silence() {
        let mut samples = vec![1.0f32; 4];
        let mut current = 1.0f32;
        apply_gain(&mut samples, 2, &mut current, 0.0);
        assert!(samples.iter().all(|sample| sample.abs() < 0.6));
        assert!(current.abs() < f32::EPSILON);
    }
}
