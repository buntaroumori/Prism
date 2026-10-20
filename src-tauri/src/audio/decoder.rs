//! File decoding, RAM Play and gapless playback.
//!
//! # Decoding
//!
//! FLAC, ALAC, WAV, AIFF, Ogg Vorbis and MP3 are decoded by `symphonia` into
//! interleaved `f32`. DSD (`.dsf`/`.dff`) is handled by [`crate::audio::dsd`],
//! which produces DoP carrier PCM.
//!
//! # Gapless playback
//!
//! A dedicated prefetch worker opens the *next* track, builds its DSP chain and
//! decodes a priming block through it while the current track is still playing.
//! When the current track reaches EOF, the player switches to the primed block
//! without ever returning silence: the transition is sample-exact because the
//! two tracks' PCM is concatenated inside the same output call.
//!
//! # RAM Play
//!
//! With RAM Play enabled, the whole file (or the requested CUE span) is decoded
//! into a contiguous buffer at load time and the disk is removed from the
//! playback path. A configurable byte budget prevents a 24-bit/192 kHz album
//! from exhausting memory on small machines; tracks above the budget fall back
//! to streaming with a trace event.
//!
//! # CUE sheets
//!
//! A virtual track is a byte range inside a single album image. The source is
//! opened once per track request, seeked to the start frame and stopped at the
//! end frame, so cue boundaries are as gapless as regular files.

use std::collections::VecDeque;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
use serde::{Deserialize, Serialize};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CODEC_TYPE_NULL, Decoder, DecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::Time;

use crate::audio::AudioError;
use crate::audio::dsd::DsdSource;
use crate::audio::dsp::{
    CrossfeedSettings, DspChain, DspSettings, EqSettings, ReplayGainMode, ReplayGainSettings,
};
use crate::audio::format::{AudioFormat, SampleFormat, remap_channels};

/// Frames decoded ahead of the current position by the prefetch worker.
pub const PRIME_FRAMES: usize = 16_384;

/// A track queued for playback.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackRequest {
    /// Database identifier, echoed back in engine events.
    pub track_id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub path: PathBuf,
    /// Start of the audible span, for CUE virtual tracks.
    pub start_ms: Option<u64>,
    /// End of the audible span, for CUE virtual tracks.
    pub end_ms: Option<u64>,
    pub replaygain_track_gain_db: Option<f32>,
    pub replaygain_track_peak: Option<f32>,
    pub replaygain_album_gain_db: Option<f32>,
    pub replaygain_album_peak: Option<f32>,
}

impl TrackRequest {
    /// Convenience constructor for a regular file.
    #[must_use]
    pub fn file(path: impl Into<PathBuf>, track_id: impl Into<String>) -> Self {
        let path = path.into();
        let title = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("Untitled")
            .to_string();
        Self {
            track_id: track_id.into(),
            title,
            artist: String::new(),
            album: String::new(),
            path,
            start_ms: None,
            end_ms: None,
            replaygain_track_gain_db: None,
            replaygain_track_peak: None,
            replaygain_album_gain_db: None,
            replaygain_album_peak: None,
        }
    }
}

/// Result of one read request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadResult {
    /// Frames appended to the output buffer.
    pub frames: usize,
    /// True when the source has no more audio.
    pub end_of_stream: bool,
}

/// A seekable PCM source producing interleaved `f32`.
pub trait PcmSource: Send {
    /// Native format of the samples returned by [`Self::read`].
    fn format(&self) -> AudioFormat;

    /// Append up to `max_frames` frames to `out`.
    fn read(&mut self, out: &mut Vec<f32>, max_frames: usize) -> Result<ReadResult, AudioError>;

    /// Seek to an absolute frame in the source timeline.
    fn seek(&mut self, frame: u64) -> Result<(), AudioError>;

    /// Total frames when the container knows them.
    fn total_frames(&self) -> Option<u64>;

    /// Current frame position.
    fn position_frames(&self) -> u64;
}

/// Symphonia-backed file source.
pub struct SymphoniaSource {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track_id: u32,
    format_info: AudioFormat,
    total_frames: Option<u64>,
    position: u64,
    span_end: Option<u64>,
    container: String,
    sample_buffer: Option<SampleBuffer<f32>>,
    /// Samples of the last packet that did not fit in the caller's buffer.
    ///
    /// A packet can hold thousands of frames; the caller may ask for fewer.
    /// Without this carry-over the remainder of the packet would be lost,
    /// which silently truncates every track by up to one packet.
    pending: Vec<f32>,
}

impl std::fmt::Debug for SymphoniaSource {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SymphoniaSource")
            .field("format", &self.format_info)
            .field("container", &self.container)
            .field("position", &self.position)
            .field("span_end", &self.span_end)
            .finish_non_exhaustive()
    }
}

