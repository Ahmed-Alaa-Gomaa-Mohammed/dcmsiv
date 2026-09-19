use crate::common::synthetic_dicom::{generate_synthetic_dicom, SyntheticDicomOptions};
use crossbeam_channel::unbounded;
use dcmsiv::db::csv_reader::{MediaBaseIndex, PatientIndex};
use dcmsiv::dicom::types::ProcessingEvent;
use dcmsiv::engine::processor::{discover_candidate_files, run_pipeline};
use std::collections::HashSet;
use std::fs;
use tempfile::tempdir;

/// Reads current resident set size (RSS) in megabytes on Linux
fn get_current_rss_mb() -> usize {
    #[cfg(target_os = "linux")]
    {
        if let Ok(statm) = fs::read_to_string("/proc/self/statm") {
            if let Some(rss_pages_str) = statm.split_whitespace().nth(1) {
                if let Ok(pages) = rss_pages_str.parse::<usize>() {
                    // Page size is typically 4096 bytes
                    return (pages * 4096) / (1024 * 1024);
                }
            }
        }
    }
    0
}

#[test]
fn test_memory_consumption_bounded_under_load() {
    let temp_workspace = tempdir().unwrap();
    let input_dir = temp_workspace.path().join("input");
    let output_dir = temp_workspace.path().join("output");
    fs::create_dir_all(&input_dir).unwrap();
    fs::create_dir_all(&output_dir).unwrap();

    // Generate 50 synthetic multi-frame DICOM files
    for i in 0..50 {
        let file_path = input_dir.join(format!("batch_{i:03}.dcm"));
        let opts = SyntheticDicomOptions {
            patient_id: Some(format!("PAT_{i}")),
            number_of_frames: 10,
            rows: 128,
            columns: 128,
            bits_allocated: 16,
            pixel_fill_byte: Some((i % 255) as u8),
            ..Default::default()
        };
        generate_synthetic_dicom(&file_path, &opts).unwrap();
    }

    let patient_index = PatientIndex::default();
    let mediabase_index = MediaBaseIndex::default();

    let candidates = discover_candidate_files(&input_dir);
    assert_eq!(candidates.len(), 50);

    let (sender, _receiver) = unbounded::<ProcessingEvent>();
    let already_processed = HashSet::new();

    let initial_rss = get_current_rss_mb();

    let scans = run_pipeline(
        candidates,
        &output_dir,
        &patient_index,
        &mediabase_index,
        &sender,
        &already_processed,
        false, // move
        false,
        |_count, _scan| {},
    );

    assert_eq!(scans.len(), 50);

    let final_rss = get_current_rss_mb();
    println!("Initial RSS: {initial_rss} MB, Final RSS: {final_rss} MB");

    // Must be strictly below 500 MB (SC-005)
    assert!(
        final_rss < 500,
        "RSS memory usage ({final_rss} MB) exceeded 500 MB limit!"
    );
}
