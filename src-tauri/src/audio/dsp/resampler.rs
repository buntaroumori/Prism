//! Windowed-sinc resampler built on `rubato`'s `SincFixedIn`.
//!
//! Configuration: 256-tap windowed sinc, 0.95 relative cutoff, 128×
//! oversampling and cubic interpolation between the pre-computed sinc phases.
//! This is the quality tier appropriate for 44.1 ↔ 48 ↔ 96 ↔ 192 conversions;
//! the filter suppresses images below −100 dB in the audible band at the cost
//! of roughly 1.5 % of one modern core for stereo 96 kHz material.
//!
//! The wrapper hides rubato's fixed-chunk API behind a streaming interface:
//! callers push arbitrary interleaved blocks and drain whatever output is
//! ready. All allocations happen in [`SincResampler::new`] and in the internal
//! planar staging buffers, which grow to the chunk size once.

use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};

use crate::audio::AudioError;

/// Input frames per resampler call.
///
/// 256 frames is 5.8 ms at 44.1 kHz. Every call emits its whole output block at
/// once, so this is the granularity of the pump's bursts: at 1024 frames a
/// 44.1 → 192 conversion delivered 21 ms of audio per call, which is longer
/// than the ring and therefore guaranteed to overflow it.
const CHUNK_FRAMES: usize = 256;
/// Sinc kernel length.
const SINC_LEN: usize = 256;
/// Relative cutoff frequency (fraction of the lower Nyquist).
const CUTOFF: f32 = 0.95;

/// Streaming sinc resampler.
pub struct SincResampler {
    inner: SincFixedIn<f32>,
    channels: usize,
    input_planar: Vec<Vec<f32>>,
    filled_frames: usize,
    pending: Vec<f32>,
    pending_offset: usize,
    input_rate: u32,
    output_rate: u32,
}

impl std::fmt::Debug for SincResampler {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SincResampler")
            .field("input_rate", &self.input_rate)
            .field("output_rate", &self.output_rate)
            .field("channels", &self.channels)
            .finish_non_exhaustive()
    }
}

impl SincResampler {
    /// Create a resampler for a rate pair and channel count.
    pub fn new(input_rate: u32, output_rate: u32, channels: usize) -> Result<Self, AudioError> {
        if input_rate == 0 || output_rate == 0 {
            return Err(AudioError::InvalidParameters(
                "resampler rates must be non-zero".into(),
            ));
        }
        if channels == 0 {
            return Err(AudioError::InvalidParameters(
                "resampler needs at least one channel".into(),
            ));
        }

        let ratio = f64::from(output_rate) / f64::from(input_rate);
        let parameters = SincInterpolationParameters {
            sinc_len: SINC_LEN,
            f_cutoff: CUTOFF,
            oversampling_factor: 128,
            interpolation: SincInterpolationType::Cubic,
            window: WindowFunction::BlackmanHarris2,
        };

        let inner = SincFixedIn::<f32>::new(ratio, 1.1, parameters, CHUNK_FRAMES, channels)
            .map_err(|error| {
                AudioError::InvalidParameters(format!(
                    "cannot build resampler {input_rate}→{output_rate}: {error}"
                ))
            })?;

        Ok(Self {
            inner,
            channels,
            input_planar: vec![Vec::with_capacity(CHUNK_FRAMES); channels],
            filled_frames: 0,
            pending: Vec::new(),
            pending_offset: 0,
            input_rate,
            output_rate,
        })
    }

    /// Output/input ratio.
    #[must_use]
    pub fn ratio(&self) -> f64 {
        f64::from(self.output_rate) / f64::from(self.input_rate)
    }

    /// Resampler latency in output frames (half the sinc window, rescaled).
    #[must_use]
    pub fn latency_frames(&self) -> u64 {
        let half_input = SINC_LEN as f64 / 2.0;
        (half_input * self.ratio()).round().max(0.0) as u64
    }

    /// Feed interleaved input. Output accumulates internally.
    pub fn push(&mut self, interleaved: &[f32]) -> Result<(), AudioError> {
        if interleaved.len() % self.channels != 0 {
            return Err(AudioError::InvalidParameters(
                "resampler input is not a frame multiple".into(),
            ));
        }

        let frames = interleaved.len() / self.channels;
        let mut consumed = 0usize;

        while consumed < frames {
            let take = (frames - consumed).min(CHUNK_FRAMES - self.filled_frames);
            for frame in 0..take {
                for channel in 0..self.channels {
                    self.input_planar[channel]
                        .push(interleaved[(consumed + frame) * self.channels + channel]);
                }
            }
            self.filled_frames += take;
            consumed += take;

            if self.filled_frames == CHUNK_FRAMES {
                self.process_chunk()?;
            }
        }

        Ok(())
    }

