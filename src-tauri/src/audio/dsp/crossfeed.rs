//! Headphone crossfeed.
//!
//! Crossfeeds a delayed, low-passed copy of each channel into the opposite ear.
//! This restores the interaural cues that headphone listening removes and
//! substantially reduces listening fatigue on long sessions without collapsing
//! the stereo image (the direct path is never attenuated).
//!
//! Two response families are offered:
//!
//! * **Bauer** (default) — 0.3 ms interaural delay, one-pole 700 Hz low-pass;
//!   the classic bs2b "Bauer" curve.
//! * **Meier** — shorter delay (0.18 ms), gentle 550 Hz low-pass and slightly
//!   lower cross gain, for a more conservative effect.
//!
//! Only stereo input is processed; mono and multichannel layouts pass through.

use serde::{Deserialize, Serialize};

use crate::audio::AudioError;

/// Crossfeed response family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CrossfeedAlgorithm {
    #[default]
    Bauer,
    Meier,
}

/// User-facing crossfeed configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrossfeedSettings {
    pub enabled: bool,
    #[serde(default)]
    pub algorithm: CrossfeedAlgorithm,
    /// Cross gain in dB, always ≤ 0.
    pub level_db: f32,
    /// Interaural delay in milliseconds.
    pub delay_ms: f32,
    /// Low-pass corner of the cross path in Hz.
    pub lowpass_hz: f32,
}

impl Default for CrossfeedSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            algorithm: CrossfeedAlgorithm::Bauer,
            level_db: -6.0,
            delay_ms: 0.3,
            lowpass_hz: 700.0,
        }
    }
}

impl CrossfeedSettings {
    /// Validate the configuration against a sample rate.
    pub fn validate(&self, sample_rate: u32) -> Result<(), AudioError> {
        if !self.level_db.is_finite() || !(-30.0..=0.0).contains(&self.level_db) {
            return Err(AudioError::InvalidParameters(format!(
                "crossfeed level {} dB is outside -30..=0 dB",
                self.level_db
            )));
        }
        if !self.delay_ms.is_finite() || !(0.0..=2.0).contains(&self.delay_ms) {
            return Err(AudioError::InvalidParameters(format!(
                "crossfeed delay {} ms is outside 0..=2 ms",
                self.delay_ms
            )));
        }
        let nyquist = sample_rate as f32 / 2.0;
        if !self.lowpass_hz.is_finite() || self.lowpass_hz <= 0.0 || self.lowpass_hz >= nyquist {
            return Err(AudioError::InvalidParameters(format!(
                "crossfeed low-pass {} Hz is outside (0, {nyquist})",
                self.lowpass_hz
            )));
        }
        Ok(())
    }

    /// Apply the algorithm's characteristic delay and corner frequency,
    /// keeping the user's level.
    #[must_use]
    pub fn with_algorithm(mut self, algorithm: CrossfeedAlgorithm) -> Self {
        self.algorithm = algorithm;
        match algorithm {
            CrossfeedAlgorithm::Bauer => {
                self.delay_ms = 0.3;
                self.lowpass_hz = 700.0;
            }
            CrossfeedAlgorithm::Meier => {
                self.delay_ms = 0.18;
                self.lowpass_hz = 550.0;
            }
        }
        self
    }
}

/// One-pole low-pass state.
#[derive(Debug, Clone, Copy, Default)]
struct OnePole {
    state: f32,
    coefficient: f32,
}

impl OnePole {
    fn new(corner_hz: f32, sample_rate: f32) -> Self {
        let coefficient = (-2.0 * std::f32::consts::PI * corner_hz / sample_rate).exp();
        Self {
            state: 0.0,
            coefficient,
        }
    }

    #[inline]
    fn process(&mut self, input: f32) -> f32 {
        // y[n] = (1 - a) x[n] + a y[n-1]
        self.state = (1.0 - self.coefficient) * input + self.coefficient * self.state;
        self.state
    }

    fn reset(&mut self) {
        self.state = 0.0;
    }
}