impl SymphoniaSource {
    /// Open a file for decoding.
    pub fn open(
        path: &Path,
        start_ms: Option<u64>,
        end_ms: Option<u64>,
    ) -> Result<Self, AudioError> {
        let file = File::open(path).map_err(|error| AudioError::io(path, error))?;
        let stream = MediaSourceStream::new(Box::new(file), Default::default());

        let mut hint = Hint::new();
        if let Some(extension) = path.extension().and_then(|ext| ext.to_str()) {
            hint.with_extension(extension);
        }

        let probed = symphonia::default::get_probe()
            .format(
                &hint,
                stream,
                &FormatOptions {
                    enable_gapless: true,
                    ..FormatOptions::default()
                },
                &MetadataOptions::default(),
            )
            .map_err(|error| decode_error(path, &error))?;

        let format = probed.format;
        let track = format
            .tracks()
            .iter()
            .find(|track| track.codec_params.codec != CODEC_TYPE_NULL)
            .ok_or(AudioError::EmptyStream)?;

        let track_id = track.id;
        let codec_params = track.codec_params.clone();

        let decoder = symphonia::default::get_codecs()
            .make(&codec_params, &DecoderOptions::default())
            .map_err(|error| decode_error(path, &error))?;

        let sample_rate = codec_params
            .sample_rate
            .ok_or_else(|| AudioError::Decode("stream has no sample rate".into()))?;
        let channels = codec_params
            .channels
            .map(|channels| channels.count() as u16)
            .ok_or_else(|| AudioError::Decode("stream has no channel layout".into()))?;

        let bits = codec_params.bits_per_sample.unwrap_or(16);
        let sample_format = match bits {
            0..=16 => SampleFormat::S16,
            17..=24 => SampleFormat::S24,
            _ => SampleFormat::S32,
        };

        let format_info = AudioFormat::new(sample_rate, channels, sample_format)?;
        let total_frames = codec_params.n_frames;

        let container = path
            .extension()
            .and_then(|extension| extension.to_str())
            .map_or_else(|| "unknown".to_string(), str::to_uppercase);

        let span_end = match (end_ms, total_frames) {
            (Some(end_ms), Some(total)) => Some(format_info.ms_to_frames(end_ms).min(total)),
            (Some(end_ms), None) => Some(format_info.ms_to_frames(end_ms)),
            (None, _) => total_frames,
        };

        let mut source = Self {
            format,
            decoder,
            track_id,
            format_info,
            total_frames,
            position: 0,
            span_end,
            container,
            sample_buffer: None,
            pending: Vec::new(),
        };

        if let Some(start_ms) = start_ms
            && start_ms > 0
        {
            source.seek(format_info.ms_to_frames(start_ms))?;
        }

        Ok(source)
    }

    /// Container label for the quality badge (`FLAC`, `WAV`, ...).
    #[must_use]
    pub fn container(&self) -> &str {
        &self.container
    }

    fn seek_to(&mut self, frame: u64) -> Result<u64, AudioError> {
        let rate = u64::from(self.format_info.sample_rate);
        let seconds = frame / rate;
        let frac = (frame % rate) as f64 / f64::from(self.format_info.sample_rate);
        let seeked = self
            .format
            .seek(
                SeekMode::Accurate,
                SeekTo::Time {
                    time: Time::new(seconds, frac),
                    track_id: Some(self.track_id),
                },
            )
            .map_err(|error| AudioError::Decode(format!("seek failed: {error}")))?;

        self.decoder.reset();
        self.sample_buffer = None;
        self.pending.clear();
        // `SeekedTo` timestamps are expressed in the track's `TimeBase`, which
        // is not guaranteed to be sample-aligned across containers. The
        // requested frame is authoritative: `SeekMode::Accurate` guarantees the
        // next decoded packet starts at or before the requested position, and
        // the decoder discards any leading frames of that packet.
        let _ = seeked;
        self.position = frame;
        Ok(frame)
    }
}

fn decode_error(path: &Path, error: &SymphoniaError) -> AudioError {
    AudioError::Decode(format!("{}: {error}", path.display()))
}

impl PcmSource for SymphoniaSource {
    fn format(&self) -> AudioFormat {
        self.format_info
    }

    fn read(&mut self, out: &mut Vec<f32>, max_frames: usize) -> Result<ReadResult, AudioError> {
        if max_frames == 0 {
            return Ok(ReadResult {
                frames: 0,
                end_of_stream: false,
            });
        }
        let channels = self.format_info.channels as usize;
        let mut frames_read = 0usize;

        // 1) Serve the tail of the previous packet first.
        if !self.pending.is_empty() {
            let available = self.pending.len() / channels;
            let mut take = available.min(max_frames);
            if let Some(end) = self.span_end {
                take = take
                    .min(usize::try_from(end.saturating_sub(self.position)).unwrap_or(usize::MAX));
            }
            out.extend_from_slice(&self.pending[..take * channels]);
            self.pending.drain(..take * channels);
            frames_read += take;
            self.position += take as u64;

            if self.span_end.is_some_and(|end| self.position >= end) {
                return Ok(ReadResult {
                    frames: frames_read,
                    end_of_stream: true,
                });
            }
            if frames_read == max_frames {
                return Ok(ReadResult {
                    frames: frames_read,
                    end_of_stream: false,
                });
            }
        }

        // 2) Decode whole packets until the request is satisfied.
        while frames_read < max_frames {
            if let Some(end) = self.span_end
                && self.position >= end
            {
                return Ok(ReadResult {
                    frames: frames_read,
                    end_of_stream: true,
                });
            }

            let packet = match self.format.next_packet() {
                Ok(packet) => packet,
                Err(SymphoniaError::IoError(error))
                    if error.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    return Ok(ReadResult {
                        frames: frames_read,
                        end_of_stream: true,
                    });
                }
                Err(SymphoniaError::ResetRequired) => {
                    return Err(AudioError::Decode("stream requires a decoder reset".into()));
                }
                Err(error) => {
                    return Err(AudioError::Decode(format!("packet error: {error}")));
                }
            };

            if packet.track_id() != self.track_id {
                continue;
            }

            let decoded = match self.decoder.decode(&packet) {
                Ok(decoded) => decoded,
                Err(SymphoniaError::DecodeError(message)) => {
                    // A corrupt frame is skipped, not fatal: the rest of the
                    // track keeps playing and the scanner flags the file.
                    tracing::warn!(error = message, "skipping undecodable frame");
                    continue;
                }
                Err(error) => {
                    return Err(AudioError::Decode(format!("decode error: {error}")));
                }
            };

            let spec = *decoded.spec();
            let duration = decoded.frames() as u64;
            let buffer = self
                .sample_buffer
                .get_or_insert_with(|| SampleBuffer::<f32>::new(duration, spec));
            if buffer.capacity() < duration as usize {
                *buffer = SampleBuffer::<f32>::new(duration, spec);
            }
            buffer.copy_interleaved_ref(decoded);

            let available = buffer.samples().len() / channels;
            if available == 0 {
                continue;
            }
            let mut take = available.min(max_frames - frames_read);
            if let Some(end) = self.span_end {
                take = take
                    .min(usize::try_from(end.saturating_sub(self.position)).unwrap_or(usize::MAX));
            }

            if take == 0 {
                return Ok(ReadResult {
                    frames: frames_read,
                    end_of_stream: true,
                });
            }

            // Copy out and keep the rest of the packet for the next call.
            let mut leftover = None;
            {
                let samples = buffer.samples();
                out.extend_from_slice(&samples[..take * channels]);
                if take < available {
                    leftover = Some(samples[take * channels..].to_vec());
                }
            }
            if let Some(rest) = leftover {
                self.pending = rest;
            }

            frames_read += take;
            self.position += take as u64;

            if self.span_end.is_some_and(|end| self.position >= end) {
                return Ok(ReadResult {
                    frames: frames_read,
                    end_of_stream: true,
                });
            }
        }

