//! Sample formats, channel handling and DSD-over-PCM framing.
//!
//! All internal processing is interleaved `f32` in the nominal range
//! `[-1.0, 1.0]`. Conversion to the device's native integer format happens
//! once, in the backend writer, with round-to-nearest and saturation — never
//! with a lossy float round-trip through another integer width.
//!
//! Bit-perfect rule: when the source is integer PCM and no DSP stage is
//! active, the decoder produces integer samples that are widened to `f32`
//! exactly (every integer in 16/24-bit fits an f32 mantissa), and the backend
//! converts back with `round()`. The value is therefore preserved exactly for
//! 16- and 24-bit sources; 32-bit integer sources are preserved when the
//! backend is also 32-bit integer.

use serde::{Deserialize, Serialize};

use crate::audio::AudioError;

/// Physical sample representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SampleFormat {
    /// Signed 16-bit little-endian.
    S16,
    /// Signed 24-bit packed in 3 bytes little-endian.
    S24,
    /// Signed 24-bit stored in the low 24 bits of a 32-bit word (ALSA `S24_LE`).
    S24In32,
    /// Signed 32-bit little-endian.
    S32,
    /// 32-bit IEEE float.
    F32,
    /// 64-bit IEEE float.
    F64,
}

impl SampleFormat {
    /// Bytes occupied by one sample per channel.
    #[must_use]
    pub const fn bytes(self) -> usize {
        match self {
            Self::S16 => 2,
            Self::S24 => 3,
            Self::S24In32 | Self::S32 | Self::F32 => 4,
            Self::F64 => 8,
        }
    }

    /// Effective integer bit depth (`None` for float formats).
    #[must_use]
    pub const fn bit_depth(self) -> Option<u8> {
        match self {
            Self::S16 => Some(16),
            Self::S24 | Self::S24In32 => Some(24),
            Self::S32 => Some(32),
            Self::F32 | Self::F64 => None,
        }
    }

    /// Bit depth reported to the UI: floats expose their mantissa precision.
    #[must_use]
    pub const fn display_bits(self) -> u8 {
        match self {
            Self::S16 => 16,
            Self::S24 | Self::S24In32 => 24,
            Self::S32 | Self::F32 => 32,
            Self::F64 => 64,
        }
    }

    #[must_use]
    pub const fn is_float(self) -> bool {
        matches!(self, Self::F32 | Self::F64)
    }

    /// Human label used in the player bar quality badge.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::S16 => "16-bit",
            Self::S24 => "24-bit",
            Self::S24In32 => "24-bit",
            Self::S32 => "32-bit",
            Self::F32 => "32-bit float",
            Self::F64 => "64-bit float",
        }
    }
}

/// Sample rate and channel layout accepted by Prism.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioFormat {
    pub sample_rate: u32,
    pub channels: u16,
    pub sample_format: SampleFormat,
}

/// Lowest rate accepted by the engine (telephony-grade material).
pub const MIN_SAMPLE_RATE: u32 = 8_000;
/// Highest rate accepted by the engine (DSD512 via DoP).
pub const MAX_SAMPLE_RATE: u32 = 1_536_000;
/// Highest channel count accepted by the engine.
pub const MAX_CHANNELS: u16 = 16;

impl AudioFormat {
    /// Validate and construct a format.
    pub fn new(
        sample_rate: u32,
        channels: u16,
        sample_format: SampleFormat,
    ) -> Result<Self, AudioError> {
        if !(MIN_SAMPLE_RATE..=MAX_SAMPLE_RATE).contains(&sample_rate) {
            return Err(AudioError::InvalidParameters(format!(
                "sample rate {sample_rate} Hz is outside {MIN_SAMPLE_RATE}..={MAX_SAMPLE_RATE} Hz"
            )));
        }
        if !(1..=MAX_CHANNELS).contains(&channels) {
            return Err(AudioError::InvalidParameters(format!(
                "channel count {channels} is outside 1..={MAX_CHANNELS}"
            )));
        }
        Ok(Self {
            sample_rate,
            channels,
            sample_format,
        })
    }

    /// Bytes in one interleaved frame (all channels, one sample each).
    #[must_use]
    pub const fn frame_size(&self) -> usize {
        self.sample_format.bytes() * self.channels as usize
    }

    /// Bytes per second at this format.
    #[must_use]
    pub const fn byte_rate(&self) -> u64 {
        self.sample_rate as u64 * self.frame_size() as u64
    }

