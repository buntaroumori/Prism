//! EBU R128 / ITU-R BS.1770 loudness meter.
//!
//! Used at scan time to compute ReplayGain 2 values (which are simply
//! `-18 LUFS − measured integrated loudness`), and by the diagnostics panel to
//! show the current programme loudness.
//!
//! Implementation details:
//!
//! * K-weighting = high-shelf (+4 dB above ~1.7 kHz) followed by a
//!   38 Hz high-pass, both designed from the analog prototypes given in
//!   BS.1770 with the RBJ bilinear-transform formulas. The classic 48 kHz
//!   coefficient table is reproduced to within 0.01 dB by this design.
//! * Gating follows the standard: 400 ms blocks with 75 % overlap, absolute
//!   gate at −70 LUFS, relative gate 10 LU below the ungated mean.
//! * Channel weights: 1.0 for L/R/C, 1.41 (+1.5 dB) for surround channels.

use super::eq::BiquadCoefficients;
use crate::audio::AudioError;

/// Target loudness for ReplayGain 2 (EBU R128 reference).
pub const R128_TARGET_LUFS: f64 = -18.0;
/// Absolute gate from BS.1770.
pub const R128_ABSOLUTE_GATE: f64 = -70.0;
/// Relative gate offset from BS.1770.
pub const R128_RELATIVE_GATE: f64 = -10.0;
/// Sub-block duration used by the gating implementation, in seconds.
pub const R128_SUBBLOCK_SECONDS: f64 = 0.1;

/// First K-weighting stage: the +4 dB high shelf.
///
/// De Man's normalisation (`Vb = Vh^0.4996667741545416`) differs from the
/// audio-EQ-cookbook shelf: an RBJ shelf shifts the 997 Hz reference by
/// 0.25 dB and every absolute loudness reading with it. At 48 kHz this
/// reproduces BS.1770-4 Table 1 exactly.
fn k_weighting_shelf(sample_rate: u32) -> Result<BiquadCoefficients, AudioError> {
    if sample_rate < 8_000 {
        return Err(AudioError::InvalidParameters(format!(
            "the loudness meter needs at least 8 kHz, got {sample_rate} Hz"
        )));
    }
    let fs = f64::from(sample_rate);
    let k = (std::f64::consts::PI * 1_681.974_450_955_533 / fs).tan();
    let vh = 10f64.powf(3.999_843_853_973_347 / 20.0);
    let vb = vh.powf(0.499_666_774_154_541_6);
    let q = 0.707_175_236_955_419_6;
    let a0 = 1.0 + k / q + k * k;
    Ok(BiquadCoefficients {
        b0: (vh + vb * k / q + k * k) / a0,
        b1: 2.0 * (k * k - vh) / a0,
        b2: (vh - vb * k / q + k * k) / a0,
        a1: 2.0 * (k * k - 1.0) / a0,
        a2: (1.0 - k / q + k * k) / a0,
    })
}

/// Second K-weighting stage: the 38 Hz RLB high-pass.
///
/// The numerator stays unnormalised (`b = [1, -2, 1]`) exactly as the standard
/// specifies: its +0.043 dB pass-band gain is part of the reference
/// calibration, so normalising it would bias every measurement by exactly
/// that amount and every absolute loudness reading with it.
fn k_weighting_high_pass(sample_rate: u32) -> Result<BiquadCoefficients, AudioError> {
    if sample_rate < 8_000 {
        return Err(AudioError::InvalidParameters(format!(
            "the loudness meter needs at least 8 kHz, got {sample_rate} Hz"
        )));
    }
    let fs = f64::from(sample_rate);
    let k = (std::f64::consts::PI * 38.135_470_876_024_44 / fs).tan();
    let q = 0.500_327_037_323_877_3;
    let a0 = 1.0 + k / q + k * k;
    Ok(BiquadCoefficients {
        b0: 1.0,
        b1: -2.0,
        b2: 1.0,
        a1: 2.0 * (k * k - 1.0) / a0,
        a2: (1.0 - k / q + k * k) / a0,
    })
}

/// Frames in one 400 ms block of sub-blocks.
pub const R128_BLOCK_FRAMES: usize = 4;

/// A filter with memory, used twice per channel.
#[derive(Debug, Clone)]
struct MeterFilter {
    coefficients: BiquadCoefficients,
    z1: f64,
    z2: f64,
}