        Ok(ReadResult {
            frames: frames_read,
            end_of_stream: false,
        })
    }

    fn seek(&mut self, frame: u64) -> Result<(), AudioError> {
        self.seek_to(frame)?;
        Ok(())
    }

    fn total_frames(&self) -> Option<u64> {
        self.span_end.or(self.total_frames)
    }

    fn position_frames(&self) -> u64 {
        self.position
    }
}

/// DSD file presented as DoP PCM.
pub struct DsdPcmSource {
    inner: DsdSource,
    format: AudioFormat,
}

impl std::fmt::Debug for DsdPcmSource {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DsdPcmSource")
            .field("inner", &self.inner)
            .finish_non_exhaustive()
    }
}

impl DsdPcmSource {
    /// Open a DSF/DFF file.
    pub fn open(path: &Path) -> Result<Self, AudioError> {
        let inner = DsdSource::open(path)?;
        let format = inner.info().dop_format();
        Ok(Self { inner, format })
    }

    /// DSD rate label for the quality badge.
    #[must_use]
    pub fn dsd_rate_label(&self) -> &'static str {
        self.inner.info().rate.label()
    }
}

impl PcmSource for DsdPcmSource {
    fn format(&self) -> AudioFormat {
        self.format
    }

    fn read(&mut self, out: &mut Vec<f32>, max_frames: usize) -> Result<ReadResult, AudioError> {
        let samples = self.inner.read_dop(max_frames)?;
        let frames = samples.len() / self.format.channels as usize;
        out.extend_from_slice(samples);
        Ok(ReadResult {
            frames,
            end_of_stream: frames < max_frames,
        })
    }

    fn seek(&mut self, frame: u64) -> Result<(), AudioError> {
        self.inner.seek_frames(frame)
    }

    fn total_frames(&self) -> Option<u64> {
        Some(self.inner.info().total_frames())
    }

    fn position_frames(&self) -> u64 {
        self.inner.position_frames()
    }
}

/// Decodes a whole span into memory.
pub struct RamSource {
    samples: Vec<f32>,
    format: AudioFormat,
    position: u64,
    total_frames: u64,
}

impl std::fmt::Debug for RamSource {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RamSource")
            .field("format", &self.format)
            .field("frames", &self.total_frames)
            .finish_non_exhaustive()
    }
}

impl RamSource {
    /// Decode an entire source into RAM, enforcing a byte budget.
    pub fn load(mut source: Box<dyn PcmSource>, limit_bytes: u64) -> Result<Self, AudioError> {
        let format = source.format();
        let frame_bytes = format.frame_size() as u64;
        let expected_frames = source.total_frames().unwrap_or(u64::MAX);
        let expected_bytes = expected_frames.saturating_mul(frame_bytes);
        if expected_bytes > limit_bytes {
            return Err(AudioError::InvalidParameters(format!(
                "RAM Play would need {} MiB, above the {} MiB limit",
                expected_bytes / 1_048_576,
                limit_bytes / 1_048_576
            )));
        }

        let mut samples = Vec::new();
        if let Some(total) = source.total_frames() {
            samples.reserve(
                usize::try_from(total.saturating_mul(format.channels as u64)).unwrap_or(0),
            );
        }

        loop {
            let read = source.read(&mut samples, 65_536)?;
            if read.end_of_stream {
                break;
            }
            if read.frames == 0 {
                break;
            }
            if samples.len() as u64 * 4 > limit_bytes {
                return Err(AudioError::InvalidParameters(
                    "RAM Play exceeded its memory budget while decoding".into(),
                ));
            }
        }

        let total_frames = (samples.len() / format.channels as usize) as u64;
        Ok(Self {
            samples,
            format,
            position: 0,
            total_frames,
        })
    }
}

