//! Metadata extraction and library scanning.
//!
//! The scanner is the only writer of the `tracks`, `albums` and `artists`
//! tables. It is split in three layers so each one can be tested alone:
//!
//! * [`tags`] — read one file with `lofty` and normalise every tag into
//!   [`tags::TrackTags`], including the technical properties the UI badges
//!   (bit depth, sample rate, DSD rate) and the ReplayGain 2 values.
//! * [`scanner`] — walk a root with `walkdir`, parse files in parallel with
//!   `rayon`, expand CUE sheets into virtual tracks and hand batches to the
//!   database writer.
//! * [`crate::db`] — the SQL side: deterministic ids, upserts and the FTS5
//!   index that search reads.
//!
//! Errors are per-file: a corrupt FLAC never aborts a scan, it lands in
//! `scan_errors` with a stable code and the walk continues.

pub mod scanner;
pub mod tags;

pub use scanner::{ScanConfig, ScanOutcome, ScanProgress, ScanTrigger, Scanner};
pub use tags::{TrackTags, read};

use std::path::PathBuf;

/// Reasons a single file could not be indexed.
#[derive(Debug, thiserror::Error)]
pub enum MetadataError {
    /// The file could not be opened or read.
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),

    /// The container is not supported by the metadata reader.
    #[error("unsupported format: {0}")]
    Unsupported(String),

    /// The file is readable but its tag block is malformed.
    #[error("malformed metadata: {0}")]
    Malformed(String),

    /// A CUE sheet references an image that does not exist.
    #[error("CUE sheet error: {0}")]
    Cue(String),
}

impl MetadataError {
    /// Stable machine-readable code stored in `scan_errors.error_code`.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Io(_) => "io",
            Self::Unsupported(_) => "unsupported",
            Self::Malformed(_) => "malformed",
            Self::Cue(_) => "cue",
        }
    }
}

/// Audio file extensions the scanner indexes.
///
/// DSD and CUE are handled separately because their metadata source is not the
/// file itself; everything else goes through `lofty`.
pub const AUDIO_EXTENSIONS: &[&str] = &[
    "flac", "alac", "m4a", "mp4", "aac", "mp3", "ogg", "oga", "opus", "wav", "wave", "aif", "aiff",
    "aifc", "wv", "ape", "dsf", "dff", "wma", "mpc", "tta",
];

/// True when the path looks like an audio file the scanner should parse.
#[must_use]
pub fn is_audio_file(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| AUDIO_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// File name of a cover art image the scanner may pick up next to the audio.
#[must_use]
pub fn is_cover_image(path: &std::path::Path) -> bool {
    let Some(name) = path.file_stem().and_then(|stem| stem.to_str()) else {
        return false;
    };
    let supported = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "jpg" | "jpeg" | "png" | "webp" | "bmp"
            )
        });
    supported
        && matches!(
            name.to_ascii_lowercase().as_str(),
            "cover" | "folder" | "front" | "album" | "artwork" | "albumart"
        )
}

/// Technical badge shown in the UI: `FLAC`, `96 kHz / 24 bit`, `DSD64`…
///
/// One implementation shared by the scanner, the search read model and the
/// album grid, so a track never shows a different badge depending on which
/// query returned it.
#[must_use]
pub fn quality_label(
    bit_depth: Option<i64>,
    sample_rate: Option<i64>,
    dsd_rate: Option<i64>,
    format: &str,
) -> String {
    if let Some(dsd) = dsd_rate {
        return match dsd {
            2_822_400 => "DSD64",
            5_644_800 => "DSD128",
            11_289_600 => "DSD256",
            22_579_200 => "DSD512",
            _ => "DSD",
        }
        .to_string();
    }
    match (bit_depth, sample_rate) {
        (Some(bits), Some(rate)) => format!("{} kHz / {} bit", format_rate(rate), bits),
        (None, Some(rate)) => format!("{} kHz", format_rate(rate)),
        _ => {
            if format.is_empty() {
                "—".to_string()
            } else {
                format.to_uppercase()
            }
        }
    }
}

/// `44100` → `44.1`, `96000` → `96`.
pub(crate) fn format_rate(rate: i64) -> String {
    if rate % 1_000 == 0 {
        format!("{}", rate / 1_000)
    } else {
        format!("{:.1}", rate as f64 / 1_000.0)
    }
}

/// Normalise a tag value: trim, collapse whitespace, drop control characters.
#[must_use]
pub fn clean(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut previous_space = true;
    for character in value.chars() {
        if character.is_control() {
            continue;
        }
        if character.is_whitespace() {
            if !previous_space {
                out.push(' ');
                previous_space = true;
            }
        } else {
            out.push(character);
            previous_space = false;
        }
    }
    out.trim().to_string()
}

/// Sort key that ignores leading articles, so "The Beatles" files under B.
#[must_use]
pub fn sort_key(value: &str) -> String {
    let cleaned = clean(value);
    let lower = cleaned.to_lowercase();
    for article in [
        "the ", "a ", "an ", "los ", "las ", "el ", "la ", "le ", "les ",
    ] {
        if let Some(rest) = lower.strip_prefix(article) {
            return rest.to_string();
        }
    }
    lower
}