impl MeterFilter {
    fn new(coefficients: BiquadCoefficients) -> Self {
        Self {
            coefficients,
            z1: 0.0,
            z2: 0.0,
        }
    }

    #[inline]
    fn process(&mut self, input: f64) -> f64 {
        let output = self.coefficients.b0 * input + self.z1;
        self.z1 = self.coefficients.b1 * input - self.coefficients.a1 * output + self.z2;
        self.z2 = self.coefficients.b2 * input - self.coefficients.a2 * output;
        output
    }

    fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}

/// Integrated loudness meter for one stream.
#[derive(Debug)]
pub struct LoudnessMeter {
    channels: usize,
    sample_rate: u32,
    filters: Vec<[MeterFilter; 2]>,
    weights: Vec<f64>,
    subblock_frames: usize,
    frame_in_subblock: usize,
    /// Sum of squared K-weighted samples inside the current sub-block.
    subblock_sum: Vec<f64>,
    /// Mean-square value of the last four completed sub-blocks.
    subblock_history: [f64; R128_BLOCK_FRAMES],
    history_index: usize,
    history_filled: usize,
    /// Mean squares of gated 400 ms blocks.
    blocks: Vec<f64>,
    subblock_counter: u64,
    total_frames: u64,
}

impl LoudnessMeter {
    /// Create a meter for a rate and channel layout.
    pub fn new(sample_rate: u32, channels: usize) -> Result<Self, AudioError> {
        if channels == 0 || channels > 16 {
            return Err(AudioError::InvalidParameters(format!(
                "loudness meter supports 1..=16 channels, got {channels}"
            )));
        }
        let rate = f64::from(sample_rate);

        // BS.1770 K-weighting, De Man's published coefficient form.
        //
        // The shelf is *not* an RBJ shelf: the ITU prototype normalises by a0
        // through `Vb = Vh^0.4996667741545416`, which is what makes the
        // combined response exactly +0.691 dB at 997 Hz — and therefore what
        // makes the `-0.691` offset in `lufs_from_mean_square` correct. At
        // 48 kHz these expressions reproduce the coefficients printed in
        // BS.1770-4 Table 1 bit for bit.
        let shelf = k_weighting_shelf(sample_rate)?;
        let high_pass = k_weighting_high_pass(sample_rate)?;

        let weights = (0..channels)
            .map(|channel| if channel >= 3 { 1.41 } else { 1.0 })
            .collect();

        let subblock_frames = (rate * R128_SUBBLOCK_SECONDS).round() as usize;
        let subblock_frames = subblock_frames.max(1);

        Ok(Self {
            channels,
            sample_rate,
            filters: (0..channels)
                .map(|_| [MeterFilter::new(shelf), MeterFilter::new(high_pass)])
                .collect(),
            weights,
            subblock_frames,
            frame_in_subblock: 0,
            subblock_sum: vec![0.0; channels],
            subblock_history: [0.0; R128_BLOCK_FRAMES],
            history_index: 0,
            history_filled: 0,
            blocks: Vec::new(),
            subblock_counter: 0,
            total_frames: 0,
        })
    }

    #[must_use]
    pub const fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    #[must_use]
    pub const fn channels(&self) -> usize {
        self.channels
    }

    /// Feed interleaved audio.
    pub fn push(&mut self, samples: &[f32]) -> Result<(), AudioError> {
        if samples.len() % self.channels != 0 {
            return Err(AudioError::InvalidParameters(
                "loudness input is not a frame multiple".into(),
            ));
        }

        for frame in samples.chunks_exact(self.channels) {
            for (channel, &sample) in frame.iter().enumerate() {
                let filtered = self.filters[channel][0].process(f64::from(sample));
                let filtered = self.filters[channel][1].process(filtered);
                self.subblock_sum[channel] += filtered * filtered;
            }

            self.frame_in_subblock += 1;
            self.total_frames += 1;

            if self.frame_in_subblock == self.subblock_frames {
                self.finish_subblock();
            }
        }

        Ok(())
    }