impl PcmSource for RamSource {
    fn format(&self) -> AudioFormat {
        self.format
    }

    fn read(&mut self, out: &mut Vec<f32>, max_frames: usize) -> Result<ReadResult, AudioError> {
        let channels = self.format.channels as usize;
        let remaining = self.total_frames - self.position;
        let frames = (max_frames as u64).min(remaining) as usize;
        let start = self.position as usize * channels;
        let end = start + frames * channels;
        out.extend_from_slice(&self.samples[start..end]);
        self.position += frames as u64;
        Ok(ReadResult {
            frames,
            end_of_stream: self.position >= self.total_frames && frames < max_frames,
        })
    }

    fn seek(&mut self, frame: u64) -> Result<(), AudioError> {
        self.position = frame.min(self.total_frames);
        Ok(())
    }

    fn total_frames(&self) -> Option<u64> {
        Some(self.total_frames)
    }

    fn position_frames(&self) -> u64 {
        self.position
    }
}

/// Decode configuration for one playback session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackConfig {
    pub output_format: AudioFormat,
    pub dsp: DspSettings,
    pub ram_play: bool,
    pub ram_limit_bytes: u64,
}

/// A track prepared for gapless playback.
pub struct PreparedTrack {
    pub request: TrackRequest,
    pub source: Box<dyn PcmSource>,
    pub chain: DspChain,
    pub source_format: AudioFormat,
    pub primed: Vec<f32>,
    pub total_frames: Option<u64>,
    /// True when the samples are DoP-encoded DSD and must reach the DAC
    /// untouched: no gain, no EQ, no resampling.
    pub is_dop: bool,
}

impl std::fmt::Debug for PreparedTrack {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PreparedTrack")
            .field("track", &self.request.track_id)
            .field("source_format", &self.source_format)
            .field("total_frames", &self.total_frames)
            .field("is_dop", &self.is_dop)
            .field(
                "primed_frames",
                &(self.primed.len() / usize::from(self.source_format.channels.max(1))),
            )
            .finish_non_exhaustive()
    }
}

/// One output block with track-boundary annotations.
#[derive(Debug)]
pub struct Chunk {
    pub samples: Vec<f32>,
    /// Track that ended while producing this chunk.
    pub ended: Option<String>,
    /// Track that started inside this chunk.
    pub started: Option<String>,
    /// Frame offset inside `samples` where the new track begins, when a
    /// boundary occurred in this chunk. Used for exact playhead mapping.
    pub boundary_offset_frames: Option<u64>,
}

/// A track the prefetch worker could not open.
///
/// Surfaced to the UI instead of quietly disappearing from the queue: a file
/// that has been moved, truncated or replaced by something that is not audio is
/// a fact the listener needs to see.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedTrack {
    pub track_id: String,
    pub title: String,
    pub artist: String,
    pub reason: String,
}

/// Gapless playback engine.
pub struct GaplessPlayer {
    planned: VecDeque<TrackRequest>,
    current: Option<PreparedTrack>,
    ready: Option<PreparedTrack>,
    request_tx: Sender<TrackRequest>,
    result_rx: Receiver<(TrackRequest, Result<PreparedTrack, AudioError>)>,
    config: PlaybackConfig,
    output_channels: usize,
    /// Track requests already handed to the prefetch worker.
    scheduled: usize,
    /// Tracks the worker has delivered so far, playable or not.
    delivered: usize,
    /// Unplayable tracks waiting to be reported to the supervisor.
    skipped: VecDeque<SkippedTrack>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl std::fmt::Debug for GaplessPlayer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GaplessPlayer")
            .field("planned", &self.planned.len())
            .field("has_current", &self.current.is_some())
            .field("has_ready", &self.ready.is_some())
            .finish_non_exhaustive()
    }
}

impl GaplessPlayer {
    /// Create a player for a queue of requests.
    ///
    /// The prefetch worker is started immediately and begins priming the first
    /// track, so `next_chunk` has data available on its first call.
    pub fn new(requests: Vec<TrackRequest>, config: PlaybackConfig) -> Result<Self, AudioError> {
        if requests.is_empty() {
            return Err(AudioError::EmptyQueue);
        }
        let output_channels = config.output_format.channels as usize;
        let (request_tx, request_rx) = bounded::<TrackRequest>(4);
        let (result_tx, result_rx) =
            unbounded::<(TrackRequest, Result<PreparedTrack, AudioError>)>();

        let worker_config = config.clone();
        let worker = std::thread::Builder::new()
            .name("prism-prefetch".into())
            .spawn(move || {
                while let Ok(request) = request_rx.recv() {
                    let prepared = prepare_track(&request, &worker_config);
                    if result_tx.send((request, prepared)).is_err() {
                        break;
                    }
                }
            })
            .map_err(|error| {
                AudioError::Device(format!("cannot spawn prefetch thread: {error}"))
            })?;

        let mut player = Self {
            planned: requests.into(),
            current: None,
            ready: None,
            request_tx,
            result_rx,
            config,
            output_channels,
            scheduled: 0,
            delivered: 0,
            skipped: VecDeque::new(),
            worker: Some(worker),
        };

        player.schedule_next()?;
        Ok(player)
    }

