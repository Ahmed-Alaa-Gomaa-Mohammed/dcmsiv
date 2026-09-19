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
        let rows = metadata.rows as usize;
        let cols = metadata.columns as usize;
        let pixel_len = rows * cols * 3;
        if pixel_len == 0 {
            return Err(HasherError::InvalidDimensions);
        }

        // Seek to PixelData value start and read the raw pixel data
        file.seek(SeekFrom::Start(metadata.pixel_data_offset))?;
        let mut raw_pixels = vec![0u8; pixel_len];
        file.read_exact(&mut raw_pixels)?;

        // Reconstruct into Windows DIB memory layout:
        // 1. Swap R and B channels (BGR byte order)
        // 2. Pad each row to a 4-byte (DWORD) boundary with 0x00 bytes
        // 3. Stored top-to-bottom
        let pad_per_row = (4 - ((cols * 3) % 4)) % 4;
        let dib_row_len = cols * 3 + pad_per_row;
        let mut dib_buffer = vec![0u8; rows * dib_row_len];

        for r in 0..rows {
            let src_row_start = r * cols * 3;
            let dst_row_start = r * dib_row_len;

            for c in 0..cols {
                let src_idx = src_row_start + c * 3;
                let dst_idx = dst_row_start + c * 3;
                let r_val = raw_pixels[src_idx];
                let g_val = raw_pixels[src_idx + 1];
                let b_val = raw_pixels[src_idx + 2];
                dib_buffer[dst_idx] = b_val;
                dib_buffer[dst_idx + 1] = g_val;
                dib_buffer[dst_idx + 2] = r_val;
            }
        }

        let mut hasher = Sha1::new();
        hasher.update(&dib_buffer);
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