    fn finish_subblock(&mut self) {
        let frames = self.subblock_frames as f64;
        // Mean square across channels, weighted (BS.1770 channel weighting).
        let weighted: f64 = self
            .subblock_sum
            .iter()
            .zip(&self.weights)
            .map(|(sum, weight)| (sum / frames) * weight)
            .sum();

        self.subblock_sum.fill(0.0);
        self.frame_in_subblock = 0;
        self.subblock_counter += 1;

        self.subblock_history[self.history_index] = weighted;
        self.history_index = (self.history_index + 1) % R128_BLOCK_FRAMES;
        self.history_filled = (self.history_filled + 1).min(R128_BLOCK_FRAMES);

        if self.history_filled == R128_BLOCK_FRAMES {
            let mean = self.subblock_history.iter().sum::<f64>() / R128_BLOCK_FRAMES as f64;
            if mean > 0.0 {
                let loudness = lufs_from_mean_square(mean);
                if loudness > R128_ABSOLUTE_GATE {
                    self.blocks.push(mean);
                }
            }
        }
    }

    /// Integrated loudness in LUFS, or `None` when no programme audio exists.
    #[must_use]
    pub fn integrated_lufs(&self) -> Option<f64> {
        if self.blocks.is_empty() {
            return None;
        }

        let mean_all = self.blocks.iter().sum::<f64>() / self.blocks.len() as f64;
        if mean_all <= 0.0 {
            return None;
        }
        let relative_gate = lufs_from_mean_square(mean_all) + R128_RELATIVE_GATE;

        let mut sum = 0.0;
        let mut count = 0usize;
        for &block in &self.blocks {
            let loudness = lufs_from_mean_square(block);
            if loudness > relative_gate {
                sum += block;
                count += 1;
            }
        }

        if count == 0 {
            return None;
        }
        Some(lufs_from_mean_square(sum / count as f64))
    }

    /// ReplayGain 2 track gain in dB for the measured programme.
    #[must_use]
    pub fn replaygain_db(&self) -> Option<f32> {
        self.integrated_lufs()
            .map(|lufs| (R128_TARGET_LUFS - lufs) as f32)
    }

    /// Number of frames analysed so far.
    #[must_use]
    pub const fn analysed_frames(&self) -> u64 {
        self.total_frames
    }

    /// Reset accumulated statistics, keeping the configuration.
    pub fn reset(&mut self) {
        self.filters.iter_mut().for_each(|channel| {
            channel[0].reset();
            channel[1].reset();
        });
        self.subblock_sum.fill(0.0);
        self.subblock_history = [0.0; R128_BLOCK_FRAMES];
        self.history_index = 0;
        self.history_filled = 0;
        self.frame_in_subblock = 0;
        self.blocks.clear();
        self.subblock_counter = 0;
        self.total_frames = 0;
    }
}

