use crate::dicom::types::{DicomMetadata, LayerCount};
use sha1::{Digest, Sha1};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

#[derive(Debug)]
pub enum HasherError {
    Io(io::Error),
    InvalidDimensions,
    InvalidFrameIndex,
}

impl std::fmt::Display for HasherError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HasherError::Io(e) => write!(f, "I/O error during pixel hashing: {e}"),
            HasherError::InvalidDimensions => write!(f, "Invalid image dimensions or frame size"),
            HasherError::InvalidFrameIndex => write!(f, "Frame index out of bounds"),
        }
    }
}

impl std::error::Error for HasherError {}

impl From<io::Error> for HasherError {
    fn from(e: io::Error) -> Self {
        HasherError::Io(e)
    }
}

/// Computes the SHA-1 checksum of the raw uncompressed pixel data:
/// - 8-bit RGB single-layer scans (interleaved): SHA-1 after swapping R and B channels (BGR byte order), keeping trailing pad byte
/// - 16-bit monochrome single-layer scans: SHA-1 of the single image frame as stored
/// - Multi-layer scans: SHA-1 of the middle layer (floor(N / 2))
pub fn compute_pixel_layer_hash(
    path: &Path,
    metadata: &DicomMetadata,
) -> Result<(String, LayerCount), HasherError> {
    let mut file = File::open(path)?;
    let frames = metadata.number_of_frames;

    // 1. Special handling for 8-bit RGB interleaved single-layer scans (PlanarConfiguration = 0)
    let is_rgb_single_layer = frames <= 1
        && metadata.samples_per_pixel == 3
        && metadata.bits_allocated == 8
        && metadata.planar_configuration == 0;

    if is_rgb_single_layer {
        let pixel_len = (metadata.rows as usize) * (metadata.columns as usize) * 3;
        if pixel_len == 0 {
            return Err(HasherError::InvalidDimensions);
        }

        let is_odd = !pixel_len.is_multiple_of(2);
        let expected_total_len = if is_odd { pixel_len + 1 } else { pixel_len };
        let total_read_len = if metadata.pixel_data_length > 0 {
            (metadata.pixel_data_length as usize).max(pixel_len)
        } else {
            expected_total_len
        };

        file.seek(SeekFrom::Start(metadata.pixel_data_offset))?;
        let mut pixel_buf = vec![0u8; total_read_len];
        file.read_exact(&mut pixel_buf)?;

        // Swap R and B channels (bytes 1 and 3 of every 3-byte pixel) in-place
        for i in (0..pixel_len).step_by(3) {
            pixel_buf.swap(i, i + 2);
        }

        // Trailing pad byte (if present) is kept unswapped and intact
        let mut hasher = Sha1::new();
        hasher.update(&pixel_buf);
        let hash_str = format!("{:040x}", hasher.finalize());
        return Ok((hash_str, LayerCount::Single));
    }

    // 2. Standard single-layer monochrome and multi-layer volumetric scans
    let (target_frame_idx, frame_size, layer_count) = if frames <= 1 {
        let frame_size = if metadata.pixel_data_length > 0 {
            metadata.pixel_data_length as usize
        } else {
            let bpp = if metadata.bits_allocated > 8 { 2 } else { 1 };
            let samples = if metadata.samples_per_pixel > 0 {
                metadata.samples_per_pixel as usize
            } else {
                1
            };
            (metadata.rows as usize) * (metadata.columns as usize) * bpp * samples
        };
        (0usize, frame_size, LayerCount::Single)
    } else {
        let mid_idx = frames / 2; // floor(N / 2)
        let frame_size = if metadata.pixel_data_length > 0 {
            (metadata.pixel_data_length as usize) / frames
        } else {
            let bpp = if metadata.bits_allocated > 8 { 2 } else { 1 };
            let samples = if metadata.samples_per_pixel > 0 {
                metadata.samples_per_pixel as usize
            } else {
                1
            };
            (metadata.rows as usize) * (metadata.columns as usize) * bpp * samples
        };
        (mid_idx, frame_size, LayerCount::Multi(frames))
    };

    if frame_size == 0 {
        return Err(HasherError::InvalidDimensions);
    }

    let seek_offset = metadata.pixel_data_offset + (target_frame_idx as u64) * (frame_size as u64);
    file.seek(SeekFrom::Start(seek_offset))?;

    let mut hasher = Sha1::new();
    let mut remaining = frame_size;
    let mut buffer = [0u8; 65536]; // 64 KB buffer for bounded memory

    while remaining > 0 {
        let to_read = remaining.min(buffer.len());
        let n = file.read(&mut buffer[..to_read])?;
        if n == 0 {
            break; // Premature EOF
        }
        hasher.update(&buffer[..n]);
        remaining -= n;
    }

    let hash_str = format!("{:040x}", hasher.finalize());
    Ok((hash_str, layer_count))
}

/// Evaluates all 256 possible pad byte values for 8-bit RGB rasters with odd pixel length
/// Returns a vector of (SHA-1 hash, pad_byte)
pub fn compute_pad_sweep_hashes(
    path: &Path,
    metadata: &DicomMetadata,
) -> Result<Vec<(String, u8)>, HasherError> {
    let frames = metadata.number_of_frames;
    let is_rgb_single_layer = frames <= 1
        && metadata.samples_per_pixel == 3
        && metadata.bits_allocated == 8
        && metadata.planar_configuration == 0;

    if !is_rgb_single_layer {
        return Ok(Vec::new());
    }

    let pixel_len = (metadata.rows as usize) * (metadata.columns as usize) * 3;
    if pixel_len.is_multiple_of(2) {
        return Ok(Vec::new());
    }

    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(metadata.pixel_data_offset))?;

    let mut pixel_buf = vec![0u8; pixel_len + 1];
    file.read_exact(&mut pixel_buf[..pixel_len])?;

    // Swap R and B
    for i in (0..pixel_len).step_by(3) {
        pixel_buf.swap(i, i + 2);
    }

    let mut results = Vec::with_capacity(256);
    for pad in 0u8..=255u8 {
        pixel_buf[pixel_len] = pad;
        let mut hasher = Sha1::new();
        hasher.update(&pixel_buf);
        let h = format!("{:040x}", hasher.finalize());
        results.push((h, pad));
    }

    Ok(results)
}
