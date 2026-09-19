#![allow(dead_code)]

use sha1::{Digest, Sha1};
use std::fs::File;
use std::io::{self, Write};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct SyntheticDicomOptions {
    pub patient_id: Option<String>,
    pub other_patient_ids: Option<String>,
    pub acquisition_date_time: Option<String>,
    pub number_of_frames: usize,
    pub rows: u16,
    pub columns: u16,
    pub bits_allocated: u16,
    pub pixel_fill_byte: Option<u8>,
}

impl Default for SyntheticDicomOptions {
    fn default() -> Self {
        Self {
            patient_id: Some("SYNTH_P001".to_string()),
            other_patient_ids: None,
            acquisition_date_time: Some("20230515123045".to_string()),
            number_of_frames: 1,
            rows: 64,
            columns: 64,
            bits_allocated: 16,
            pixel_fill_byte: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SyntheticDicomInfo {
    pub middle_layer_hash: String,
    pub total_pixel_bytes: usize,
    pub frame_size_bytes: usize,
    pub target_frame_index: usize,
    pub patient_id: Option<String>,
    pub other_patient_ids: Option<String>,
    pub acquisition_date_time: Option<String>,
    pub number_of_frames: usize,
}

/// Helper to write an Explicit VR element with standard 2-byte length
fn write_explicit_element(
    w: &mut impl Write,
    group: u16,
    element: u16,
    vr: &[u8; 2],
    data: &[u8],
) -> io::Result<()> {
    w.write_all(&group.to_le_bytes())?;
    w.write_all(&element.to_le_bytes())?;
    w.write_all(vr)?;

    let mut len = data.len();
    let pad = len % 2 != 0;
    if pad {
        len += 1;
    }
    w.write_all(&(len as u16).to_le_bytes())?;
    w.write_all(data)?;
    if pad {
        let pad_byte = match vr {
            b"UI" | b"OB" | b"OW" => 0x00,
            _ => 0x20, // space padding for text
        };
        w.write_all(&[pad_byte])?;
    }
    Ok(())
}

/// Helper to write an Explicit VR element with 4-byte length (e.g. OB, OW)
fn write_explicit_long_element(
    w: &mut impl Write,
    group: u16,
    element: u16,
    vr: &[u8; 2],
    data: &[u8],
) -> io::Result<()> {
    w.write_all(&group.to_le_bytes())?;
    w.write_all(&element.to_le_bytes())?;
    w.write_all(vr)?;
    w.write_all(&[0x00, 0x00])?; // reserved 2 bytes

    let mut len = data.len();
    let pad = len % 2 != 0;
    if pad {
        len += 1;
    }
    w.write_all(&(len as u32).to_le_bytes())?;
    w.write_all(data)?;
    if pad {
        w.write_all(&[0x00])?;
    }
    Ok(())
}

/// Generates a valid DICOM Part 10 synthetic file with zero PHI.
pub fn generate_synthetic_dicom(
    path: &Path,
    options: &SyntheticDicomOptions,
) -> io::Result<SyntheticDicomInfo> {
    let mut file = File::create(path)?;

    // 1. 128 bytes preamble
    let preamble = [0u8; 128];
    file.write_all(&preamble)?;

    // 2. DICM magic prefix
    file.write_all(b"DICM")?;

    // 3. File Meta Information (Group 0002)
    // TransferSyntaxUID = 1.2.840.10008.1.2.1 (Explicit VR Little Endian)
    let transfer_syntax = b"1.2.840.10008.1.2.1";
    write_explicit_element(&mut file, 0x0002, 0x0010, b"UI", transfer_syntax)?;

    // MediaStorageSOPClassUID (Secondary Capture Image Storage)
    write_explicit_element(
        &mut file,
        0x0002,
        0x0002,
        b"UI",
        b"1.2.840.10008.5.1.4.1.1.7",
    )?;

    // MediaStorageSOPInstanceUID
    write_explicit_element(
        &mut file,
        0x0002,
        0x0003,
        b"UI",
        b"1.2.826.0.1.3680043.8.498.12345",
    )?;

    // 4. Main Dataset Elements (in ascending tag order)
    // (0008, 002A) AcquisitionDateTime
    if let Some(ref dt) = options.acquisition_date_time {
        write_explicit_element(&mut file, 0x0008, 0x002A, b"DT", dt.as_bytes())?;
    }

    // (0010, 0020) PatientID
    if let Some(ref pid) = options.patient_id {
        write_explicit_element(&mut file, 0x0010, 0x0020, b"LO", pid.as_bytes())?;
    }

    // (0010, 1000) OtherPatientIDs
    if let Some(ref opid) = options.other_patient_ids {
        write_explicit_element(&mut file, 0x0010, 0x1000, b"LO", opid.as_bytes())?;
    }

    // (0028, 0008) NumberOfFrames
    let frames_str = options.number_of_frames.to_string();
    write_explicit_element(&mut file, 0x0028, 0x0008, b"IS", frames_str.as_bytes())?;

    // (0028, 0010) Rows
    write_explicit_element(
        &mut file,
        0x0028,
        0x0010,
        b"US",
        &options.rows.to_le_bytes(),
    )?;

    // (0028, 0011) Columns
    write_explicit_element(
        &mut file,
        0x0028,
        0x0011,
        b"US",
        &options.columns.to_le_bytes(),
    )?;

    // (0028, 0100) BitsAllocated
    write_explicit_element(
        &mut file,
        0x0028,
        0x0100,
        b"US",
        &options.bits_allocated.to_le_bytes(),
    )?;

    // (0028, 0101) BitsStored
    write_explicit_element(
        &mut file,
        0x0028,
        0x0101,
        b"US",
        &options.bits_allocated.to_le_bytes(),
    )?;

    // (0028, 0102) HighBit
    let high_bit = options.bits_allocated - 1;
    write_explicit_element(&mut file, 0x0028, 0x0102, b"US", &high_bit.to_le_bytes())?;

    // (0028, 0103) PixelRepresentation
    write_explicit_element(&mut file, 0x0028, 0x0103, b"US", &0u16.to_le_bytes())?;

    // Calculate pixel payload size
    let bytes_per_pixel = (options.bits_allocated as usize) / 8;
    let frame_size = (options.rows as usize) * (options.columns as usize) * bytes_per_pixel;
    let total_pixel_bytes = frame_size * options.number_of_frames;

    // Generate deterministic pixel bytes
    let mut pixel_data = Vec::with_capacity(total_pixel_bytes);
    for frame_idx in 0..options.number_of_frames {
        for pixel_idx in 0..frame_size {
            let byte = match options.pixel_fill_byte {
                Some(b) => b,
                None => ((frame_idx * 17 + pixel_idx) % 256) as u8,
            };
            pixel_data.push(byte);
        }
    }

    // Determine middle frame index: floor(N / 2)
    let target_frame_idx = options.number_of_frames / 2;
    let target_frame_start = target_frame_idx * frame_size;
    let target_frame_end = target_frame_start + frame_size;
    let target_frame_bytes = &pixel_data[target_frame_start..target_frame_end];

    // Compute SHA-1 of target frame
    let mut hasher = Sha1::new();
    hasher.update(target_frame_bytes);
    let hash_result = hasher.finalize();
    let middle_layer_hash = format!("{:040x}", hash_result);

    // (7FE0, 0010) PixelData (OW with 4-byte length)
    write_explicit_long_element(&mut file, 0x7FE0, 0x0010, b"OW", &pixel_data)?;

    Ok(SyntheticDicomInfo {
        middle_layer_hash,
        total_pixel_bytes,
        frame_size_bytes: frame_size,
        target_frame_index: target_frame_idx,
        patient_id: options.patient_id.clone(),
        other_patient_ids: options.other_patient_ids.clone(),
        acquisition_date_time: options.acquisition_date_time.clone(),
        number_of_frames: options.number_of_frames,
    })
}

/// Generates a corrupt DICOM file (truncated header or invalid magic)
pub fn generate_corrupt_dicom(path: &Path, corrupt_magic: bool) -> io::Result<()> {
    let mut file = File::create(path)?;
    let preamble = [0u8; 128];
    file.write_all(&preamble)?;
    if corrupt_magic {
        file.write_all(b"NOPE")?;
    } else {
        file.write_all(b"DICM")?;
        // Truncate immediately
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct SyntheticRgbOptions {
    pub patient_id: Option<String>,
    pub other_patient_ids: Option<String>,
    pub acquisition_date_time: Option<String>,
    pub rows: u16,
    pub columns: u16,
    pub custom_pad_byte: Option<u8>,
}

impl Default for SyntheticRgbOptions {
    fn default() -> Self {
        Self {
            patient_id: Some("SYNTH_RGB_01".to_string()),
            other_patient_ids: None,
            acquisition_date_time: Some("20230515123045".to_string()),
            rows: 65, // 65 * 65 * 3 = 12675 (odd, triggers pad byte)
            columns: 65,
            custom_pad_byte: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SyntheticRgbInfo {
    pub bgr_swapped_hash: String,
    pub total_pixel_bytes_with_pad: usize,
    pub patient_id: Option<String>,
    pub pad_byte: Option<u8>,
}

/// Generates a valid single-layer 8-bit RGB DICOM Part 10 file with zero PHI.
pub fn generate_synthetic_rgb_dicom(
    path: &Path,
    options: &SyntheticRgbOptions,
) -> io::Result<SyntheticRgbInfo> {
    let mut file = File::create(path)?;

    // 1. 128 bytes preamble + DICM magic
    file.write_all(&[0u8; 128])?;
    file.write_all(b"DICM")?;

    // 2. File Meta Information
    write_explicit_element(&mut file, 0x0002, 0x0010, b"UI", b"1.2.840.10008.1.2.1")?; // Explicit VR Little Endian
    write_explicit_element(&mut file, 0x0002, 0x0002, b"UI", b"1.2.840.10008.5.1.4.1.1.77.1.4")?; // VL Photographic
    write_explicit_element(&mut file, 0x0002, 0x0003, b"UI", b"1.2.826.0.1.3680043.8.498.99999")?;

    // 3. Dataset elements
    if let Some(ref dt) = options.acquisition_date_time {
        write_explicit_element(&mut file, 0x0008, 0x002A, b"DT", dt.as_bytes())?;
    }
    if let Some(ref pid) = options.patient_id {
        write_explicit_element(&mut file, 0x0010, 0x0020, b"LO", pid.as_bytes())?;
    }
    if let Some(ref opid) = options.other_patient_ids {
        write_explicit_element(&mut file, 0x0010, 0x1000, b"LO", opid.as_bytes())?;
    }

    // (0028, 0002) SamplesPerPixel = 3
    write_explicit_element(&mut file, 0x0028, 0x0002, b"US", &3u16.to_le_bytes())?;
    // (0028, 0004) PhotometricInterpretation = RGB
    write_explicit_element(&mut file, 0x0028, 0x0004, b"CS", b"RGB")?;
    // (0028, 0006) PlanarConfiguration = 0
    write_explicit_element(&mut file, 0x0028, 0x0006, b"US", &0u16.to_le_bytes())?;
    // (0028, 0008) NumberOfFrames = 1
    write_explicit_element(&mut file, 0x0028, 0x0008, b"IS", b"1")?;
    // (0028, 0010) Rows
    write_explicit_element(&mut file, 0x0028, 0x0010, b"US", &options.rows.to_le_bytes())?;
    // (0028, 0011) Columns
    write_explicit_element(&mut file, 0x0028, 0x0011, b"US", &options.columns.to_le_bytes())?;
    // (0028, 0100) BitsAllocated = 8
    write_explicit_element(&mut file, 0x0028, 0x0100, b"US", &8u16.to_le_bytes())?;
    // (0028, 0101) BitsStored = 8
    write_explicit_element(&mut file, 0x0028, 0x0101, b"US", &8u16.to_le_bytes())?;
    // (0028, 0102) HighBit = 7
    write_explicit_element(&mut file, 0x0028, 0x0102, b"US", &7u16.to_le_bytes())?;
    // (0028, 0103) PixelRepresentation = 0
    write_explicit_element(&mut file, 0x0028, 0x0103, b"US", &0u16.to_le_bytes())?;

    // Generate raw pixel payload
    let raw_pixel_len = (options.rows as usize) * (options.columns as usize) * 3;
    let mut raw_pixels = Vec::with_capacity(raw_pixel_len + 1);
    for i in 0..raw_pixel_len {
        // Generate non-gray pixels so R != B (test channel swapping)
        let channel = i % 3;
        let pixel_idx = i / 3;
        let val = match channel {
            0 => ((pixel_idx * 7 + 10) % 256) as u8,  // R
            1 => ((pixel_idx * 13 + 50) % 256) as u8, // G
            2 => ((pixel_idx * 19 + 90) % 256) as u8, // B
            _ => unreachable!(),
        };
        raw_pixels.push(val);
    }

    let is_odd = raw_pixel_len % 2 != 0;
    let pad_byte = if is_odd {
        let b = options.custom_pad_byte.unwrap_or(0x00);
        raw_pixels.push(b);
        Some(b)
    } else {
        None
    };

    // Calculate BGR-swapped reference hash
    let mut swapped_pixels = raw_pixels.clone();
    for i in (0..raw_pixel_len).step_by(3) {
        swapped_pixels.swap(i, i + 2); // Swap R and B
    }
    let mut hasher = Sha1::new();
    hasher.update(&swapped_pixels);
    let bgr_swapped_hash = format!("{:040x}", hasher.finalize());

    // Write (7FE0, 0010) OB element
    write_explicit_long_element(&mut file, 0x7FE0, 0x0010, b"OB", &raw_pixels)?;

    Ok(SyntheticRgbInfo {
        bgr_swapped_hash,
        total_pixel_bytes_with_pad: raw_pixels.len(),
        patient_id: options.patient_id.clone(),
        pad_byte,
    })
}