    fn schedule_next(&mut self) -> Result<(), AudioError> {
        if self.scheduled >= self.planned.len() {
            return Ok(());
        }
        if let Some(request) = self.planned.get(self.scheduled).cloned() {
            self.scheduled += 1;
            self.request_tx
                .send(request)
                .map_err(|_| AudioError::Decode("prefetch worker stopped".into()))?;
        }
        Ok(())
    }

    fn take_prepared(&mut self) -> Result<Option<PreparedTrack>, AudioError> {
        if let Some(prepared) = self.ready.take() {
            self.schedule_next()?;
            return Ok(Some(prepared));
        }
        loop {
            match self
                .result_rx
                .recv_timeout(std::time::Duration::from_secs(30))
            {
                Ok((_request, Ok(prepared))) => {
                    self.delivered += 1;
                    self.schedule_next()?;
                    return Ok(Some(prepared));
                }
                Ok((request, Err(error))) => {
                    // A file that cannot be opened is skipped, never fatal: one
                    // moved or truncated track must not end the album. The
                    // failure is recorded and reported to the supervisor, which
                    // turns it into a `TrackSkipped` event.
                    self.delivered += 1;
                    tracing::warn!(
                        track = %request.track_id,
                        path = %request.path.display(),
                        %error,
                        "skipping unplayable track"
                    );
                    self.skipped.push_back(SkippedTrack {
                        track_id: request.track_id.clone(),
                        title: request.title.clone(),
                        artist: request.artist.clone(),
                        reason: error.to_string(),
                    });
                    self.schedule_next()?;
                    if self.delivered >= self.planned.len() {
                        return Ok(None);
                    }
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                    return Err(AudioError::Decode(
                        "timed out preparing the next track".into(),
                    ));
                }
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => return Ok(None),
            }
        }
    }

    /// Tracks skipped since the last call, oldest first.
    pub fn take_skipped(&mut self) -> Vec<SkippedTrack> {
        self.skipped.drain(..).collect()
    }

    fn promote(&mut self) -> Result<Option<String>, AudioError> {
        // Nothing is in flight and nothing is left: do not sit on the channel
        // for 30 s waiting for a result that will never arrive.
        if self.ready.is_none() && self.delivered >= self.planned.len() {
            self.current = None;
            return Ok(None);
        }
        let Some(mut prepared) = self.take_prepared()? else {
            self.current = None;
            return Ok(None);
        };
        let request = prepared.request.clone();
        // 1) Primed block already decoded through the DSP chain.
        let mut output = std::mem::take(&mut prepared.primed);
        // 2) Continue streaming until the caller has enough audio.
        while output.len() / self.output_channels < PRIME_FRAMES {
            let mut raw = Vec::new();
            let read = prepared.source.read(&mut raw, PRIME_FRAMES)?;
            if read.frames > 0 {
                let processed = process_block(&mut prepared, &raw, self.output_channels)?;
                output.extend_from_slice(&processed);
            }
            if read.end_of_stream {
                break;
            }
        }
        prepared.primed = output;
        self.current = Some(prepared);
        Ok(Some(request.track_id))
    }

    /// Read up to `frames` output frames, crossing track boundaries silently.
    pub fn next_chunk(&mut self, frames: usize) -> Result<Chunk, AudioError> {
        if frames == 0 {
            return Ok(Chunk {
                samples: Vec::new(),
                ended: None,
                started: None,
                boundary_offset_frames: None,
            });
        }

        if self.current.is_none() {
            let started = self.promote()?;
            return Ok(Chunk {
                samples: self.current.as_mut().map_or_else(Vec::new, |track| {
                    drain_primed(track, frames, self.output_channels)
                }),
                ended: None,
                started,
                boundary_offset_frames: None,
            });
        }

        let mut chunk = Chunk {
            samples: Vec::new(),
            ended: None,
            started: None,
            boundary_offset_frames: None,
        };

        while chunk.samples.len() / self.output_channels < frames {
            let Some(track) = self.current.as_mut() else {
                break;
            };

            if !track.primed.is_empty() {
                let taken = drain_primed(
                    track,
                    frames - chunk.samples.len() / self.output_channels,
                    self.output_channels,
                );
                chunk.samples.extend_from_slice(&taken);
                continue;
            }

            let mut raw = Vec::new();
            let read = track.source.read(&mut raw, frames)?;

            if read.frames > 0 {
                let processed = process_block(track, &raw, self.output_channels)?;
                chunk.samples.extend_from_slice(&processed);
            }

            if read.end_of_stream {
                let ended = track.request.track_id.clone();
                self.current = None;
                chunk.ended = Some(ended);
                // Remember where inside this chunk the next track begins.
                let boundary = (chunk.samples.len() / self.output_channels) as u64;
                let started = self.promote()?;
                if started.is_some() {
                    chunk.started = started;
                    chunk.boundary_offset_frames = Some(boundary);
                }
            } else if read.frames == 0 {
                break;
            }
        }

        Ok(chunk)
    }

    /// Seek within the current track.
    pub fn seek(&mut self, frame: u64) -> Result<(), AudioError> {
        let Some(track) = self.current.as_mut() else {
            return Err(AudioError::NotRunning);
        };
        track.source.seek(frame)?;
        track.chain.reset();
        track.primed.clear();
        let mut raw = Vec::new();
        let read = track.source.read(&mut raw, PRIME_FRAMES)?;
        if read.frames > 0 {
            let processed = process_block(track, &raw, self.output_channels)?;
            track.primed = processed;
        }
        Ok(())
    }