/// Parse "3", "3/12" or "03" into `(position, total)`.
#[must_use]
pub fn parse_position(value: &str) -> (Option<i64>, Option<i64>) {
    let cleaned = clean(value);
    let mut parts = cleaned.split('/');
    let position = parts
        .next()
        .and_then(|part| part.trim().parse::<i64>().ok())
        .filter(|position| (0..=9999).contains(position));
    let total = parts
        .next()
        .and_then(|part| part.trim().parse::<i64>().ok())
        .filter(|total| (0..=9999).contains(total));
    (position, total)
}

/// Extract a four-digit year from any date-ish tag value.
#[must_use]
pub fn parse_year(value: &str) -> Option<i64> {
    let cleaned = clean(value);
    for token in cleaned.split(['-', '/', '.', ' ']) {
        if token.len() == 4
            && let Ok(year) = token.parse::<i64>()
            && (1000..=2999).contains(&year)
        {
            return Some(year);
        }
    }
    None
}

/// Parse a ReplayGain gain tag ("-7.25 dB", "-7.25dB", "-7.25").
#[must_use]
pub fn parse_gain(value: &str) -> Option<f64> {
    let cleaned = clean(value);
    let number = cleaned
        .trim_end_matches(|c: char| c.is_ascii_alphabetic() || c == ' ')
        .trim();
    number
        .parse::<f64>()
        .ok()
        .filter(|gain| (-60.0..=30.0).contains(gain))
}

/// Parse a ReplayGain peak tag ("0.988525", "1.0").
#[must_use]
pub fn parse_peak(value: &str) -> Option<f64> {
    clean(value)
        .parse::<f64>()
        .ok()
        .filter(|peak| (0.0..=1.0).contains(peak))
}

/// Path of the album image referenced by a CUE sheet, resolved relative to it.
#[must_use]
pub fn resolve_cue_image(cue_path: &std::path::Path, image: &str) -> PathBuf {
    let candidate = std::path::Path::new(image);
    if candidate.is_absolute() {
        return candidate.to_path_buf();
    }
    cue_path
        .parent()
        .map(|parent| parent.join(candidate))
        .unwrap_or_else(|| candidate.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_collapses_whitespace_and_drops_controls() {
        assert_eq!(
            clean("  Radio\u{0}head \t Amnesiac  "),
            "Radiohead Amnesiac"
        );
        assert_eq!(clean(""), "");
    }

    #[test]
    fn sort_key_ignores_leading_articles() {
        assert_eq!(sort_key("The Beatles"), "beatles");
        assert_eq!(sort_key("A Tribe Called Quest"), "tribe called quest");
        assert_eq!(sort_key("An Ending"), "ending");
        assert_eq!(sort_key("Los Planetas"), "planetas");
        assert_eq!(sort_key("Massive Attack"), "massive attack");
    }

    #[test]
    fn positions_accept_track_slash_total() {
        assert_eq!(parse_position("3"), (Some(3), None));
        assert_eq!(parse_position("3/12"), (Some(3), Some(12)));
        assert_eq!(parse_position("03/12"), (Some(3), Some(12)));
        assert_eq!(parse_position(""), (None, None));
        assert_eq!(parse_position("x/12"), (None, Some(12)));
    }

    #[test]
    fn years_are_extracted_from_dates() {
        assert_eq!(parse_year("1997"), Some(1997));
        assert_eq!(parse_year("1997-05-21"), Some(1997));
        assert_eq!(parse_year("21/05/1997"), Some(1997));
        assert_eq!(parse_year("unknown"), None);
        assert_eq!(parse_year("12"), None);
    }

    #[test]
    fn replaygain_values_are_parsed_with_units() {
        assert_eq!(parse_gain("-7.25 dB"), Some(-7.25));
        assert_eq!(parse_gain("3.0dB"), Some(3.0));
        assert_eq!(parse_gain("-72 dB"), None);
        assert_eq!(parse_peak("0.988525"), Some(0.988525));
        assert_eq!(parse_peak("1.4"), None);
    }

    #[test]
    fn audio_and_cover_detection_are_extension_based() {
        assert!(is_audio_file(std::path::Path::new("x.FLAC")));
        assert!(is_audio_file(std::path::Path::new("x.dsf")));
        assert!(!is_audio_file(std::path::Path::new("x.txt")));
        assert!(is_cover_image(std::path::Path::new("Cover.JPG")));
        assert!(is_cover_image(std::path::Path::new("folder.png")));
        assert!(!is_cover_image(std::path::Path::new("booklet.jpg")));
    }

    #[test]
    fn cue_images_resolve_relative_to_the_sheet() {
        let resolved =
            resolve_cue_image(std::path::Path::new("/music/album/disc.cue"), "cover.jpg");
        assert_eq!(resolved, PathBuf::from("/music/album/cover.jpg"));
        let absolute = resolve_cue_image(std::path::Path::new("/a/b.cue"), "/x/y.flac");
        assert_eq!(absolute, PathBuf::from("/x/y.flac"));
    }
}
