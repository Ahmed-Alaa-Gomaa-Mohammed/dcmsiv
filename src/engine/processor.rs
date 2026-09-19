use crate::db::csv_reader::{MediaBaseIndex, PatientIndex};
use crate::dicom::hasher::compute_pixel_layer_hash;
use crate::dicom::header::read_dicom_header;
use crate::dicom::types::{DicomScan, FileDisposition, LayerCount, ProcessingEvent};
use crate::engine::sorter::{compute_target_path, place_file};
use rayon::prelude::*;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex};

/// Discovers candidate recovery files in the input directory
pub fn discover_candidate_files(input_dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = fs::read_dir(input_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                // Ignore state db or hidden files in input
                if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                    if name.starts_with('.')
                        || name.ends_with(".db")
                        || name.ends_with(".sqlite")
                        || name.ends_with(".csv")
                        || name.ends_with(".json")
                        || name.ends_with(".txt")
                    {
                        continue;
                    }
                }
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

/// Evaluates a single candidate file against headers and SIDEXIS indexes
pub fn evaluate_candidate(
    path: &Path,
    patient_index: &PatientIndex,
    mediabase_index: &MediaBaseIndex,
    seen_hashes: &Arc<Mutex<HashSet<String>>>,
) -> DicomScan {
    let metadata_res = fs::metadata(path);
    let (file_size, mtime) = match metadata_res {
        Ok(m) => {
            let size = m.len();
            let mtime = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            (size, mtime)
        }
        Err(e) => {
            return DicomScan {
                source_path: path.to_path_buf(),
                file_size: 0,
                mtime: 0,
                metadata: None,
                layer_count: LayerCount::Single,
                computed_hash: None,
                disposition: FileDisposition::Corrupt,
                destination_path: None,
                error_reason: Some(format!("Failed to read file metadata: {e}")),
                matched_patient_id: None,
                matched_internal_card_id: None,
                matched_root_node: None,
            };
        }
    };

    let header = match read_dicom_header(path) {
        Ok(h) => h,
        Err(e) => {
            return DicomScan {
                source_path: path.to_path_buf(),
                file_size,
                mtime,
                metadata: None,
                layer_count: LayerCount::Single,
                computed_hash: None,
                disposition: FileDisposition::Corrupt,
                destination_path: None,
                error_reason: Some(e.to_string()),
                matched_patient_id: None,
                matched_internal_card_id: None,
                matched_root_node: None,
            };
        }
    };

    let (computed_hash, layer_count) = match compute_pixel_layer_hash(path, &header) {
        Ok((h, lc)) => (h, lc),
        Err(e) => {
            return DicomScan {
                source_path: path.to_path_buf(),
                file_size,
                mtime,
                metadata: Some(header),
                layer_count: LayerCount::Single,
                computed_hash: None,
                disposition: FileDisposition::Corrupt,
                destination_path: None,
                error_reason: Some(e.to_string()),
                matched_patient_id: None,
                matched_internal_card_id: None,
                matched_root_node: None,
            };
        }
    };

    // Cross-reference against Patient and MediaBase tables
    let raw_patient_tag = header
        .patient_id
        .as_ref()
        .or(header.other_patient_ids.as_ref());

    // Resolve patient record
    let resolved_patient = if let Some(tag_val) = raw_patient_tag {
        // Tag value might be InternalCardId
        if let Some(p) = patient_index.get_by_internal_card_id(tag_val) {
            Some(p.clone())
        } else if let Ok(pid) = tag_val.parse::<i64>() {
            patient_index.get_by_patient_id(pid).cloned()
        } else {
            None
        }
    } else {
        None
    };

    // Match in MediaBase
    let mut final_matched_hash = computed_hash.clone();
    let mb_match = if let Some(ref pat) = resolved_patient {
        mediabase_index
            .get_by_patient_and_hash(pat.patient_id, &computed_hash)
            .cloned()
            .or_else(|| {
                // If direct match failed, and file is 8-bit RGB single layer with odd length, check pad-sweep candidates
                if let Ok(candidates) = crate::dicom::hasher::compute_pad_sweep_hashes(path, &header) {
                    for (cand_hash, _pad) in candidates {
                        if let Some(mb) = mediabase_index.get_by_patient_and_hash(pat.patient_id, &cand_hash) {
                            final_matched_hash = cand_hash;
                            return Some(mb.clone());
                        }
                    }
                }
                // If patient record didn't match directly, check if hash matches any entry
                mediabase_index
                    .get_by_hash(&computed_hash)
                    .and_then(|list| list.first().cloned())
            })
    } else {
        mediabase_index
            .get_by_hash(&computed_hash)
            .and_then(|list| list.first().cloned())
            .or_else(|| {
                // If no resolved patient, check if any pad-sweep candidate matches any MediaBase entry
                if let Ok(candidates) = crate::dicom::hasher::compute_pad_sweep_hashes(path, &header) {
                    for (cand_hash, _pad) in candidates {
                        if let Some(list) = mediabase_index.get_by_hash(&cand_hash) {
                            if let Some(first) = list.first() {
                                final_matched_hash = cand_hash;
                                return Some(first.clone());
                            }
                        }
                    }
                }
                None
            })
    };

    if let Some(mb_rec) = mb_match {
        // Resolve patient if we only matched via hash
        let final_patient = resolved_patient.or_else(|| {
            patient_index.get_by_patient_id(mb_rec.patient_id).cloned()
        });

        let (matched_pid, matched_card_id) = if let Some(p) = final_patient {
            (Some(p.patient_id), Some(p.internal_card_id))
        } else {
            (Some(mb_rec.patient_id), Some(mb_rec.patient_id.to_string()))
        };

        // Check duplicate
        let mut seen = seen_hashes.lock().unwrap();
        let is_duplicate = !seen.insert(final_matched_hash.clone());

        let disposition = if is_duplicate {
            FileDisposition::Duplicate
        } else {
            FileDisposition::Recovered
        };

        DicomScan {
            source_path: path.to_path_buf(),
            file_size,
            mtime,
            metadata: Some(header),
            layer_count,
            computed_hash: Some(final_matched_hash),
            disposition,
            destination_path: None,
            error_reason: None,
            matched_patient_id: matched_pid,
            matched_internal_card_id: matched_card_id,
            matched_root_node: Some(mb_rec.root_node),
        }
    } else {
        DicomScan {
            source_path: path.to_path_buf(),
            file_size,
            mtime,
            metadata: Some(header),
            layer_count,
            computed_hash: Some(computed_hash),
            disposition: FileDisposition::Unmatched,
            destination_path: None,
            error_reason: None,
            matched_patient_id: resolved_patient.as_ref().map(|p| p.patient_id),
            matched_internal_card_id: resolved_patient.as_ref().map(|p| p.internal_card_id.clone()),
            matched_root_node: None,
        }
    }
}

/// Statistics collected during pipeline execution
#[derive(Debug, Default)]
pub struct PipelineStats {
    pub total_scanned: AtomicUsize,
    pub recovered: AtomicUsize,
    pub corrupt: AtomicUsize,
    pub duplicates: AtomicUsize,
    pub unmatched: AtomicUsize,
}

/// Executes the parallel sorting recovery pipeline over discovered files
pub fn run_pipeline(
    candidates: Vec<PathBuf>,
    output_dir: &Path,
    patient_index: &PatientIndex,
    mediabase_index: &MediaBaseIndex,
    writer_sender: &crossbeam_channel::Sender<ProcessingEvent>,
    already_processed: &HashSet<String>,
    copy_mode: bool,
    dry_run: bool,
    progress_callback: impl Fn(usize, &DicomScan) + Sync + Send,
) -> Vec<DicomScan> {
    let seen_hashes = Arc::new(Mutex::new(HashSet::new()));

    candidates
        .into_par_iter()
        .filter_map(|path| {
            let path_str = path.to_string_lossy().to_string();
            if already_processed.contains(&path_str) {
                return None;
            }

            let mut scan = evaluate_candidate(&path, patient_index, mediabase_index, &seen_hashes);
            let target = compute_target_path(output_dir, &scan);

            match place_file(&scan.source_path, &target, copy_mode, dry_run) {
                Ok(final_path) => {
                    scan.destination_path = Some(final_path.clone());
                    if !dry_run {
                        let op = if copy_mode { "copy" } else { "move" };
                        let _ = writer_sender.send(ProcessingEvent::TransactionCommitted {
                            source_path: scan.source_path.clone(),
                            destination_path: final_path,
                            operation_type: op.to_string(),
                        });
                    }
                }
                Err(e) => {
                    scan.disposition = FileDisposition::Corrupt;
                    scan.error_reason = Some(format!("Failed placing file: {e}"));
                }
            }

            if let Some(ref root_node) = scan.matched_root_node {
                if scan.disposition == FileDisposition::Recovered {
                    let _ = writer_sender.send(ProcessingEvent::RootNodeRecovered {
                        root_node: root_node.clone(),
                        first_recovered_file: scan.source_path.to_string_lossy().to_string(),
                        patient_id: scan.matched_patient_id.unwrap_or(0),
                    });
                }
            }

            if !dry_run {
                let _ = writer_sender.send(ProcessingEvent::FileProcessed {
                    scan: scan.clone(),
                });
            }

            progress_callback(1, &scan);
            Some(scan)
        })
        .collect()
}
