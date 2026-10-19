//! Parametric equaliser: RBJ-cookbook biquads, 10 to 31 bands, per channel.
//!
//! Every band is a second-order section in direct form II transposed, which
//! keeps numerical noise low and needs two state words per channel per band.
//! Coefficients are recomputed on the UI thread and published to the audio
//! thread by swapping the whole [`ParametricEq`] configuration via the engine's
//! command channel; the audio thread never sees a half-written coefficient set.

use serde::{Deserialize, Serialize};

use crate::audio::AudioError;

/// Filter response shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BiquadType {
    Peak,
    LowShelf,
    HighShelf,
    LowPass,
    HighPass,
    BandPass,
    Notch,
}

/// One EQ band as stored in `eq_presets.bands`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EqBand {
    /// Stable identifier so the UI can address a band across reorders.
    pub id: u32,
    #[serde(rename = "type")]
    pub kind: BiquadType,
    /// Centre/corner frequency in Hz.
    pub frequency_hz: f32,
    /// Gain in dB (ignored by pass/notch filters).
    pub gain_db: f32,
    /// Quality factor.
    pub q: f32,
    pub enabled: bool,
}

impl EqBand {
    #[must_use]
    pub fn new(id: u32, kind: BiquadType, frequency_hz: f32, gain_db: f32, q: f32) -> Self {
        Self {
            id,
            kind,
            frequency_hz,
            gain_db,
            q,
            enabled: true,
        }
    }
}

/// Complete EQ configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EqSettings {
    pub enabled: bool,
    /// Overall pre-amp applied before the bands, in dB.
    pub preamp_db: f32,
    pub bands: Vec<EqBand>,
}

impl Default for EqSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            preamp_db: 0.0,
            bands: Vec::new(),
        }
    }
}

impl EqSettings {
    /// True when at least one band could alter the signal.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.enabled
            && (self.preamp_db.abs() > f32::EPSILON
                || self
                    .bands
                    .iter()
                    .any(|band| band.enabled && band.gain_db.abs() > f32::EPSILON))
    }

    /// Build a 10-band graphic preset spanning 31 Hz – 16 kHz.
    #[must_use]
    pub fn graphic_10_band() -> Self {
        let frequencies = [
            31.0, 62.0, 125.0, 250.0, 500.0, 1_000.0, 2_000.0, 4_000.0, 8_000.0, 16_000.0,
        ];
        Self {
            enabled: true,
            preamp_db: 0.0,
            bands: frequencies
                .iter()
                .enumerate()
                .map(|(index, &frequency)| {
                    EqBand::new(index as u32, BiquadType::Peak, frequency, 0.0, 1.41)
                })
                .collect(),
        }
    }

    /// Build a 31-band graphic preset (1/3-octave centres, ISO frequencies).
    #[must_use]
    pub fn graphic_31_band() -> Self {
        let frequencies = [
            20.0, 25.0, 31.5, 40.0, 50.0, 63.0, 80.0, 100.0, 125.0, 160.0, 200.0, 250.0, 315.0,
            400.0, 500.0, 630.0, 800.0, 1_000.0, 1_250.0, 1_600.0, 2_000.0, 2_500.0, 3_150.0,
            4_000.0, 5_000.0, 6_300.0, 8_000.0, 10_000.0, 12_500.0, 16_000.0, 20_000.0,
        ];
        Self {
            enabled: true,
            preamp_db: 0.0,
            bands: frequencies
                .iter()
                .enumerate()
                .map(|(index, &frequency)| {
                    EqBand::new(index as u32, BiquadType::Peak, frequency, 0.0, 4.36)
                })
                .collect(),
        }
    }

    /// Validate every band against the sample rate.
    pub fn validate(&self, sample_rate: u32) -> Result<(), AudioError> {
        let nyquist = sample_rate as f32 / 2.0;
        for band in &self.bands {
            if !band.frequency_hz.is_finite() || band.frequency_hz <= 0.0 {
                return Err(AudioError::InvalidParameters(format!(
                    "EQ band {} has invalid frequency {}",
                    band.id, band.frequency_hz
                )));
            }
            if band.frequency_hz >= nyquist {
                return Err(AudioError::InvalidParameters(format!(
                    "EQ band {} at {} Hz is at or above Nyquist ({nyquist} Hz)",
                    band.id, band.frequency_hz
                )));
            }
            if !band.q.is_finite() || band.q <= 0.0 || band.q > 100.0 {
                return Err(AudioError::InvalidParameters(format!(
                    "EQ band {} has invalid Q {}",
                    band.id, band.q
                )));
            }
            if !band.gain_db.is_finite() || band.gain_db.abs() > 30.0 {
                return Err(AudioError::InvalidParameters(format!(
                    "EQ band {} gain {} dB is outside ±30 dB",
                    band.id, band.gain_db
                )));
            }
        }
        if self.bands.len() > 31 {
            return Err(AudioError::InvalidParameters(
                "at most 31 EQ bands are supported".into(),
            ));
        }
        Ok(())
    }
}

