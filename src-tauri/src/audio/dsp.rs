//! Real-time DSP graph.
//!
//! Signal order (every stage bypassable, bypassed stages cost nothing):
//!
//! ```text
//!   ReplayGain 2 → Resampler (rubato sinc) → Parametric EQ → Crossfeed → Master gain
//! ```
//!
//! The graph runs on the producer thread, never in the device callback. It
//! consumes and produces interleaved `f32`; format conversion to the device's
//! integer type happens afterwards in the backend writer.
//!
//! Latency accounting: only the resampler introduces latency (half the sinc
//! window). [`DspChain::latency_frames`] reports it at the *output* rate so the
//! engine can compensate the reported playhead and keep the waveform seekbar
//! aligned with what the listener hears.

mod crossfeed;
mod eq;
mod r128;
mod replaygain;
mod resampler;

use serde::{Deserialize, Serialize};

pub use crossfeed::{Crossfeed, CrossfeedAlgorithm, CrossfeedSettings};
pub use eq::{BiquadType, EqBand, EqSettings, ParametricEq};
pub use r128::{LoudnessMeter, R128_BLOCK_FRAMES, R128_TARGET_LUFS};
pub use replaygain::{ReplayGain, ReplayGainMode, ReplayGainSettings};
pub use resampler::SincResampler;

use crate::audio::AudioError;
use crate::audio::format::AudioFormat;

/// Complete DSP configuration, persisted as JSON in the `settings` table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DspSettings {
    #[serde(default)]
    pub replaygain: ReplayGainSettings,
    #[serde(default)]
    pub eq: EqSettings,
    #[serde(default)]
    pub crossfeed: CrossfeedSettings,
    /// Enable sinc resampling when source and device rates differ.
    #[serde(default = "default_true")]
    pub resample_when_needed: bool,
    /// Master volume in dB (`-inf` is represented by the engine's mute flag).
    #[serde(default)]
    pub volume_db: f32,
}

const fn default_true() -> bool {
    true
}

impl Default for DspSettings {
    fn default() -> Self {
        Self {
            replaygain: ReplayGainSettings::default(),
            eq: EqSettings::default(),
            crossfeed: CrossfeedSettings::default(),
            resample_when_needed: true,
            volume_db: 0.0,
        }
    }
}

impl DspSettings {
    /// True when no stage would alter the signal and the chain can be bypassed
    /// entirely (the bit-perfect path).
    #[must_use]
    pub fn is_bit_perfect(&self, source: &AudioFormat, output: &AudioFormat) -> bool {
        !self.replaygain.is_active()
            && !self.eq.is_active()
            && !self.crossfeed.enabled
            && source.sample_rate == output.sample_rate
            && source.sample_format == output.sample_format
    }
}

/// One processing stage.
pub trait Processor: Send {
    /// Process an interleaved buffer in place.
    fn process(&mut self, samples: &mut [f32], channels: usize);

    /// Stage name for diagnostics.
    fn name(&self) -> &'static str;
}

/// Compiled processing chain for one source/output format pair.
pub struct DspChain {
    replaygain: Option<ReplayGain>,
    resampler: Option<SincResampler>,
    eq: Option<ParametricEq>,
    crossfeed: Option<Crossfeed>,
    master_gain: f32,
    channels: usize,
    pending: Vec<f32>,
    latency_frames: u64,
    output_rate: u32,
}

impl std::fmt::Debug for DspChain {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DspChain")
            .field("replaygain", &self.replaygain.is_some())
            .field("resampler", &self.resampler.is_some())
            .field("eq", &self.eq.is_some())
            .field("crossfeed", &self.crossfeed.is_some())
            .field("master_gain", &self.master_gain)
            .field("channels", &self.channels)
            .finish_non_exhaustive()
    }
}

