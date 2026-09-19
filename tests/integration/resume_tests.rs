use crate::common::synthetic_dicom::{generate_synthetic_dicom, SyntheticDicomOptions};
use crossbeam_channel::unbounded;
use dcmsiv::db::csv_reader::{parse_mediabase_csv, parse_patient_csv};
use dcmsiv::db::state_store::StateStore;
use dcmsiv::dicom::types::{DicomScan, FileDisposition, LayerCount, ProcessingEvent};
use dcmsiv::engine::processor::{discover_candidate_files, run_pipeline};
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::Write;
use tempfile::tempdir;

#[test]
fn test_resumption_skips_already_processed_files() {
    let temp_workspace = tempdir().unwrap();
    let input_dir = temp_workspace.path().join("input");
    let output_dir = temp_workspace.path().join("output");
    fs::create_dir_all(&input_dir).unwrap();
    fs::create_dir_all(&output_dir).unwrap();

    let db_path = output_dir.join(".dcmsiv_state.db");
    let mut store = StateStore::open(&db_path).unwrap();

    // 1. Generate 3 files
    let file1 = input_dir.join("f1.dcm");
    let file2 = input_dir.join("f2.dcm");
    let file3 = input_dir.join("f3.dcm");

    let info1 = generate_synthetic_dicom(
        &file1,
        &SyntheticDicomOptions {
            patient_id: Some("P1".to_string()),
            ..Default::default()
        },
    )
    .unwrap();

    let info2 = generate_synthetic_dicom(
        &file2,
        &SyntheticDicomOptions {
            patient_id: Some("P2".to_string()),
            ..Default::default()
        },
    )
    .unwrap();

    let _info3 = generate_synthetic_dicom(
        &file3,
        &SyntheticDicomOptions {
            patient_id: Some("P3".to_string()),
            ..Default::default()
        },
    )
    .unwrap();

    // 2. Mock that file1 was already sorted in a previous session
    let dummy_scan = DicomScan {
        source_path: file1.clone(),
        file_size: 1000,
        mtime: 1234567,
        metadata: None,
        layer_count: LayerCount::Single,
        computed_hash: Some(info1.middle_layer_hash.clone()),
        disposition: FileDisposition::Recovered,
        destination_path: Some(output_dir.join("P1").join("RasterImage_2023-01-01.dcm")),
        error_reason: None,
        matched_patient_id: Some(1),
        matched_internal_card_id: Some("P1".to_string()),
        matched_root_node: Some("RN_1".to_string()),
    };
    store.record_file_scan(&dummy_scan).unwrap();

    // 3. Create CSVs
    let patient_csv_path = temp_workspace.path().join("Patient.csv");
    {
        let mut f = File::create(&patient_csv_path).unwrap();
        writeln!(f, "PatientId,InternalCardId").unwrap();
        writeln!(f, "1,P1").unwrap();
        writeln!(f, "2,P2").unwrap();
        writeln!(f, "3,P3").unwrap();
    }

    let mediabase_csv_path = temp_workspace.path().join("MediaBase.csv");
    {
        let mut f = File::create(&mediabase_csv_path).unwrap();
        writeln!(f, "PatientId,RootNode,CreationDate,MediaHash").unwrap();
        writeln!(f, "1,RN_1,2023-01-01 12:00:00,{}", info1.middle_layer_hash).unwrap();
        writeln!(f, "2,RN_2,2023-01-01 12:00:00,{}", info2.middle_layer_hash).unwrap();
    }

    let patient_index = parse_patient_csv(&patient_csv_path).unwrap();
    let mediabase_index = parse_mediabase_csv(&mediabase_csv_path).unwrap();

    // 4. Query already_processed from store
    let already_processed = store.get_processed_files().unwrap();
    assert_eq!(already_processed.len(), 1);
    assert!(already_processed.contains(&file1.to_string_lossy().to_string()));

    // 5. Run pipeline
    let candidates = discover_candidate_files(&input_dir);
    assert_eq!(candidates.len(), 3);

    let (sender, _receiver) = unbounded::<ProcessingEvent>();
    let scans = run_pipeline(
        candidates,
        &output_dir,
        &patient_index,
        &mediabase_index,
        &sender,
        &already_processed,
        true, // copy mode for test repeatability
        false,
        |_count, _scan| {},
    );

    // Only f2 and f3 should have been evaluated! f1 was skipped.
    assert_eq!(scans.len(), 2);
    let paths: HashSet<String> = scans
        .iter()
        .map(|s| s.source_path.to_string_lossy().to_string())
        .collect();
    assert!(!paths.contains(&file1.to_string_lossy().to_string()));
    assert!(paths.contains(&file2.to_string_lossy().to_string()));
    assert!(paths.contains(&file3.to_string_lossy().to_string()));
}

#[test]
fn test_reset_state_clears_database() {
    let temp_workspace = tempdir().unwrap();
    let output_dir = temp_workspace.path().join("output");
    fs::create_dir_all(&output_dir).unwrap();

    let db_path = output_dir.join(".dcmsiv_state.db");
    let mut store = StateStore::open(&db_path).unwrap();

    let dummy_scan = DicomScan {
        source_path: output_dir.join("test.dcm"),
        file_size: 100,
        mtime: 1234,
        metadata: None,
        layer_count: LayerCount::Single,
        computed_hash: Some("abcd".to_string()),
        disposition: FileDisposition::Recovered,
        destination_path: None,
        error_reason: None,
        matched_patient_id: Some(1),
        matched_internal_card_id: Some("P1".to_string()),
        matched_root_node: None,
    };
    store.record_file_scan(&dummy_scan).unwrap();
    assert_eq!(store.get_processed_files().unwrap().len(), 1);

    store.reset_state().unwrap();
    assert_eq!(store.get_processed_files().unwrap().len(), 0);
}
