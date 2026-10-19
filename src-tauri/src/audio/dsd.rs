//! DSM (1-bit) support: DSF and DSDIFF readers with DSD-over-PCM output.
//!
//! Native DSD playback is only possible with specific DACs and drivers. The
//! portable path — and the one used by every USB DAC that accepts DoP — is to
//! pack 16 consecutive DSD bits per channel into the 16 most significant bits
//! of a 24-bit PCM carrier frame, with the `0x05`/`0xFA` markers in the low
//! byte. The carrier rates are 176.4 / 352.8 / 705.6 / 1411.2 kHz.
//!
//! Parsers implemented here:
//!
//! * **DSF** — Sony's DSD Stream File. Block-interleaved channel data,
//!   LSB-first bit order when `bits_per_sample = 1`.
//! * **DSDIFF (DFF)** — Philips' DSDIFF container, `FRM8`/`DSD ` form,
//!   byte-interleaved channel data, MSB-first.
//!
//! Both readers stream from disk; a multi-gigabyte album image never has to
//! be fully resident.

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::audio::AudioError;
use crate::audio::format::{AudioFormat, DopEncoder, DsdRate, SampleFormat};

/// Container of the DSD stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DsdContainer {
    Dsf,
    Dff,
}

impl DsdContainer {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Dsf => "DSF",
            Self::Dff => "DFF",
        }
    }
}

/// Parsed stream properties.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DsdInfo {
    pub container: DsdContainer,
    pub rate: DsdRate,
    pub channels: u16,
    /// DSD bytes per channel in the data chunk.
    pub bytes_per_channel: u64,
    /// Offset of the first audio byte inside the file.
    pub data_offset: u64,
    /// Block size per channel (DSF only; 0 for DFF).
    pub block_size: u32,
    /// Bit order inside each byte.
    pub lsb_first: bool,
}

impl DsdInfo {
    /// Total DoP carrier frames available.
    #[must_use]
    pub fn total_frames(&self) -> u64 {
        self.bytes_per_channel / 2
    }

    /// Duration in milliseconds.
    #[must_use]
    pub fn duration_ms(&self) -> u64 {
        let bits = self.bytes_per_channel.saturating_mul(8);
        bits.saturating_mul(1000) / u64::from(self.rate.bit_rate())
    }

    /// PCM format of the DoP carrier.
    #[must_use]
    pub fn dop_format(&self) -> AudioFormat {
        AudioFormat {
            sample_rate: self.rate.dop_rate(),
            channels: self.channels,
            sample_format: SampleFormat::S24,
        }
    }
}

/// Streaming reader that converts DSD to interleaved DoP carrier samples.
pub struct DsdSource {
    reader: BufReader<File>,
    info: DsdInfo,
    encoder: DopEncoder,
    /// Interleaved DSD bytes chunk for the current read, reused per call.
    channel_bytes: Vec<u8>,
    /// Sample buffer reused by `read_dop`.
    sample_buffer: Vec<f32>,
    /// Per-channel 16-bit payloads for the frame being encoded.
    payloads: Vec<u16>,
    /// Carrier words produced by [`DopEncoder::encode_frame`].
    words: Vec<u32>,
    bytes_consumed: u64,
    pub path: PathBuf,
}

impl std::fmt::Debug for DsdSource {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DsdSource")
            .field("path", &self.path)
            .field("info", &self.info)
            .field("bytes_consumed", &self.bytes_consumed)
            .finish_non_exhaustive()
    }
}

impl DsdSource {
    /// Open a `.dsf` or `.dff` file and parse its headers.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, AudioError> {
        let path = path.as_ref();
        let file = File::open(path).map_err(|error| AudioError::io(path, error))?;
        let mut reader = BufReader::with_capacity(256 * 1024, file);

        let mut magic = [0u8; 4];
        reader
            .read_exact(&mut magic)
            .map_err(|error| AudioError::io(path, error))?;
        reader
            .seek(SeekFrom::Start(0))
            .map_err(|error| AudioError::io(path, error))?;

        let info = match &magic {
            b"DSD " => parse_dsf(&mut reader, path)?,
            b"FRM8" => parse_dff(&mut reader, path)?,
            _ => {
                return Err(AudioError::Dsd(format!(
                    "{} is neither a DSF nor a DSDIFF file",
                    path.display()
                )));
            }
        };

