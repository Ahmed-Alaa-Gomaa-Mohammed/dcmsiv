use crate::dicom::types::{MediaBaseRecord, PatientRecord};
use chrono::NaiveDateTime;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{self, BufReader};
use std::path::Path;

#[derive(Debug)]
pub enum CsvError {
    Io(io::Error),
    Csv(csv::Error),
    MissingColumn {
        column: &'static str,
        file_name: String,
    },
    InvalidInteger {
        column: &'static str,
        value: String,
    },
}

impl std::fmt::Display for CsvError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CsvError::Io(e) => write!(f, "I/O error reading CSV: {e}"),
            CsvError::Csv(e) => write!(f, "CSV parsing error: {e}"),
            CsvError::MissingColumn { column, file_name } => {
                write!(f, "Missing required column '{column}' in {file_name}")
            }
            CsvError::InvalidInteger { column, value } => {
                write!(f, "Invalid integer in column '{column}': {value}")
            }
        }
    }
}

impl std::error::Error for CsvError {}

/// In-memory index of Patient.csv
#[derive(Debug, Clone, Default)]
pub struct PatientIndex {
    pub by_patient_id: HashMap<i64, PatientRecord>,
    pub by_internal_card_id: HashMap<String, PatientRecord>,
}

impl PatientIndex {
    pub fn get_by_patient_id(&self, id: i64) -> Option<&PatientRecord> {
        self.by_patient_id.get(&id)
    }

    pub fn get_by_internal_card_id(&self, card_id: &str) -> Option<&PatientRecord> {
        self.by_internal_card_id.get(card_id)
    }
}

/// In-memory index of MediaBase.csv
#[derive(Debug, Clone, Default)]
pub struct MediaBaseIndex {
    /// Lookup by normalized lowercase media_hash -> list of matching records
    pub by_hash: HashMap<String, Vec<MediaBaseRecord>>,
    /// Lookup by (patient_id, lowercase media_hash) -> record
    pub by_patient_and_hash: HashMap<(i64, String), MediaBaseRecord>,
    /// Set of all unique non-empty RootNodes in the database
    pub distinct_root_nodes: HashSet<String>,
}

impl MediaBaseIndex {
    pub fn total_distinct_root_nodes(&self) -> usize {
        self.distinct_root_nodes.len()
    }

    pub fn get_by_hash(&self, hash: &str) -> Option<&Vec<MediaBaseRecord>> {
        self.by_hash.get(hash)
    }

    pub fn get_by_patient_and_hash(&self, patient_id: i64, hash: &str) -> Option<&MediaBaseRecord> {
        self.by_patient_and_hash.get(&(patient_id, hash.to_string()))
    }
}

/// Helper to sanitize and trim column headers, stripping UTF-8 BOM
fn clean_header(header: &str) -> &str {
    let s = header.trim();
    s.strip_prefix('\u{feff}').unwrap_or(s).trim()
}

/// Parses Patient.csv into an indexed lookup structure
pub fn parse_patient_csv(path: &Path) -> Result<PatientIndex, CsvError> {
    let file = File::open(path).map_err(CsvError::Io)?;
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_reader(BufReader::new(file));

    let headers = reader.headers().map_err(CsvError::Csv)?.clone();

    let mut patient_id_col = None;
    let mut internal_card_id_col = None;

    for (idx, h) in headers.iter().enumerate() {
        match clean_header(h) {
            "PatientId" => patient_id_col = Some(idx),
            "InternalCardId" => internal_card_id_col = Some(idx),
            _ => {}
        }
    }

    let patient_id_idx = patient_id_col.ok_or_else(|| CsvError::MissingColumn {
        column: "PatientId",
        file_name: path.display().to_string(),
    })?;

    let internal_card_id_idx = internal_card_id_col.ok_or_else(|| CsvError::MissingColumn {
        column: "InternalCardId",
        file_name: path.display().to_string(),
    })?;

    let mut index = PatientIndex::default();

    for result in reader.records() {
        let record = result.map_err(CsvError::Csv)?;
        let patient_id_str = record.get(patient_id_idx).unwrap_or("").trim();
        let internal_card_id_str = record.get(internal_card_id_idx).unwrap_or("").trim();

        if patient_id_str.is_empty() || internal_card_id_str.is_empty() {
            continue;
        }

        let patient_id = match patient_id_str.parse::<i64>() {
            Ok(id) => id,
            Err(_) => continue, // Skip unparseable rows
        };

        let pat_rec = PatientRecord {
            patient_id,
            internal_card_id: internal_card_id_str.to_string(),
        };

        index.by_patient_id.insert(patient_id, pat_rec.clone());
        index
            .by_internal_card_id
            .insert(internal_card_id_str.to_string(), pat_rec);
    }

    Ok(index)
}