    /// Duration in milliseconds of `frames` frames.
    #[must_use]
    pub fn frames_to_ms(&self, frames: u64) -> u64 {
        (frames.saturating_mul(1000)) / u64::from(self.sample_rate)
    }

    /// Frame index containing the given millisecond.
    #[must_use]
    pub fn ms_to_frames(&self, ms: u64) -> u64 {
        ms.saturating_mul(u64::from(self.sample_rate)) / 1000
    }

    /// Technical quality badge: `24-bit / 96.0 kHz FLAC`.
    #[must_use]
    pub fn quality_label(&self, container: &str) -> String {
        let khz = f64::from(self.sample_rate) / 1000.0;
        let rate = if (khz - khz.round()).abs() < f64::EPSILON {
            format!("{khz:.0}")
        } else {
            format!("{khz:.1}")
        };
        format!("{} / {rate} kHz {container}", self.sample_format.label())
    }
}

/// Clamp a float sample to the representable interval.
#[inline]
#[must_use]
pub fn clamp_sample(sample: f32) -> f32 {
    if sample.is_nan() {
        0.0
    } else {
        sample.clamp(-1.0, 1.0)
    }
}

/// Convert interleaved `f32` into bytes of the requested sample format.
///
/// Rounding is round-half-away-from-zero via `f32::round`, which matches the
/// conversion used by professional PCM writers and keeps integer sources
/// bit-exact across the `i32 → f32 → i32` round trip for ≤24 bits.
pub fn write_samples_f32(
    samples: &[f32],
    format: SampleFormat,
    out: &mut Vec<u8>,
) -> Result<(), AudioError> {
    out.clear();
    out.reserve(samples.len() * format.bytes());

    match format {
        SampleFormat::S16 => {
            for &sample in samples {
                let value = (f64::from(clamp_sample(sample)) * 32_768.0)
                    .round()
                    .clamp(-32_768.0, 32_767.0) as i16;
                out.extend_from_slice(&value.to_le_bytes());
            }
        }
        SampleFormat::S24 => {
            for &sample in samples {
                let value = (f64::from(clamp_sample(sample)) * 8_388_608.0)
                    .round()
                    .clamp(-8_388_608.0, 8_388_607.0) as i32;
                let bytes = value.to_le_bytes();
                out.extend_from_slice(&bytes[0..3]);
            }
        }
        SampleFormat::S24In32 | SampleFormat::S32 => {
            let scale = if format == SampleFormat::S24In32 {
                8_388_608.0
            } else {
                2_147_483_648.0
            };
            let max = if format == SampleFormat::S24In32 {
                8_388_607.0
            } else {
                2_147_483_647.0
            };
            for &sample in samples {
                let value = (f64::from(clamp_sample(sample)) * scale)
                    .round()
                    .clamp(-scale, max) as i32;
                out.extend_from_slice(&value.to_le_bytes());
            }
        }
        SampleFormat::F32 => {
            for &sample in samples {
                out.extend_from_slice(&clamp_sample(sample).to_le_bytes());
            }
        }
        SampleFormat::F64 => {
            for &sample in samples {
                out.extend_from_slice(&f64::from(clamp_sample(sample)).to_le_bytes());
            }
        }
    }

    Ok(())
}

/// Read bytes of the given format into interleaved `f32`.
///
/// Used by decoders that hand raw integer PCM (WAV/DSD tests) and by the
/// spectrum analyser when it consumes device-format buffers.
pub fn read_samples_f32(bytes: &[u8], format: SampleFormat) -> Result<Vec<f32>, AudioError> {
    let width = format.bytes();
    if width == 0 || bytes.len() % width != 0 {
        return Err(AudioError::InvalidParameters(format!(
            "byte length {} is not a multiple of the {width}-byte sample width",
            bytes.len()
        )));
    }

    let mut samples = Vec::with_capacity(bytes.len() / width);
    for chunk in bytes.chunks_exact(width) {
        let value = match format {
            SampleFormat::S16 => f32::from(i16::from_le_bytes([chunk[0], chunk[1]])) / 32_768.0,
            SampleFormat::S24 => {
                let raw = i32::from_le_bytes([chunk[0], chunk[1], chunk[2], 0]) << 8 >> 8;
                raw as f32 / 8_388_608.0
            }
            SampleFormat::S24In32 => {
                let raw = i32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                let value = (raw << 8) >> 8;
                value as f32 / 8_388_608.0
            }
            SampleFormat::S32 => {
                let raw = i32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                raw as f32 / 2_147_483_648.0
            }
            SampleFormat::F32 => f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]),
            SampleFormat::F64 => {
                let raw = f64::from_le_bytes([
                    chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
                ]);
                raw as f32
            }
        };
        samples.push(value);
    }

    Ok(samples)
}