/// Active crossfeed processor (stereo only).
#[derive(Debug)]
pub struct Crossfeed {
    gain: f32,
    delay_frames: usize,
    delay_line: [Vec<f32>; 2],
    write_index: usize,
    lowpass: [OnePole; 2],
    channels: usize,
}

impl Crossfeed {
    /// Compile the processor for a rate and channel count.
    pub fn new(
        settings: &CrossfeedSettings,
        sample_rate: u32,
        channels: usize,
    ) -> Result<Self, AudioError> {
        settings.validate(sample_rate)?;

        let delay_frames = ((settings.delay_ms / 1000.0) * sample_rate as f32).round() as usize;
        let delay_frames = delay_frames.max(1);
        let rate = sample_rate as f32;

        Ok(Self {
            gain: 10f32.powf(settings.level_db / 20.0),
            delay_frames,
            delay_line: [vec![0.0; delay_frames + 1], vec![0.0; delay_frames + 1]],
            write_index: 0,
            lowpass: [
                OnePole::new(settings.lowpass_hz, rate),
                OnePole::new(settings.lowpass_hz, rate),
            ],
            channels,
        })
    }

    /// Interaural delay applied by this instance, in frames.
    #[must_use]
    pub const fn delay_frames(&self) -> usize {
        self.delay_frames
    }

    /// Process interleaved stereo in place.
    pub fn process(&mut self, samples: &mut [f32], channels: usize) {
        if channels != 2 || self.channels != 2 {
            return;
        }

        for frame in samples.chunks_exact_mut(2) {
            let left = frame[0];
            let right = frame[1];

            // The ring holds `delay_frames + 1` slots. Writing at
            // `write_index` makes `write_index + 1` the oldest sample, written
            // exactly `delay_frames` frames ago.
            let read_index = (self.write_index + 1) % (self.delay_frames + 1);
            // Delayed sample of each channel (before writing the current one).
            let delayed_left = self.delay_line[0][read_index];
            let delayed_right = self.delay_line[1][read_index];

            self.delay_line[0][self.write_index] = left;
            self.delay_line[1][self.write_index] = right;
            self.write_index = (self.write_index + 1) % (self.delay_frames + 1);

            let cross_left = self.lowpass[0].process(delayed_right) * self.gain;
            let cross_right = self.lowpass[1].process(delayed_left) * self.gain;

            frame[0] = left + cross_left;
            frame[1] = right + cross_right;
        }
    }

