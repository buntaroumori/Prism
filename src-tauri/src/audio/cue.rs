//! CUE sheet parser.
//!
//! Supports the commands that matter for playback and cataloguing of
//! single-image albums: `FILE`, `TRACK`, `INDEX` (00 pregap and 01 start),
//! `PREGAP`, `POSTGAP`, `TITLE`, `PERFORMER`, `SONGWRITER`, `ISRC`, `FLAGS`,
//! `CATALOG`, `CDTEXTFILE` and `REM` metadata (date, genre, disc id, comment).
//!
//! Timecodes are `MM:SS:FF` with 75 frames per second (Red Book). Parsing is
//! strict about structure (every `INDEX` must belong to a `TRACK`, tracks must
//! be numbered, at least one `INDEX 01` is required) and lenient about
//! unknown commands, which are ignored as the format mandates.
//!
//! The parser never touches the audio file: it works on text only, so a
//! 4 GB album image is parsed in microseconds. Boundaries are converted into
//! millisecond spans which the decoder turns into sample-exact frame ranges.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::audio::AudioError;

/// Frames per second in a CUE timecode.
pub const CUE_FRAMES_PER_SECOND: u64 = 75;

/// Physical file referenced by a `FILE` command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CueFile {
    /// File name exactly as written in the sheet (relative to the sheet).
    pub name: String,
    /// Container declared by the sheet (`WAVE`, `BINARY`, `MP3`, `AIFF`).
    pub file_type: String,
    /// Absolute path resolved against the sheet's directory.
    pub resolved_path: PathBuf,
    pub tracks: Vec<CueTrack>,
}

/// One `TRACK` entry with its resolved millisecond span.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CueTrack {
    /// Track number as written (`01`..`99`).
    pub number: u8,
    /// Track number normalised to a string, preserving leading zeros.
    pub number_label: String,
    pub title: Option<String>,
    pub performer: Option<String>,
    pub songwriter: Option<String>,
    pub isrc: Option<String>,
    /// `FLAGS` tokens (`DCP`, `4CH`, `PRE`, `SCMS`).
    pub flags: Vec<String>,
    /// `PREGAP` declared before the track, in milliseconds.
    pub pregap_ms: Option<u64>,
    /// `POSTGAP` declared after the track, in milliseconds.
    pub postgap_ms: Option<u64>,
    /// `INDEX 00` (pregap start), when present.
    pub index00_ms: Option<u64>,
    /// `INDEX 01` (audible start).
    pub start_ms: u64,
    /// End of the audible span, derived from the next track or the file
    /// duration when known.
    pub end_ms: Option<u64>,
    /// Whether the sheet declared `INDEX 01`. Not serialised: it exists only
    /// to validate the sheet after parsing.
    #[serde(skip)]
    pub(crate) has_index01: bool,
}

impl CueTrack {
    /// Audible duration when the end boundary is known.
    #[must_use]
    pub fn duration_ms(&self) -> Option<u64> {
        self.end_ms.map(|end| end.saturating_sub(self.start_ms))
    }
}

/// A fully parsed sheet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CueSheet {
    /// Absolute path of the `.cue` file.
    pub path: PathBuf,
    pub title: Option<String>,
    pub performer: Option<String>,
    pub songwriter: Option<String>,
    /// `CATALOG` (UPC/EAN).
    pub catalog: Option<String>,
    /// `CDTEXTFILE`.
    pub cdtext_file: Option<String>,
    /// `REM DATE`.
    pub date: Option<String>,
    /// `REM GENRE`.
    pub genre: Option<String>,
    /// `REM DISC_ID` / `REM DISCID`.
    pub disc_id: Option<String>,
    /// `REM COMMENT`.
    pub comment: Option<String>,
    /// All `REM` keys in file order (case preserved as written).
    pub rem: Vec<(String, String)>,
    pub files: Vec<CueFile>,
}