/// Convert planar channel vectors into one interleaved buffer.
pub fn interleave(planar: &[Vec<f32>], frames: usize, channels: usize) -> Vec<f32> {
    let mut out = vec![0.0; frames * channels];
    for channel in 0..channels.min(planar.len()) {
        let source = &planar[channel];
        let count = frames.min(source.len());
        for frame in 0..count {
            out[frame * channels + channel] = source[frame];
        }
    }
    out
}

/// Split an interleaved buffer into planar channel vectors.
#[must_use]
pub fn deinterleave(interleaved: &[f32], channels: usize) -> Vec<Vec<f32>> {
    if channels == 0 {
        return Vec::new();
    }
    let frames = interleaved.len() / channels;
    let mut planar = vec![Vec::with_capacity(frames); channels];
    for frame in 0..frames {
        for channel in 0..channels {
            planar[channel].push(interleaved[frame * channels + channel]);
        }
    }
    planar
}

/// Downmix or upmix an interleaved buffer to a new channel count.
///
/// * mono → N: the mono signal is copied to every channel at unity gain.
/// * N → 1: average of all channels (equal-power is wrong for a mono fold-down
///   of already-correlated content).
/// * stereo → N and N → 2: ITU-R BS.775-style coefficients applied to the
///   first two channels; remaining channels are mixed into the nearest side.
pub fn remap_channels(input: &[f32], from: usize, to: usize) -> Result<Vec<f32>, AudioError> {
    if from == 0 || to == 0 {
        return Err(AudioError::InvalidParameters(
            "channel count must be greater than zero".into(),
        ));
    }
    if input.len() % from != 0 {
        return Err(AudioError::InvalidParameters(
            "interleaved buffer length is not a multiple of the channel count".into(),
        ));
    }
    if from == to {
        return Ok(input.to_vec());
    }

    let frames = input.len() / from;
    let mut out = vec![0.0; frames * to];

    match (from, to) {
        (1, _) => {
            for frame in 0..frames {
                let sample = input[frame];
                for channel in 0..to {
                    out[frame * to + channel] = sample;
                }
            }
        }
        (_, 1) => {
            let scale = 1.0 / from as f32;
            for frame in 0..frames {
                let sum: f32 = (0..from).map(|channel| input[frame * from + channel]).sum();
                out[frame] = sum * scale;
            }
        }
        _ => {
            for frame in 0..frames {
                let left = input[frame * from];
                let right = input[frame * from + 1];
                for channel in 0..to {
                    out[frame * to + channel] = if channel % 2 == 0 { left } else { right };
                }
                // Fold any additional source channels into the pair.
                for extra in 2..from {
                    let sample = input[frame * from + extra] * 0.5;
                    out[frame * to + extra % to] += sample;
                }
            }
        }
    }

    Ok(out)
}

/// DSD rate families supported by the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DsdRate {
    Dsd64,
    Dsd128,
    Dsd256,
    Dsd512,
}

impl DsdRate {
    /// Native 1-bit rate in Hz.
    #[must_use]
    pub const fn bit_rate(self) -> u32 {
        match self {
            Self::Dsd64 => 2_822_400,
            Self::Dsd128 => 5_644_800,
            Self::Dsd256 => 11_289_600,
            Self::Dsd512 => 22_579_200,
        }
    }

    /// PCM carrier rate used by DSD-over-PCM (bit rate / 16).
    #[must_use]
    pub const fn dop_rate(self) -> u32 {
        self.bit_rate() / 16
    }

    /// Human label for the quality badge.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Dsd64 => "DSD64",
            Self::Dsd128 => "DSD128",
            Self::Dsd256 => "DSD256",
            Self::Dsd512 => "DSD512",
        }
    }

    /// Detect from the native 1-bit rate.
    #[must_use]
    pub const fn from_bit_rate(rate: u32) -> Option<Self> {
        match rate {
            2_822_400 => Some(Self::Dsd64),
            5_644_800 => Some(Self::Dsd128),
            11_289_600 => Some(Self::Dsd256),
            22_579_200 => Some(Self::Dsd512),
            _ => None,
        }
    }
}

/// DoP framing markers (alternate every 24-bit carrier frame).
pub const DOP_MARKER_EVEN: u8 = 0x05;
pub const DOP_MARKER_ODD: u8 = 0xFA;

