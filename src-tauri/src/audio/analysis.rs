//! Spectrum analysis for the visualiser.
//!
//! The FFT runs where the samples are: the supervisor hands every post-DSP block
//! to [`Analyzer`], which keeps the most recent window in a small ring. A
//! background task turns that window into 31 log-spaced bands at the display
//! rate and broadcasts them. The webview only ever sees normalised numbers —
//! it never decodes audio, and it never computes a transform on the render
//! thread.
//!
//! Why 31 bands: it is the classic third-octave spectrum analyser layout, it maps
//! cleanly onto the equaliser page (same 31 bands), and it keeps each IPC frame
//! under 400 bytes.

use std::sync::atomic::{AtomicBool, Ordering};

use parking_lot::Mutex;
use realfft::RealFftPlanner;
use realfft::num_complex::Complex;
use serde::Serialize;

/// Transform size. 2048 at 44.1 kHz is a 46 ms window — long enough to resolve
/// 21.5 Hz bins, short enough that the display does not lag the music.
pub const FFT_SIZE: usize = 2048;
/// Third-octave bands across the audible range.
pub const BAND_COUNT: usize = 31;
/// Lowest band centre.
const MIN_HZ: f32 = 20.0;
/// Highest band centre; clamped to Nyquist for 44.1 kHz and below.
const MAX_HZ: f32 = 20_000.0;
/// Floor of the display range in dBFS.
const FLOOR_DB: f32 = -80.0;
/// Samples retained: two windows, so a slow reader still finds a full frame.
const RING_CAPACITY: usize = FFT_SIZE * 2;

/// One display frame.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpectrumFrame {
    /// Per-band magnitude, `0.0..=1.0`, already attack/release smoothed.
    pub bands: Vec<f32>,
    /// Broadband level, `0.0..=1.0`, for the level meter.
    pub level: f32,
    /// True when the frame carries signal; a silent frame keeps the last shape
    /// but decays to zero.
    pub silent: bool,
    pub sample_rate: u32,
}

impl SpectrumFrame {
    /// A frame of silence at the given rate.
    #[must_use]
    pub fn silent(sample_rate: u32) -> Self {
        Self {
            bands: vec![0.0; BAND_COUNT],
            level: 0.0,
            silent: true,
            sample_rate,
        }
    }
}

/// Shared window between the audio thread and the display task.
#[derive(Debug)]
pub struct Analyzer {
    inner: Mutex<Window>,
    enabled: AtomicBool,
    frames: AtomicBool,
}

#[derive(Debug)]
struct Window {
    ring: Vec<f32>,
    write: usize,
    filled: usize,
    sample_rate: u32,
}

impl Default for Window {
    fn default() -> Self {
        Self {
            ring: vec![0.0; RING_CAPACITY],
            write: 0,
            filled: 0,
            sample_rate: 0,
        }
    }
}

impl Default for Analyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl Analyzer {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Window::default()),
            enabled: AtomicBool::new(false),
            frames: AtomicBool::new(false),
        }
    }

    /// Turn capture on or off. The panel calls this when it mounts and unmounts,
    /// so a hidden visualiser costs exactly nothing.
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// True once at least one full window has been captured.
    #[must_use]
    pub fn has_frames(&self) -> bool {
        self.frames.load(Ordering::Relaxed)
    }

    /// Feed one interleaved block. Called from the supervisor thread, so it does
    /// the minimum possible work: downmix and copy.
    pub fn push(&self, samples: &[f32], channels: usize, sample_rate: u32) {
        if !self.is_enabled() || samples.is_empty() || channels == 0 {
            return;
        }
        let mut window = self.inner.lock();
        if window.sample_rate != sample_rate {
            window.sample_rate = sample_rate;
            window.filled = 0;
        }
        let gain = 1.0 / channels as f32;
        for frame in samples.chunks_exact(channels) {
            let mono: f32 = frame.iter().sum::<f32>() * gain;
            let cursor = window.write;
            window.ring[cursor] = mono;
            window.write = (cursor + 1) % RING_CAPACITY;
            window.filled = (window.filled + 1).min(RING_CAPACITY);
        }
        if window.filled >= FFT_SIZE {
            self.frames.store(true, Ordering::Relaxed);
        }
    }

    /// Copy the newest window and its rate. Returns `None` until a full window
    /// has arrived, so the display never renders a half-filled buffer as a
    /// spectrum.
    fn snapshot(&self) -> Option<(Vec<f32>, u32)> {
        let window = self.inner.lock();
        if window.filled < FFT_SIZE {
            return None;
        }
        let mut samples = Vec::with_capacity(FFT_SIZE);
        let start = (window.write + RING_CAPACITY - FFT_SIZE) % RING_CAPACITY;
        for offset in 0..FFT_SIZE {
            samples.push(window.ring[(start + offset) % RING_CAPACITY]);
        }
        Some((samples, window.sample_rate))
    }

    /// Compute one frame, applying the smoother that carries the previous state.
    ///
    /// Returns `None` when no full window exists yet (the transport is stopped
    /// or the panel has just been opened).
    pub fn frame(&self, smoother: &mut BandSmoother) -> Option<SpectrumFrame> {
        let (samples, sample_rate) = self.snapshot()?;
        Some(smoother.process(&samples, sample_rate))
    }
}

