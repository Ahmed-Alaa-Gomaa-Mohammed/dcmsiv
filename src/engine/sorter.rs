use crate::dicom::types::{DicomScan, FileDisposition, LayerCount};
use chrono::{DateTime, NaiveDateTime, Utc};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum SorterError {
    Io(io::Error),
    TargetParentCreationFailed(io::Error),
}

impl std::fmt::Display for SorterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SorterError::Io(e) => write!(f, "File operation failed: {e}"),
            SorterError::TargetParentCreationFailed(e) => {
                write!(f, "Failed to create target directory: {e}")
            }
        }
    }
}

impl std::error::Error for SorterError {}

impl From<io::Error> for SorterError {
    fn from(e: io::Error) -> Self {
        SorterError::Io(e)
    }
}

/// Sanitizes folder name by replacing invalid filesystem characters with '_'
pub fn sanitize_folder_name(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' | '\0' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();

    let trimmed = sanitized.trim();
    if trimmed.is_empty() {
        "UNKNOWN_PATIENT".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Formats date time into YYYY-MM-DD_HH-mm-ss
pub fn format_datetime(dt: NaiveDateTime) -> String {
    dt.format("%Y-%m-%d_%H-%M-%S").to_string()
}

/// Computes the candidate destination path for a scan
pub fn compute_target_path(
    output_dir: &Path,
    scan: &DicomScan,
) -> PathBuf {
    let original_file_name = scan
        .source_path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "scan.dcm".to_string());

    match scan.disposition {
        FileDisposition::Recovered => {
            let card_id = scan
                .matched_internal_card_id
                .as_deref()
                .unwrap_or("UNKNOWN_PATIENT");
            let safe_folder = sanitize_folder_name(card_id);
            let folder_path = output_dir.join(safe_folder);

            let prefix = match scan.layer_count {
                LayerCount::Single => "RasterImage_",
                LayerCount::Multi(_) => "Volume_",
            };

            let formatted_dt = if let Some(dt) = scan.metadata.as_ref().and_then(|m| m.acquisition_datetime) {
                format_datetime(dt)
            } else {
                let dt = DateTime::from_timestamp(scan.mtime, 0)
                    .unwrap_or_else(Utc::now)
                    .naive_utc();
                format_datetime(dt)
            };

            let file_name = format!("{prefix}{formatted_dt}.dcm");
            folder_path.join(file_name)
        }
        FileDisposition::Corrupt => output_dir.join("corrupt").join(original_file_name),
        FileDisposition::Duplicate => output_dir.join("duplicates").join(original_file_name),
        FileDisposition::Unmatched => output_dir.join("unmatched").join(original_file_name),
    }
}

/// Finds a non-colliding destination path by appending _1, _2 if target exists
pub fn resolve_collision_path(target: PathBuf) -> PathBuf {
    if !target.exists() {
        return target;
    }

    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    let stem = target
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".to_string());
    let ext = target
        .extension()
        .map(|s| format!(".{}", s.to_string_lossy()))
        .unwrap_or_default();

    let mut counter = 1;
    loop {
        let candidate_name = format!("{stem}_{counter}{ext}");
        let candidate_path = parent.join(candidate_name);
        if !candidate_path.exists() {
            return candidate_path;
        }
        counter += 1;
    }
}

/// Executes file move or copy operation with zero-overwrite guarantee and cross-device fallback
pub fn place_file(
    source: &Path,
    destination: &Path,
    copy_mode: bool,
    dry_run: bool,
) -> Result<PathBuf, SorterError> {
    let final_dest = resolve_collision_path(destination.to_path_buf());

    if dry_run {
        return Ok(final_dest);
    }

    if let Some(parent) = final_dest.parent() {
        fs::create_dir_all(parent).map_err(SorterError::TargetParentCreationFailed)?;
    }

    if copy_mode {
        fs::copy(source, &final_dest)?;
    } else {
        // Try atomic rename first
        match fs::rename(source, &final_dest) {
            Ok(()) => {}
            Err(e) => {
                // Cross-device link fallback (or unsupported rename)
                if e.kind() == io::ErrorKind::CrossesDevices || e.raw_os_error() == Some(18) /* EXDEV */ {
                    fs::copy(source, &final_dest)?;
                    fs::remove_file(source)?;
                } else {
                    return Err(SorterError::Io(e));
                }
            }
        }
    }

    Ok(final_dest)
}