/// Packs DSD bitstreams into 24-bit DoP carrier frames.
///
/// Layout of one carrier frame for one channel:
///
/// ```text
///   bits 23..8 : 16 consecutive DSD bits (MSB first)
///   bits  7..0 : marker, 0x05 on even frames / 0xFA on odd frames
/// ```
///
/// The encoder is shared by all channels: the marker alternates per *frame*,
/// not per channel, which is what downstream DoP decoders expect.
#[derive(Debug, Clone)]
pub struct DopEncoder {
    channels: usize,
    frame_parity: bool,
    /// Kept for diagnostics and the quality badge.
    pub dsd_rate: DsdRate,
}

impl DopEncoder {
    #[must_use]
    pub const fn new(channels: usize, dsd_rate: DsdRate) -> Self {
        Self {
            channels,
            frame_parity: false,
            dsd_rate,
        }
    }

    /// Reset the marker phase (must be re-aligned after a seek).
    pub fn reset(&mut self) {
        self.frame_parity = false;
    }

    /// Current marker value (exposed for tests and diagnostics).
    #[must_use]
    pub const fn marker(&self) -> u8 {
        if self.frame_parity {
            DOP_MARKER_ODD
        } else {
            DOP_MARKER_EVEN
        }
    }

    /// Pack one DSD byte per channel into one carrier frame.
    ///
    /// `dsd_bytes[channel]` holds 8 DSD bits for that channel; two
    /// consecutive calls to [`Self::push_bits`] form a complete 16-bit payload.
    /// Returns `channels` 24-bit words, one per channel.
    pub fn push_bits(&mut self, channel_bits: &[u8]) -> Result<Vec<u32>, AudioError> {
        if channel_bits.len() != self.channels {
            return Err(AudioError::InvalidParameters(format!(
                "DoP expects {} channel bytes per step, got {}",
                self.channels,
                channel_bits.len()
            )));
        }

        let marker = u32::from(self.marker());
        let words = channel_bits
            .iter()
            .map(|&bits| (u32::from(bits) << 8) | marker)
            .collect();

        self.frame_parity = !self.frame_parity;
        Ok(words)
    }

    /// Encode one carrier frame from per-channel 16-bit payloads.
    ///
    /// This is the hot path used by the DSD reader: it writes into a caller
    /// buffer, allocates nothing and advances the marker exactly once, which
    /// keeps the 0x05/0xFA alternation in lock-step with the frame index.
    pub fn encode_frame(&mut self, payloads: &[u16], out: &mut Vec<u32>) -> Result<(), AudioError> {
        if payloads.len() != self.channels {
            return Err(AudioError::InvalidParameters(format!(
                "DoP expects {} payloads per frame, got {}",
                self.channels,
                payloads.len()
            )));
        }
        let marker = u32::from(self.marker());
        out.clear();
        out.extend(
            payloads
                .iter()
                .map(|&payload| (u32::from(payload) << 8) | marker),
        );
        self.frame_parity = !self.frame_parity;
        Ok(())
    }

    /// Pack already-aligned 16-bit DSD payloads into carrier words.
    #[must_use]
    pub fn pack_payload(&self, payload: u16) -> u32 {
        (u32::from(payload) << 8) | u32::from(self.marker())
    }

