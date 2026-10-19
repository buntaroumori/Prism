//! Cover-art sources, tried in order until one produces an image.
//!
//! | provider | latency | quality | offline |
//! |----------|---------|---------|---------|
//! | embedded | < 1 ms  | exact   | yes     |
//! | folder   | < 1 ms  | exact   | yes     |
//! | iTunes   | ~200 ms | good    | no      |
//! | MusicBrainz + Cover Art Archive | ~1 s (rate-limited) | archival | no |
//!
//! Local sources win because they are what the user actually filed with the
//! album. The network providers only run for albums with no local artwork, and
//! every network hit is cached for [`super::SUCCESS_TTL_DAYS`] days.

use std::path::{Path, PathBuf};

use lofty::prelude::TaggedFileExt;
use serde_json::Value;

use crate::metadata::is_cover_image;

/// Where a cover came from; mirrors the `cover_cache.provider` CHECK.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverProvider {
    Embedded,
    Folder,
    Itunes,
    MusicBrainz,
    CoverArtArchive,
    Unknown,
}

impl CoverProvider {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Embedded => "embedded",
            Self::Folder => "folder",
            Self::Itunes => "itunes",
            Self::MusicBrainz => "musicbrainz",
            Self::CoverArtArchive => "coverartarchive",
            Self::Unknown => "unknown",
        }
    }
}

/// A provider result, before cache normalisation.
#[derive(Debug, Clone)]
pub struct ProviderImage {
    pub provider: CoverProvider,
    pub bytes: Vec<u8>,
    pub remote_url: Option<String>,
}

/// Highest-resolution embedded picture in `path`.
///
/// `lofty` returns every APIC/FLAC picture; the front cover is preferred and
/// the largest wins, which is what a mastering engineer would expect a player
/// to pick from a file carrying both a 300 px thumb and a 1400 px scan.
#[must_use]
pub fn embedded(path: &Path) -> Option<ProviderImage> {
    let tagged = lofty::read_from_path(path).ok()?;
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag())?;

    // `Picture` does not expose its dimensions without decoding, so the byte
    // length is used as the resolution proxy: a 1400 px scan is an order of
    // magnitude larger than a 300 px thumbnail. Front covers always win.
    let picture = tag
        .pictures()
        .iter()
        .filter(|picture| picture.data().len() > 512)
        .max_by_key(|picture| {
            (
                u8::from(matches!(
                    picture.pic_type(),
                    lofty::picture::PictureType::CoverFront
                )),
                picture.data().len(),
            )
        })?;

    Some(ProviderImage {
        provider: CoverProvider::Embedded,
        bytes: picture.data().to_vec(),
        remote_url: None,
    })
}

/// Cover image filed next to the album (`cover.jpg`, `folder.png`, …).
///
/// A file literally called `front.jpg` wins over `album.jpg`; when several
/// candidates exist the biggest one is taken, which picks the 1600 px scan
/// over the 300 px thumbnail that ships in the same folder surprisingly often.
#[must_use]
pub fn folder(folder: &Path) -> Option<ProviderImage> {
    let mut best: Option<(u64, PathBuf)> = None;
    let entries = std::fs::read_dir(folder).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !is_cover_image(&path) {
            continue;
        }
        let size = entry.metadata().map(|meta| meta.len()).unwrap_or(0);
        let priority = if path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .is_some_and(|stem| stem.eq_ignore_ascii_case("front"))
        {
            size + 1_000_000_000
        } else {
            size
        };
        if best
            .as_ref()
            .is_none_or(|(best_size, _)| priority > *best_size)
        {
            best = Some((priority, path));
        }
    }
    let (_, path) = best?;
    let bytes = std::fs::read(path).ok()?;
    if bytes.is_empty() {
        return None;
    }
    Some(ProviderImage {
        provider: CoverProvider::Folder,
        bytes,
        remote_url: None,
    })
}

/// Case- and punctuation-insensitive comparison key.
#[must_use]
pub fn normalise(value: &str) -> String {
    value
        .to_lowercase()
        .replace("&", "and")
        .chars()
        .filter(|character| character.is_alphanumeric())
        .collect()
}

/// Fuzzy acceptance: the catalogue's title must contain the local one or vice
/// versa, after normalisation. This is what stops "Greatest Hits" by ABBA from
/// being illustrated with "Greatest Hits" by Queen.
#[must_use]
pub fn titles_match(candidate: &str, wanted: &str) -> bool {
    let candidate = normalise(candidate);
    let wanted = normalise(wanted);
    if candidate.is_empty() || wanted.is_empty() {
        return false;
    }
    if candidate.len() < 4 || wanted.len() < 4 {
        return candidate == wanted;
    }
    candidate.contains(&wanted) || wanted.contains(&candidate)
}

