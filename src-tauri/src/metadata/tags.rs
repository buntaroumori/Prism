//! One file, one [`TrackTags`].
//!
//! `lofty` abstracts the container (FLAC/Vorbis, ID3v2, MP4 ilst, APEv2, RIFF
//! INFO, AIFF text) and exposes a unified [`ItemKey`] vocabulary; this module
//! turns that into the flat row the database stores. Every value is normalised
//! here, never in SQL: the scanner must be able to index a 500k library without
//! a single `COALESCE` in the hot path.

use std::collections::BTreeMap;
use std::path::Path;

use lofty::file::{FileType, TaggedFile};
use lofty::prelude::{AudioFile, ItemKey, TaggedFileExt};
use serde_json::{Map, Value};

use super::{MetadataError, clean, parse_gain, parse_peak, parse_position, parse_year, sort_key};

/// Everything Prism knows about one audio file after reading its tags.
///
/// The field set maps one-to-one onto the `tracks` columns the scanner writes,
/// plus the album-level values (label, catalogue number, barcode) that the
/// scanner folds into the album row.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackTags {
    pub title: String,
    pub title_sort: String,
    pub artist: String,
    pub artist_sort: String,
    pub album: String,
    pub album_sort: String,
    pub album_artist: String,
    pub album_artist_sort: String,
    pub composer: String,
    pub genre: String,
    pub comment: String,
    pub year: Option<i64>,
    pub original_year: Option<i64>,
    pub release_date: Option<String>,
    pub track_no: Option<i64>,
    pub track_total: Option<i64>,
    pub disc_no: Option<i64>,
    pub disc_total: Option<i64>,
    pub format: String,
    pub container: Option<String>,
    pub codec_profile: Option<String>,
    pub bit_depth: Option<i64>,
    pub sample_rate: Option<i64>,
    pub channels: Option<i64>,
    pub bitrate_kbps: Option<i64>,
    pub dsd_rate: Option<i64>,
    pub duration_ms: i64,
    pub file_size: i64,
    pub replaygain_track_gain_db: Option<f64>,
    pub replaygain_track_peak: Option<f64>,
    pub replaygain_album_gain_db: Option<f64>,
    pub replaygain_album_peak: Option<f64>,
    pub musicbrainz_track_id: Option<String>,
    pub musicbrainz_release_id: Option<String>,
    pub musicbrainz_release_track_id: Option<String>,
    pub musicbrainz_artist_id: Option<String>,
    pub musicbrainz_release_group_id: Option<String>,
    pub isrc: Option<String>,
    pub label: Option<String>,
    pub catalog_number: Option<String>,
    pub barcode: Option<String>,
    pub has_embedded_art: bool,
    pub is_compilation: bool,
    pub extra: Map<String, Value>,
}

impl TrackTags {
    /// Album key used for the deterministic album id and for artwork lookup.
    #[must_use]
    pub fn album_key(&self) -> String {
        let artist = if self.album_artist.is_empty() {
            self.artist.as_str()
        } else {
            self.album_artist.as_str()
        };
        format!("{}|{}", self.album.to_lowercase(), artist.to_lowercase())
    }

    /// Human-readable technical summary used by the quality badge.
    #[must_use]
    pub fn quality_label(&self) -> String {
        super::quality_label(
            self.bit_depth,
            self.sample_rate,
            self.dsd_rate,
            &self.format,
        )
    }
}

/// Read one file.
///
/// Fails only when the file cannot be opened or its container is unknown; a
/// file with no tags at all is perfectly valid and gets a filename-derived
/// title, which is how a well-behaved player treats an untagged rip.
pub fn read(path: &Path) -> Result<TrackTags, MetadataError> {
    let metadata = std::fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(MetadataError::Unsupported(format!(
            "{} is not a regular file",
            path.display()
        )));
    }
    let tagged = lofty::read_from_path(path).map_err(|error| match error.kind() {
        // `kind()` borrows, so the io error is rebuilt rather than moved.
        lofty::error::ErrorKind::Io(io) => {
            MetadataError::Io(std::io::Error::new(io.kind(), io.to_string()))
        }
        lofty::error::ErrorKind::UnknownFormat => MetadataError::Unsupported(error.to_string()),
        _ => MetadataError::Malformed(error.to_string()),
    })?;

    let file_size = i64::try_from(metadata.len()).unwrap_or(i64::MAX);
    Ok(from_tagged_file(path, &tagged, file_size))
}