impl DspChain {
    /// Compile a chain for the given source and output formats.
    ///
    /// `gain_db` and `peak` come from the ReplayGain tags (track or album,
    /// selected by the settings).
    pub fn build(
        settings: &DspSettings,
        source: &AudioFormat,
        output: &AudioFormat,
        gain_db: Option<f32>,
        peak: Option<f32>,
    ) -> Result<Self, AudioError> {
        // The engine remaps the decoded block to the output layout *before*
        // the chain runs, so every stage is compiled for `output.channels`.
        let channels = output.channels as usize;
        let replaygain = settings
            .replaygain
            .is_active()
            .then(|| ReplayGain::new(&settings.replaygain, gain_db, peak));

        let resampler = if settings.resample_when_needed && source.sample_rate != output.sample_rate
        {
            Some(SincResampler::new(
                source.sample_rate,
                output.sample_rate,
                channels,
            )?)
        } else {
            None
        };

        let eq = settings
            .eq
            .is_active()
            .then(|| ParametricEq::new(&settings.eq, output.sample_rate, channels))
            .transpose()?;

        let crossfeed = settings
            .crossfeed
            .enabled
            .then(|| Crossfeed::new(&settings.crossfeed, output.sample_rate, channels))
            .transpose()?;

        let master_gain = if settings.volume_db.abs() < f32::EPSILON {
            1.0
        } else {
            10f32.powf(settings.volume_db / 20.0)
        };

        let latency_frames = resampler.as_ref().map_or(0, SincResampler::latency_frames);

        Ok(Self {
            replaygain,
            resampler,
            eq,
            crossfeed,
            master_gain,
            channels,
            pending: Vec::new(),
            latency_frames,
            output_rate: output.sample_rate,
        })
    }

    /// Process an input block, returning the produced output.
    ///
    /// The resampler is stateful and may hold back input until a full chunk
    /// arrives, so the returned buffer can be shorter than the input (or empty
    /// early on). The engine handles that naturally because it feeds the ring
    /// buffer from this output.
    pub fn process(&mut self, input: &[f32]) -> Result<Vec<f32>, AudioError> {
        if input.is_empty() {
            return Ok(Vec::new());
        }
        if input.len() % self.channels != 0 {
            return Err(AudioError::InvalidParameters(
                "DSP input length is not a frame multiple".into(),
            ));
        }

        let mut working = match &mut self.resampler {
            Some(resampler) => {
                resampler.push(input)?;
                resampler.drain(usize::MAX)
            }
            None => input.to_vec(),
        };

        if working.is_empty() {
            return Ok(Vec::new());
        }

        if let Some(replaygain) = &mut self.replaygain {
            replaygain.process(&mut working, self.channels);
        }
        if let Some(eq) = &mut self.eq {
            eq.process(&mut working, self.channels);
        }
        if let Some(crossfeed) = &mut self.crossfeed {
            crossfeed.process(&mut working, self.channels);
        }
        if (self.master_gain - 1.0).abs() > f32::EPSILON {
            for sample in &mut working {
                *sample *= self.master_gain;
            }
        }

        Ok(working)
    }

    /// Resampler latency expressed in output frames.
    #[must_use]
    pub const fn latency_frames(&self) -> u64 {
        self.latency_frames
    }

    /// Output sample rate the chain produces samples at.
    #[must_use]
    pub const fn output_rate(&self) -> u32 {
        self.output_rate
    }

    /// True when every stage is inactive (bit-perfect passthrough).
    #[must_use]
    pub fn is_bypass(&self) -> bool {
        self.replaygain.is_none()
            && self.resampler.is_none()
            && self.eq.is_none()
            && self.crossfeed.is_none()
            && (self.master_gain - 1.0).abs() < f32::EPSILON
    }