/// Pick the best iTunes search result for `(artist, album)`.
///
/// Returns `(artwork_url_600, collection_name)`.
#[must_use]
pub fn select_itunes_result(json: &Value, artist: &str, album: &str) -> Option<(String, String)> {
    let results = json.get("results")?.as_array()?;
    let mut best: Option<(u8, String, String)> = None;
    for result in results {
        let Some(collection) = result.get("collectionName").and_then(Value::as_str) else {
            continue;
        };
        let Some(candidate_artist) = result.get("artistName").and_then(Value::as_str) else {
            continue;
        };
        let Some(artwork) = result.get("artworkUrl100").and_then(Value::as_str) else {
            continue;
        };
        if !titles_match(collection, album) {
            continue;
        }
        let artist_score = if normalise(candidate_artist) == normalise(artist) {
            2
        } else if titles_match(candidate_artist, artist) {
            1
        } else {
            0
        };
        if artist_score == 0 {
            continue;
        }
        // Artwork URLs are size templates: `.../100x100bb.jpg`.
        let large = artwork.replace("100x100bb", "600x600bb");
        if best
            .as_ref()
            .is_none_or(|(score, _, _)| artist_score > *score)
        {
            best = Some((artist_score, large, collection.to_string()));
        }
    }
    best.map(|(_, url, collection)| (url, collection))
}

/// Extract the best MusicBrainz release id from a search response.
#[must_use]
pub fn select_musicbrainz_release(json: &Value, album: &str) -> Option<String> {
    let releases = json.get("releases")?.as_array()?;
    let mut best: Option<(u64, String)> = None;
    for release in releases {
        let Some(title) = release.get("title").and_then(Value::as_str) else {
            continue;
        };
        let Some(id) = release.get("id").and_then(Value::as_str) else {
            continue;
        };
        let score: u64 = release.get("score").and_then(Value::as_u64).unwrap_or(0);
        if score < 70 || !titles_match(title, album) {
            continue;
        }
        if best
            .as_ref()
            .is_none_or(|(best_score, _)| score > *best_score)
        {
            best = Some((score, id.to_string()));
        }
    }
    best.map(|(_, id)| id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalisation_folds_case_and_punctuation() {
        assert_eq!(normalise("Simon & Garfunkel"), "simonandgarfunkel");
        assert_eq!(normalise("Beyoncé — LEMONADE"), "beyoncélemonade");
    }

    #[test]
    fn titles_match_is_a_two_way_substring_test() {
        assert!(titles_match("Kid A", "kid a"));
        assert!(titles_match("Kid A (Remastered)", "Kid A"));
        assert!(titles_match(
            "The Dark Side of the Moon",
            "Dark Side of the Moon"
        ));
        // A deluxe/remaster suffix must still match its parent release.
        assert!(titles_match("Greatest Hits", "Greatest Hits II"));
        assert!(!titles_match("Kid A", "Amnesiac"));
        // Short titles must match exactly to avoid "A" matching everything.
        assert!(!titles_match("A", "AB"));
        assert!(titles_match("A", "a"));
    }

    #[test]
    fn itunes_selection_requires_the_artist_to_match() {
        let json = json!({
            "results": [
                {"collectionName": "Greatest Hits", "artistName": "Queen", "artworkUrl100": "https://x/100x100bb.jpg"},
                {"collectionName": "Greatest Hits", "artistName": "ABBA", "artworkUrl100": "https://y/100x100bb.jpg"}
            ]
        });
        let (url, collection) =
            select_itunes_result(&json, "ABBA", "Greatest Hits").expect("match");
        assert_eq!(url, "https://y/600x600bb.jpg");
        assert_eq!(collection, "Greatest Hits");
        assert!(select_itunes_result(&json, "Radiohead", "Greatest Hits").is_none());
    }

    #[test]
    fn musicbrainz_selection_respects_score_and_title() {
        let json = json!({
            "releases": [
                {"id": "low", "title": "Kid A", "score": 40},
                {"id": "good", "title": "Kid A", "score": 95},
                {"id": "other", "title": "Amnesiac", "score": 100}
            ]
        });
        assert_eq!(
            select_musicbrainz_release(&json, "Kid A").as_deref(),
            Some("good")
        );
        assert!(select_musicbrainz_release(&json, "Hail to the Thief").is_none());
    }
}