/// Pure conversion from a parsed `TaggedFile` — no I/O, therefore unit-testable.
#[must_use]
pub fn from_tagged_file(path: &Path, tagged: &TaggedFile, file_size: i64) -> TrackTags {
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag());
    let properties = tagged.properties();

    let get = |key: &ItemKey| -> String {
        tag.and_then(|tag| tag.get_string(key))
            .map(clean)
            .unwrap_or_default()
    };
    let get_optional = |key: &ItemKey| -> Option<String> {
        let value = get(key);
        (!value.is_empty()).then_some(value)
    };

    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .unwrap_or_default();

    // ---- Titles and people -------------------------------------------------
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(clean)
        .unwrap_or_else(|| "Untitled".to_string());

    let mut title = get(&ItemKey::TrackTitle);
    if title.is_empty() {
        // "01 - Song" or "01. Song" is a filename convention, not a title.
        title = strip_track_prefix(&stem);
    }
    if title.is_empty() {
        title = stem.clone();
    }

    // `TrackArtists` is the multi-value tag; fall back to the scalar one.
    let mut artist = get(&ItemKey::TrackArtist);
    if artist.is_empty()
        && let Some(tag) = tag
        && let Some(first) = tag.get_strings(&ItemKey::TrackArtists).next()
    {
        artist = clean(first);
    }

    let album = get(&ItemKey::AlbumTitle);
    let album_artist = {
        let value = get(&ItemKey::AlbumArtist);
        if value.is_empty() {
            artist.clone()
        } else {
            value
        }
    };

    let title_sort = {
        let value = get(&ItemKey::TrackTitleSortOrder);
        if value.is_empty() {
            sort_key(&title)
        } else {
            value
        }
    };
    let artist_sort = {
        let value = get(&ItemKey::TrackArtistSortOrder);
        if value.is_empty() {
            sort_key(&artist)
        } else {
            value
        }
    };
    let album_sort = {
        let value = get(&ItemKey::AlbumTitleSortOrder);
        if value.is_empty() {
            if album.is_empty() {
                String::new()
            } else {
                sort_key(&album)
            }
        } else {
            value
        }
    };
    let album_artist_sort = {
        let value = get(&ItemKey::AlbumArtistSortOrder);
        if value.is_empty() {
            sort_key(&album_artist)
        } else {
            value
        }
    };

    // ---- Positions ---------------------------------------------------------
    let (track_no, track_total_tag) = tag
        .and_then(|tag| tag.get_string(&ItemKey::TrackNumber))
        .map_or((None, None), parse_position);
    let (disc_no, disc_total_tag) = tag
        .and_then(|tag| tag.get_string(&ItemKey::DiscNumber))
        .map_or((None, None), parse_position);
    let track_total = tag
        .and_then(|tag| tag.get_string(&ItemKey::TrackTotal))
        .and_then(|value| clean(value).parse::<i64>().ok())
        .or(track_total_tag);
    let disc_total = tag
        .and_then(|tag| tag.get_string(&ItemKey::DiscTotal))
        .and_then(|value| clean(value).parse::<i64>().ok())
        .or(disc_total_tag);

    // ---- Dates -------------------------------------------------------------
    let year = get_optional(&ItemKey::Year)
        .as_deref()
        .and_then(parse_year)
        .or_else(|| {
            get_optional(&ItemKey::RecordingDate)
                .as_deref()
                .and_then(parse_year)
        });
    let original_year = get_optional(&ItemKey::OriginalReleaseDate)
        .as_deref()
        .and_then(parse_year);
    let release_date = get_optional(&ItemKey::ReleaseDate)
        .or_else(|| get_optional(&ItemKey::RecordingDate))
        .or_else(|| get_optional(&ItemKey::Year));

    // ---- Technical properties ---------------------------------------------
    let duration_ms = i64::try_from(properties.duration().as_millis()).unwrap_or(i64::MAX);
    let channels = properties.channels().map(i64::from);
    let mut bit_depth = properties.bit_depth().map(i64::from);
    let mut sample_rate = properties.sample_rate().map(i64::from);
    let bitrate_kbps = properties
        .overall_bitrate()
        .or_else(|| properties.audio_bitrate())
        .map(|bitrate| i64::from(bitrate / 1_000));

    let is_dsd = matches!(extension.as_str(), "dsf" | "dff");
    let dsd_rate = is_dsd.then(|| {
        // DSF/DFF report the 1-bit rate; the DSD family table only accepts the
        // four standard rates, so snap to the nearest known one.
        const FAMILIES: [i64; 4] = [2_822_400, 5_644_800, 11_289_600, 22_579_200];
        let reported = sample_rate.unwrap_or(2_822_400);
        FAMILIES
            .into_iter()
            .min_by_key(|family| (family - reported).abs())
            .unwrap_or(2_822_400)
    });
    if let Some(dsd) = dsd_rate {
        // The PCM carrier of DoP is 1/16 of the DSD rate; that value is what
        // the `tracks.sample_rate` constraint expects.
        sample_rate = Some(dsd / 16);
        bit_depth = Some(1);
    } else if extension == "wav" || extension == "wave" {
        // Nothing to compensate: RIFF carries real PCM properties.
    }

    let container = Some(file_type_label(tagged.file_type()));
    let codec_profile = lossless_profile(extension.as_str(), bit_depth, sample_rate);

    // ---- ReplayGain 2 ------------------------------------------------------
    let replaygain_track_gain_db = tag
        .and_then(|tag| tag.get_string(&ItemKey::ReplayGainTrackGain))
        .and_then(parse_gain);
    let replaygain_track_peak = tag
        .and_then(|tag| tag.get_string(&ItemKey::ReplayGainTrackPeak))
        .and_then(parse_peak);
    let replaygain_album_gain_db = tag
        .and_then(|tag| tag.get_string(&ItemKey::ReplayGainAlbumGain))
        .and_then(parse_gain);
    let replaygain_album_peak = tag
        .and_then(|tag| tag.get_string(&ItemKey::ReplayGainAlbumPeak))
        .and_then(parse_peak);

    // ---- Identifiers -------------------------------------------------------
    let musicbrainz_track_id = get_optional(&ItemKey::MusicBrainzRecordingId)
        .or_else(|| get_optional(&ItemKey::MusicBrainzTrackId));
    let musicbrainz_release_track_id = get_optional(&ItemKey::MusicBrainzTrackId);
    let musicbrainz_release_id = get_optional(&ItemKey::MusicBrainzReleaseId);
    let musicbrainz_artist_id = get_optional(&ItemKey::MusicBrainzArtistId);
    let musicbrainz_release_group_id = get_optional(&ItemKey::MusicBrainzReleaseGroupId);

    let is_compilation = {
        let flag = get(&ItemKey::FlagCompilation);
        matches!(flag.to_ascii_lowercase().as_str(), "1" | "true" | "yes")
    };

    // ---- Leftover tags -----------------------------------------------------
    let mut extra = Map::new();
    if let Some(tag) = tag {
        let mut collected: BTreeMap<String, String> = BTreeMap::new();
        for item in tag.items() {
            if let ItemKey::Unknown(key) = item.key()
                && let Some(text) = item.value().text()
            {
                let cleaned = clean(text);
                if !cleaned.is_empty() && cleaned.len() <= 256 {
                    collected.insert(key.clone(), cleaned);
                }
            }
        }
        // Bound the JSON payload: this column is read for every visible row.
        for (key, value) in collected.into_iter().take(24) {
            extra.insert(key, Value::String(value));
        }
    }

    TrackTags {
        title,
        title_sort,
        artist,
        artist_sort,
        album,
        album_sort,
        album_artist,
        album_artist_sort,
        composer: get(&ItemKey::Composer),
        genre: get(&ItemKey::Genre),
        comment: get(&ItemKey::Comment),
        year,
        original_year,
        release_date,
        track_no,
        track_total,
        disc_no,
        disc_total,
        format: extension,
        container,
        codec_profile,
        bit_depth,
        sample_rate,
        channels,
        bitrate_kbps,
        dsd_rate,
        duration_ms,
        file_size,
        replaygain_track_gain_db,
        replaygain_track_peak,
        replaygain_album_gain_db,
        replaygain_album_peak,
        musicbrainz_track_id,
        musicbrainz_release_id,
        musicbrainz_release_track_id,
        musicbrainz_artist_id,
        musicbrainz_release_group_id,
        isrc: get_optional(&ItemKey::Isrc),
        label: get_optional(&ItemKey::Label),
        catalog_number: get_optional(&ItemKey::CatalogNumber),
        barcode: get_optional(&ItemKey::Barcode),
        has_embedded_art: tag.is_some_and(|tag| !tag.pictures().is_empty()),
        is_compilation,
        extra,
    }
}

