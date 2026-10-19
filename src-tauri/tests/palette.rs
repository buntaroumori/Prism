//! Integration tests for artwork palette extraction.
//!
//! These drive the production `PaletteService` end to end: a real JPEG is
//! encoded, written to the artwork cache directory, decoded on the blocking
//! pool, quantised, tone-mapped and cached — the same path a track change takes
//! in the application.

use std::path::Path;

use prism_lib::palette::{CoverPalette, PaletteService, PaletteSource};
use tempfile::TempDir;

/// Write a JPEG of `size`×`size` filled by `shade(x, y) -> [u8; 3]`.
fn write_jpeg(path: &Path, size: u32, shade: impl Fn(u32, u32) -> [u8; 3]) {
    let mut buffer = Vec::with_capacity((size * size * 3) as usize);
    for y in 0..size {
        for x in 0..size {
            buffer.extend_from_slice(&shade(x, y));
        }
    }
    let image = image::RgbImage::from_raw(size, size, buffer).expect("image buffer");
    let file = std::fs::File::create(path).expect("create jpeg");
    let mut writer = std::io::BufWriter::new(file);
    image::DynamicImage::ImageRgb8(image)
        .write_to(&mut writer, image::ImageFormat::Jpeg)
        .expect("encode jpeg");
}

fn channel(hex: &str, shift: u32) -> u8 {
    let value = u32::from_str_radix(hex.trim_start_matches('#'), 16).expect("hex");
    ((value >> shift) & 0xFF) as u8
}

const TABLE: [[u8; 3]; 8] = [
    [0, 0, 0],
    [255, 255, 255],
    [128, 128, 128],
    [180, 40, 60],
    [40, 120, 200],
    [200, 170, 30],
    [60, 160, 90],
    [140, 80, 190],
];

#[tokio::test]
async fn a_warm_cover_becomes_a_readable_palette() {
    let temp = TempDir::new().expect("tempdir");
    let directory = temp.path().to_path_buf();
    let key = "a".repeat(64);
    write_jpeg(&directory.join(format!("{key}.jpg")), 96, |x, y| {
        // A sunburst: a burnt-orange disc on a near-black field.
        let centre = 48.0_f32;
        let distance = (((x as f32) - centre).powi(2) + ((y as f32) - centre).powi(2)).sqrt();
        if distance < 34.0 {
            [205, 98, 42]
        } else {
            [12, 10, 12]
        }
    });

    let service = PaletteService::new(directory);
    let palette = service.palette(&key).await.expect("palette");

    assert_eq!(palette.source, PaletteSource::Cover);
    assert_eq!(palette.key, key);
    assert!(
        palette.contrast >= 4.5,
        "accent must be readable on the page"
    );
    assert!(
        palette.swatches.len() >= 2,
        "a two-tone cover yields two swatches"
    );

    // The accent keeps the artwork's warmth rather than collapsing to grey.
    let accent = (
        channel(&palette.accent, 16),
        channel(&palette.accent, 8),
        channel(&palette.accent, 0),
    );
    assert!(
        accent.0 > accent.2,
        "accent {accent:?} lost the red dominance of the cover"
    );

    // The background must stay dark: this is an OLED player, not a website.
    let background = (
        channel(&palette.background, 16),
        channel(&palette.background, 8),
        channel(&palette.background, 0),
    );
    assert!(
        background.0.max(background.1).max(background.2) < 40,
        "background {background:?} is too bright"
    );
}

#[tokio::test]
async fn a_missing_cover_derives_a_stable_palette() {
    let temp = TempDir::new().expect("tempdir");
    let service = PaletteService::new(temp.path().to_path_buf());
    let key = "b".repeat(64);

    let first = service.palette(&key).await.expect("palette");
    let second = service.palette(&key).await.expect("palette");

    assert_eq!(first.source, PaletteSource::Derived);
    assert_eq!(first, second, "the derived palette must be deterministic");
    assert!(first.contrast >= 4.5);
    assert!(first.swatches.is_empty());

    // A different key must not inherit the same hue.
    let other = service.palette(&"c".repeat(64)).await.expect("palette");
    assert_ne!(first.accent, other.accent);
}

#[tokio::test]
async fn a_greyscale_cover_falls_back_instead_of_producing_mud() {
    let temp = TempDir::new().expect("tempdir");
    let directory = temp.path().to_path_buf();
    let key = "d".repeat(64);
    write_jpeg(&directory.join(format!("{key}.jpg")), 64, |x, _| {
        let value = 40_u8.saturating_add((x as u8).saturating_mul(2));
        [value, value, value]
    });

    let service = PaletteService::new(directory);
    let palette = service.palette(&key).await.expect("palette");
    assert_eq!(
        palette.source,
        PaletteSource::Derived,
        "a grey sleeve has no usable chroma and must use the derived palette"
    );
}

#[tokio::test]
async fn a_corrupt_cover_does_not_fail_the_request() {
    let temp = TempDir::new().expect("tempdir");
    let directory = temp.path().to_path_buf();
    let key = "e".repeat(64);
    std::fs::write(directory.join(format!("{key}.jpg")), b"not a jpeg at all").expect("write");

    let service = PaletteService::new(directory);
    let palette = service.palette(&key).await.expect("palette");
    assert_eq!(palette.source, PaletteSource::Derived);
    assert!(palette.contrast >= 4.5);
}

#[tokio::test]
async fn the_second_lookup_is_served_from_memory() {
    let temp = TempDir::new().expect("tempdir");
    let directory = temp.path().to_path_buf();
    let key = "f".repeat(64);
    write_jpeg(&directory.join(format!("{key}.jpg")), 48, |x, y| {
        TABLE[((x + y) % TABLE.len() as u32) as usize]
    });

    let service = PaletteService::new(directory);
    let first = service.palette(&key).await.expect("palette");
    // Delete the file: the cached entry must still answer.
    std::fs::remove_file(temp.path().join(format!("{key}.jpg"))).expect("remove");
    let second = service.palette(&key).await.expect("palette");
    assert_eq!(first, second);

    service.clear();
    let third = service.palette(&key).await.expect("palette");
    assert_eq!(
        third.source,
        PaletteSource::Derived,
        "after clear() the missing file must fall back to the derived palette"
    );
}

#[tokio::test]
async fn an_unsafe_key_is_rejected() {
    let temp = TempDir::new().expect("tempdir");
    let service = PaletteService::new(temp.path().to_path_buf());
    assert!(service.palette("../../etc/passwd").await.is_err());
    assert!(service.palette(&"z".repeat(64)).await.is_err());
}

#[test]
fn the_neutral_palette_is_always_valid() {
    let neutral: CoverPalette = CoverPalette::neutral();
    assert!(neutral.contrast >= 4.5);
    assert_eq!(neutral.key.len(), 64);
    for hex in [
        &neutral.background,
        &neutral.surface,
        &neutral.accent,
        &neutral.accent_soft,
        &neutral.highlight,
        &neutral.highlight_alt,
    ] {
        assert_eq!(hex.len(), 7);
    }
}