/// Helper to parse datetime strings formatted like "2017-11-23 18:38:47.443" or "2017-11-23 18:38:47"
fn parse_datetime(dt_str: &str) -> Option<NaiveDateTime> {
    let trimmed = dt_str.trim();
    if trimmed.is_empty() || trimmed == "NULL" {
        return None;
    }
    // Try format with milliseconds first
    if let Ok(dt) = NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M:%S%.f") {
        return Some(dt);
    }
    // Try standard format
    if let Ok(dt) = NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M:%S") {
        return Some(dt);
    }
    // Try ISO format
    if let Ok(dt) = NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%dT%H:%M:%S%.f") {
        return Some(dt);
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%dT%H:%M:%S") {
        return Some(dt);
    }
    None
}

/// Parses MediaBase.csv into an indexed lookup structure
pub fn parse_mediabase_csv(path: &Path) -> Result<MediaBaseIndex, CsvError> {
    let file = File::open(path).map_err(CsvError::Io)?;
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_reader(BufReader::new(file));

    let headers = reader.headers().map_err(CsvError::Csv)?.clone();

    let mut patient_id_col = None;
    let mut root_node_col = None;
    let mut creation_date_col = None;
    let mut media_hash_col = None;

    for (idx, h) in headers.iter().enumerate() {
        match clean_header(h) {
            "PatientId" => patient_id_col = Some(idx),
            "RootNode" => root_node_col = Some(idx),
            "CreationDate" => creation_date_col = Some(idx),
            "MediaHash" => media_hash_col = Some(idx),
            _ => {}
        }
    }

    let patient_id_idx = patient_id_col.ok_or_else(|| CsvError::MissingColumn {
        column: "PatientId",
        file_name: path.display().to_string(),
    })?;

    let root_node_idx = root_node_col.ok_or_else(|| CsvError::MissingColumn {
        column: "RootNode",
        file_name: path.display().to_string(),
    })?;

    let creation_date_idx = creation_date_col.ok_or_else(|| CsvError::MissingColumn {
        column: "CreationDate",
        file_name: path.display().to_string(),
    })?;

    let media_hash_idx = media_hash_col.ok_or_else(|| CsvError::MissingColumn {
        column: "MediaHash",
        file_name: path.display().to_string(),
    })?;

    let mut index = MediaBaseIndex::default();

    for result in reader.records() {
        let record = result.map_err(CsvError::Csv)?;
        let patient_id_str = record.get(patient_id_idx).unwrap_or("").trim();
        let root_node_str = record.get(root_node_idx).unwrap_or("").trim();
        let creation_date_str = record.get(creation_date_idx).unwrap_or("").trim();
        let media_hash_str = record.get(media_hash_idx).unwrap_or("").trim();

        if !root_node_str.is_empty() && root_node_str != "00000000-0000-0000-0000-000000000000" {
            index.distinct_root_nodes.insert(root_node_str.to_string());
        }

        if patient_id_str.is_empty() {
            continue;
        }

        let patient_id = match patient_id_str.parse::<i64>() {
            Ok(id) => id,
            Err(_) => continue,
        };

        // If media_hash is empty or NULL, this is an intermediate folder/grouping node
        if media_hash_str.is_empty() || media_hash_str == "NULL" {
            continue;
        }

        let normalized_hash = media_hash_str.to_lowercase();
        let creation_date = parse_datetime(creation_date_str);

        let mb_rec = MediaBaseRecord {
            patient_id,
            root_node: root_node_str.to_string(),
            creation_date,
            media_hash: normalized_hash.clone(),
        };

        index
            .by_patient_and_hash
            .insert((patient_id, normalized_hash.clone()), mb_rec.clone());

        index
            .by_hash
            .entry(normalized_hash)
            .or_default()
            .push(mb_rec);
    }

    Ok(index)
}