        reader
            .seek(SeekFrom::Start(info.data_offset))
            .map_err(|error| AudioError::io(path, error))?;

        let channels = info.channels as usize;
        let encoder = DopEncoder::new(channels, info.rate);

        Ok(Self {
            reader,
            info,
            encoder,
            channel_bytes: vec![0; channels * 2],
            sample_buffer: Vec::new(),
            payloads: vec![0; channels],
            words: Vec::with_capacity(channels),
            bytes_consumed: 0,
            path: path.to_path_buf(),
        })
    }

    #[must_use]
    pub const fn info(&self) -> &DsdInfo {
        &self.info
    }

    /// DoP carrier frames consumed so far.
    #[must_use]
    pub const fn position_frames(&self) -> u64 {
        self.bytes_consumed / 2
    }

    /// Seek to a DoP frame boundary (16 DSD bits per channel).
    pub fn seek_frames(&mut self, frame: u64) -> Result<(), AudioError> {
        let frame = frame.min(self.info.total_frames());
        let byte_offset = frame * 2;
        let position = self
            .info
            .data_offset
            .saturating_add(self.dsd_offset_for(byte_offset));

        self.reader
            .seek(SeekFrom::Start(position))
            .map_err(|error| AudioError::io(&self.path, error))?;
        self.bytes_consumed = byte_offset;
        self.encoder.reset();
        Ok(())
    }

    /// Map a per-channel byte offset to a file offset, honouring DSF block
    /// interleaving. DFF is byte-interleaved, so the mapping is the identity.
    fn dsd_offset_for(&self, bytes_per_channel: u64) -> u64 {
        if self.info.container != DsdContainer::Dsf || self.info.block_size == 0 {
            return bytes_per_channel * u64::from(self.info.channels);
        }

        let block = u64::from(self.info.block_size);
        let channels = u64::from(self.info.channels);
        let blocks = bytes_per_channel / block;
        let remainder = bytes_per_channel % block;
        // Whole blocks are channels * block bytes; the partial block is read
        // channel by channel, so the target lands in the first channel's slice.
        blocks * channels * block + remainder
    }

    /// Read up to `frames` DoP carrier frames as interleaved `f32`.
    ///
    /// The returned vector holds `frames * channels` samples. Output frame `n`
    /// contains 16 DSD bits per channel and the alternating DoP marker.
    pub fn read_dop(&mut self, frames: usize) -> Result<&[f32], AudioError> {
        if frames == 0 {
            return Ok(&[]);
        }

        let remaining = self
            .info
            .total_frames()
            .saturating_sub(self.bytes_consumed / 2);
        let frames = frames.min(usize::try_from(remaining).unwrap_or(usize::MAX));
        if frames == 0 {
            self.sample_buffer.clear();
            return Ok(&self.sample_buffer);
        }

        let channels = self.info.channels as usize;
        let necessary_bytes = frames * 2;
        self.channel_bytes.resize(channels * necessary_bytes, 0);

        match self.info.container {
            DsdContainer::Dff => {
                // Byte-interleaved: frame, channel, byte.
                let mut interleaved = vec![0u8; necessary_bytes * channels];
                self.reader
                    .read_exact(&mut interleaved)
                    .map_err(|error| AudioError::io(&self.path, error))?;
                for frame in 0..frames {
                    for channel in 0..channels {
                        let base = (frame * channels + channel) * 2;
                        self.channel_bytes[channel * necessary_bytes + frame * 2] =
                            interleaved[base];
                        self.channel_bytes[channel * necessary_bytes + frame * 2 + 1] =
                            interleaved[base + 1];
                    }
                }
            }
            DsdContainer::Dsf => {
                // Block-interleaved: blocks of `block_size` bytes per channel.
                let block = self.info.block_size as usize;
                let mut offset = 0usize;
                while offset < necessary_bytes {
                    let take = (necessary_bytes - offset).min(block);
                    for channel in 0..channels {
                        let mut block_buffer = vec![0u8; block];
                        self.reader
                            .read_exact(&mut block_buffer[..take])
                            .map_err(|error| AudioError::io(&self.path, error))?;
                        // Skip the unused tail of the block so the next
                        // channel starts at the right place.
                        if take < block {
                            self.reader
                                .seek(SeekFrom::Current((block - take) as i64))
                                .map_err(|error| AudioError::io(&self.path, error))?;
                        }
                        self.channel_bytes[channel * necessary_bytes + offset
                            ..channel * necessary_bytes + offset + take]
                            .copy_from_slice(&block_buffer[..take]);
                    }
                    offset += take;
                }
            }
        }

        self.bytes_consumed += necessary_bytes as u64;

        // Build the DoP carrier: two DSD bytes per channel → one 24-bit word.
        // The encoder alternates the marker per *frame* and allocates nothing.
        self.sample_buffer.clear();
        self.sample_buffer.resize(frames * channels, 0.0);
        for frame in 0..frames {
            for channel in 0..channels {
                let base = channel * necessary_bytes + frame * 2;
                let first = self.channel_bytes[base];
                let second = self.channel_bytes[base + 1];
                self.payloads[channel] = if self.info.lsb_first {
                    // DSF stores bits LSB-first; the DoP payload is MSB-first.
                    (u16::from(first.reverse_bits()) << 8) | u16::from(second.reverse_bits())
                } else {
                    (u16::from(first) << 8) | u16::from(second)
                };
            }
            self.encoder.encode_frame(&self.payloads, &mut self.words)?;
            for (channel, &word) in self.words.iter().enumerate() {
                // Sign-extend the 24-bit carrier word and scale by 2²³, which
                // is exact in f32 (24-bit mantissa) and therefore survives the
                // f32 → S24 round trip in the backend bit-for-bit.
                let signed = (word << 8) as i32 >> 8;
                self.sample_buffer[frame * channels + channel] = signed as f32 / 8_388_608.0;
            }
        }

        Ok(&self.sample_buffer)
    }
}

