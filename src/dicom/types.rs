use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Patient mapping record from Patient.csv
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatientRecord {
    pub patient_id: i64,
    pub internal_card_id: String,
}

/// MediaBase entry from MediaBase.csv
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaBaseRecord {
    pub patient_id: i64,
    pub root_node: String,
    pub creation_date: Option<NaiveDateTime>,
    pub media_hash: String, // lowercase 40-char hex
}

/// Image frame layer count classification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LayerCount {
    Single,
    Multi(usize),
}

impl LayerCount {
    pub fn frames(&self) -> usize {
        match self {
            LayerCount::Single => 1,
            LayerCount::Multi(n) => *n,
        }
    }

    pub fn is_multi(&self) -> bool {
        match self {
            LayerCount::Single => false,
            LayerCount::Multi(n) => *n > 1,
        }
    }

    pub fn middle_layer_index(&self) -> usize {
        match self {
            LayerCount::Single => 0,
            LayerCount::Multi(n) => n / 2, // floor(n / 2)
        }
    }
}

/// Final disposition classification of a scanned candidate file
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileDisposition {
    Recovered,
    Corrupt,
    Duplicate,
    Unmatched,
}

impl std::fmt::Display for FileDisposition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FileDisposition::Recovered => write!(f, "recovered"),
            FileDisposition::Corrupt => write!(f, "corrupt"),
            FileDisposition::Duplicate => write!(f, "duplicate"),
            FileDisposition::Unmatched => write!(f, "unmatched"),
        }
    }
}

impl std::str::FromStr for FileDisposition {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "recovered" => Ok(FileDisposition::Recovered),
            "corrupt" => Ok(FileDisposition::Corrupt),
            "duplicate" => Ok(FileDisposition::Duplicate),
            "unmatched" => Ok(FileDisposition::Unmatched),
            other => Err(format!("Unknown file disposition: {other}")),
        }
    }
}

/// Extracted DICOM metadata and properties
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DicomMetadata {
    pub patient_id: Option<String>,
    pub other_patient_ids: Option<String>,
    pub acquisition_datetime: Option<NaiveDateTime>,
    pub number_of_frames: usize,
    pub rows: u16,
    pub columns: u16,
    pub bits_allocated: u16,
    pub samples_per_pixel: u16,
    pub photometric_interpretation: Option<String>,
    pub planar_configuration: u16,
    pub pixel_data_offset: u64,
    pub pixel_data_length: u64,
}

/// Evaluated candidate DICOM scan asset
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DicomScan {
    pub source_path: PathBuf,
    pub file_size: u64,
    pub mtime: i64,
    pub metadata: Option<DicomMetadata>,
    pub layer_count: LayerCount,
    pub computed_hash: Option<String>,
    pub disposition: FileDisposition,
    pub destination_path: Option<PathBuf>,
    pub error_reason: Option<String>,
    pub matched_patient_id: Option<i64>,
    pub matched_internal_card_id: Option<String>,
    pub matched_root_node: Option<String>,
}

/// Transaction journal entry recorded in SQLite
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionRecord {
    pub id: Option<i64>,
    pub source_path: PathBuf,
    pub destination_path: PathBuf,
    pub operation_type: String, // "move" or "copy"
    pub status: String,         // "committed" or "rolled_back"
    pub timestamp: String,
}

/// Scanned file state record in SQLite
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScannedFileRecord {
    pub file_path: String,
    pub file_size: u64,
    pub mtime: i64,
    pub patient_identifier: Option<String>,
    pub acquisition_datetime: Option<String>,
    pub layer_count: usize,
    pub middle_layer_index: usize,
    pub media_hash: Option<String>,
    pub status: String,
    pub target_path: Option<String>,
    pub error_message: Option<String>,
    pub processed_at: String,
}

/// Channel event sent from Rayon worker threads to the SQLite state writer
#[derive(Debug, Clone)]
pub enum ProcessingEvent {
    FileProcessed {
        scan: DicomScan,
    },
    TransactionCommitted {
        source_path: PathBuf,
        destination_path: PathBuf,
        operation_type: String,
    },
    RootNodeRecovered {
        root_node: String,
        first_recovered_file: String,
        patient_id: i64,
    },
    Flush,
}