    /// Decode a carrier word back into its 16 DSD bits (test helper).
    #[must_use]
    pub fn unpack_payload(word: u32) -> (u16, u8) {
        (((word >> 8) & 0xFFFF) as u16, (word & 0xFF) as u8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_out_of_range_formats() {
        assert!(AudioFormat::new(7_000, 2, SampleFormat::S16).is_err());
        assert!(AudioFormat::new(48_000, 0, SampleFormat::S16).is_err());
        assert!(AudioFormat::new(48_000, 17, SampleFormat::S16).is_err());
        assert!(AudioFormat::new(768_000, 8, SampleFormat::S24).is_ok());
    }

    #[test]
    fn frame_and_byte_rate_are_consistent() {
        let format = AudioFormat::new(96_000, 2, SampleFormat::S24).expect("format");
        assert_eq!(format.frame_size(), 6);
        assert_eq!(format.byte_rate(), 576_000);
        assert_eq!(format.ms_to_frames(1_000), 96_000);
        assert_eq!(format.frames_to_ms(96_000), 1_000);
    }

    #[test]
    fn integer_round_trip_is_exact_for_16_and_24_bit() {
        for format in [SampleFormat::S16, SampleFormat::S24, SampleFormat::S24In32] {
            let original: Vec<f32> = vec![0.0, 0.5, -0.5, 0.999_969_5, -0.999_969_5];
            let mut bytes = Vec::new();
            write_samples_f32(&original, format, &mut bytes).expect("write");
            let decoded = read_samples_f32(&bytes, format).expect("read");

            for (a, b) in original.iter().zip(decoded.iter()) {
                assert!((a - b).abs() <= 1.0 / 8_388_608.0, "{format:?}: {a} != {b}");
            }
        }
    }

    #[test]
    fn float_formats_are_lossless() {
        let original: Vec<f32> = vec![0.123_456_79, -0.987_654_3, 1.0, -1.0];
        let mut bytes = Vec::new();
        write_samples_f32(&original, SampleFormat::F32, &mut bytes).expect("write");
        let decoded = read_samples_f32(&bytes, SampleFormat::F32).expect("read");
        assert_eq!(original, decoded);
    }

    #[test]
    fn conversion_saturates_instead_of_wrapping() {
        let samples = vec![2.0, -2.0];
        let mut bytes = Vec::new();
        write_samples_f32(&samples, SampleFormat::S16, &mut bytes).expect("write");
        let decoded = read_samples_f32(&bytes, SampleFormat::S16).expect("read");
        assert!(decoded[0] > 0.999);
        assert!(decoded[0] < 1.0);
        assert!(decoded[1] <= -0.999);
        // -1.0 is exactly representable as i16::MIN and round-trips unchanged;
        // only the positive side loses one step to the asymmetric range.
        assert_eq!(decoded[1], -1.0);
    }

    #[test]
    fn planar_and_interleaved_are_inverses() {
        let planar = vec![vec![1.0, 2.0, 3.0], vec![-1.0, -2.0, -3.0]];
        let interleaved = interleave(&planar, 3, 2);
        assert_eq!(interleaved, vec![1.0, -1.0, 2.0, -2.0, 3.0, -3.0]);
        assert_eq!(deinterleave(&interleaved, 2), planar);
    }

    #[test]
    fn channel_remap_handles_mono_and_fold_down() {
        let mono = vec![0.5, -0.5];
        let stereo = remap_channels(&mono, 1, 2).expect("mono to stereo");
        assert_eq!(stereo, vec![0.5, 0.5, -0.5, -0.5]);

        let stereo = vec![0.5, 0.25, -0.5, -0.25];
        let downmix = remap_channels(&stereo, 2, 1).expect("stereo to mono");
        assert_eq!(downmix, vec![0.375, -0.375]);
    }

    #[test]
    fn dop_marker_alternates_and_payload_round_trips() {
        let mut encoder = DopEncoder::new(2, DsdRate::Dsd64);
        assert_eq!(encoder.marker(), DOP_MARKER_EVEN);

        let first = encoder
            .push_bits(&[0b1010_1010, 0b0101_0101])
            .expect("pack");
        assert_eq!(first.len(), 2);
        let (_, marker) = DopEncoder::unpack_payload(first[0]);
        assert_eq!(marker, DOP_MARKER_EVEN);

        let second = encoder.push_bits(&[0xFF, 0x00]).expect("pack");
        let (payload, marker) = DopEncoder::unpack_payload(second[1]);
        assert_eq!(marker, DOP_MARKER_ODD);
        assert_eq!(payload, 0x0000);

        encoder.reset();
        assert_eq!(encoder.marker(), DOP_MARKER_EVEN);
    }

    #[test]
    fn dsds_rates_map_to_dop_carriers() {
        assert_eq!(DsdRate::Dsd64.dop_rate(), 176_400);
        assert_eq!(DsdRate::Dsd128.dop_rate(), 352_800);
        assert_eq!(DsdRate::Dsd256.dop_rate(), 705_600);
        assert_eq!(DsdRate::Dsd512.dop_rate(), 1_411_200);
        assert_eq!(DsdRate::from_bit_rate(11_289_600), Some(DsdRate::Dsd256));
        assert_eq!(DsdRate::from_bit_rate(44_100), None);
    }

    #[test]
    fn quality_label_formats_rates() {
        let format = AudioFormat::new(96_000, 2, SampleFormat::S24).expect("format");
        assert_eq!(format.quality_label("FLAC"), "24-bit / 96 kHz FLAC");
        let format = AudioFormat::new(44_100, 2, SampleFormat::S16).expect("format");
        assert_eq!(format.quality_label("ALAC"), "16-bit / 44.1 kHz ALAC");
    }
}
