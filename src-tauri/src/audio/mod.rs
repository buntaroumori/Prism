//! Audio engine — formats, decoding, DSP and output backends.
//!
//! Layering (bottom → top):
//!
//! ```text
//!   format   sample formats, conversions, DSD-over-PCM framing
//!   cue      CUE sheet parser → virtual track spans
//!   dsd      DSF / DSDIFF readers → DoP PCM
//!   decoder  symphonia file decoding, RAM Play, gapless double buffer
//!   dsp      ReplayGain → resampler → parametric EQ → crossfeed → volume
//!   ring     lock-free SPSC sample ring between producer and device callback
//!   engine   device lifecycle, hot-swap, transport state and event fan-out
//! ```
//!
//! Threading contract: the device callback ([`engine::AudioSink`] consumer
//! side) only reads from the pre-allocated ring buffer and applies the final
//! gain ramp. It never allocates, locks, logs or touches the filesystem. All
//! decoding and DSP runs on the producer side, ahead of the callback.

pub mod analysis;
pub mod cue;
pub mod decoder;
pub mod dsd;
pub mod dsp;
pub mod engine;
pub mod format;
pub mod ring;

use thiserror::Error;

/// Errors produced anywhere in the audio subsystem.
#[derive(Debug, Error)]
pub enum AudioError {
    #[error("unsupported audio format: {0}")]
    UnsupportedFormat(String),

    #[error("invalid audio parameters: {0}")]
    InvalidParameters(String),

    #[error("decoder error: {0}")]
    Decode(String),

    #[error("audio device error: {0}")]
    Device(String),

    #[error("no audio output device available")]
    NoDevice,

    #[error("selected output format is not supported by the device: {0}")]
    UnsupportedByDevice(String),

    #[error("the track has no decodable audio")]
    EmptyStream,

    #[error("seek target {target_ms} ms is outside the track ({duration_ms} ms)")]
    SeekOutOfRange { target_ms: u64, duration_ms: u64 },

    #[error("i/o error on {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("CUE sheet error: {0}")]
    Cue(String),

    #[error("DSD stream error: {0}")]
    Dsd(String),

    #[error("playback queue is empty")]
    EmptyQueue,

    #[error("audio engine is not running")]
    NotRunning,
}

impl AudioError {
    /// Attach a path to a raw I/O error without losing the source.
    pub fn io(path: impl AsRef<std::path::Path>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.as_ref().display().to_string(),
            source,
        }
    }

    /// Stable code exposed to the frontend alongside the message.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::UnsupportedFormat(_) => "audio_unsupported_format",
            Self::InvalidParameters(_) => "audio_invalid_parameters",
            Self::Decode(_) => "audio_decode",
            Self::Device(_) => "audio_device",
            Self::NoDevice => "audio_no_device",
            Self::UnsupportedByDevice(_) => "audio_format_not_supported",
            Self::EmptyStream => "audio_empty_stream",
            Self::SeekOutOfRange { .. } => "audio_seek_out_of_range",
            Self::Io { .. } => "audio_io",
            Self::Cue(_) => "audio_cue",
            Self::Dsd(_) => "audio_dsd",
            Self::EmptyQueue => "audio_empty_queue",
            Self::NotRunning => "audio_not_running",
        }
    }
}