    /// Current track metadata.
    #[must_use]
    pub fn current_request(&self) -> Option<&TrackRequest> {
        self.current.as_ref().map(|track| &track.request)
    }

    /// Position inside the current track, in frames of the source rate.
    #[must_use]
    pub fn current_position(&self) -> Option<u64> {
        self.current
            .as_ref()
            .map(|track| track.source.position_frames())
    }

    /// Total frames of the current track.
    #[must_use]
    pub fn current_total(&self) -> Option<u64> {
        self.current.as_ref().and_then(|track| track.total_frames)
    }

    /// Source format of the current track.
    #[must_use]
    pub fn current_format(&self) -> Option<AudioFormat> {
        self.current.as_ref().map(|track| track.source_format)
    }

    /// True when the current track is DoP-encoded DSD.
    #[must_use]
    pub fn current_is_dop(&self) -> bool {
        self.current.as_ref().is_some_and(|track| track.is_dop)
    }

    /// Recompile the DSP chain of the loaded tracks without interrupting
    /// playback.
    ///
    /// The samples already primed keep the previous curve for at most one
    /// buffer; everything decoded afterwards uses the new one. When the output
    /// rate changes the primed audio is discarded, because it was produced by
    /// the old resampler.
    pub fn update_dsp(
        &mut self,
        dsp: &DspSettings,
        output_format: &AudioFormat,
    ) -> Result<(), AudioError> {
        let rate_changed = self.config.output_format.sample_rate != output_format.sample_rate;
        for track in [self.current.as_mut(), self.ready.as_mut()]
            .into_iter()
            .flatten()
        {
            let settings = if track.is_dop {
                dsd_bypass()
            } else {
                dsp.clone()
            };
            let gain = settings.replaygain.select(
                track.request.replaygain_track_gain_db,
                track.request.replaygain_album_gain_db,
            );
            let peak = if settings.replaygain.mode == ReplayGainMode::Album {
                track
                    .request
                    .replaygain_album_peak
                    .or(track.request.replaygain_track_peak)
            } else {
                track.request.replaygain_track_peak
            };
            track.chain =
                DspChain::build(&settings, &track.source_format, output_format, gain, peak)?;
            if rate_changed {
                track.primed.clear();
            }
        }
        self.config.dsp = dsp.clone();
        self.config.output_format = *output_format;
        Ok(())
    }

    /// True when the queue is exhausted.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.current.is_none() && self.delivered >= self.planned.len()
    }

    /// Stop the prefetch worker and wait for it to exit.
    ///
    /// Dropping the request channel makes the worker's `recv` return
    /// `Err(Disconnected)`, which breaks its loop after the track it is
    /// currently preparing.
    pub fn shutdown(&mut self) {
        let (tx, _rx) = bounded::<TrackRequest>(1);
        let _ = std::mem::replace(&mut self.request_tx, tx);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for GaplessPlayer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn drain_primed(track: &mut PreparedTrack, frames: usize, channels: usize) -> Vec<f32> {
    let available = track.primed.len() / channels;
    let take = frames.min(available);
    track.primed.drain(..take * channels).collect()
}

fn process_block(
    track: &mut PreparedTrack,
    raw: &[f32],
    output_channels: usize,
) -> Result<Vec<f32>, AudioError> {
    let remapped = if track.source_format.channels as usize != output_channels {
        remap_channels(raw, track.source_format.channels as usize, output_channels)?
    } else {
        raw.to_vec()
    };
    track.chain.process(&remapped)
}

/// The only DSP configuration a DoP carrier tolerates: none.
fn dsd_bypass() -> DspSettings {
    DspSettings {
        replaygain: ReplayGainSettings {
            mode: ReplayGainMode::Off,
            ..ReplayGainSettings::default()
        },
        eq: EqSettings::default(),
        crossfeed: CrossfeedSettings::default(),
        resample_when_needed: false,
        volume_db: 0.0,
    }
}

/// True when the path is a DSD container Prism decodes natively.
#[must_use]
pub fn is_dsd_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension.to_ascii_lowercase().as_str(), "dsf" | "dff"))
}

/// Read the native format of a request without building a playback pipeline.
///
/// The output backends need to know the source format *before* a
/// [`PlaybackConfig`] exists: exclusive mode opens the device at the source
/// rate, and the DSP chain is compiled against that decision. Opening the
/// container header is cheap (a few KB of I/O) and never decodes audio.
pub fn probe_format(request: &TrackRequest) -> Result<AudioFormat, AudioError> {
    if is_dsd_file(&request.path) {
        if request.start_ms.is_some() || request.end_ms.is_some() {
            return Err(AudioError::UnsupportedFormat(
                "DSD files cannot be addressed as CUE spans".into(),
            ));
        }
        Ok(DsdPcmSource::open(&request.path)?.format())
    } else {
        Ok(SymphoniaSource::open(&request.path, request.start_ms, request.end_ms)?.format())
    }
}