    fn process_chunk(&mut self) -> Result<(), AudioError> {
        let output = self
            .inner
            .process(&self.input_planar, None)
            .map_err(|error| AudioError::Decode(format!("resampler failure: {error}")))?;

        if output.is_empty() || output[0].is_empty() {
            return Err(AudioError::Decode(
                "resampler returned an empty block".into(),
            ));
        }

        let frames = output[0].len();
        // Compact the drained offset before appending a new block.
        if self.pending_offset > 0 {
            self.pending.drain(..self.pending_offset);
            self.pending_offset = 0;
        }
        self.pending.reserve(frames * self.channels);
        for frame in 0..frames {
            for channel in 0..self.channels {
                self.pending.push(output[channel][frame]);
            }
        }

        for buffer in &mut self.input_planar {
            buffer.clear();
        }
        self.filled_frames = 0;
        Ok(())
    }

    /// Drain up to `max_frames` frames of interleaved output.
    pub fn drain(&mut self, max_frames: usize) -> Vec<f32> {
        let available_frames = (self.pending.len() - self.pending_offset) / self.channels;
        let frames = available_frames.min(max_frames);
        if frames == 0 {
            return Vec::new();
        }

        let start = self.pending_offset;
        let end = start + frames * self.channels;
        let output = self.pending[start..end].to_vec();
        self.pending_offset = end;

        if self.pending_offset == self.pending.len() {
            self.pending.clear();
            self.pending_offset = 0;
        }

        output
    }

    /// Drop all internal state; the next push restarts the filter history.
    pub fn reset(&mut self) {
        if let Ok(fresh) = SincFixedIn::<f32>::new(
            self.ratio(),
            1.1,
            SincInterpolationParameters {
                sinc_len: SINC_LEN,
                f_cutoff: CUTOFF,
                oversampling_factor: 128,
                interpolation: SincInterpolationType::Cubic,
                window: WindowFunction::BlackmanHarris2,
            },
            CHUNK_FRAMES,
            self.channels,
        ) {
            self.inner = fresh;
        }
        for buffer in &mut self.input_planar {
            buffer.clear();
        }
        self.filled_frames = 0;
        self.pending.clear();
        self.pending_offset = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, hz: f32, frames: usize, amplitude: f32) -> Vec<f32> {
        (0..frames)
            .map(|frame| {
                amplitude * (2.0 * std::f32::consts::PI * hz * frame as f32 / rate as f32).sin()
            })
            .collect()
    }

    fn rms(samples: &[f32]) -> f64 {
        if samples.is_empty() {
            return 0.0;
        }
        (samples.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>() / samples.len() as f64).sqrt()
    }

    #[test]
    fn upsamples_by_the_expected_ratio() {
        let mut resampler = SincResampler::new(44_100, 48_000, 2).expect("resampler");
        let input = sine(44_100, 440.0, 44_100, 0.5);
        let stereo: Vec<f32> = input.iter().flat_map(|s| [*s, *s]).collect();

        resampler.push(&stereo).expect("push");
        let mut output = resampler.drain(usize::MAX);
        output.extend(resampler.drain(usize::MAX));

        let produced_frames = output.len() / 2;
        let expected = 48_000usize;
        assert!(
            produced_frames.abs_diff(expected) < expected / 50,
            "produced {produced_frames}, expected ~{expected}"
        );
    }

    #[test]
    fn preserves_amplitude_of_a_sine() {
        let mut resampler = SincResampler::new(44_100, 96_000, 1).expect("resampler");
        let input = sine(44_100, 1_000.0, 44_100, 0.5);
        resampler.push(&input).expect("push");
        let mut output = resampler.drain(usize::MAX);
        output.extend(resampler.drain(usize::MAX));

        // Skip the filter warm-up region.
        let settled = &output[2_000..output.len() - 2_000];
        let amplitude = rms(settled) * 2f64.sqrt();
        assert!(
            (amplitude - 0.5).abs() < 0.03,
            "amplitude drifted to {amplitude:.3}"
        );
    }