/// Convert a K-weighted mean-square value to LUFS.
#[must_use]
pub fn lufs_from_mean_square(mean_square: f64) -> f64 {
    -0.691 + 10.0 * mean_square.max(1e-20).log10()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, hz: f64, amplitude: f64, seconds: f64) -> Vec<f32> {
        let frames = (rate as f64 * seconds) as usize;
        (0..frames)
            .map(|frame| {
                (amplitude * (2.0 * std::f64::consts::PI * hz * frame as f64 / rate as f64).sin())
                    as f32
            })
            .collect()
    }

    #[test]
    fn full_scale_1khz_sine_measures_minus_3_lufs() {
        // Single channel: a 0 dBFS sine has a mean square of 0.5 (-3.01 dBFS
        // RMS) and the K-weighting is calibrated to read that as -3.01 LKFS.
        // Duplicating the sine into two channels adds +3 dB by definition of
        // BS.1770 (channel energies are summed), which is asserted below.
        let rate = 48_000;
        let mut meter = LoudnessMeter::new(rate, 1).expect("meter");
        let mono = sine(rate, 997.0, 1.0, 5.0);
        meter.push(&mono).expect("push");

        let lufs = meter.integrated_lufs().expect("measurement");
        assert!(
            (lufs - (-3.01)).abs() < 0.1,
            "0 dBFS 997 Hz mono sine should measure -3.01 LUFS, got {lufs:.2}"
        );
    }

    #[test]
    fn stereo_reference_sums_channel_energy() {
        let rate = 48_000;
        let mut mono_meter = LoudnessMeter::new(rate, 1).expect("meter");
        let mut stereo_meter = LoudnessMeter::new(rate, 2).expect("meter");
        let mono = sine(rate, 997.0, 1.0, 5.0);
        let stereo: Vec<f32> = mono.iter().flat_map(|sample| [*sample, *sample]).collect();
        mono_meter.push(&mono).expect("push");
        stereo_meter.push(&stereo).expect("push");

        let mono_lufs = mono_meter.integrated_lufs().expect("mono");
        let stereo_lufs = stereo_meter.integrated_lufs().expect("stereo");
        assert!(
            (stereo_lufs - mono_lufs - 3.01).abs() < 0.05,
            "an identical second channel must add 3.01 LU, got {mono_lufs:.2} → {stereo_lufs:.2}"
        );
    }

    #[test]
    fn k_weighting_matches_bs1770_coefficients() {
        // BS.1770-4 Table 1, 48 kHz.
        let shelf = k_weighting_shelf(48_000).expect("shelf");
        assert!((shelf.b0 - 1.535_124_859_586_97).abs() < 1e-12);
        assert!((shelf.b1 - (-2.691_696_189_406_38)).abs() < 1e-12);
        assert!((shelf.b2 - 1.198_392_810_852_85).abs() < 1e-12);
        assert!((shelf.a1 - (-1.690_659_293_182_41)).abs() < 1e-12);
        assert!((shelf.a2 - 0.732_480_774_215_85).abs() < 1e-12);

        let high_pass = k_weighting_high_pass(48_000).expect("high pass");
        assert!((high_pass.a1 - (-1.990_047_454_833_98)).abs() < 1e-12);
        assert!((high_pass.a2 - 0.990_072_250_366_21).abs() < 1e-12);
    }

    #[test]
    fn halving_amplitude_drops_6_lu() {
        let rate = 48_000;
        let mut full = LoudnessMeter::new(rate, 1).expect("meter");
        let mut half = LoudnessMeter::new(rate, 1).expect("meter");
        full.push(&sine(rate, 1_000.0, 1.0, 4.0)).expect("push");
        half.push(&sine(rate, 1_000.0, 0.5, 4.0)).expect("push");

        let difference = full.integrated_lufs().unwrap() - half.integrated_lufs().unwrap();
        assert!(
            (difference - 6.02).abs() < 0.3,
            "difference was {difference:.2} LU"
        );
    }

    #[test]
    fn silence_is_not_measurable() {
        let mut meter = LoudnessMeter::new(48_000, 2).expect("meter");
        meter.push(&vec![0.0f32; 48_000 * 2]).expect("push");
        assert!(meter.integrated_lufs().is_none());
        assert!(meter.replaygain_db().is_none());
    }

    #[test]
    fn very_quiet_audio_below_absolute_gate_is_ignored() {
        let mut meter = LoudnessMeter::new(48_000, 1).expect("meter");
        // ~ -80 LUFS: far below the -70 absolute gate.
        meter
            .push(&sine(48_000, 1_000.0, 0.000_1, 3.0))
            .expect("push");
        assert!(meter.integrated_lufs().is_none());
    }

    #[test]
    fn replaygain_targets_minus_18_lufs() {
        let mut meter = LoudnessMeter::new(48_000, 2).expect("meter");
        let mono = sine(48_000, 1_000.0, 0.1, 4.0);
        let stereo: Vec<f32> = mono.iter().flat_map(|sample| [*sample, *sample]).collect();
        meter.push(&stereo).expect("push");

        let lufs = meter.integrated_lufs().unwrap();
        let gain = meter.replaygain_db().unwrap();
        assert!((lufs + gain as f64 - R128_TARGET_LUFS).abs() < 1e-3);
    }

    #[test]
    fn rejects_bad_channel_counts_and_partial_frames() {
        assert!(LoudnessMeter::new(48_000, 0).is_err());
        assert!(LoudnessMeter::new(48_000, 17).is_err());

        let mut meter = LoudnessMeter::new(48_000, 2).expect("meter");
        assert!(meter.push(&[0.0, 0.0, 0.0]).is_err());
    }

    #[test]
    fn reset_clears_the_measurement() {
        let mut meter = LoudnessMeter::new(48_000, 1).expect("meter");
        meter.push(&sine(48_000, 1_000.0, 0.5, 2.0)).expect("push");
        assert!(meter.integrated_lufs().is_some());
        meter.reset();
        assert!(meter.integrated_lufs().is_none());
        assert_eq!(meter.analysed_frames(), 0);
    }
}