fn file_type_label(file_type: FileType) -> String {
    match file_type {
        FileType::Aac => "AAC",
        FileType::Aiff => "AIFF",
        FileType::Ape => "Monkey's Audio",
        FileType::Flac => "FLAC",
        FileType::Mpeg => "MPEG",
        FileType::Mp4 => "MP4",
        FileType::Mpc => "Musepack",
        FileType::Opus => "Opus",
        FileType::Vorbis => "Ogg Vorbis",
        FileType::Speex => "Speex",
        FileType::Wav => "WAV",
        FileType::WavPack => "WavPack",
        _ => "Audio",
    }
    .to_string()
}

/// Lossless profile shown next to the format badge (e.g. "24-bit master").
fn lossless_profile(
    extension: &str,
    bit_depth: Option<i64>,
    sample_rate: Option<i64>,
) -> Option<String> {
    let lossless = matches!(
        extension,
        "flac" | "alac" | "wav" | "wave" | "aiff" | "aif" | "dsf" | "dff" | "ape" | "wv"
    );
    if !lossless {
        return None;
    }
    match (bit_depth, sample_rate) {
        (Some(1), _) => Some("1-bit DSD".to_string()),
        (Some(bits), Some(rate)) if bits >= 24 && rate >= 96_000 => {
            Some(format!("{bits}-bit hi-res"))
        }
        (Some(bits), _) if bits >= 24 => Some(format!("{bits}-bit lossless")),
        (_, Some(rate)) if rate >= 88_200 => Some("hi-res sample rate".to_string()),
        _ => Some("lossless".to_string()),
    }
}

