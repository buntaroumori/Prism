//! Deterministic, accessibility-checked colour extraction for the interface.
//!
//! The ambient canvas and every accent surface in Prism are tinted from the
//! artwork that is currently on screen. Two properties matter more than
//! "pretty":
//!
//! 1. **Legibility.** A washed-out sleeve must not produce an accent that is
//!    invisible on OLED black, so every accent is pushed through a tone map and
//!    then checked against the WCAG 2.1 contrast ratio until it clears 4.5:1.
//! 2. **Stability.** The same cover must always produce byte-identical output,
//!    or the shell would flash a different colour every time a track is
//!    re-opened. Quantisation is deterministic (`color_thief` is a fixed-point
//!    median cut) and the seeded fallback is a pure function of the cache key.
//!
//! Extraction runs on the blocking pool: decoding a 1000 px JPEG and running
//! median cut takes single-digit milliseconds, which is fine off the audio
//! thread and far too slow for the render thread.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;

use color_thief::{ColorFormat, get_palette};
use palette::color_difference::Wcag21RelativeContrast;
use palette::{Hsv, IntoColor, Srgb};
use serde::Serialize;

use crate::error::PrismError;

/// Longest edge used for quantisation. Median cut does not benefit from more
/// pixels — 128² is ~16k samples, decades more than the 5-bit histogram needs.
const SAMPLE_EDGE: u32 = 128;
/// Swatches kept in the answer (the two best are used for accents).
const MAX_SWATCHES: usize = 6;
/// Pixels skipped between samples. 5 keeps the histogram stable on flat art
/// while making the common case ~10× cheaper than a full scan.
const SAMPLE_QUALITY: u8 = 5;
/// Minimum WCAG ratio for the accent over the generated background.
const MIN_ACCENT_CONTRAST: f32 = 4.5;
/// Bound on the in-memory palette cache (one entry per distinct cover).
const CACHE_CAPACITY: usize = 512;

/// Where a palette came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PaletteSource {
    /// Median cut over a decoded cover image.
    Cover,
    /// No cover on disk: a hue derived from the cache key.
    Derived,
}

/// One quantised swatch of the artwork, in display order.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Swatch {
    /// `#rrggbb`.
    pub hex: String,
    /// Share of sampled pixels, `0.0..=1.0`.
    pub population: f32,
    /// Perceived lightness (`0.0..=1.0`, CIE L*).
    pub lightness: f32,
}

/// Tone-mapped palette consumed by the ambient canvas and the accent tokens.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverPalette {
    /// Hash of the cover this palette was extracted from (or derived from).
    pub key: String,
    pub source: PaletteSource,
    /// Deep, slightly hue-tinted page background (`#rrggbb`).
    pub background: String,
    /// One step above the background: cards and the player bar.
    pub surface: String,
    /// Primary interactive accent, guaranteed readable on `background`.
    pub accent: String,
    /// Lighter tint of the accent for text and hover states.
    pub accent_soft: String,
    /// Secondary hue for the ambient bloom.
    pub highlight: String,
    /// Third hue for the ambient bloom; falls back to a rotation of `accent`.
    pub highlight_alt: String,
    /// WCAG contrast ratio of `accent` over `background` (always ≥ 4.5).
    pub contrast: f32,
    /// Mean lightness of the artwork: the canvas uses it to decide how much
    /// bloom the cover can carry without washing out the interface.
    pub average_lightness: f32,
    pub swatches: Vec<Swatch>,
}

impl CoverPalette {
    /// Neutral fallback used before the first track loads, or when the palette
    /// service is unavailable. Identical to the static FASE 1 accent so the
    /// shell does not visibly change when the real palette arrives.
    #[must_use]
    pub fn neutral() -> Self {
        let accent = Hsv::new(233.0, 0.52, 0.94);
        Self::build(
            "0".repeat(64),
            PaletteSource::Derived,
            accent,
            accent,
            accent,
            0.0,
            Vec::new(),
        )
    }

