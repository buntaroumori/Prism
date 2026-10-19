//! ReplayGain 2 application with clipping protection.
//!
//! The gain values themselves are produced by the R128 meter (scan time) or
//! read from tags; this stage applies them with an optional pre-amp and a
//! peak-aware safety policy:
//!
//! * **Peak-aware attenuation** — when the peak is known (tag or analysis) and
//!   `gain × peak > 1.0`, the gain is reduced to `1 / peak` instead of
//!   clipping. This preserves the waveform shape at the cost of a few dB.
//! * **Hard guard** — when the peak is unknown and `prevent_clipping` is on,
//!   samples are clamped at full scale and the limiter counts them so the UI
//!   can flag the track as "gain-limited".

use serde::{Deserialize, Serialize};

/// Normalisation source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ReplayGainMode {
    #[default]
    Off,
    Track,
    Album,
}

/// User-facing ReplayGain configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayGainSettings {
    pub enabled: bool,
    #[serde(default)]
    pub mode: ReplayGainMode,
    /// Pre-amp applied on top of the tag gain, in dB.
    pub preamp_db: f32,
    pub prevent_clipping: bool,
}

impl Default for ReplayGainSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: ReplayGainMode::Track,
            preamp_db: 0.0,
            prevent_clipping: true,
        }
    }
}

impl ReplayGainSettings {
    /// True when gain might be applied.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.enabled && self.mode != ReplayGainMode::Off
    }

    /// Select the tag value matching the configured mode.
    #[must_use]
    pub fn select(&self, track_gain: Option<f32>, album_gain: Option<f32>) -> Option<f32> {
        match self.mode {
            ReplayGainMode::Off => None,
            ReplayGainMode::Track => track_gain,
            ReplayGainMode::Album => album_gain.or(track_gain),
        }
    }
}

/// Active gain stage.
#[derive(Debug, Clone)]
pub struct ReplayGain {
    applied_db: f32,
    factor: f32,
    peak: Option<f32>,
    prevent_clipping: bool,
    limited_samples: u64,
    attenuation_db: f32,
}

impl ReplayGain {
    /// Create the stage. `gain_db`/`peak` are the selected ReplayGain tag
    /// values; missing tags produce unity gain.
    #[must_use]
    pub fn new(settings: &ReplayGainSettings, gain_db: Option<f32>, peak: Option<f32>) -> Self {
        let tag_gain = gain_db.unwrap_or(0.0);
        let requested_db = tag_gain + settings.preamp_db;
        let mut factor = 10f32.powf(requested_db / 20.0);
        let mut attenuation_db = 0.0;

        if settings.prevent_clipping
            && let Some(peak) = peak
            && peak > 0.0
            && factor * peak > 1.0
        {
            let safe = 1.0 / peak;
            attenuation_db = 20.0 * (safe / factor).log10();
            factor = safe;
        }

        Self {
            applied_db: 20.0 * factor.log10(),
            factor,
            peak,
            prevent_clipping: settings.prevent_clipping,
            limited_samples: 0,
            attenuation_db,
        }
    }

    /// Linear gain applied to every sample.
    #[must_use]
    pub const fn factor(&self) -> f32 {
        self.factor
    }

    /// Effective gain in dB after the anti-clipping policy.
    #[must_use]
    pub const fn applied_db(&self) -> f32 {
        self.applied_db
    }

    /// How much gain the clipping guard removed (≤ 0 dB).
    #[must_use]
    pub const fn attenuation_db(&self) -> f32 {
        self.attenuation_db
    }

    /// Peak value the guard used, when known.
    #[must_use]
    pub const fn peak(&self) -> Option<f32> {
        self.peak
    }

    /// Number of samples clamped by the hard guard since the last reset.
    #[must_use]
    pub const fn limited_samples(&self) -> u64 {
        self.limited_samples
    }

