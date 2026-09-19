use crate::dicom::types::DicomMetadata;
use chrono::NaiveDateTime;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

#[derive(Debug)]
pub enum DicomHeaderError {
    Io(io::Error),
    InvalidMagic,
    PrematureEof,
    MissingPixelData,
    CorruptTag(String),
}

impl std::fmt::Display for DicomHeaderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DicomHeaderError::Io(e) => write!(f, "I/O error reading DICOM: {e}"),
            DicomHeaderError::InvalidMagic => write!(f, "Missing DICM prefix at offset 128"),
            DicomHeaderError::PrematureEof => write!(f, "Unexpected end of file reading DICOM header"),
            DicomHeaderError::MissingPixelData => write!(f, "PixelData tag (7FE0, 0010) not found"),
            DicomHeaderError::CorruptTag(msg) => write!(f, "Corrupted tag in DICOM header: {msg}"),
        }
    }
}

impl std::error::Error for DicomHeaderError {}

impl From<io::Error> for DicomHeaderError {
    fn from(e: io::Error) -> Self {
        DicomHeaderError::Io(e)
    }
}

/// Helper to parse datetime strings from DICOM DT tag (e.g., "20260315223419.000000" or "20260315223419")
fn parse_dicom_datetime(s: &str) -> Option<NaiveDateTime> {
    let trimmed = s.trim().trim_matches('\0').trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(trimmed, "%Y%m%d%H%M%S%.f") {
        return Some(dt);
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(trimmed, "%Y%m%d%H%M%S") {
        return Some(dt);
    }
    None
}

/// Helper to parse fallback DA (YYYYMMDD) and TM (HHMMSS.ffffff) tags
fn parse_dicom_da_tm(da: &str, tm: &str) -> Option<NaiveDateTime> {
    let da_clean = da.trim().trim_matches('\0').trim();
    let tm_clean = tm.trim().trim_matches('\0').trim();
    if da_clean.len() < 8 {
        return None;
    }
    let full = format!("{da_clean}{tm_clean}");
    parse_dicom_datetime(&full)
}

/// Scans forward in the file to find the PixelData tag (7FE0, 0010): \xe0\x7f\x10\x00
fn scan_for_pixel_data(file: &mut File) -> io::Result<(u64, u64)> {
    let file_len = file.metadata()?.len();
    let start_pos = file.stream_position()?;

    // Read in 64KB chunks to locate \xe0\x7f\x10\x00
    let mut chunk = [0u8; 65536];
    let mut search_pos = start_pos;

    while search_pos < file_len {
        file.seek(SeekFrom::Start(search_pos))?;
        let bytes_read = file.read(&mut chunk)?;
        if bytes_read < 4 {
            break;
        }

        let slice = &chunk[..bytes_read];
        for i in 0..=(bytes_read - 4) {
            if slice[i] == 0xE0 && slice[i + 1] == 0x7F && slice[i + 2] == 0x10 && slice[i + 3] == 0x00 {
                let tag_offset = search_pos + i as u64;
                file.seek(SeekFrom::Start(tag_offset + 4))?;

                // Read VR
                let mut vr_buf = [0u8; 2];
                file.read_exact(&mut vr_buf)?;
                let (offset, length) = if matches!(&vr_buf, b"OB" | b"OW" | b"UN") {
                    let mut reserved = [0u8; 2];
                    file.read_exact(&mut reserved)?;
                    let mut len_buf = [0u8; 4];
                    file.read_exact(&mut len_buf)?;
                    let len = u32::from_le_bytes(len_buf) as u64;
                    let off = file.stream_position()?;
                    (off, len)
                } else {
                    // Implicit VR or 4-byte length
                    let mut len_buf = [0u8; 4];
                    file.seek(SeekFrom::Start(tag_offset + 4))?;
                    file.read_exact(&mut len_buf)?;
                    let len = u32::from_le_bytes(len_buf) as u64;
                    let off = file.stream_position()?;
                    (off, len)
                };

                let effective_len = if length == 0 || length == 0xFFFFFFFF || offset + length > file_len {
                    file_len.saturating_sub(offset)
                } else {
                    length
                };

                return Ok((offset, effective_len));
            }
        }

        // Overlap by 3 bytes so we don't miss tags straddling chunk boundary
        search_pos += (bytes_read.saturating_sub(3)) as u64;
    }

    Err(io::Error::new(io::ErrorKind::NotFound, "PixelData tag (7FE0, 0010) not found"))
}

/// Reads a DICOM Part 10 header up to the PixelData tag (7FE0, 0010)
pub fn read_dicom_header(path: &Path) -> Result<DicomMetadata, DicomHeaderError> {
    let mut file = File::open(path)?;

    // 1. Check preamble (128 bytes) + DICM magic prefix (4 bytes)
    let mut prefix_buf = [0u8; 132];
    file.read_exact(&mut prefix_buf).map_err(|_| DicomHeaderError::PrematureEof)?;

    if &prefix_buf[128..132] != b"DICM" {
        return Err(DicomHeaderError::InvalidMagic);
    }

    let mut is_explicit_vr = true;
    let mut patient_id: Option<String> = None;
    let mut other_patient_ids: Option<String> = None;
    let mut acquisition_datetime: Option<NaiveDateTime> = None;
    let mut study_date: Option<String> = None;
    let mut study_time: Option<String> = None;
    let mut number_of_frames: usize = 1;
    let mut rows: u16 = 0;
    let mut columns: u16 = 0;
    let mut bits_allocated: u16 = 16;
    let mut samples_per_pixel: u16 = 1;
    let mut photometric_interpretation: Option<String> = None;
    let mut planar_configuration: u16 = 0;
    let mut pixel_data_offset: Option<u64> = None;
    let mut pixel_data_length: Option<u64> = None;

    let mut tag_buf = [0u8; 4];
    while file.read_exact(&mut tag_buf).is_ok() {
        let group = u16::from_le_bytes([tag_buf[0], tag_buf[1]]);
        let element = u16::from_le_bytes([tag_buf[2], tag_buf[3]]);

        let (_vr, length) = if group == 0x0002 {
            let mut vr_buf = [0u8; 2];
            file.read_exact(&mut vr_buf)?;
            let vr = vr_buf;
            if matches!(&vr, b"OB" | b"OW" | b"OF" | b"SQ" | b"UC" | b"UR" | b"UT" | b"UN") {
                let mut reserved = [0u8; 2];
                file.read_exact(&mut reserved)?;
                let mut len_buf = [0u8; 4];
                file.read_exact(&mut len_buf)?;
                (vr, u32::from_le_bytes(len_buf) as u64)
            } else {
                let mut len_buf = [0u8; 2];
                file.read_exact(&mut len_buf)?;
                (vr, u16::from_le_bytes(len_buf) as u64)
            }
        } else if is_explicit_vr {
            let mut vr_buf = [0u8; 2];
            file.read_exact(&mut vr_buf)?;
            let vr = vr_buf;
            if matches!(&vr, b"OB" | b"OW" | b"OF" | b"SQ" | b"UC" | b"UR" | b"UT" | b"UN") {
                let mut reserved = [0u8; 2];
                file.read_exact(&mut reserved)?;
                let mut len_buf = [0u8; 4];
                file.read_exact(&mut len_buf)?;
                (vr, u32::from_le_bytes(len_buf) as u64)
            } else if vr[0].is_ascii_uppercase() && vr[1].is_ascii_uppercase() {
                let mut len_buf = [0u8; 2];
                file.read_exact(&mut len_buf)?;
                (vr, u16::from_le_bytes(len_buf) as u64)
            } else {
                is_explicit_vr = false;
                let mut len_buf = [0u8; 2];
                file.read_exact(&mut len_buf)?;
                let full_len_bytes = [vr[0], vr[1], len_buf[0], len_buf[1]];
                (b"UN".to_owned(), u32::from_le_bytes(full_len_bytes) as u64)
            }
        } else {
            let mut len_buf = [0u8; 4];
            file.read_exact(&mut len_buf)?;
            (b"UN".to_owned(), u32::from_le_bytes(len_buf) as u64)
        };

        // TransferSyntaxUID check
        if group == 0x0002 && element == 0x0010 {
            let mut val_buf = vec![0u8; length as usize];
            file.read_exact(&mut val_buf)?;
            let ts = String::from_utf8_lossy(&val_buf)
                .trim_matches('\0')
                .trim()
                .to_string();
            if ts == "1.2.840.10008.1.2" {
                is_explicit_vr = false;
            }
            continue;
        }

        // Direct hit on PixelData
        if group == 0x7FE0 && element == 0x0010 {
            pixel_data_offset = Some(file.stream_position()?);
            pixel_data_length = Some(length);
            break;
        }

        // If we hit an undefined length sequence, scan directly for PixelData tag
        if length == 0xFFFFFFFF {
            let (off, len) = scan_for_pixel_data(&mut file).map_err(|_| DicomHeaderError::MissingPixelData)?;
            pixel_data_offset = Some(off);
            pixel_data_length = Some(len);
            break;
        }

        match (group, element) {
            (0x0010, 0x0020) => {
                let mut buf = vec![0u8; length as usize];
                file.read_exact(&mut buf)?;
                let s = String::from_utf8_lossy(&buf)
                    .trim_matches('\0')
                    .trim()
                    .to_string();
                if !s.is_empty() {
                    patient_id = Some(s);
                }
            }
            (0x0010, 0x1000) => {
                let mut buf = vec![0u8; length as usize];
                file.read_exact(&mut buf)?;
                let s = String::from_utf8_lossy(&buf)
                    .trim_matches('\0')
                    .trim()
                    .to_string();
                if !s.is_empty() {
                    other_patient_ids = Some(s);
                }
            }
            (0x0008, 0x002A) => {
                let mut buf = vec![0u8; length as usize];
                file.read_exact(&mut buf)?;
                let s = String::from_utf8_lossy(&buf);
                acquisition_datetime = parse_dicom_datetime(&s);
            }
            (0x0008, 0x0020) => {
                let mut buf = vec![0u8; length as usize];
                file.read_exact(&mut buf)?;
                study_date = Some(
                    String::from_utf8_lossy(&buf)
                        .trim_matches('\0')
                        .trim()
                        .to_string(),
                );
            }
            (0x0008, 0x0030) => {
                let mut buf = vec![0u8; length as usize];
                file.read_exact(&mut buf)?;
                study_time = Some(
                    String::from_utf8_lossy(&buf)
                        .trim_matches('\0')
                        .trim()
                        .to_string(),
                );
            }
            (0x0028, 0x0002) => {
                let mut buf = vec![0u8; length as usize];
                file.read_exact(&mut buf)?;
                if buf.len() >= 2 {
                    samples_per_pixel = u16::from_le_bytes([buf[0], buf[1]]);
                }
            }
            (0x0028, 0x0004) => {
                let mut buf = vec![0u8; length as usize];
                file.read_exact(&mut buf)?;
                let s = String::from_utf8_lossy(&buf)
                    .trim_matches('\0')
                    .trim()
                    .to_string();
                if !s.is_empty() {
                    photometric_interpretation = Some(s);
                }
            }
            (0x0028, 0x0006) => {
                let mut buf = vec![0u8; length as usize];
                file.read_exact(&mut buf)?;
                if buf.len() >= 2 {
                    planar_configuration = u16::from_le_bytes([buf[0], buf[1]]);
                }
            }
            (0x0028, 0x0008) => {
                let mut buf = vec![0u8; length as usize];
                file.read_exact(&mut buf)?;
                let s = String::from_utf8_lossy(&buf)
                    .trim_matches('\0')
                    .trim()
                    .to_string();
                if let Ok(n) = s.parse::<usize>() {
                    if n > 0 {
                        number_of_frames = n;
                    }
                }
            }
            (0x0028, 0x0010) => {
                let mut buf = vec![0u8; length as usize];
                file.read_exact(&mut buf)?;
                if buf.len() >= 2 {
                    rows = u16::from_le_bytes([buf[0], buf[1]]);
                }
            }
            (0x0028, 0x0011) => {
                let mut buf = vec![0u8; length as usize];
                file.read_exact(&mut buf)?;
                if buf.len() >= 2 {
                    columns = u16::from_le_bytes([buf[0], buf[1]]);
                }
            }
            (0x0028, 0x0100) => {
                let mut buf = vec![0u8; length as usize];
                file.read_exact(&mut buf)?;
                if buf.len() >= 2 {
                    bits_allocated = u16::from_le_bytes([buf[0], buf[1]]);
                }
            }
            _ => {
                file.seek(SeekFrom::Current(length as i64))?;
            }
        }
    }

    // If loop finished without finding pixel_data_offset, try scanning for it
    if pixel_data_offset.is_none() {
        let (off, len) = scan_for_pixel_data(&mut file).map_err(|_| DicomHeaderError::MissingPixelData)?;
        pixel_data_offset = Some(off);
        pixel_data_length = Some(len);
    }

    let pixel_data_offset = pixel_data_offset.ok_or(DicomHeaderError::MissingPixelData)?;
    let pixel_data_length = pixel_data_length.unwrap_or(0);

    if acquisition_datetime.is_none() {
        if let (Some(ref d), Some(ref t)) = (&study_date, &study_time) {
            acquisition_datetime = parse_dicom_da_tm(d, t);
        }
    }

    Ok(DicomMetadata {
        patient_id,
        other_patient_ids,
        acquisition_datetime,
        number_of_frames,
        rows,
        columns,
        bits_allocated,
        samples_per_pixel,
        photometric_interpretation,
        planar_configuration,
        pixel_data_offset,
        pixel_data_length,
    })
}