    /// Build a palette from the three most useful hues.
    fn build(
        key: String,
        source: PaletteSource,
        accent: Hsv,
        highlight: Hsv,
        alt: Hsv,
        average_lightness: f32,
        swatches: Vec<Swatch>,
    ) -> Self {
        let background = rgb_hex(&Hsv::new(accent.hue, accent.saturation * 0.62, 0.075));
        let surface = rgb_hex(&Hsv::new(accent.hue, accent.saturation * 0.52, 0.125));

        // Tone map the accent for a dark surface: fully saturated colours read
        // as neon, and very bright ones vibrate against black.
        let mut accent_rgb = Hsv::new(
            accent.hue,
            accent.saturation.clamp(0.46, 0.92),
            accent.value.clamp(0.66, 0.88),
        );
        let background_srgb = srgb_from_hex(&background);
        let mut contrast = relative_contrast(accent_rgb, background_srgb);
        // Lighten in perceptual steps until the accent is readable. Bounded to
        // 24 iterations by the value ceiling, so this always terminates.
        let mut value = accent_rgb.value;
        while contrast < MIN_ACCENT_CONTRAST && value < 0.98 {
            value = (value + 0.02).min(0.98);
            accent_rgb = Hsv::new(accent_rgb.hue, accent_rgb.saturation, value);
            contrast = relative_contrast(accent_rgb, background_srgb);
        }

        let soft = Hsv::new(
            accent_rgb.hue,
            (accent_rgb.saturation * 0.72).max(0.22),
            (accent_rgb.value + 0.10).min(0.96),
        );

        Self {
            key,
            source,
            background,
            surface,
            accent: rgb_hex(&accent_rgb),
            accent_soft: rgb_hex(&soft),
            highlight: rgb_hex(&tone(highlight, 0.60, 0.80)),
            highlight_alt: rgb_hex(&tone(alt, 0.55, 0.78)),
            contrast: round2(contrast),
            average_lightness: round2(average_lightness),
            swatches,
        }
    }
}

/// Cache front for [`extract`] plus disk access to the artwork directory.
#[derive(Debug)]
pub struct PaletteService {
    directory: PathBuf,
    cache: parking_lot::Mutex<Cache>,
}

#[derive(Debug, Default)]
struct Cache {
    entries: std::collections::HashMap<String, Arc<CoverPalette>>,
    order: VecDeque<String>,
}

impl Cache {
    fn get(&self, key: &str) -> Option<Arc<CoverPalette>> {
        self.entries.get(key).cloned()
    }

    fn insert(&mut self, key: String, palette: Arc<CoverPalette>) {
        if self.entries.insert(key.clone(), palette).is_none() {
            self.order.push_back(key);
        }
        while self.order.len() > CACHE_CAPACITY {
            if let Some(oldest) = self.order.pop_front() {
                self.entries.remove(&oldest);
            }
        }
    }
}

impl PaletteService {
    #[must_use]
    pub fn new(directory: PathBuf) -> Self {
        Self {
            directory,
            cache: parking_lot::Mutex::new(Cache::default()),
        }
    }

    /// Palette for a cached cover key.
    ///
    /// `key` must be the 64-character hex digest of the `cover_cache` row; it is
    /// validated before it is used as a file name, so a caller can never point
    /// the decoder at an arbitrary path.
    pub async fn palette(&self, key: &str) -> Result<CoverPalette, PrismError> {
        let key = validate_key(key)?;
        if let Some(hit) = self.cache.lock().get(&key) {
            return Ok((*hit).clone());
        }

        let path = self.directory.join(format!("{key}.jpg"));
        let worker_key = key.clone();
        let palette = tokio::task::spawn_blocking(move || match std::fs::read(&path) {
            Ok(bytes) => extract(&worker_key, &bytes).unwrap_or_else(|| {
                tracing::debug!("cover has no usable chroma, deriving palette");
                derive(&worker_key)
            }),
            Err(_) => derive(&worker_key),
        })
        .await
        .map_err(|error| PrismError::Internal(format!("palette task panicked: {error}")))?;

        let palette = Arc::new(palette);
        self.cache.lock().insert(key, Arc::clone(&palette));
        Ok((*palette).clone())
    }

    /// Drop every cached palette (used when covers are invalidated).
    pub fn clear(&self) {
        let mut cache = self.cache.lock();
        cache.entries.clear();
        cache.order.clear();
    }
}

/// `cover_cache.hash` is always 64 lowercase hex characters; anything else is a
/// programming error, not user input, but it is still rejected.
fn validate_key(key: &str) -> Result<String, PrismError> {
    let normalised = key.trim().to_ascii_lowercase();
    if normalised.len() != 64 || !normalised.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(PrismError::invalid(
            "a cover key is 64 hexadecimal characters",
        ));
    }
    Ok(normalised)
}