/// Attack/release smoothing and band mapping.
///
/// Fast attack (a transient must be visible on the frame it happens) and slow
/// release (bars fall like meters, not like a flicker).
#[derive(Debug, Clone)]
pub struct BandSmoother {
    bands: Vec<f32>,
    level: f32,
    attack: f32,
    release: f32,
}

impl Default for BandSmoother {
    fn default() -> Self {
        Self::new(0.55, 0.14)
    }
}

impl BandSmoother {
    #[must_use]
    pub fn new(attack: f32, release: f32) -> Self {
        Self {
            bands: vec![0.0; BAND_COUNT],
            level: 0.0,
            attack: attack.clamp(0.01, 1.0),
            release: release.clamp(0.01, 1.0),
        }
    }

    #[must_use]
    pub fn process(&mut self, samples: &[f32], sample_rate: u32) -> SpectrumFrame {
        let raw = spectrum_bands(samples, sample_rate);
        let mut energy = 0.0_f32;
        for (index, value) in raw.iter().enumerate() {
            let previous = self.bands[index];
            let coeff = if *value > previous {
                self.attack
            } else {
                self.release
            };
            let next = previous + (value - previous) * coeff;
            self.bands[index] = next;
            energy += next * next;
        }
        let level = (energy / BAND_COUNT as f32).sqrt().clamp(0.0, 1.0);
        self.level += (level - self.level) * if level > self.level { self.attack } else { self.release };

        SpectrumFrame {
            bands: self.bands.clone(),
            level: round3(self.level),
            silent: level < 1e-4,
            sample_rate,
        }
    }

    /// Decay a stopped transport towards silence without new samples.
    pub fn decay(&mut self) -> SpectrumFrame {
        for value in &mut self.bands {
            *value *= 0.82;
        }
        self.level *= 0.82;
        SpectrumFrame {
            bands: self.bands.iter().map(|value| round3(*value)).collect(),
            level: round3(self.level),
            silent: self.level < 1e-3,
            sample_rate: 0,
        }
    }
}

/// Band edges (Hz) for the log-spaced display, clamped to Nyquist.
#[must_use]
pub fn band_edges(sample_rate: u32) -> Vec<f32> {
    let nyquist = sample_rate as f32 / 2.0;
    let top = MAX_HZ.min(nyquist * 0.98).max(MIN_HZ * 2.0);
    let ratio = (top / MIN_HZ).powf(1.0 / BAND_COUNT as f32);
    (0..=BAND_COUNT)
        .map(|index| MIN_HZ * ratio.powi(index as i32))
        .collect()
}

/// Magnitudes mapped onto [`BAND_COUNT`] normalised bands.
#[must_use]
pub fn spectrum_bands(samples: &[f32], sample_rate: u32) -> Vec<f32> {
    let mut bands = vec![0.0_f32; BAND_COUNT];
    if samples.len() < FFT_SIZE || sample_rate == 0 {
        return bands;
    }

    let mut planner = RealFftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FFT_SIZE);
    let mut input = fft.make_input_vec();
    let mut output = fft.make_output_vec();

    // Hann window: without it, a 1 kHz tone leaks across the whole display.
    let denom = (FFT_SIZE - 1) as f32;
    for (index, sample) in samples.iter().take(FFT_SIZE).enumerate() {
        let window = 0.5 - 0.5 * (std::f32::consts::TAU * index as f32 / denom).cos();
        input[index] = sample * window;
    }

    if fft.process(&mut input, &mut output).is_err() {
        return bands;
    }

    let bin_hz = sample_rate as f32 / FFT_SIZE as f32;
    let edges = band_edges(sample_rate);
    // Amplitude of a full-scale sine after a Hann window is 0.5; scale so a
    // 0 dBFS tone maps to 0 dB.
    let scale = 2.0 / (FFT_SIZE as f32 * 0.5);
    let peak = |bin: usize| -> f32 {
        output
            .get(bin)
            .map_or(0.0, |value: &Complex<f32>| value.norm() * scale)
    };

    for (index, band) in bands.iter_mut().enumerate() {
        let low = edges[index];
        let high = edges[index + 1];
        let first = ((low / bin_hz).floor() as usize).max(1);
        let last = ((high / bin_hz).ceil() as usize).min(output.len().saturating_sub(1));
        let mut magnitude = 0.0_f32;
        if last >= first {
            for bin in first..=last {
                magnitude = magnitude.max(peak(bin));
            }
        } else {
            magnitude = peak(first.min(output.len().saturating_sub(1)));
        }
        *band = db_to_unit(magnitude);
    }

    bands
}

/// dB amplitude → `0..=1` display unit.
fn db_to_unit(magnitude: f32) -> f32 {
    if magnitude <= 1e-9 {
        return 0.0;
    }
    let db = 20.0 * magnitude.log10();
    ((db - FLOOR_DB) / -FLOOR_DB).clamp(0.0, 1.0)
}

