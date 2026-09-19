use crate::common::synthetic_dicom::{
    generate_corrupt_dicom, generate_synthetic_dicom, SyntheticDicomOptions,
};
use crossbeam_channel::unbounded;
use dcmsiv::db::csv_reader::{parse_mediabase_csv, parse_patient_csv};
use dcmsiv::db::state_store::StateStore;
use dcmsiv::dicom::types::ProcessingEvent;
use dcmsiv::engine::processor::{discover_candidate_files, run_pipeline};
use dcmsiv::engine::undo::execute_undo;
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::Write;
use tempfile::tempdir;

#[test]
fn test_all_quickstart_scenarios_end_to_end() {
    let temp_workspace = tempdir().unwrap();
    let unsorted_dir = temp_workspace.path().join("unsorted");
    let sorted_dir = temp_workspace.path().join("sorted");
    fs::create_dir_all(&unsorted_dir).unwrap();
    fs::create_dir_all(&sorted_dir).unwrap();

    // 1. Setup fixtures (zero PHI)
    let single_path = unsorted_dir.join("01.dcm");
    let single_info = generate_synthetic_dicom(
        &single_path,
        &SyntheticDicomOptions {
            patient_id: Some("CARD_1001".to_string()),
            acquisition_date_time: Some("20230512143022".to_string()),
            number_of_frames: 1,
            ..Default::default()
        },
    )
    .unwrap();

    let vol_path = unsorted_dir.join("02.dcm");
    let vol_info = generate_synthetic_dicom(
        &vol_path,
        &SyntheticDicomOptions {
            patient_id: Some("CARD_1002".to_string()),
            acquisition_date_time: Some("20230615091500".to_string()),
            number_of_frames: 10,
            ..Default::default()
        },
    )
    .unwrap();

    let corrupt_path = unsorted_dir.join("broken.dcm");
    generate_corrupt_dicom(&corrupt_path, true).unwrap();

    let dup_path = unsorted_dir.join("dup_01.dcm");
    generate_synthetic_dicom(
        &dup_path,
        &SyntheticDicomOptions {
            patient_id: Some("CARD_1001".to_string()),
            acquisition_date_time: Some("20230512143022".to_string()),
            number_of_frames: 1,
            ..Default::default()
        },
    )
    .unwrap();

    let unmatched_path = unsorted_dir.join("unmatched_01.dcm");
    generate_synthetic_dicom(
        &unmatched_path,
        &SyntheticDicomOptions {
            patient_id: Some("CARD_ORPHAN".to_string()),
            acquisition_date_time: Some("20230707070707".to_string()),
            pixel_fill_byte: Some(0x77),
            ..Default::default()
        },
    )
    .unwrap();

    // CSV tables
    let patient_csv = temp_workspace.path().join("Patient.csv");
    {
        let mut f = File::create(&patient_csv).unwrap();
        writeln!(f, "PatientId,InternalCardId").unwrap();
        writeln!(f, "1,CARD_1001").unwrap();
        writeln!(f, "2,CARD_1002").unwrap();
    }

    let mediabase_csv = temp_workspace.path().join("MediaBase.csv");
    {
        let mut f = File::create(&mediabase_csv).unwrap();
        writeln!(f, "PatientId,RootNode,CreationDate,MediaHash").unwrap();
        writeln!(
            f,
            "1,ROOT_1,2023-05-12 14:30:22.000,{}",
            single_info.middle_layer_hash
        )
        .unwrap();
        writeln!(
            f,
            "2,ROOT_2,2023-06-15 09:15:00.000,{}",
            vol_info.middle_layer_hash
        )
        .unwrap();
    }

    let patient_index = parse_patient_csv(&patient_csv).unwrap();
    let mediabase_index = parse_mediabase_csv(&mediabase_csv).unwrap();

    // SCENARIO 1: Dry-run preview
    let candidates = discover_candidate_files(&unsorted_dir);
    assert_eq!(candidates.len(), 5);

    let (sender, _receiver) = unbounded::<ProcessingEvent>();
    let already_processed = HashSet::new();

    let dry_scans = run_pipeline(
        candidates.clone(),
        &sorted_dir,
        &patient_index,
        &mediabase_index,
        &sender,
        &already_processed,
        false,
        true, // dry run!
        |_count, _scan| {},
    );
    assert_eq!(dry_scans.len(), 5);
    // Verify unsorted still contains all 5 files
    assert_eq!(discover_candidate_files(&unsorted_dir).len(), 5);

    // SCENARIO 2 & 5: Standard Recovery (Move Mode) + Quarantine
    let db_path = sorted_dir.join(".dcmsiv_state.db");
    let mut store = StateStore::open(&db_path).unwrap();

    let real_scans = run_pipeline(
        candidates,
        &sorted_dir,
        &patient_index,
        &mediabase_index,
        &sender,
        &already_processed,
        false, // move
        false,
        |_count, _scan| {},
    );

    // Log transactions to state db to simulate writer
    for s in &real_scans {
        store.record_file_scan(s).unwrap();
        if let Some(ref dest) = s.destination_path {
            store.record_transaction(&s.source_path, dest, "move").unwrap();
        }
    }

    // Verify recovery destinations
    let target_single = sorted_dir.join("CARD_1001/RasterImage_2023-05-12_14-30-22.dcm");
    let target_vol = sorted_dir.join("CARD_1002/Volume_2023-06-15_09-15-00.dcm");
    assert!(target_single.exists());
    assert!(target_vol.exists());

    // Scenario 5: Quarantine
    assert!(sorted_dir.join("corrupt/broken.dcm").exists());
    assert!(
        sorted_dir.join("duplicates/dup_01.dcm").exists()
            || sorted_dir.join("duplicates/01.dcm").exists()
    );
    assert!(sorted_dir.join("unmatched/unmatched_01.dcm").exists());

    // SCENARIO 3: Resumption
    let remaining_candidates = discover_candidate_files(&unsorted_dir);
    let completed_set = store.get_processed_files().unwrap();
    let resume_scans = run_pipeline(
        remaining_candidates,
        &sorted_dir,
        &patient_index,
        &mediabase_index,
        &sender,
        &completed_set,
        false,
        false,
        |_count, _scan| {},
    );
    assert_eq!(resume_scans.len(), 0, "No new files to process on resume");

    // SCENARIO 4: Rollback via --undo
    let restored = execute_undo(&sorted_dir).expect("Undo failed");
    assert_eq!(restored, 5, "Must restore all 5 moved files");

    // Verify all 5 files returned to unsorted
    assert!(single_path.exists());
    assert!(vol_path.exists());
    assert!(corrupt_path.exists());
    assert!(dup_path.exists());
    assert!(unmatched_path.exists());
}