impl CueSheet {
    /// Total number of tracks across every referenced file.
    #[must_use]
    pub fn track_count(&self) -> usize {
        self.files.iter().map(|file| file.tracks.len()).sum()
    }

    /// Sum of all known track durations.
    #[must_use]
    pub fn total_ms(&self) -> u64 {
        self.files
            .iter()
            .flat_map(|file| &file.tracks)
            .filter_map(CueTrack::duration_ms)
            .sum()
    }

    /// Flattened spans in playback order.
    #[must_use]
    pub fn spans(&self) -> Vec<&CueTrack> {
        self.files.iter().flat_map(|file| &file.tracks).collect()
    }

    /// Parse a `.cue` file from disk, resolving `FILE` paths against its
    /// directory.
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, AudioError> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path).map_err(|error| AudioError::io(path, error))?;
        let base = path.parent().unwrap_or_else(|| Path::new("."));
        Self::parse(&text, path, base)
    }

    /// Parse sheet text with an explicit sheet path and resolution base.
    pub fn parse(text: &str, sheet_path: &Path, base_dir: &Path) -> Result<Self, AudioError> {
        let mut sheet = CueSheet {
            path: sheet_path.to_path_buf(),
            title: None,
            performer: None,
            songwriter: None,
            catalog: None,
            cdtext_file: None,
            date: None,
            genre: None,
            disc_id: None,
            comment: None,
            rem: Vec::new(),
            files: Vec::new(),
        };

        // State machine: current file and current track indices.
        let mut current_file: Option<usize> = None;
        let mut current_track: Option<usize> = None;
        let mut line_number = 0usize;

        for raw_line in text.lines() {
            line_number += 1;
            let line = raw_line.trim_start_matches('\u{feff}').trim();
            if line.is_empty() || line.starts_with(';') {
                continue;
            }

            let (command, rest) = split_command(line);
            let command_upper = command.to_ascii_uppercase();

            match command_upper.as_str() {
                "FILE" => {
                    let (name, file_type) = parse_file(&rest).ok_or_else(|| {
                        cue_error(line_number, "FILE requires a quoted name and a type")
                    })?;
                    let resolved = resolve_file(base_dir, &name);
                    sheet.files.push(CueFile {
                        name,
                        file_type,
                        resolved_path: resolved,
                        tracks: Vec::new(),
                    });
                    current_file = Some(sheet.files.len() - 1);
                    current_track = None;
                }
                "TRACK" => {
                    let file_index = current_file.ok_or_else(|| {
                        cue_error(line_number, "TRACK appears before any FILE command")
                    })?;
                    let (number_label, kind) = parse_track(&rest).ok_or_else(|| {
                        cue_error(line_number, "TRACK requires a number and an audio mode")
                    })?;
                    if !matches!(kind.to_ascii_uppercase().as_str(), "AUDIO") {
                        return Err(cue_error(
                            line_number,
                            &format!("unsupported track mode '{kind}' (only AUDIO is valid)"),
                        ));
                    }
                    let number: u8 = number_label.parse().map_err(|_| {
                        cue_error(
                            line_number,
                            &format!("invalid track number '{number_label}'"),
                        )
                    })?;
                    if number == 0 {
                        return Err(cue_error(line_number, "track numbers start at 01"));
                    }
                    let file = &sheet.files[file_index];
                    if file.tracks.iter().any(|track| track.number == number) {
                        return Err(cue_error(
                            line_number,
                            &format!("duplicate track number {number_label}"),
                        ));
                    }

                    sheet.files[file_index].tracks.push(CueTrack {
                        number,
                        number_label,
                        title: None,
                        performer: None,
                        songwriter: None,
                        isrc: None,
                        flags: Vec::new(),
                        pregap_ms: None,
                        postgap_ms: None,
                        index00_ms: None,
                        start_ms: 0,
                        end_ms: None,
                        has_index01: false,
                    });
                    current_track = Some(sheet.files[file_index].tracks.len() - 1);
                }
                "INDEX" => {
                    let (file_index, track_index) =
                        current_location(current_file, current_track, line_number, "INDEX")?;
                    let (index_number, ms) = parse_index(&rest).ok_or_else(|| {
                        cue_error(
                            line_number,
                            "INDEX requires a number and an MM:SS:FF timecode",
                        )
                    })?;
                    match index_number {
                        0 => {
                            sheet.files[file_index].tracks[track_index].index00_ms = Some(ms);
                        }
                        1 => {
                            sheet.files[file_index].tracks[track_index].start_ms = ms;
                            sheet.files[file_index].tracks[track_index].has_index01 = true;
                        }
                        _ => {
                            // INDEX 02..99 mark sub-indexes; they do not change
                            // the audible start and are intentionally ignored.
                        }
                    }
                }
                "TITLE" => {
                    let value = unquote(&rest);
                    match current_track_location(current_file, current_track) {
                        Some((file_index, track_index)) => {
                            sheet.files[file_index].tracks[track_index].title = Some(value);
                        }
                        // A TITLE directly inside FILE (before any TRACK) names
                        // the album, not a track.
                        None => sheet.title = Some(value),
                    }
                }
                "PERFORMER" => {
                    let value = unquote(&rest);
                    match current_track_location(current_file, current_track) {
                        Some((file_index, track_index)) => {
                            sheet.files[file_index].tracks[track_index].performer = Some(value);
                        }
                        None => sheet.performer = Some(value),
                    }
                }
                "SONGWRITER" => {
                    let value = unquote(&rest);
                    match current_track_location(current_file, current_track) {
                        Some((file_index, track_index)) => {
                            sheet.files[file_index].tracks[track_index].songwriter = Some(value);
                        }
                        None => sheet.songwriter = Some(value),
                    }
                }
                "ISRC" => {
                    let (file_index, track_index) =
                        current_location(current_file, current_track, line_number, "ISRC")?;
                    sheet.files[file_index].tracks[track_index].isrc =
                        Some(rest.trim().to_string());
                }
                "FLAGS" => {
                    let (file_index, track_index) =
                        current_location(current_file, current_track, line_number, "FLAGS")?;
                    sheet.files[file_index].tracks[track_index].flags = rest
                        .split_whitespace()
                        .map(str::to_ascii_uppercase)
                        .collect();
                }
                "PREGAP" => {
                    let (file_index, track_index) =
                        current_location(current_file, current_track, line_number, "PREGAP")?;
                    let ms = parse_timecode(rest.trim()).ok_or_else(|| {
                        cue_error(line_number, "PREGAP requires an MM:SS:FF timecode")
                    })?;
                    sheet.files[file_index].tracks[track_index].pregap_ms = Some(ms);
                }
                "POSTGAP" => {
                    let (file_index, track_index) =
                        current_location(current_file, current_track, line_number, "POSTGAP")?;
                    let ms = parse_timecode(rest.trim()).ok_or_else(|| {
                        cue_error(line_number, "POSTGAP requires an MM:SS:FF timecode")
                    })?;
                    sheet.files[file_index].tracks[track_index].postgap_ms = Some(ms);
                }
                "CATALOG" => {
                    sheet.catalog = Some(rest.trim().to_string());
                }
                "CDTEXTFILE" => {
                    sheet.cdtext_file = Some(unquote(&rest));
                }
                "REM" => {
                    let (key, value) = split_command(&rest);
                    let key_upper = key.to_ascii_uppercase();
                    let value = unquote(&value);
                    match key_upper.as_str() {
                        "DATE" => sheet.date = Some(value.clone()),
                        "GENRE" => sheet.genre = Some(value.clone()),
                        "DISC_ID" | "DISCID" => sheet.disc_id = Some(value.clone()),
                        "COMMENT" => sheet.comment = Some(value.clone()),
                        _ => {}
                    }
                    sheet.rem.push((key, value));
                }
                _ => {
                    // Unknown command: the CUE specification requires parsers
                    // to ignore unrecognised lines.
                    tracing::trace!(line = line_number, command, "ignoring unknown CUE command");
                }
            }
        }

        // Every track must declare INDEX 01 (the audible start).
        for file in &sheet.files {
            for track in &file.tracks {
                if !track.has_index01 {
                    return Err(AudioError::Cue(format!(
                        "track {} has no INDEX 01",
                        track.number_label
                    )));
                }
            }
        }

        if sheet.files.is_empty() {
            return Err(AudioError::Cue("sheet contains no FILE command".into()));
        }
        if sheet.files.iter().all(|file| file.tracks.is_empty()) {
            return Err(AudioError::Cue("sheet contains no TRACK entries".into()));
        }

        Ok(sheet)
    }

    /// Resolve each track's end boundary from the next track's start.
    ///
    /// `file_duration_ms` is called for every referenced file and may return
    /// `None` when the container cannot report a duration; in that case the
    /// last track keeps an open end and playback stops at the decoder's EOF.
    pub fn resolve_boundaries<F>(&mut self, mut file_duration_ms: F)
    where
        F: FnMut(&Path) -> Option<u64>,
    {
        for file in &mut self.files {
            let file_duration = file_duration_ms(&file.resolved_path);
            let count = file.tracks.len();
            for index in 0..count {
                let end = if index + 1 < count {
                    Some(file.tracks[index + 1].start_ms)
                } else {
                    file_duration
                };
                file.tracks[index].end_ms = end;
            }
        }
    }
}