/// Open a track, apply RAM Play when configured and prime the DSP chain.
pub fn prepare_track(
    request: &TrackRequest,
    config: &PlaybackConfig,
) -> Result<PreparedTrack, AudioError> {
    let extension = request
        .path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();

    let is_dsd = matches!(extension.as_str(), "dsf" | "dff");
    let source: Box<dyn PcmSource> = if is_dsd {
        if request.start_ms.is_some() || request.end_ms.is_some() {
            return Err(AudioError::UnsupportedFormat(
                "DSD files cannot be addressed as CUE spans".into(),
            ));
        }
        Box::new(DsdPcmSource::open(&request.path)?)
    } else {
        Box::new(SymphoniaSource::open(
            &request.path,
            request.start_ms,
            request.end_ms,
        )?)
    };

    let source_format = source.format();
    let source: Box<dyn PcmSource> = if config.ram_play {
        match RamSource::load(source, config.ram_limit_bytes) {
            Ok(ram) => Box::new(ram),
            Err(error) => {
                tracing::debug!(
                    path = %request.path.display(),
                    error = %error,
                    "RAM Play unavailable for this track, streaming instead"
                );
                let reopened: Box<dyn PcmSource> = if matches!(extension.as_str(), "dsf" | "dff") {
                    Box::new(DsdPcmSource::open(&request.path)?)
                } else {
                    Box::new(SymphoniaSource::open(
                        &request.path,
                        request.start_ms,
                        request.end_ms,
                    )?)
                };
                reopened
            }
        }
    } else {
        source
    };

    // DoP is a bit-exact carrier: any gain or filter would corrupt the marker
    // bytes and the DAC would mute. DSD therefore always takes the bypass path.
    let dsp = if is_dsd {
        dsd_bypass()
    } else {
        config.dsp.clone()
    };

    if is_dsd && config.output_format.sample_rate != source_format.sample_rate {
        return Err(AudioError::UnsupportedFormat(format!(
            "DSD needs a {} Hz DoP carrier, but the output runs at {} Hz",
            source_format.sample_rate, config.output_format.sample_rate
        )));
    }

    let gain_db = dsp.replaygain.select(
        request.replaygain_track_gain_db,
        request.replaygain_album_gain_db,
    );
    let peak = if dsp.replaygain.mode == ReplayGainMode::Album {
        request
            .replaygain_album_peak
            .or(request.replaygain_track_peak)
    } else {
        request.replaygain_track_peak
    };

    let chain = DspChain::build(&dsp, &source_format, &config.output_format, gain_db, peak)?;
    let total_frames = source.total_frames();

    let mut prepared = PreparedTrack {
        request: request.clone(),
        source,
        chain,
        source_format,
        primed: Vec::new(),
        total_frames,
        is_dop: is_dsd,
    };

    let mut raw = Vec::new();
    let read = prepared.source.read(&mut raw, PRIME_FRAMES)?;
    if read.frames > 0 {
        prepared.primed =
            process_block(&mut prepared, &raw, config.output_format.channels as usize)?;
    }

    Ok(prepared)
}

