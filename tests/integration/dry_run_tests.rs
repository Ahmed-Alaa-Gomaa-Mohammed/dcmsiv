use crate::common::synthetic_dicom::{generate_synthetic_dicom, SyntheticDicomOptions};
use crossbeam_channel::unbounded;
use dcmsiv::db::csv_reader::{parse_mediabase_csv, parse_patient_csv};
use dcmsiv::dicom::types::{FileDisposition, ProcessingEvent};
use dcmsiv::engine::processor::{discover_candidate_files, run_pipeline};
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::Write;
use tempfile::tempdir;

#[test]
fn test_dry_run_mode_zero_filesystem_mutations() {
    let temp_workspace = tempdir().unwrap();
    let input_dir = temp_workspace.path().join("input");
    let output_dir = temp_workspace.path().join("output");
    fs::create_dir_all(&input_dir).unwrap();
    fs::create_dir_all(&output_dir).unwrap();

    let scan_file = input_dir.join("sample.dcm");
    let info = generate_synthetic_dicom(
        &scan_file,
        &SyntheticDicomOptions {
            patient_id: Some("P_DRY".to_string()),
            ..Default::default()
        },
    )
    .unwrap();

    let patient_csv_path = temp_workspace.path().join("Patient.csv");
    {
        let mut f = File::create(&patient_csv_path).unwrap();
        writeln!(f, "PatientId,InternalCardId").unwrap();
        writeln!(f, "1,P_DRY").unwrap();
    }

    let mediabase_csv_path = temp_workspace.path().join("MediaBase.csv");
    {
        let mut f = File::create(&mediabase_csv_path).unwrap();
        writeln!(f, "PatientId,RootNode,CreationDate,MediaHash").unwrap();
        writeln!(f, "1,RN_DRY,2023-01-01 12:00:00,{}", info.middle_layer_hash).unwrap();
    }

    let patient_index = parse_patient_csv(&patient_csv_path).unwrap();
    let mediabase_index = parse_mediabase_csv(&mediabase_csv_path).unwrap();

    let candidates = discover_candidate_files(&input_dir);
    assert_eq!(candidates.len(), 1);

    let (sender, _receiver) = unbounded::<ProcessingEvent>();
    let already_processed = HashSet::new();

    // Run pipeline with dry_run = true
    let scans = run_pipeline(
        candidates,
        &output_dir,
        &patient_index,
        &mediabase_index,
        &sender,
        &already_processed,
        false, // move mode
        true,  // dry_run = true!
        |_count, _scan| {},
    );

    assert_eq!(scans.len(), 1);
    assert_eq!(scans[0].disposition, FileDisposition::Recovered);

    // 1. Source file must STILL exist in input_dir (untouched)
    assert!(scan_file.exists(), "Source file must not be modified in dry-run mode");

    // 2. Output directory must be completely empty (zero filesystem mutations)
    let output_entries: Vec<_> = fs::read_dir(&output_dir).unwrap().collect();
    assert_eq!(
        output_entries.len(),
        0,
        "No files or folders should be created in output_dir in dry-run mode"
    );
}