fn cue_error(line: usize, message: &str) -> AudioError {
    AudioError::Cue(format!("line {line}: {message}"))
}

fn split_command(line: &str) -> (String, String) {
    let mut parts = line.splitn(2, char::is_whitespace);
    let command = parts.next().unwrap_or_default().to_string();
    let rest = parts.next().unwrap_or_default().trim().to_string();
    (command, rest)
}

fn unquote(value: &str) -> String {
    let value = value.trim();
    if let Some(stripped) = value.strip_prefix('"') {
        if let Some(end) = stripped.rfind('"') {
            return stripped[..end].to_string();
        }
        return stripped.to_string();
    }
    value.to_string()
}

fn tokenize(value: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;

    for character in value.chars() {
        match character {
            '"' => {
                in_quotes = !in_quotes;
                if !in_quotes {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            c if c.is_whitespace() && !in_quotes => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn parse_file(rest: &str) -> Option<(String, String)> {
    let tokens = tokenize(rest);
    match tokens.as_slice() {
        [name, file_type] => Some((name.clone(), file_type.to_ascii_uppercase())),
        _ => None,
    }
}

fn parse_track(rest: &str) -> Option<(String, String)> {
    let tokens = tokenize(rest);
    match tokens.as_slice() {
        [number, mode] => Some((number.clone(), mode.clone())),
        _ => None,
    }
}

fn parse_index(rest: &str) -> Option<(u8, u64)> {
    let mut parts = rest.split_whitespace();
    let number: u8 = parts.next()?.parse().ok()?;
    let timecode = parts.next()?;
    let ms = parse_timecode(timecode)?;
    Some((number, ms))
}

/// Parse `MM:SS:FF` (75 CUE frames per second) into milliseconds.
///
/// Minutes above 99 are accepted for long DJ sets; the resulting value is
/// rounded down, matching how CD players address frames.
#[must_use]
pub fn parse_timecode(value: &str) -> Option<u64> {
    let mut parts = value.split(':');
    let minutes: u64 = parts.next()?.trim().parse().ok()?;
    let seconds: u64 = parts.next()?.trim().parse().ok()?;
    let frames: u64 = parts.next()?.trim().parse().ok()?;
    if seconds >= 60 || frames >= CUE_FRAMES_PER_SECOND {
        return None;
    }
    let total_frames = (minutes * 60 + seconds) * CUE_FRAMES_PER_SECOND + frames;
    Some(total_frames * 1000 / CUE_FRAMES_PER_SECOND)
}

fn resolve_file(base_dir: &Path, name: &str) -> PathBuf {
    let candidate = Path::new(name);
    if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        base_dir.join(candidate)
    }
}

/// Indices of the file and track block currently being parsed.
///
/// `None` when the command appears outside a `TRACK` (album-level metadata).
fn current_track_location(file: Option<usize>, track: Option<usize>) -> Option<(usize, usize)> {
    Some((file?, track?))
}

fn current_location(
    file: Option<usize>,
    track: Option<usize>,
    line: usize,
    command: &str,
) -> Result<(usize, usize), AudioError> {
    match (file, track) {
        (Some(file), Some(track)) => Ok((file, track)),
        _ => Err(cue_error(
            line,
            &format!("{command} appears outside a TRACK block"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALBUM: &str = r#"
REM GENRE "Electronic"
REM DATE 1997
REM COMMENT "Remastered"
CATALOG 0724352156123
PERFORMER "Björk"
TITLE "Homogenic"
FILE "Homogenic.flac" WAVE
  TRACK 01 AUDIO
    TITLE "Hunter"
    PERFORMER "Björk"
    ISRC GBAYC9700001
    INDEX 01 00:00:00
  TRACK 02 AUDIO
    TITLE "Jóga"
    FLAGS PRE
    INDEX 00 04:14:12
    INDEX 01 04:16:30
  TRACK 03 AUDIO
    TITLE "Unravel"
    PREGAP 00:02:00
    INDEX 01 09:12:05
"#;

    fn parse(text: &str) -> CueSheet {
        CueSheet::parse(text, Path::new("/music/Homogenic.cue"), Path::new("/music"))
            .expect("parse")
    }

    #[test]
    fn parses_album_metadata() {
        let sheet = parse(ALBUM);
        assert_eq!(sheet.title.as_deref(), Some("Homogenic"));
        assert_eq!(sheet.performer.as_deref(), Some("Björk"));
        assert_eq!(sheet.genre.as_deref(), Some("Electronic"));
        assert_eq!(sheet.date.as_deref(), Some("1997"));
        assert_eq!(sheet.catalog.as_deref(), Some("0724352156123"));
        assert_eq!(sheet.track_count(), 3);
        assert_eq!(
            sheet.files[0].resolved_path,
            PathBuf::from("/music/Homogenic.flac")
        );
    }

    #[test]
    fn parses_indices_and_pregap() {
        let sheet = parse(ALBUM);
        let tracks = &sheet.files[0].tracks;

        assert_eq!(tracks[0].start_ms, 0);
        assert_eq!(tracks[0].title.as_deref(), Some("Hunter"));

        assert_eq!(tracks[1].start_ms, parse_timecode("04:16:30").unwrap());
        assert_eq!(
            tracks[1].index00_ms,
            Some(parse_timecode("04:14:12").unwrap())
        );
        assert_eq!(tracks[1].flags, vec!["PRE"]);

        assert_eq!(tracks[2].pregap_ms, Some(2_000));
    }

    #[test]
    fn resolves_track_boundaries_from_starts() {
        let mut sheet = parse(ALBUM);
        sheet.resolve_boundaries(|_| Some(15 * 60 * 1000));
        let tracks = &sheet.files[0].tracks;

        assert_eq!(tracks[0].end_ms, Some(tracks[1].start_ms));
        assert_eq!(tracks[1].end_ms, Some(tracks[2].start_ms));
        assert_eq!(tracks[2].end_ms, Some(15 * 60 * 1000));
        assert_eq!(
            tracks[2].duration_ms(),
            Some(15 * 60 * 1000 - tracks[2].start_ms)
        );
    }

    #[test]
    fn boundary_resolution_leaves_open_end_without_duration() {
        let mut sheet = parse(ALBUM);
        sheet.resolve_boundaries(|_| None);
        assert_eq!(sheet.files[0].tracks[2].end_ms, None);
    }

    #[test]
    fn parses_timecodes_precisely() {
        assert_eq!(parse_timecode("00:00:00"), Some(0));
        assert_eq!(parse_timecode("01:00:00"), Some(60_000));
        assert_eq!(parse_timecode("00:00:75"), None);
        assert_eq!(parse_timecode("00:60:00"), None);
        assert_eq!(parse_timecode("00:00"), None);
        // 74 CUE frames = 986.66 ms, truncated to 986.
        assert_eq!(parse_timecode("00:00:74"), Some(986));
        // One CD frame is 13.33 ms.
        assert_eq!(parse_timecode("00:00:01"), Some(13));
    }

    #[test]
    fn handles_quoted_names_with_spaces() {
        let text = "FILE \"My Album (Deluxe).wav\" WAVE\nTRACK 01 AUDIO\nINDEX 01 00:00:00\n";
        let sheet = parse(text);
        assert_eq!(sheet.files[0].name, "My Album (Deluxe).wav");
        assert_eq!(
            sheet.files[0].resolved_path,
            PathBuf::from("/music/My Album (Deluxe).wav")
        );
    }

    #[test]
    fn ignores_unknown_commands_and_blank_lines() {
        let text = "\n\n; comment\nREM TOOL \"EAC\"\nFILE \"a.wav\" WAVE\nTRACK 01 AUDIO\nUNKNOWN 1 2 3\nINDEX 01 00:00:00\n";
        let sheet = parse(text);
        assert_eq!(sheet.rem.len(), 1);
        assert_eq!(sheet.rem[0].0, "TOOL");
    }

    #[test]
    fn rejects_structural_errors() {
        let missing_file = "TRACK 01 AUDIO\nINDEX 01 00:00:00\n";
        assert!(CueSheet::parse(missing_file, Path::new("x.cue"), Path::new(".")).is_err());

        let missing_tracks = "FILE \"a.wav\" WAVE\n";
        assert!(CueSheet::parse(missing_tracks, Path::new("x.cue"), Path::new(".")).is_err());

        let duplicate = "FILE \"a.wav\" WAVE\nTRACK 01 AUDIO\nINDEX 01 00:00:00\nTRACK 01 AUDIO\nINDEX 01 00:01:00\n";
        assert!(CueSheet::parse(duplicate, Path::new("x.cue"), Path::new(".")).is_err());

        let bad_mode = "FILE \"a.wav\" WAVE\nTRACK 01 MODE1/2352\nINDEX 01 00:00:00\n";
        assert!(CueSheet::parse(bad_mode, Path::new("x.cue"), Path::new(".")).is_err());

        let index_without_track = "FILE \"a.wav\" WAVE\nINDEX 01 00:00:00\n";
        assert!(CueSheet::parse(index_without_track, Path::new("x.cue"), Path::new(".")).is_err());
    }

    #[test]
    fn accepts_first_track_at_zero_without_false_positive() {
        let text = "FILE \"a.wav\" WAVE\nTRACK 01 AUDIO\nINDEX 01 00:00:00\n";
        let sheet = parse(text);
        assert_eq!(sheet.files[0].tracks[0].start_ms, 0);
    }

    #[test]
    fn parses_multiple_files() {
        let text = "FILE \"cd1.wav\" WAVE\nTRACK 01 AUDIO\nINDEX 01 00:00:00\nTRACK 02 AUDIO\nINDEX 01 03:00:00\nFILE \"cd2.wav\" WAVE\nTRACK 01 AUDIO\nINDEX 01 00:00:00\n";
        let sheet = parse(text);
        assert_eq!(sheet.files.len(), 2);
        assert_eq!(sheet.track_count(), 3);
        assert_eq!(
            sheet.files[1].resolved_path,
            PathBuf::from("/music/cd2.wav")
        );
    }
}