    /// Clear delay lines and filter memory.
    pub fn reset(&mut self) {
        for line in &mut self.delay_line {
            line.fill(0.0);
        }
        self.write_index = 0;
        for filter in &mut self.lowpass {
            filter.reset();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn settings() -> CrossfeedSettings {
        CrossfeedSettings {
            enabled: true,
            ..CrossfeedSettings::default()
        }
    }

    #[test]
    fn rejects_invalid_configuration() {
        let mut invalid = settings();
        invalid.level_db = 3.0;
        assert!(CrossfeedSettings::validate(&invalid, RATE).is_err());

        invalid = settings();
        invalid.delay_ms = 5.0;
        assert!(CrossfeedSettings::validate(&invalid, RATE).is_err());

        invalid = settings();
        invalid.lowpass_hz = 30_000.0;
        assert!(CrossfeedSettings::validate(&invalid, RATE).is_err());
    }

    #[test]
    fn algorithm_presets_set_known_values() {
        let bauer = settings().with_algorithm(CrossfeedAlgorithm::Bauer);
        assert!((bauer.delay_ms - 0.3).abs() < 1e-6);
        assert!((bauer.lowpass_hz - 700.0).abs() < 1e-6);

        let meier = settings().with_algorithm(CrossfeedAlgorithm::Meier);
        assert!((meier.delay_ms - 0.18).abs() < 1e-6);
        assert!((meier.lowpass_hz - 550.0).abs() < 1e-6);
    }

    #[test]
    fn mono_and_multichannel_pass_through() {
        let mut crossfeed = Crossfeed::new(&settings(), RATE, 2).expect("crossfeed");
        let mut mono = vec![0.5f32; 32];
        crossfeed.process(&mut mono, 1);
        assert!(mono.iter().all(|sample| (*sample - 0.5).abs() < 1e-9));
    }

    #[test]
    fn crossfeeds_energy_into_the_opposite_channel() {
        let mut crossfeed = Crossfeed::new(&settings(), RATE, 2).expect("crossfeed");
        let frames = 9_600; // 200 ms, long enough to leave the delay line

        // Left-only content: the direct path must stay bit-identical while the
        // cross path lights up the silent right channel.
        let mut buffer = vec![0.0f32; frames * 2];
        for frame in 0..frames {
            let sample =
                0.5 * (2.0 * std::f32::consts::PI * 300.0 * frame as f32 / RATE as f32).sin();
            buffer[frame * 2] = sample;
        }
        let input: Vec<f32> = buffer.clone();

        crossfeed.process(&mut buffer, 2);

        let direct_ok = buffer
            .chunks_exact(2)
            .zip(input.chunks_exact(2))
            .all(|(out, src)| out[0] == src[0]);
        assert!(direct_ok, "the direct path must not be modified");

        let left_energy: f64 = buffer
            .chunks_exact(2)
            .skip(4_800)
            .map(|frame| f64::from(frame[0]).powi(2))
            .sum();
        let right_energy: f64 = buffer
            .chunks_exact(2)
            .skip(4_800)
            .map(|frame| f64::from(frame[1]).powi(2))
            .sum();

        assert!(right_energy > 0.0, "the cross path must carry energy");
        assert!(
            right_energy < left_energy,
            "the cross path must stay below the direct path ({right_energy:e} vs {left_energy:e})"
        );
        assert!(
            right_energy > left_energy * 1e-4,
            "the cross path must be audible, not a rounding artefact"
        );
    }

    #[test]
    fn delay_is_applied_before_the_cross_path() {
        let mut crossfeed = Crossfeed::new(&settings(), RATE, 2).expect("crossfeed");
        let expected_delay = ((0.3f32 / 1000.0) * RATE as f32).round() as usize;
        assert_eq!(crossfeed.delay_frames(), expected_delay.max(1));

        // Impulse in the left channel: the right (cross) channel must stay
        // silent until the delay has elapsed, while the direct path passes
        // through untouched at t=0.
        let delay = crossfeed.delay_frames();
        let mut buffer = vec![0.0f32; (delay + 64) * 2];
        buffer[0] = 1.0;

        crossfeed.process(&mut buffer, 2);

        assert_eq!(buffer[0], 1.0, "the direct-left path is passed through");
        for frame in 0..delay {
            assert_eq!(
                buffer[frame * 2 + 1],
                0.0,
                "the cross path must be silent during the delay window"
            );
        }
        assert!(
            buffer[delay * 2 + 1] > 0.0,
            "the cross path must start after {delay} frames"
        );
    }

    #[test]
    fn mono_content_stays_centred() {
        let mut crossfeed = Crossfeed::new(&settings(), RATE, 2).expect("crossfeed");
        let frames = 4_800;
        let mut buffer = vec![0.0f32; frames * 2];
        for frame in 0..frames {
            let sample =
                (2.0 * std::f32::consts::PI * 440.0 * frame as f32 / RATE as f32).sin() * 0.5;
            buffer[frame * 2] = sample;
            buffer[frame * 2 + 1] = sample;
        }
        crossfeed.process(&mut buffer, 2);

        for frame in buffer.chunks_exact(2).skip(1_000) {
            assert!(
                (frame[0] - frame[1]).abs() < 1e-6,
                "channels must stay identical"
            );
        }
    }

    #[test]
    fn reset_clears_state() {
        let mut crossfeed = Crossfeed::new(&settings(), RATE, 2).expect("crossfeed");
        let mut buffer = vec![0.5f32; 64];
        crossfeed.process(&mut buffer, 2);
        crossfeed.reset();

        let mut silence = vec![0.0f32; 64];
        crossfeed.process(&mut silence, 2);
        assert!(silence.iter().all(|sample| sample.abs() < 1e-9));
    }
}