fn read_exact_at<R: Read>(
    reader: &mut R,
    buffer: &mut [u8],
    path: &Path,
) -> Result<(), AudioError> {
    reader
        .read_exact(buffer)
        .map_err(|error| AudioError::io(path, error))
}

fn parse_dsf(reader: &mut BufReader<File>, path: &Path) -> Result<DsdInfo, AudioError> {
    // DSD chunk: id(4) size(8) total_size(8) metadata_ptr(8)
    let mut header = [0u8; 28];
    read_exact_at(reader, &mut header, path)?;
    let chunk_size = u64::from_le_bytes(header[4..12].try_into().unwrap());
    if chunk_size != 28 {
        return Err(AudioError::Dsd(format!(
            "unexpected DSF header chunk size {chunk_size}"
        )));
    }

    let mut fmt = [0u8; 52];
    read_exact_at(reader, &mut fmt, path)?;
    if &fmt[0..4] != b"fmt " {
        return Err(AudioError::Dsd("missing DSF fmt chunk".into()));
    }
    let format_version = u32::from_le_bytes(fmt[12..16].try_into().unwrap());
    let format_id = u32::from_le_bytes(fmt[16..20].try_into().unwrap());
    let channel_num = u32::from_le_bytes(fmt[24..28].try_into().unwrap());
    let sampling_frequency = u32::from_le_bytes(fmt[28..32].try_into().unwrap());
    let bits_per_sample = u32::from_le_bytes(fmt[32..36].try_into().unwrap());
    let sample_count = u64::from_le_bytes(fmt[36..44].try_into().unwrap());
    let block_size = u32::from_le_bytes(fmt[44..48].try_into().unwrap());

    if format_version != 1 {
        return Err(AudioError::Dsd(format!(
            "unsupported DSF format version {format_version}"
        )));
    }
    if format_id != 0 {
        return Err(AudioError::Dsd(format!(
            "unsupported DSF format id {format_id} (expected raw DSD)"
        )));
    }
    if !matches!(bits_per_sample, 1 | 8) {
        return Err(AudioError::Dsd(format!(
            "unsupported DSF bit order {bits_per_sample}"
        )));
    }
    if channel_num == 0 || channel_num > 6 {
        return Err(AudioError::Dsd(format!(
            "unsupported DSF channel count {channel_num}"
        )));
    }
    let rate = DsdRate::from_bit_rate(sampling_frequency)
        .ok_or_else(|| AudioError::Dsd(format!("unsupported DSD rate {sampling_frequency} Hz")))?;

    // data chunk header
    let mut data_header = [0u8; 12];
    read_exact_at(reader, &mut data_header, path)?;
    if &data_header[0..4] != b"data" {
        return Err(AudioError::Dsd("missing DSF data chunk".into()));
    }
    let data_size = u64::from_le_bytes(data_header[4..12].try_into().unwrap());
    let data_bytes = data_size.saturating_sub(12);

    let channels = u64::from(channel_num);
    let bytes_per_channel = if sample_count > 0 {
        // DSF sample_count is per channel and expressed in bits.
        sample_count / 8
    } else {
        data_bytes / channels
    };

    Ok(DsdInfo {
        container: DsdContainer::Dsf,
        rate,
        channels: channel_num as u16,
        bytes_per_channel,
        data_offset: 28 + 52 + 12,
        block_size,
        lsb_first: bits_per_sample == 1,
    })
}