fn round3(value: f32) -> f32 {
    (value * 1000.0).round() / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(frequency: f32, sample_rate: u32, amplitude: f32) -> Vec<f32> {
        (0..FFT_SIZE)
            .map(|index| {
                amplitude
                    * (std::f32::consts::TAU * frequency * index as f32 / sample_rate as f32).sin()
            })
            .collect()
    }

    fn dominant_band(bands: &[f32]) -> usize {
        bands
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map_or(0, |(index, _)| index)
    }

    #[test]
    fn a_one_kilohertz_tone_lands_in_the_right_band() {
        let edges = band_edges(48_000);
        let bands = spectrum_bands(&sine(1_000.0, 48_000, 0.5), 48_000);
        let dominant = dominant_band(&bands);
        let low = edges[dominant];
        let high = edges[dominant + 1];
        assert!(
            (low..=high).contains(&1_000.0),
            "1 kHz fell into {low:.0}–{high:.0} Hz (band {dominant})"
        );
        assert!(bands[dominant] > 0.5, "a half-scale tone must be clearly visible");
    }

    #[test]
    fn silence_produces_no_bands_and_silence_is_reported() {
        let bands = spectrum_bands(&vec![0.0; FFT_SIZE], 44_100);
        assert_eq!(bands.len(), BAND_COUNT);
        assert!(bands.iter().all(|value| *value == 0.0));

        let mut smoother = BandSmoother::default();
        let frame = smoother.process(&vec![0.0; FFT_SIZE], 44_100);
        assert!(frame.silent);
        assert_eq!(frame.level, 0.0);
    }

    #[test]
    fn a_short_buffer_is_not_analysed() {
        let bands = spectrum_bands(&vec![0.25; 128], 44_100);
        assert!(bands.iter().all(|value| *value == 0.0));
    }

    #[test]
    fn the_bands_are_log_spaced_and_monotonic() {
        let edges = band_edges(48_000);
        assert_eq!(edges.len(), BAND_COUNT + 1);
        for pair in edges.windows(2) {
            assert!(pair[1] > pair[0], "edges must ascend: {pair:?}");
        }
        assert!(edges[BAND_COUNT] <= 24_000.5);
    }

    #[test]
    fn low_sample_rates_clamp_the_top_band_to_nyquist() {
        let edges = band_edges(8_000);
        assert!(edges[BAND_COUNT] < 4_000.0);
    }

    #[test]
    fn the_smoother_attacks_faster_than_it_releases() {
        let mut smoother = BandSmoother::new(0.9, 0.1);
        let loud = sine(1_000.0, 44_100, 0.8);
        let first = smoother.process(&loud, 44_100);
        let second = smoother.process(&loud, 44_100);
        assert!(second.level >= first.level, "attack must not reduce level");

        let quiet = vec![0.0; FFT_SIZE];
        let after = smoother.process(&quiet, 44_100);
        assert!(after.level < second.level, "release must fall towards silence");
        let decayed = smoother.decay();
        assert!(decayed.level < after.level);
    }

    #[test]
    fn the_analyzer_keeps_only_what_it_was_given() {
        let analyzer = Analyzer::new();
        // Disabled by default: capture is opt-in from the panel.
        analyzer.push(&vec![0.5; FFT_SIZE], 2, 44_100);
        assert!(!analyzer.has_frames());
        assert!(analyzer.snapshot().is_none());

        analyzer.set_enabled(true);
        // One channel: 2048 samples fill the window exactly.
        analyzer.push(&vec![0.5; FFT_SIZE], 1, 44_100);
        assert!(analyzer.has_frames());
        let (window, rate) = analyzer.snapshot().expect("window");
        assert_eq!(window.len(), FFT_SIZE);
        assert_eq!(rate, 44_100);
        assert!((window[0] - 0.5).abs() < 1e-6, "mono content is copied verbatim");
    }

    #[test]
    fn interleaved_stereo_is_downmixed_before_analysis() {
        let analyzer = Analyzer::new();
        analyzer.set_enabled(true);
        // Left = +1, right = -1 → mono = 0, which must read as silence.
        let mut block = Vec::with_capacity(FFT_SIZE * 2);
        for _ in 0..FFT_SIZE {
            block.push(1.0);
            block.push(-1.0);
        }
        analyzer.push(&block, 2, 44_100);
        let (window, _) = analyzer.snapshot().expect("window");
        assert!(window.iter().all(|value| value.abs() < 1e-6));
    }

    #[test]
    fn the_analyzer_survives_a_sample_rate_change() {
        let analyzer = Analyzer::new();
        analyzer.set_enabled(true);
        analyzer.push(&vec![0.1; FFT_SIZE], 1, 44_100);
        assert!(analyzer.snapshot().is_some());
        // A new clock invalidates the old window rather than pretending it fits
        // at the wrong rate: only samples captured at 96 kHz count.
        analyzer.push(&vec![0.1; FFT_SIZE / 2], 1, 96_000);
        assert!(analyzer.snapshot().is_none());
        analyzer.push(&vec![0.1; FFT_SIZE / 2], 1, 96_000);
        assert_eq!(analyzer.snapshot().map(|(_, rate)| rate), Some(96_000));
    }
}
