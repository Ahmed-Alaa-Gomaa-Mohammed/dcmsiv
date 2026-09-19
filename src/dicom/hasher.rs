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
/// - Single-layer scans: SHA-1 of the single image frame
/// - Multi-layer scans: SHA-1 of the middle layer (floor(N / 2))
pub fn compute_pixel_layer_hash(
    path: &Path,
    metadata: &DicomMetadata,
) -> Result<(String, LayerCount), HasherError> {
    let mut file = File::open(path)?;

    let frames = metadata.number_of_frames;
    let (target_frame_idx, frame_size, layer_count) = if frames <= 1 {
        let frame_size = if metadata.pixel_data_length > 0 {
            metadata.pixel_data_length as usize
        } else {
            let bpp = if metadata.bits_allocated > 8 { 2 } else { 1 };
            (metadata.rows as usize) * (metadata.columns as usize) * bpp
        };
        (0usize, frame_size, LayerCount::Single)
    } else {
        let mid_idx = frames / 2; // floor(N / 2)
        let frame_size = if metadata.pixel_data_length > 0 {
            (metadata.pixel_data_length as usize) / frames
        } else {
            let bpp = if metadata.bits_allocated > 8 { 2 } else { 1 };
            (metadata.rows as usize) * (metadata.columns as usize) * bpp
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