    /// Active stage names, for the player bar's DSP indicator.
    #[must_use]
    pub fn active_stages(&self) -> Vec<&'static str> {
        let mut stages = Vec::new();
        if self.replaygain.is_some() {
            stages.push("ReplayGain 2");
        }
        if self.resampler.is_some() {
            stages.push("Sinc resampler");
        }
        if self.eq.is_some() {
            stages.push("Parametric EQ");
        }
        if self.crossfeed.is_some() {
            stages.push("Crossfeed");
        }
        stages
    }

    /// Discard internal state (used after a seek).
    pub fn reset(&mut self) {
        if let Some(resampler) = &mut self.resampler {
            resampler.reset();
        }
        if let Some(eq) = &mut self.eq {
            eq.reset();
        }
        if let Some(crossfeed) = &mut self.crossfeed {
            crossfeed.reset();
        }
        if let Some(replaygain) = &mut self.replaygain {
            replaygain.reset();
        }
        self.pending.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::format::SampleFormat;

    fn format(rate: u32, channels: u16) -> AudioFormat {
        AudioFormat::new(rate, channels, SampleFormat::F32).expect("format")
    }

    fn sine(rate: u32, hz: f32, seconds: f32, amplitude: f32) -> Vec<f32> {
        let frames = (rate as f32 * seconds) as usize;
        (0..frames)
            .map(|frame| {
                amplitude * (2.0 * std::f32::consts::PI * hz * frame as f32 / rate as f32).sin()
            })
            .collect()
    }

    #[test]
    fn default_settings_are_bit_perfect_at_matching_formats() {
        let settings = DspSettings::default();
        let source = format(44_100, 2);
        let output = format(44_100, 2);
        assert!(settings.is_bit_perfect(&source, &output));

        let chain = DspChain::build(&settings, &source, &output, None, None).expect("chain");
        assert!(chain.is_bypass());
        assert_eq!(chain.latency_frames(), 0);
    }

    #[test]
    fn rate_mismatch_inserts_a_resampler() {
        let settings = DspSettings::default();
        let source = format(44_100, 2);
        let output = format(48_000, 2);
        assert!(!settings.is_bit_perfect(&source, &output));

        let chain = DspChain::build(&settings, &source, &output, None, None).expect("chain");
        assert!(!chain.is_bypass());
        assert!(chain.latency_frames() > 0);
        assert!(chain.active_stages().contains(&"Sinc resampler"));
    }

    #[test]
    fn disabled_resampling_allows_rate_mismatch_passthrough() {
        let settings = DspSettings {
            resample_when_needed: false,
            ..DspSettings::default()
        };
        let chain = DspChain::build(
            &settings,
            &format(44_100, 2),
            &format(48_000, 2),
            None,
            None,
        )
        .expect("chain");
        assert!(chain.is_bypass());
    }

    #[test]
    fn master_gain_scales_samples() {
        let settings = DspSettings {
            volume_db: -6.0,
            ..DspSettings::default()
        };
        let source = format(48_000, 2);
        let mut chain = DspChain::build(&settings, &source, &source, None, None).expect("chain");

        let input = vec![1.0, -1.0];
        let output = chain.process(&input).expect("process");
        assert!((output[0] - 0.501_187_2).abs() < 1e-5);
        assert!((output[1] + 0.501_187_2).abs() < 1e-5);
    }

    #[test]
    fn resampling_produces_the_expected_frame_count() {
        let settings = DspSettings::default();
        let source = format(44_100, 1);
        let output = format(48_000, 1);
        let mut chain = DspChain::build(&settings, &source, &output, None, None).expect("chain");

        let input = sine(44_100, 440.0, 1.0, 0.5);
        let mut produced = 0usize;
        for chunk in input.chunks(1024) {
            produced += chain.process(chunk).expect("process").len();
        }
        let expected = 48_000;
        let tolerance = expected / 50; // ±2 %, covers filter latency
        assert!(
            produced.abs_diff(expected) < tolerance,
            "produced {produced} samples, expected ~{expected}"
        );
    }

    #[test]
    fn builds_for_the_output_channel_layout() {
        // The decoder remaps mono → stereo *before* the chain, so a chain
        // compiled with different source/output layouts is valid and runs at
        // the output channel count.
        let settings = DspSettings::default();
        let chain = DspChain::build(
            &settings,
            &format(44_100, 1),
            &format(44_100, 2),
            None,
            None,
        )
        .expect("chain");
        assert_eq!(chain.output_rate(), 44_100);
        assert!(
            chain.is_bypass(),
            "mono → stereo remapping happens upstream, so the chain stays bypassed"
        );

        // A rate change does engage the resampler, and it reports latency.
        let resampled = DspChain::build(
            &settings,
            &format(44_100, 2),
            &format(48_000, 2),
            None,
            None,
        )
        .expect("chain");
        assert!(!resampled.is_bypass());
        assert!(resampled.latency_frames() > 0);
    }

    #[test]
    fn rejects_incomplete_frames() {
        let settings = DspSettings::default();
        let source = format(48_000, 2);
        let mut chain = DspChain::build(&settings, &source, &source, None, None).expect("chain");
        assert!(chain.process(&[1.0, 0.5, 0.25]).is_err());
    }
}