    /// Apply gain in place.
    pub fn process(&mut self, samples: &mut [f32], _channels: usize) {
        if (self.factor - 1.0).abs() < f32::EPSILON {
            return;
        }

        if self.prevent_clipping {
            for sample in samples.iter_mut() {
                let scaled = *sample * self.factor;
                if scaled > 1.0 || scaled < -1.0 {
                    self.limited_samples += 1;
                    *sample = scaled.clamp(-1.0, 1.0);
                } else {
                    *sample = scaled;
                }
            }
        } else {
            for sample in samples.iter_mut() {
                *sample *= self.factor;
            }
        }
    }

    /// Reset the limiter counter (called at every track boundary).
    pub fn reset(&mut self) {
        self.limited_samples = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> ReplayGainSettings {
        ReplayGainSettings {
            enabled: true,
            mode: ReplayGainMode::Track,
            preamp_db: 0.0,
            prevent_clipping: true,
        }
    }

    #[test]
    fn mode_selects_track_or_album_gain() {
        let mut config = settings();
        assert_eq!(config.select(Some(-7.0), Some(-9.0)), Some(-7.0));

        config.mode = ReplayGainMode::Album;
        assert_eq!(config.select(Some(-7.0), Some(-9.0)), Some(-9.0));

        // Album falls back to track when the album tag is missing.
        assert_eq!(config.select(Some(-7.0), None), Some(-7.0));

        config.mode = ReplayGainMode::Off;
        assert_eq!(config.select(Some(-7.0), Some(-9.0)), None);
    }

    #[test]
    fn applies_tag_gain_plus_preamp() {
        let mut config = settings();
        config.preamp_db = 2.0;
        let mut gain = ReplayGain::new(&config, Some(-6.0), Some(0.5));

        // -6 + 2 = -4 dB → factor 0.6310
        assert!((gain.factor() - 10f32.powf(-4.0 / 20.0)).abs() < 1e-6);
        assert!((gain.applied_db() + 4.0).abs() < 1e-4);

        let mut samples = vec![0.5f32, -0.5];
        gain.process(&mut samples, 2);
        assert!((samples[0] - 0.5 * gain.factor()).abs() < 1e-6);
        assert_eq!(gain.limited_samples(), 0);
    }

    #[test]
    fn attenuates_when_the_peak_would_clip() {
        let config = settings();
        let gain = ReplayGain::new(&config, Some(6.0), Some(0.9));
        // 6 dB on a 0.9 peak would reach 1.79; the guard limits to 1/0.9.
        assert!((gain.factor() - 1.0 / 0.9).abs() < 1e-5);
        assert!(gain.attenuation_db() < -4.0);
    }

    #[test]
    fn hard_guard_clamps_and_counts_without_known_peak() {
        let config = settings();
        let mut gain = ReplayGain::new(&config, Some(12.0), None);

        let mut samples = vec![0.9f32, -0.9, 0.1];
        gain.process(&mut samples, 1);
        assert_eq!(samples[0], 1.0);
        assert_eq!(samples[1], -1.0);
        assert!(samples[2] < 0.1 * gain.factor() + 1e-6);
        assert_eq!(gain.limited_samples(), 2);
    }

    #[test]
    fn clipping_guard_can_be_disabled() {
        let mut config = settings();
        config.prevent_clipping = false;
        let mut gain = ReplayGain::new(&config, Some(12.0), None);
        let mut samples = vec![0.9f32];
        gain.process(&mut samples, 1);
        assert!(samples[0] > 1.0);
        assert_eq!(gain.limited_samples(), 0);
    }

    #[test]
    fn missing_tags_are_unity_gain() {
        let config = settings();
        let mut gain = ReplayGain::new(&config, None, None);
        assert!((gain.factor() - 1.0).abs() < f32::EPSILON);
        let mut samples = vec![0.33f32];
        gain.process(&mut samples, 1);
        assert_eq!(samples[0], 0.33);
    }

    #[test]
    fn reset_clears_the_limiter_counter() {
        let config = settings();
        let mut gain = ReplayGain::new(&config, Some(12.0), None);
        let mut samples = vec![2.0f32];
        gain.process(&mut samples, 1);
        assert_eq!(gain.limited_samples(), 1);
        gain.reset();
        assert_eq!(gain.limited_samples(), 0);
    }
}