fn parse_dff(reader: &mut BufReader<File>, path: &Path) -> Result<DsdInfo, AudioError> {
    reader
        .seek(SeekFrom::Start(0))
        .map_err(|error| AudioError::io(path, error))?;

    let mut header = [0u8; 16];
    read_exact_at(reader, &mut header, path)?;
    if &header[0..4] != b"FRM8" || &header[12..16] != b"DSD " {
        return Err(AudioError::Dsd("not a DSDIFF file".into()));
    }

    let mut rate = None;
    let mut channels = None;
    let mut data_offset = None;
    let mut data_bytes = 0u64;

    let mut position = 16u64;
    let file_len = reader
        .get_ref()
        .metadata()
        .map_err(|error| AudioError::io(path, error))?
        .len();

    while position + 12 <= file_len {
        reader
            .seek(SeekFrom::Start(position))
            .map_err(|error| AudioError::io(path, error))?;
        let mut chunk_header = [0u8; 12];
        read_exact_at(reader, &mut chunk_header, path)?;
        let id = &chunk_header[0..4];
        let size = u64::from_be_bytes(chunk_header[4..12].try_into().unwrap());
        let body_start = position + 12;

        match id {
            b"FVER" => {
                // Version chunk, validated for completeness.
                let mut version = [0u8; 4];
                read_exact_at(reader, &mut version, path)?;
                let version = u32::from_be_bytes(version);
                if version != 0x0105_0000 {
                    return Err(AudioError::Dsd(format!(
                        "unsupported DSDIFF version {version:#010x}"
                    )));
                }
            }
            b"PROP" => {
                let mut prop_type = [0u8; 4];
                read_exact_at(reader, &mut prop_type, path)?;
                if &prop_type != b"SND " {
                    position = body_start + size + (size % 2);
                    continue;
                }
                let mut cursor = body_start + 4;
                let prop_end = body_start + size;
                while cursor + 12 <= prop_end {
                    reader
                        .seek(SeekFrom::Start(cursor))
                        .map_err(|error| AudioError::io(path, error))?;
                    let mut sub = [0u8; 12];
                    read_exact_at(reader, &mut sub, path)?;
                    let sub_id = &sub[0..4];
                    let sub_size = u64::from_be_bytes(sub[4..12].try_into().unwrap());
                    let sub_body = cursor + 12;

                    match sub_id {
                        b"FS  " => {
                            let mut bytes = [0u8; 4];
                            read_exact_at(reader, &mut bytes, path)?;
                            rate = Some(u32::from_be_bytes(bytes));
                        }
                        b"CHNL" => {
                            let mut bytes = [0u8; 2];
                            read_exact_at(reader, &mut bytes, path)?;
                            channels = Some(u16::from_be_bytes(bytes));
                        }
                        _ => {}
                    }
                    cursor = sub_body + sub_size + (sub_size % 2);
                }
            }
            b"DSD " => {
                data_offset = Some(body_start);
                data_bytes = size;
                // Audio data is the last chunk of consequence.
                break;
            }
            _ => {}
        }

        position = body_start + size + (size % 2);
    }

    let rate = rate.ok_or_else(|| AudioError::Dsd("DSDIFF has no FS chunk".into()))?;
    let rate = DsdRate::from_bit_rate(rate)
        .ok_or_else(|| AudioError::Dsd(format!("unsupported DSD rate {rate} Hz")))?;
    let channels = channels.ok_or_else(|| AudioError::Dsd("DSDIFF has no CHNL chunk".into()))?;
    let data_offset =
        data_offset.ok_or_else(|| AudioError::Dsd("DSDIFF has no DSD data chunk".into()))?;

    Ok(DsdInfo {
        container: DsdContainer::Dff,
        rate,
        channels,
        bytes_per_channel: data_bytes / u64::from(channels),
        data_offset,
        block_size: 0,
        lsb_first: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::format::{DOP_MARKER_EVEN, DOP_MARKER_ODD};
    use std::io::Write;

    /// Build a minimal but structurally valid DSF file with 2 channels and a
    /// caller-provided payload (block size 8 bytes per channel).
    fn write_dsf(path: &Path, payload: &[u8], bits_per_sample: u32) {
        let channels = 2u32;
        let block_size = 8u32;
        let sample_count = (payload.len() as u64 / u64::from(channels)) * 8;

        let mut file = File::create(path).expect("create dsf");

        // DSD chunk
        file.write_all(b"DSD ").unwrap();
        file.write_all(&28u64.to_le_bytes()).unwrap();
        file.write_all(&0u64.to_le_bytes()).unwrap(); // total size (unchecked)
        file.write_all(&0u64.to_le_bytes()).unwrap(); // metadata pointer

        // fmt chunk
        file.write_all(b"fmt ").unwrap();
        file.write_all(&52u64.to_le_bytes()).unwrap();
        file.write_all(&1u32.to_le_bytes()).unwrap(); // version
        file.write_all(&0u32.to_le_bytes()).unwrap(); // format id
        file.write_all(&2u32.to_le_bytes()).unwrap(); // channel type
        file.write_all(&channels.to_le_bytes()).unwrap();
        file.write_all(&2_822_400u32.to_le_bytes()).unwrap();
        file.write_all(&bits_per_sample.to_le_bytes()).unwrap();
        file.write_all(&sample_count.to_le_bytes()).unwrap();
        file.write_all(&block_size.to_le_bytes()).unwrap();
        file.write_all(&0u32.to_le_bytes()).unwrap(); // reserved

        // data chunk
        file.write_all(b"data").unwrap();
        file.write_all(&(payload.len() as u64 + 12).to_le_bytes())
            .unwrap();
        file.write_all(payload).unwrap();
    }

    fn write_dff(path: &Path, payload: &[u8]) {
        let mut body = Vec::new();

        // FVER
        body.extend_from_slice(b"FVER");
        body.extend_from_slice(&4u64.to_be_bytes());
        body.extend_from_slice(&0x0105_0000u32.to_be_bytes());

        // PROP/SND with FS and CHNL
        let mut prop = Vec::new();
        prop.extend_from_slice(b"SND ");
        prop.extend_from_slice(b"FS  ");
        prop.extend_from_slice(&4u64.to_be_bytes());
        prop.extend_from_slice(&2_822_400u32.to_be_bytes());
        prop.extend_from_slice(b"CHNL");
        prop.extend_from_slice(&6u64.to_be_bytes());
        prop.extend_from_slice(&2u16.to_be_bytes());
        prop.extend_from_slice(b"SLFTSRGT");
        body.extend_from_slice(b"PROP");
        body.extend_from_slice(&(prop.len() as u64).to_be_bytes());
        body.extend_from_slice(&prop);

        // DSD data
        body.extend_from_slice(b"DSD ");
        body.extend_from_slice(&(payload.len() as u64).to_be_bytes());
        body.extend_from_slice(payload);

        let mut file = File::create(path).expect("create dff");
        file.write_all(b"FRM8").unwrap();
        file.write_all(&((body.len() + 4) as u64).to_be_bytes())
            .unwrap();
        file.write_all(b"DSD ").unwrap();
        file.write_all(&body).unwrap();
    }

    #[test]
    fn parses_dsf_header_and_reports_carrier_format() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("album.dsf");
        write_dsf(&path, &[0u8; 32], 8);

        let source = DsdSource::open(&path).expect("open dsf");
        let info = source.info();
        assert_eq!(info.container, DsdContainer::Dsf);
        assert_eq!(info.channels, 2);
        assert_eq!(info.rate, DsdRate::Dsd64);
        assert_eq!(info.bytes_per_channel, 16);
        assert_eq!(info.total_frames(), 8);
        assert_eq!(info.dop_format().sample_rate, 176_400);
        assert_eq!(info.dop_format().sample_format, SampleFormat::S24);
    }

    #[test]
    fn reads_dsf_payload_into_dop_frames() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("album.dsf");
        // 2 blocks of 8 bytes per channel, MSB-first (bits_per_sample = 8).
        let mut payload = Vec::new();
        payload.extend(std::iter::repeat_n(0xAAu8, 8)); // left block 1
        payload.extend(std::iter::repeat_n(0x55u8, 8)); // right block 1
        payload.extend(std::iter::repeat_n(0xAAu8, 8)); // left block 2
        payload.extend(std::iter::repeat_n(0x55u8, 8)); // right block 2
        write_dsf(&path, &payload, 8);

        let mut source = DsdSource::open(&path).expect("open");
        let samples = source.read_dop(4).expect("read").to_vec();
        assert_eq!(samples.len(), 4 * 2);

        // Frame 0 must carry 0xAAAA on the left and 0x5555 on the right,
        // with the 0x05 marker in the low byte of the 24-bit word.
        let left = (samples[0] * 8_388_608.0).round() as i32;
        let right = (samples[1] * 8_388_608.0).round() as i32;
        assert_eq!((left as u32) & 0xFF, u32::from(DOP_MARKER_EVEN));
        assert_eq!(((left as u32) >> 8) & 0xFFFF, 0xAAAA);
        assert_eq!(((right as u32) >> 8) & 0xFFFF, 0x5555);
    }

    #[test]
    fn dop_markers_alternate_across_frames() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("album.dsf");
        write_dsf(&path, &[0x00u8; 64], 8);

        let mut source = DsdSource::open(&path).expect("open");
        let samples = source.read_dop(4).expect("read").to_vec();
        let markers: Vec<u8> = (0..4)
            .map(|frame| ((samples[frame * 2] * 8_388_608.0).round() as i32 as u32 & 0xFF) as u8)
            .collect();
        assert_eq!(
            markers,
            vec![
                DOP_MARKER_EVEN,
                DOP_MARKER_ODD,
                DOP_MARKER_EVEN,
                DOP_MARKER_ODD
            ]
        );
    }

    #[test]
    fn parses_dff_header_and_data() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("album.dff");
        write_dff(&path, &[0x12u8; 40]);

        let mut source = DsdSource::open(&path).expect("open dff");
        assert_eq!(source.info().container, DsdContainer::Dff);
        assert_eq!(source.info().channels, 2);
        assert_eq!(source.info().bytes_per_channel, 20);
        assert_eq!(source.info().total_frames(), 10);

        let samples = source.read_dop(2).expect("read").to_vec();
        assert_eq!(samples.len(), 4);
        // Data bytes are 0x12 for both channels; marker alternates.
        let marker_frame0 = (samples[0] * 8_388_608.0).round() as i32 as u32 & 0xFF;
        assert_eq!(marker_frame0 as u8, DOP_MARKER_EVEN);
    }

    #[test]
    fn rejects_files_that_are_not_dsd() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("not.dsf");
        std::fs::write(&path, b"RIFFxxxxWAVE").expect("write");
        assert!(DsdSource::open(&path).is_err());
    }

    #[test]
    fn rejects_unsupported_rates() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("weird.dsf");

        let mut file = File::create(&path).expect("create");
        file.write_all(b"DSD ").unwrap();
        file.write_all(&28u64.to_le_bytes()).unwrap();
        file.write_all(&0u64.to_le_bytes()).unwrap();
        file.write_all(&0u64.to_le_bytes()).unwrap();
        file.write_all(b"fmt ").unwrap();
        file.write_all(&52u64.to_le_bytes()).unwrap();
        file.write_all(&1u32.to_le_bytes()).unwrap();
        file.write_all(&0u32.to_le_bytes()).unwrap();
        file.write_all(&2u32.to_le_bytes()).unwrap();
        file.write_all(&2u32.to_le_bytes()).unwrap();
        file.write_all(&44_100u32.to_le_bytes()).unwrap(); // not DSD
        file.write_all(&1u32.to_le_bytes()).unwrap();
        file.write_all(&8u64.to_le_bytes()).unwrap();
        file.write_all(&8u32.to_le_bytes()).unwrap();
        file.write_all(&0u32.to_le_bytes()).unwrap();

        assert!(DsdSource::open(&path).is_err());
    }
}
