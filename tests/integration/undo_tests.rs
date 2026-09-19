use crate::common::synthetic_dicom::{generate_synthetic_dicom, SyntheticDicomOptions};
use dcmsiv::db::state_store::StateStore;
use dcmsiv::engine::undo::execute_undo;
use std::fs;
use tempfile::tempdir;

#[test]
fn test_undo_rollback_fidelity() {
    let temp_workspace = tempdir().unwrap();
    let input_dir = temp_workspace.path().join("input");
    let output_dir = temp_workspace.path().join("output");
    let patient_folder = output_dir.join("PATIENT_100");
    fs::create_dir_all(&input_dir).unwrap();
    fs::create_dir_all(&patient_folder).unwrap();

    let db_path = output_dir.join(".dcmsiv_state.db");
    let mut store = StateStore::open(&db_path).unwrap();

    // 1. Create a file in input_dir
    let src_file1 = input_dir.join("scan_01.dcm");
    let _info1 = generate_synthetic_dicom(
        &src_file1,
        &SyntheticDicomOptions {
            patient_id: Some("PATIENT_100".to_string()),
            ..Default::default()
        },
    )
    .unwrap();

    let dest_file1 = patient_folder.join("RasterImage_2023-01-01_10-00-00.dcm");

    // 2. Perform move manually and log transaction
    fs::rename(&src_file1, &dest_file1).unwrap();
    assert!(!src_file1.exists());
    assert!(dest_file1.exists());

    store
        .record_transaction(&src_file1, &dest_file1, "move")
        .unwrap();

    // Also test a second file
    let src_file2 = input_dir.join("scan_02.dcm");
    let _info2 = generate_synthetic_dicom(
        &src_file2,
        &SyntheticDicomOptions {
            patient_id: Some("PATIENT_100".to_string()),
            ..Default::default()
        },
    )
    .unwrap();
    let dest_file2 = patient_folder.join("RasterImage_2023-01-01_11-00-00.dcm");
    fs::rename(&src_file2, &dest_file2).unwrap();
    assert!(!src_file2.exists());
    assert!(dest_file2.exists());

    store
        .record_transaction(&src_file2, &dest_file2, "move")
        .unwrap();

    // 3. Execute undo
    let restored_count = execute_undo(&output_dir).expect("Undo execution failed");
    assert_eq!(restored_count, 2, "Must restore both moved files");

    // 4. Verify original source files exist again
    assert!(src_file1.exists(), "Original src_file1 must be restored");
    assert!(src_file2.exists(), "Original src_file2 must be restored");

    // 5. Destination files must no longer exist
    assert!(!dest_file1.exists());
    assert!(!dest_file2.exists());

    // 6. Verify state db transactions are marked rolled_back
    let remaining_committed = store.get_committed_transactions_reverse().unwrap();
    assert_eq!(remaining_committed.len(), 0, "No committed transactions should remain");
}