/// Strip a leading track number from a filename: "01 - Title" → "Title".
fn strip_track_prefix(stem: &str) -> String {
    let trimmed = stem.trim_start_matches(|c: char| c.is_ascii_digit());
    let trimmed = trimmed.trim_start_matches([' ', '-', '.', '_', ')']);
    if trimmed.len() == stem.len() {
        return stem.to_string();
    }
    clean(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lofty::tag::{ItemValue, Tag, TagItem, TagType};

    fn tags_with(items: &[(&str, &str)]) -> TrackTags {
        let mut tag = Tag::new(TagType::VorbisComments);
        for (key, value) in items {
            // Mirror what a real decoder does: map the Vorbis key through the
            // `ItemKey` table, and only fall back to `Unknown` for keys lofty
            // has no semantic for.
            let item_key = ItemKey::from_key(TagType::VorbisComments, key);
            if matches!(item_key, ItemKey::Unknown(_)) {
                // `insert_text` rejects unmapped keys; `insert_unchecked` is
                // the documented path, and it is what the reader produces for
                // a custom Vorbis comment.
                tag.insert_unchecked(TagItem::new(
                    item_key,
                    ItemValue::Text((*value).to_string()),
                ));
            } else {
                tag.insert_text(item_key, (*value).to_string());
            }
        }
        tag.insert_text(
            ItemKey::TrackTitle,
            "Everything In Its Right Place".to_string(),
        );
        tag.insert_text(ItemKey::TrackArtist, "Radiohead".to_string());
        tag.insert_text(ItemKey::AlbumTitle, "Kid A".to_string());
        tag.insert_text(ItemKey::AlbumArtist, "Radiohead".to_string());
        tag.insert_text(ItemKey::TrackNumber, "1/10".to_string());
        tag.insert_text(ItemKey::Year, "2000-10-02".to_string());
        tag.insert_text(ItemKey::Genre, "Electronic".to_string());

        let properties = lofty::properties::FileProperties::default();
        let tagged = TaggedFile::new(FileType::Flac, properties, vec![tag]);
        from_tagged_file(
            Path::new("/music/Kid A/01 - Everything.flac"),
            &tagged,
            42_000_000,
        )
    }

    #[test]
    fn maps_the_core_tags_onto_the_row() {
        let tags = tags_with(&[]);
        assert_eq!(tags.title, "Everything In Its Right Place");
        assert_eq!(tags.artist, "Radiohead");
        assert_eq!(tags.album, "Kid A");
        assert_eq!(tags.album_artist, "Radiohead");
        assert_eq!(tags.track_no, Some(1));
        assert_eq!(tags.track_total, Some(10));
        assert_eq!(tags.year, Some(2000));
        assert_eq!(tags.release_date.as_deref(), Some("2000-10-02"));
        assert_eq!(tags.genre, "Electronic");
        assert_eq!(tags.format, "flac");
        assert_eq!(tags.title_sort, "everything in its right place");
        assert_eq!(tags.album_key(), "kid a|radiohead");
        assert_eq!(tags.file_size, 42_000_000);
    }

    #[test]
    fn untagged_files_fall_back_to_the_filename() {
        let properties = lofty::properties::FileProperties::default();
        let tagged = TaggedFile::new(FileType::Flac, properties, Vec::new());
        let tags = from_tagged_file(Path::new("/music/01 - Untitled.flac"), &tagged, 1);
        assert_eq!(tags.title, "Untitled");
        assert_eq!(tags.artist, "");
        // The album artist falls back to the track artist, not to a placeholder.
        assert_eq!(tags.album_artist, "");
        assert_eq!(tags.album, "");
        assert_eq!(tags.album_key(), "|");
    }

    #[test]
    fn dsd_reports_the_family_and_a_legal_carrier_rate() {
        let properties = lofty::properties::FileProperties::default();
        let tagged = TaggedFile::new(FileType::Flac, properties, Vec::new());
        let tags = from_tagged_file(Path::new("/music/album.dsf"), &tagged, 1);
        // Defaults to DSD64 when the properties are unavailable.
        assert_eq!(tags.dsd_rate, Some(2_822_400));
        assert_eq!(tags.sample_rate, Some(176_400));
        assert_eq!(tags.bit_depth, Some(1));
        assert_eq!(tags.quality_label(), "DSD64");
        assert!(tags.bit_depth.is_some_and(|bits| (1..=64).contains(&bits)));
    }

    #[test]
    fn quality_label_is_built_from_the_technical_properties() {
        let mut tags = tags_with(&[]);
        tags.bit_depth = Some(24);
        tags.sample_rate = Some(96_000);
        assert_eq!(tags.quality_label(), "96 kHz / 24 bit");

        tags.sample_rate = Some(44_100);
        assert_eq!(tags.quality_label(), "44.1 kHz / 24 bit");

        tags.bit_depth = None;
        tags.dsd_rate = Some(11_289_600);
        assert_eq!(tags.quality_label(), "DSD256");
    }

    #[test]
    fn replaygain_and_musicbrainz_identifiers_are_read_from_any_container() {
        let tags = tags_with(&[
            ("REPLAYGAIN_TRACK_GAIN", "-6.28 dB"),
            ("REPLAYGAIN_TRACK_PEAK", "0.9772"),
            (
                "MUSICBRAINZ_TRACKID",
                "b1c2d3e4-0000-4000-8000-000000000001",
            ),
            ("X-PRISM-NOTE", "nocturnal"),
        ]);
        assert_eq!(tags.replaygain_track_gain_db, Some(-6.28));
        assert_eq!(tags.replaygain_track_peak, Some(0.9772));
        assert_eq!(
            tags.musicbrainz_track_id.as_deref(),
            Some("b1c2d3e4-0000-4000-8000-000000000001")
        );
        // Keys lofty has no semantic for are preserved as JSON, which is what
        // the track inspector shows.
        assert_eq!(
            tags.extra.get("X-PRISM-NOTE"),
            Some(&Value::String("nocturnal".into()))
        );
        assert_eq!(tags.extra.get("REPLAYGAIN_TRACK_GAIN"), None);
    }

    #[test]
    fn track_prefixes_are_stripped_only_when_a_title_is_missing() {
        assert_eq!(strip_track_prefix("01 - Song"), "Song");
        assert_eq!(strip_track_prefix("07. Song"), "Song");
        assert_eq!(strip_track_prefix("Song"), "Song");
        assert_eq!(strip_track_prefix("99 Luftballons"), "Luftballons");
    }
}