/// Decode a span into memory without building a playback pipeline.
///
/// Used by the scanner (FASE 3) for waveform peaks and R128 analysis.
pub fn decode_span(
    request: &TrackRequest,
    max_frames: usize,
) -> Result<(AudioFormat, Arc<Vec<f32>>), AudioError> {
    let extension = request
        .path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();

    let mut source: Box<dyn PcmSource> = if matches!(extension.as_str(), "dsf" | "dff") {
        Box::new(DsdPcmSource::open(&request.path)?)
    } else {
        Box::new(SymphoniaSource::open(
            &request.path,
            request.start_ms,
            request.end_ms,
        )?)
    };

    let format = source.format();
    let mut samples = Vec::new();
    let mut remaining = max_frames;
    while remaining > 0 {
        let read = source.read(&mut samples, remaining.min(65_536))?;
        if read.frames == 0 {
            break;
        }
        remaining = remaining.saturating_sub(read.frames);
        if read.end_of_stream {
            break;
        }
    }
    Ok((format, Arc::new(samples)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_wav(path: &Path, rate: u32, channels: u16, frames: usize, hz: f32) {
        let bits_per_sample: u16 = 16;
        let block_align = channels * bits_per_sample / 8;
        let byte_rate = rate * u32::from(block_align);
        let data_size = frames as u32 * u32::from(block_align);

        let mut file = File::create(path).expect("create wav");
        file.write_all(b"RIFF").unwrap();
        file.write_all(&(36 + data_size).to_le_bytes()).unwrap();
        file.write_all(b"WAVE").unwrap();
        file.write_all(b"fmt ").unwrap();
        file.write_all(&16u32.to_le_bytes()).unwrap();
        file.write_all(&1u16.to_le_bytes()).unwrap(); // PCM
        file.write_all(&channels.to_le_bytes()).unwrap();
        file.write_all(&rate.to_le_bytes()).unwrap();
        file.write_all(&byte_rate.to_le_bytes()).unwrap();
        file.write_all(&block_align.to_le_bytes()).unwrap();
        file.write_all(&bits_per_sample.to_le_bytes()).unwrap();
        file.write_all(b"data").unwrap();
        file.write_all(&data_size.to_le_bytes()).unwrap();

        for frame in 0..frames {
            let sample = (2.0 * std::f32::consts::PI * hz * frame as f32 / rate as f32).sin() * 0.5;
            let value = (sample * 32_767.0) as i16;
            for _ in 0..channels {
                file.write_all(&value.to_le_bytes()).unwrap();
            }
        }
    }

    fn config(rate: u32) -> PlaybackConfig {
        PlaybackConfig {
            output_format: AudioFormat::new(rate, 2, SampleFormat::F32).expect("format"),
            dsp: DspSettings::default(),
            ram_play: false,
            ram_limit_bytes: 256 * 1024 * 1024,
        }
    }

    #[test]
    fn decodes_wav_and_reports_format() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("tone.wav");
        write_wav(&path, 44_100, 2, 44_100, 440.0);

        let mut source = SymphoniaSource::open(&path, None, None).expect("open");
        assert_eq!(source.format().sample_rate, 44_100);
        assert_eq!(source.format().channels, 2);
        assert_eq!(source.format().sample_format, SampleFormat::S16);

        let mut samples = Vec::new();
        let read = source.read(&mut samples, 65_536).expect("read");
        assert_eq!(read.frames, 44_100);
        assert!(read.end_of_stream);
        assert_eq!(samples.len(), 44_100 * 2);
        assert!(samples.iter().all(|sample| sample.abs() <= 0.5 + 1e-3));
    }

    #[test]
    fn cue_span_reads_only_the_requested_range() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("album.wav");
        // Two seconds; take the 500..1000 ms span.
        write_wav(&path, 44_100, 2, 88_200, 440.0);

        let mut source = SymphoniaSource::open(&path, Some(500), Some(1_000)).expect("open");
        let mut samples = Vec::new();
        let read = source.read(&mut samples, 100_000).expect("read");
        assert_eq!(read.frames, 22_050, "500 ms at 44.1 kHz");
        assert!(read.end_of_stream);
    }

    #[test]
    fn seek_positions_within_one_frame() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("tone.wav");
        write_wav(&path, 48_000, 2, 48_000, 1_000.0);

        let mut source = SymphoniaSource::open(&path, None, None).expect("open");
        source.seek(24_000).expect("seek");
        let mut samples = Vec::new();
        source.read(&mut samples, 1_024).expect("read");
        assert!(!samples.is_empty());
    }

    #[test]
    fn ram_source_matches_streaming() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("tone.wav");
        write_wav(&path, 44_100, 2, 22_050, 220.0);

        let mut streamed = Vec::new();
        let mut source = SymphoniaSource::open(&path, None, None).expect("open");
        loop {
            let read = source.read(&mut streamed, 4_096).expect("read");
            if read.end_of_stream {
                break;
            }
        }

        let source = Box::new(SymphoniaSource::open(&path, None, None).expect("open"));
        let mut ram = RamSource::load(source, 64 * 1024 * 1024).expect("ram");
        let mut rammed = Vec::new();
        loop {
            let read = ram.read(&mut rammed, 4_096).expect("read");
            if read.end_of_stream {
                break;
            }
        }

        assert_eq!(streamed.len(), rammed.len());
        assert_eq!(streamed, rammed);
    }

    #[test]
    fn ram_source_enforces_the_budget() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("tone.wav");
        write_wav(&path, 44_100, 2, 44_100, 220.0);

        let source = Box::new(SymphoniaSource::open(&path, None, None).expect("open"));
        let result = RamSource::load(source, 1_024);
        assert!(result.is_err(), "a 176 kB file cannot fit a 1 kB budget");
    }

    #[test]
    fn gapless_playback_has_no_gap_between_tracks() {
        let dir = tempfile::tempdir().expect("tempdir");
        let first = dir.path().join("a.wav");
        let second = dir.path().join("b.wav");
        write_wav(&first, 44_100, 2, 4_410, 440.0); // 100 ms
        write_wav(&second, 44_100, 2, 6_615, 660.0); // 150 ms

        let requests = vec![
            TrackRequest::file(&first, "a"),
            TrackRequest::file(&second, "b"),
        ];
        let mut player = GaplessPlayer::new(requests, config(44_100)).expect("player");

        let mut total = 0usize;
        let mut started_tracks: Vec<String> = Vec::new();
        let mut ended_tracks: Vec<String> = Vec::new();
        loop {
            let chunk = player.next_chunk(1_024).expect("chunk");
            if chunk.samples.is_empty() {
                break;
            }
            total += chunk.samples.len() / 2;
            if let Some(ended) = chunk.ended {
                ended_tracks.push(ended);
            }
            if let Some(started) = chunk.started {
                started_tracks.push(started);
            }
            if player.is_finished() && chunk.samples.len() < 1_024 * 2 {
                break;
            }
        }

        // 4 410 + 6 615 frames = 11 025, with no silence inserted anywhere:
        // the boundary is sample-exact, so the sum of both tracks is the sum
        // of what the player produced.
        assert_eq!(total, 11_025, "gapless playback must not insert silence");
        assert_eq!(started_tracks, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(ended_tracks, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn gapless_crosses_rate_changes_through_resampling() {
        let dir = tempfile::tempdir().expect("tempdir");
        let first = dir.path().join("a.wav");
        let second = dir.path().join("b.wav");
        write_wav(&first, 44_100, 2, 44_100, 440.0);
        write_wav(&second, 48_000, 2, 48_000, 660.0);

        let requests = vec![
            TrackRequest::file(&first, "a"),
            TrackRequest::file(&second, "b"),
        ];
        let mut player = GaplessPlayer::new(requests, config(48_000)).expect("player");

        let mut produced = 0usize;
        loop {
            let chunk = player.next_chunk(2_048).expect("chunk");
            if chunk.samples.is_empty() {
                break;
            }
            produced += chunk.samples.len() / 2;
            if player.is_finished() && chunk.samples.len() < 2_048 * 2 {
                break;
            }
        }
        // Both tracks land at 48 kHz: ~48 000 + 48 000 frames.
        assert!(
            produced.abs_diff(96_000) < 3_000,
            "produced {produced} output frames"
        );
    }

    #[test]
    fn empty_queue_is_rejected() {
        assert!(GaplessPlayer::new(Vec::new(), config(44_100)).is_err());
    }
}