/// Extract a palette from encoded image bytes, or `None` when the image has no
/// usable chromatic content (black-and-white sleeves, line art).
fn extract(key: &str, bytes: &[u8]) -> Option<CoverPalette> {
    let image = image::load_from_memory(bytes).ok()?;
    let sample = image.thumbnail(SAMPLE_EDGE, SAMPLE_EDGE).to_rgb8();
    let (width, height) = sample.dimensions();
    if width == 0 || height == 0 {
        return None;
    }

    let colours = get_palette(
        sample.as_raw(),
        ColorFormat::Rgb,
        SAMPLE_QUALITY,
        u8::try_from(MAX_SWATCHES).unwrap_or(6),
    )
    .ok()?;

    let total = sample.pixels().count().max(1) as f32;
    let mut swatches: Vec<Swatch> = Vec::with_capacity(colours.len());
    let mut candidates: Vec<Candidate> = Vec::with_capacity(colours.len());
    for colour in colours {
        let rgb = (colour.r, colour.g, colour.b);
        let population = 0.0_f32; // filled below from the histogram pass
        let swatch = to_swatch(rgb, population);
        candidates.push(Candidate {
            hsv: hsv(rgb),
            population,
            lightness: swatch.lightness,
            hex: swatch.hex.clone(),
        });
        swatches.push(swatch);
    }

    // Population: re-count the sampled pixels against the quantised swatches.
    // `color_thief` returns colours but not their exact share, and the share is
    // what decides which swatch is "dominant".
    let mut counts = vec![0_u32; candidates.len()];
    for pixel in sample.pixels() {
        let rgb = (pixel.0[0], pixel.0[1], pixel.0[2]);
        if let Some(index) = nearest(&rgb, &candidates) {
            counts[index] = counts[index].saturating_add(1);
        }
    }
    for (index, candidate) in candidates.iter_mut().enumerate() {
        let share = counts[index] as f32 / total;
        candidate.population = share;
        swatches[index].population = round2(share);
    }

    let average_lightness = mean_lightness(&sample);

    // Rank: a swatch is a good accent when it is colourful and mid-light. Dark,
    // grey and blown-out swatches make poor accents on an OLED page.
    let mut ranked: Vec<Candidate> = candidates
        .iter()
        .filter(|candidate| {
            candidate.hsv.saturation >= 0.16 && (0.10..=0.95).contains(&candidate.lightness)
        })
        .cloned()
        .collect();

    if ranked.is_empty() {
        return None;
    }

    ranked.sort_by(|a, b| {
        score(b)
            .partial_cmp(&score(a))
            .unwrap_or(std::cmp::Ordering::Equal)
            // Deterministic tie-break: the palette must be stable across runs.
            .then_with(|| a.hex.cmp(&b.hex))
    });

    let accent = ranked[0].hsv;
    let highlight = ranked
        .iter()
        .skip(1)
        .map(|candidate| candidate.hsv)
        .find(|hsv| {
            hue_distance(
                hsv.hue.into_positive_degrees(),
                accent.hue.into_positive_degrees(),
            ) > 28.0
        })
        .unwrap_or_else(|| {
            Hsv::new(
                accent.hue.into_positive_degrees() + 46.0,
                accent.saturation * 0.9,
                accent.value,
            )
        });
    let alt = ranked
        .iter()
        .skip(1)
        .map(|candidate| candidate.hsv)
        .find(|hsv| {
            hue_distance(
                hsv.hue.into_positive_degrees(),
                accent.hue.into_positive_degrees(),
            ) > 28.0
                && hue_distance(
                    hsv.hue.into_positive_degrees(),
                    highlight.hue.into_positive_degrees(),
                ) > 24.0
        })
        .unwrap_or_else(|| {
            Hsv::new(
                accent.hue.into_positive_degrees() + 312.0,
                accent.saturation * 0.8,
                accent.value,
            )
        });

    swatches.sort_by(|a, b| {
        b.population
            .partial_cmp(&a.population)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    swatches.truncate(MAX_SWATCHES);

    Some(CoverPalette::build(
        key.to_string(),
        PaletteSource::Cover,
        accent,
        highlight,
        alt,
        average_lightness,
        swatches,
    ))
}

#[derive(Debug, Clone)]
struct Candidate {
    hsv: Hsv,
    population: f32,
    lightness: f32,
    hex: String,
}

/// Accent score: colourfulness × mid-lightness × prominence.
fn score(candidate: &Candidate) -> f32 {
    let saturation = candidate.hsv.saturation;
    // Bell around L* ≈ 0.55: mid-tone swatches carry the most usable chroma.
    let bell = (-((candidate.lightness - 0.55) / 0.28).powi(2)).exp();
    (saturation.powf(1.15) * bell * (0.35 + candidate.population).sqrt()).max(0.0)
}

/// Deterministic palette for artwork that has no usable colour (or no file).
///
/// The hue is derived from the cache key, so the same album always gets the same
/// colour, even across restarts, and two different covers almost never collide.
fn derive(key: &str) -> CoverPalette {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in key.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
    }
    let hue = (hash % 360) as f32;
    let saturation = 0.44 + ((hash >> 8) % 24) as f32 / 100.0;
    let value = 0.72 + ((hash >> 16) % 16) as f32 / 100.0;

    let accent = Hsv::new(hue, saturation, value);
    let highlight = Hsv::new(hue + 48.0, saturation * 0.9, value * 0.96);
    let alt = Hsv::new(hue + 312.0, saturation * 0.8, value);
    CoverPalette::build(
        key.to_string(),
        PaletteSource::Derived,
        accent,
        highlight,
        alt,
        0.5,
        Vec::new(),
    )
}

fn hsv((r, g, b): (u8, u8, u8)) -> Hsv {
    let srgb = Srgb::new(
        f32::from(r) / 255.0,
        f32::from(g) / 255.0,
        f32::from(b) / 255.0,
    );
    srgb.into_color()
}

fn to_swatch(rgb: (u8, u8, u8), population: f32) -> Swatch {
    let colour = hsv(rgb);
    Swatch {
        hex: format!("#{:02x}{:02x}{:02x}", rgb.0, rgb.1, rgb.2),
        population,
        lightness: round2(colour.value * (1.0 - colour.saturation * 0.5)),
    }
}

/// Nearest swatch in RGB space (squared distance); used for the population pass.
fn nearest(rgb: &(u8, u8, u8), candidates: &[Candidate]) -> Option<usize> {
    let (r, g, b) = (i32::from(rgb.0), i32::from(rgb.1), i32::from(rgb.2));
    candidates
        .iter()
        .enumerate()
        .map(|(index, candidate)| {
            let parsed =
                u32::from_str_radix(candidate.hex.trim_start_matches('#'), 16).unwrap_or(0);
            let cr = ((parsed >> 16) & 0xFF) as i32;
            let cg = ((parsed >> 8) & 0xFF) as i32;
            let cb = (parsed & 0xFF) as i32;
            let distance = (r - cr).pow(2) + (g - cg).pow(2) + (b - cb).pow(2);
            (index, distance)
        })
        .min_by_key(|(_, distance)| *distance)
        .map(|(index, _)| index)
}

fn mean_lightness(image: &image::RgbImage) -> f32 {
    // Rec. 709 luma, gamma-encoded: cheap and close enough to perceived
    // lightness for deciding bloom intensity.
    let mut sum = 0.0_f64;
    let mut count = 0_u64;
    for pixel in image.pixels() {
        let [r, g, b] = pixel.0;
        sum += 0.2126 * f64::from(r) + 0.7152 * f64::from(g) + 0.0722 * f64::from(b);
        count += 1;
    }
    if count == 0 {
        return 0.5;
    }
    round2((sum / count as f64 / 255.0) as f32)
}

/// Nudge a hue towards a usable accent: clamp saturation and value.
fn tone(hsv: Hsv, saturation_ceiling: f32, value_ceiling: f32) -> Hsv {
    Hsv::new(
        hsv.hue,
        hsv.saturation.clamp(0.40, saturation_ceiling.max(0.40)),
        hsv.value.clamp(0.55, value_ceiling.max(0.55)),
    )
}

fn relative_contrast(foreground: Hsv, background: Srgb) -> f32 {
    let foreground: Srgb = foreground.into_color();
    let foreground = Srgb::new(
        clamp01(foreground.red),
        clamp01(foreground.green),
        clamp01(foreground.blue),
    );
    foreground.relative_contrast(background)
}

fn srgb_from_hex(hex: &str) -> Srgb {
    let value = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap_or(0);
    Srgb::new(
        ((value >> 16) & 0xFF) as f32 / 255.0,
        ((value >> 8) & 0xFF) as f32 / 255.0,
        (value & 0xFF) as f32 / 255.0,
    )
}

fn rgb_hex(hsv: &Hsv) -> String {
    let srgb: Srgb = (*hsv).into_color();
    let channel = |value: f32| (clamp01(value) * 255.0).round().clamp(0.0, 255.0) as u8;
    format!(
        "#{:02x}{:02x}{:02x}",
        channel(srgb.red),
        channel(srgb.green),
        channel(srgb.blue)
    )
}

fn clamp01(value: f32) -> f32 {
    if value.is_nan() {
        0.0
    } else {
        value.clamp(0.0, 1.0)
    }
}

fn round2(value: f32) -> f32 {
    (value * 100.0).round() / 100.0
}

/// Shortest distance between two hues, in degrees.
fn hue_distance(a: f32, b: f32) -> f32 {
    let delta = (a - b).abs() % 360.0;
    if delta > 180.0 { 360.0 - delta } else { delta }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(width: u32, height: u32, rgb: [u8; 3]) -> Vec<u8> {
        let mut buffer = Vec::with_capacity((width * height * 3) as usize);
        for _ in 0..(width * height) {
            buffer.extend_from_slice(&rgb);
        }
        let image = image::RgbImage::from_raw(width, height, buffer).expect("image");
        let mut encoded = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(image)
            .write_to(&mut encoded, image::ImageFormat::Png)
            .expect("encode");
        encoded.into_inner()
    }

    fn gradient(width: u32, height: u32) -> Vec<u8> {
        let mut buffer = Vec::with_capacity((width * height * 3) as usize);
        for y in 0..height {
            for x in 0..width {
                buffer.push((x * 255 / width.max(1)) as u8);
                buffer.push(40);
                buffer.push((y * 255 / height.max(1)) as u8);
            }
        }
        let image = image::RgbImage::from_raw(width, height, buffer).expect("image");
        let mut encoded = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(image)
            .write_to(&mut encoded, image::ImageFormat::Png)
            .expect("encode");
        encoded.into_inner()
    }

    fn channel(hex: &str, shift: u32) -> u8 {
        let value = u32::from_str_radix(hex.trim_start_matches('#'), 16).expect("hex");
        ((value >> shift) & 0xFF) as u8
    }

    #[test]
    fn a_saturated_cover_yields_a_usable_accent() {
        let bytes = solid(64, 64, [204, 32, 48]);
        let palette = extract(&"a".repeat(64), &bytes).expect("palette");

        assert_eq!(palette.source, PaletteSource::Cover);
        assert!(palette.contrast >= MIN_ACCENT_CONTRAST);
        // The hue survives: the red channel dominates the accent.
        let (r, g, b) = (
            channel(&palette.accent, 16),
            channel(&palette.accent, 8),
            channel(&palette.accent, 0),
        );
        assert!(r > g && r > b, "accent {r},{g},{b} is not red-dominant");
        assert!(palette.swatches[0].population > 0.9);
    }

    #[test]
    fn the_background_is_much_darker_than_the_accent() {
        let bytes = gradient(96, 96);
        let palette = extract(&"b".repeat(64), &bytes).expect("palette");
        let background = luminance(&palette.background);
        let accent = luminance(&palette.accent);
        assert!(
            accent > background * 3.0,
            "accent {accent} is not clearly brighter than background {background}"
        );
        assert!(palette.contrast >= MIN_ACCENT_CONTRAST);
    }

    #[test]
    fn greyscale_artwork_has_no_palette_and_falls_back() {
        let bytes = solid(32, 32, [128, 128, 128]);
        assert!(extract(&"c".repeat(64), &bytes).is_none());

        let derived = derive(&"c".repeat(64));
        assert_eq!(derived.source, PaletteSource::Derived);
        assert!(derived.contrast >= MIN_ACCENT_CONTRAST);
        assert!(derived.swatches.is_empty());
    }

    #[test]
    fn derived_palettes_are_deterministic_and_distinct() {
        let first = derive(&"d".repeat(64));
        let again = derive(&"d".repeat(64));
        let other = derive(&"e".repeat(64));
        assert_eq!(first, again);
        assert_ne!(first.accent, other.accent);
    }

    #[test]
    fn every_generated_colour_is_a_valid_hex_triplet() {
        let palette = derive(&"f".repeat(64));
        for hex in [
            &palette.background,
            &palette.surface,
            &palette.accent,
            &palette.accent_soft,
            &palette.highlight,
            &palette.highlight_alt,
        ] {
            assert_eq!(hex.len(), 7, "{hex} is not a hex triplet");
            assert!(hex.starts_with('#'));
            assert!(hex[1..].bytes().all(|byte| byte.is_ascii_hexdigit()));
        }
    }

    #[test]
    fn the_neutral_palette_matches_the_static_shell_accent() {
        let neutral = CoverPalette::neutral();
        assert_eq!(neutral.source, PaletteSource::Derived);
        assert!(neutral.contrast >= MIN_ACCENT_CONTRAST);
        assert_eq!(neutral.key.len(), 64);
    }

    #[test]
    fn an_invalid_key_is_rejected_before_touching_the_disk() {
        assert!(validate_key("../../etc/passwd").is_err());
        assert!(validate_key("ABC").is_err());
        assert!(validate_key(&"g".repeat(64)).is_err());
        assert!(validate_key(&"A".repeat(64)).is_ok());
    }

    fn luminance(hex: &str) -> f32 {
        let colour = srgb_from_hex(hex);
        0.2126 * colour.red + 0.7152 * colour.green + 0.0722 * colour.blue
    }
}