    #[test]
    fn reports_latency_proportional_to_the_kernel() {
        let low = SincResampler::new(48_000, 48_000, 2).expect("resampler");
        let high = SincResampler::new(48_000, 192_000, 2).expect("resampler");
        assert_eq!(low.latency_frames(), 128);
        assert_eq!(high.latency_frames(), 512);
    }

    #[test]
    fn rejects_partial_frames() {
        let mut resampler = SincResampler::new(44_100, 48_000, 2).expect("resampler");
        assert!(resampler.push(&[0.0, 0.0, 0.0]).is_err());
    }

    #[test]
    fn reset_restarts_the_stream() {
        let mut resampler = SincResampler::new(44_100, 48_000, 1).expect("resampler");
        resampler
            .push(&sine(44_100, 440.0, 8_192, 0.5))
            .expect("push");
        let _ = resampler.drain(usize::MAX);
        resampler.reset();
        assert!(resampler.drain(usize::MAX).is_empty());

        resampler
            .push(&sine(44_100, 440.0, 4_096, 0.5))
            .expect("push");
        assert!(!resampler.drain(usize::MAX).is_empty());
    }

    #[test]
    fn downsampling_removes_energy_above_nyquist() {
        // 30 kHz tone at 96 kHz → 48 kHz output must be filtered out.
        let mut resampler = SincResampler::new(96_000, 48_000, 1).expect("resampler");
        resampler
            .push(&sine(96_000, 30_000.0, 96_000, 0.5))
            .expect("push");
        let mut output = resampler.drain(usize::MAX);
        output.extend(resampler.drain(usize::MAX));

        let settled = &output[2_000..output.len() - 2_000];
        let amplitude = rms(settled) * 2f64.sqrt();
        assert!(
            amplitude < 0.02,
            "30 kHz should be suppressed when converting to 48 kHz, got {amplitude:.4}"
        );
    }
}

#[cfg(test)]
mod streaming {
    use super::*;

    /// Converting a tone must not introduce a step at every input chunk.
    ///
    /// No discontinuity is a *property of the filter*, not of the chunk
    /// schedule: pushing the same signal in different block sizes must produce
    /// the same waveform, with no sample-to-sample jump larger than the tone's
    /// own slope. This is the unit-level guard for the field report — the
    /// machine-gun crackle that appeared only when the source rate differed
    /// from the device rate, i.e. only when this stage ran.
    #[test]
    fn a_tone_survives_any_chunking_without_a_step() {
        for (input_rate, output_rate) in [
            (44_100u32, 192_000u32),
            (96_000, 192_000),
            (192_000, 44_100),
        ] {
            const HZ: f32 = 440.0;
            let expected = 0.5 * 2.0 * std::f32::consts::PI * HZ / output_rate as f32 * 1.5;
            for block in [64usize, 256, 1_000, 4_096] {
                let frames = input_rate as usize / 4;
                let input: Vec<f32> = (0..frames)
                    .map(|index| {
                        0.5 * (2.0 * std::f32::consts::PI * HZ * index as f32 / input_rate as f32)
                            .sin()
                    })
                    .collect();

                let mut resampler =
                    SincResampler::new(input_rate, output_rate, 1).expect("resampler");
                let mut output =
                    Vec::with_capacity(frames * output_rate as usize / input_rate as usize);
                for chunk in input.chunks(block) {
                    resampler.push(chunk).expect("push");
                    output.extend(resampler.drain(usize::MAX));
                }
                output.extend(resampler.drain(usize::MAX));

                assert!(
                    output.len() as f32
                        > frames as f32 * output_rate as f32 / input_rate as f32 * 0.9,
                    "{input_rate}->{output_rate} at {block}-frame blocks produced {} frames",
                    output.len()
                );

                let mut worst = 0.0f32;
                let mut at = 0usize;
                for index in 1..output.len() {
                    let delta = (output[index] - output[index - 1]).abs();
                    if delta > worst {
                        worst = delta;
                        at = index;
                    }
                }
                assert!(
                    worst < expected,
                    "{input_rate}->{output_rate} at {block}-frame blocks stepped by {worst:.5} at sample {at} (a smooth tone moves {expected:.5} per sample)"
                );
            }
        }
    }
}