/// Normalised biquad coefficients (`a0` is divided out).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BiquadCoefficients {
    pub b0: f64,
    pub b1: f64,
    pub b2: f64,
    pub a1: f64,
    pub a2: f64,
}

impl BiquadCoefficients {
    /// Magnitude response at `frequency_hz` (for the EQ curve display).
    #[must_use]
    pub fn magnitude_at(&self, frequency_hz: f64, sample_rate: f64) -> f64 {
        let omega = 2.0 * std::f64::consts::PI * frequency_hz / sample_rate;
        let (sin1, cos1) = omega.sin_cos();
        let (sin2, cos2) = (2.0 * omega).sin_cos();

        let numerator_real = self.b0 + self.b1 * cos1 + self.b2 * cos2;
        let numerator_imag = -(self.b1 * sin1 + self.b2 * sin2);
        let denominator_real = 1.0 + self.a1 * cos1 + self.a2 * cos2;
        let denominator_imag = -(self.a1 * sin1 + self.a2 * sin2);

        let numerator = numerator_real.hypot(numerator_imag);
        let denominator = denominator_real.hypot(denominator_imag);
        if denominator.abs() < f64::EPSILON {
            0.0
        } else {
            numerator / denominator
        }
    }

    /// Design one biquad from RBJ cookbook formulas.
    pub fn design(
        kind: BiquadType,
        frequency_hz: f32,
        gain_db: f32,
        q: f32,
        sample_rate: u32,
    ) -> Result<Self, AudioError> {
        let nyquist = sample_rate as f64 / 2.0;
        let frequency = f64::from(frequency_hz);
        if frequency <= 0.0 || frequency >= nyquist {
            return Err(AudioError::InvalidParameters(format!(
                "filter frequency {frequency_hz} Hz is outside (0, {nyquist})"
            )));
        }
        if !q.is_finite() || q <= 0.0 {
            return Err(AudioError::InvalidParameters(format!(
                "filter Q {q} must be positive"
            )));
        }

        let sample_rate = f64::from(sample_rate);
        let omega = 2.0 * std::f64::consts::PI * frequency / sample_rate;
        let (sin, cos) = omega.sin_cos();
        let alpha = sin / (2.0 * f64::from(q));
        let a = 10f64.powf(f64::from(gain_db) / 40.0);

        let (b0, b1, b2, a0, a1, a2) = match kind {
            BiquadType::Peak => (
                1.0 + alpha * a,
                -2.0 * cos,
                1.0 - alpha * a,
                1.0 + alpha / a,
                -2.0 * cos,
                1.0 - alpha / a,
            ),
            BiquadType::LowShelf => {
                let sqrt_a = a.sqrt();
                let two_sqrt_a_alpha = 2.0 * sqrt_a * alpha;
                (
                    a * ((a + 1.0) - (a - 1.0) * cos + two_sqrt_a_alpha),
                    2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
                    a * ((a + 1.0) - (a - 1.0) * cos - two_sqrt_a_alpha),
                    (a + 1.0) + (a - 1.0) * cos + two_sqrt_a_alpha,
                    -2.0 * ((a - 1.0) + (a + 1.0) * cos),
                    (a + 1.0) + (a - 1.0) * cos - two_sqrt_a_alpha,
                )
            }
            BiquadType::HighShelf => {
                let sqrt_a = a.sqrt();
                let two_sqrt_a_alpha = 2.0 * sqrt_a * alpha;
                (
                    a * ((a + 1.0) + (a - 1.0) * cos + two_sqrt_a_alpha),
                    -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
                    a * ((a + 1.0) + (a - 1.0) * cos - two_sqrt_a_alpha),
                    (a + 1.0) - (a - 1.0) * cos + two_sqrt_a_alpha,
                    2.0 * ((a - 1.0) - (a + 1.0) * cos),
                    (a + 1.0) - (a - 1.0) * cos - two_sqrt_a_alpha,
                )
            }
            BiquadType::LowPass => (
                (1.0 - cos) / 2.0,
                1.0 - cos,
                (1.0 - cos) / 2.0,
                1.0 + alpha,
                -2.0 * cos,
                1.0 - alpha,
            ),
            BiquadType::HighPass => (
                (1.0 + cos) / 2.0,
                -(1.0 + cos),
                (1.0 + cos) / 2.0,
                1.0 + alpha,
                -2.0 * cos,
                1.0 - alpha,
            ),
            BiquadType::BandPass => (alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
            BiquadType::Notch => (1.0, -2.0 * cos, 1.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
        };

        if a0.abs() < f64::EPSILON {
            return Err(AudioError::InvalidParameters(
                "degenerate filter coefficients".into(),
            ));
        }

        Ok(Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        })
    }
}

/// Direct-form-II-transposed biquad state for one channel.
#[derive(Debug, Clone, Copy, Default)]
struct BiquadState {
    z1: f64,
    z2: f64,
}

impl BiquadState {
    #[inline]
    fn process(&mut self, coefficients: &BiquadCoefficients, input: f64) -> f64 {
        let output = coefficients.b0 * input + self.z1;
        self.z1 = coefficients.b1 * input - coefficients.a1 * output + self.z2;
        self.z2 = coefficients.b2 * input - coefficients.a2 * output;
        output
    }
}

/// Active equaliser instance.
#[derive(Debug)]
pub struct ParametricEq {
    coefficients: Vec<BiquadCoefficients>,
    states: Vec<Vec<BiquadState>>,
    channels: usize,
    sample_rate: u32,
    preamp_gain: f64,
    active: bool,
}

impl ParametricEq {
    /// Compile a configuration for the given rate and channel count.
    pub fn new(
        settings: &EqSettings,
        sample_rate: u32,
        channels: usize,
    ) -> Result<Self, AudioError> {
        settings.validate(sample_rate)?;
        let coefficients = settings
            .bands
            .iter()
            .filter(|band| band.enabled && band.gain_db.abs() > f32::EPSILON)
            .map(|band| {
                BiquadCoefficients::design(
                    band.kind,
                    band.frequency_hz,
                    band.gain_db,
                    band.q,
                    sample_rate,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;

        let states = vec![vec![BiquadState::default(); coefficients.len()]; channels];
        let active = !coefficients.is_empty() || settings.preamp_db.abs() > f32::EPSILON;
        let preamp_gain = 10f64.powf(f64::from(settings.preamp_db) / 20.0);

        Ok(Self {
            coefficients,
            states,
            channels,
            sample_rate,
            preamp_gain,
            active,
        })
    }

    /// Sample rate the coefficients were designed for.
    #[must_use]
    pub const fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Number of active second-order sections.
    #[must_use]
    pub fn section_count(&self) -> usize {
        self.coefficients.len()
    }

    /// Combined magnitude response in dB at a frequency (for the EQ curve).
    #[must_use]
    pub fn response_db(&self, frequency_hz: f64) -> f64 {
        let rate = f64::from(self.sample_rate);
        let magnitude = self
            .coefficients
            .iter()
            .map(|coefficients| coefficients.magnitude_at(frequency_hz, rate))
            .product::<f64>()
            * self.preamp_gain;
        20.0 * magnitude.max(1e-9).log10()
    }

    /// Process interleaved samples in place.
    pub fn process(&mut self, samples: &mut [f32], channels: usize) {
        if !self.active || channels != self.channels {
            return;
        }

        for frame in samples.chunks_exact_mut(channels) {
            for channel in 0..channels {
                let mut value = f64::from(frame[channel]);
                if (self.preamp_gain - 1.0).abs() > f64::EPSILON {
                    value *= self.preamp_gain;
                }
                for (coefficients, state) in self
                    .coefficients
                    .iter()
                    .zip(self.states[channel].iter_mut())
                {
                    value = state.process(coefficients, value);
                }
                frame[channel] = value as f32;
            }
        }
    }

    /// Clear filter memory.
    pub fn reset(&mut self) {
        for channel in &mut self.states {
            channel.fill(BiquadState::default());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn sine(frequency: f32, amplitude: f32, frames: usize) -> Vec<f32> {
        (0..frames)
            .map(|frame| {
                amplitude
                    * (2.0 * std::f32::consts::PI * frequency * frame as f32 / RATE as f32).sin()
            })
            .collect()
    }

    fn rms(samples: &[f32]) -> f64 {
        if samples.is_empty() {
            return 0.0;
        }
        let sum: f64 = samples
            .iter()
            .map(|sample| f64::from(*sample).powi(2))
            .sum();
        (sum / samples.len() as f64).sqrt()
    }

    #[test]
    fn band_validation_rejects_impossible_filters() {
        let mut settings = EqSettings::graphic_10_band();
        settings.bands[0].frequency_hz = 30_000.0;
        assert!(settings.validate(RATE).is_err());

        settings.bands[0].frequency_hz = 100.0;
        settings.bands[0].q = 0.0;
        assert!(settings.validate(RATE).is_err());

        settings.bands[0].q = 1.0;
        settings.bands[0].gain_db = 60.0;
        assert!(settings.validate(RATE).is_err());
    }

    #[test]
    fn a_zero_gain_band_is_identity() {
        let mut settings = EqSettings::graphic_10_band();
        settings.enabled = true;
        // All gains are zero, so the band list compiles to no sections.
        let mut eq = ParametricEq::new(&settings, RATE, 2).expect("eq");
        assert_eq!(eq.section_count(), 0);

        let input: Vec<f32> = sine(1_000.0, 0.5, 480)
            .iter()
            .flat_map(|s| [*s, *s])
            .collect();
        let mut output = input.clone();
        eq.process(&mut output, 2);
        assert_eq!(input, output);
    }

    #[test]
    fn peak_filter_boosts_its_centre_frequency() {
        let settings = EqSettings {
            enabled: true,
            preamp_db: 0.0,
            bands: vec![EqBand::new(0, BiquadType::Peak, 1_000.0, 12.0, 1.0)],
        };
        let mut eq = ParametricEq::new(&settings, RATE, 1).expect("eq");
        assert_eq!(eq.section_count(), 1);

        let input = sine(1_000.0, 0.25, RATE as usize / 2);
        let input_rms = rms(&input[4_800..]);

        let mut output = input;
        eq.process(&mut output, 1);
        let output_rms = rms(&output[4_800..]);

        let gain_db = 20.0 * (output_rms / input_rms).log10();
        assert!(
            (gain_db - 12.0).abs() < 1.5,
            "expected ~12 dB boost, measured {gain_db:.2} dB"
        );
    }

    #[test]
    fn peak_filter_leaves_far_frequencies_untouched() {
        let settings = EqSettings {
            enabled: true,
            preamp_db: 0.0,
            bands: vec![EqBand::new(0, BiquadType::Peak, 1_000.0, 12.0, 1.0)],
        };
        let mut eq = ParametricEq::new(&settings, RATE, 1).expect("eq");

        let input = sine(100.0, 0.25, 48_000);
        let input_rms = rms(&input[9_600..]);
        let mut output = input;
        eq.process(&mut output, 1);
        let output_rms = rms(&output[9_600..]);

        let gain_db = 20.0 * (output_rms / input_rms).log10();
        assert!(
            gain_db.abs() < 1.0,
            "100 Hz should barely change, measured {gain_db:.2} dB"
        );
    }

    #[test]
    fn preamp_scales_the_whole_signal() {
        let settings = EqSettings {
            enabled: true,
            preamp_db: -6.0,
            bands: Vec::new(),
        };
        let mut eq = ParametricEq::new(&settings, RATE, 2).expect("eq");
        let input = vec![0.5f32; 64];
        let mut output = input.clone();
        eq.process(&mut output, 2);
        assert!((output[0] - 0.5 * 10f32.powf(-6.0 / 20.0)).abs() < 1e-6);
    }

    #[test]
    fn thirty_one_bands_are_accepted_and_stable() {
        let mut settings = EqSettings::graphic_31_band();
        settings
            .bands
            .iter_mut()
            .for_each(|band| band.gain_db = 3.0);
        let mut eq = ParametricEq::new(&settings, RATE, 2).expect("eq");
        assert_eq!(eq.section_count(), 31);

        let mut noise = vec![0.0f32; 2 * 48_000];
        let mut state = 0x1234_5678u32;
        for sample in &mut noise {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            *sample = ((state >> 8) as f32 / 8_388_608.0 - 1.0) * 0.25;
        }

        eq.process(&mut noise, 2);
        assert!(noise.iter().all(|sample| sample.is_finite()));
        assert!(
            noise.iter().all(|sample| sample.abs() < 8.0),
            "a +3 dB 31-band boost cannot explode the signal"
        );
    }

    #[test]
    fn response_curve_matches_design() {
        let settings = EqSettings {
            enabled: true,
            preamp_db: 0.0,
            bands: vec![EqBand::new(0, BiquadType::Peak, 1_000.0, 6.0, 1.0)],
        };
        let eq = ParametricEq::new(&settings, RATE, 2).expect("eq");
        assert!((eq.response_db(1_000.0) - 6.0).abs() < 0.3);
        assert!(eq.response_db(20_000.0).abs() < 4.0);
    }

    #[test]
    fn rejects_frequency_above_nyquist() {
        let settings = EqSettings {
            enabled: true,
            preamp_db: 0.0,
            bands: vec![EqBand::new(0, BiquadType::Peak, 24_000.0, 6.0, 1.0)],
        };
        assert!(ParametricEq::new(&settings, 48_000, 2).is_err());
    }
}
