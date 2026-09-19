use crate::common::synthetic_dicom::{
    generate_corrupt_dicom, generate_synthetic_dicom, SyntheticDicomOptions,
};
use crossbeam_channel::unbounded;
use dcmsiv::db::csv_reader::{parse_mediabase_csv, parse_patient_csv};
use dcmsiv::dicom::types::{FileDisposition, ProcessingEvent};
use dcmsiv::engine::processor::{discover_candidate_files, run_pipeline};
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::Write;
use tempfile::tempdir;

#[test]
fn test_quarantine_corrupt_duplicate_and_unmatched() {
    let temp_workspace = tempdir().unwrap();
    let input_dir = temp_workspace.path().join("input");
    let output_dir = temp_workspace.path().join("output");
    fs::create_dir_all(&input_dir).unwrap();
    fs::create_dir_all(&output_dir).unwrap();

    // 1. Valid scan (will be recovered)
    let valid_file1 = input_dir.join("valid1.dcm");
    let valid_info1 = generate_synthetic_dicom(
        &valid_file1,
        &SyntheticDicomOptions {
            patient_id: Some("CARD_A".to_string()),
            acquisition_date_time: Some("20230101100000".to_string()),
            ..Default::default()
        },
    )
    .unwrap();

    // 2. Duplicate scan (same patient & hash as valid1, but different file name)
    let dup_file = input_dir.join("dup_valid1.dcm");
    let _dup_info = generate_synthetic_dicom(
        &dup_file,
        &SyntheticDicomOptions {
            patient_id: Some("CARD_A".to_string()),
            acquisition_date_time: Some("20230101100000".to_string()),
            ..Default::default()
        },
    )
    .unwrap();

    // 3. Corrupt scan
    let corrupt_file = input_dir.join("broken.dcm");
    generate_corrupt_dicom(&corrupt_file, true).unwrap();

    // 4. Unmatched scan (valid DICOM, but not in CSV tables)
    let unmatched_file = input_dir.join("orphan.dcm");
    let _unmatched_info = generate_synthetic_dicom(
        &unmatched_file,
        &SyntheticDicomOptions {
            patient_id: Some("CARD_UNKNOWN".to_string()),
            acquisition_date_time: Some("20230909120000".to_string()),
            pixel_fill_byte: Some(0x99),
            ..Default::default()
        },
    )
    .unwrap();

    // Create CSV tables containing ONLY valid1
    let patient_csv_path = temp_workspace.path().join("Patient.csv");
    {
        let mut f = File::create(&patient_csv_path).unwrap();
        writeln!(f, "PatientId,InternalCardId").unwrap();
        writeln!(f, "100,CARD_A").unwrap();
    }

    let mediabase_csv_path = temp_workspace.path().join("MediaBase.csv");
    {
        let mut f = File::create(&mediabase_csv_path).unwrap();
        writeln!(f, "PatientId,RootNode,CreationDate,MediaHash").unwrap();
        writeln!(
            f,
            "100,ROOT_A,2023-01-01 10:00:00.000,{}",
            valid_info1.middle_layer_hash
        )
        .unwrap();
    }

    let patient_index = parse_patient_csv(&patient_csv_path).unwrap();
    let mediabase_index = parse_mediabase_csv(&mediabase_csv_path).unwrap();

    let candidates = discover_candidate_files(&input_dir);
    assert_eq!(candidates.len(), 4);

    let (sender, _receiver) = unbounded::<ProcessingEvent>();
    let already_processed = HashSet::new();

    let scans = run_pipeline(
        candidates,
        &output_dir,
        &patient_index,
        &mediabase_index,
        &sender,
        &already_processed,
        false, // move mode
        false, // not dry run
        |_count, _scan| {},
    );

    assert_eq!(scans.len(), 4);

    // Verify Dispositions
    let mut recovered_count = 0;
    let mut duplicate_count = 0;
    let mut corrupt_count = 0;
    let mut unmatched_count = 0;

    for s in &scans {
        match s.disposition {
            FileDisposition::Recovered => recovered_count += 1,
            FileDisposition::Duplicate => duplicate_count += 1,
            FileDisposition::Corrupt => corrupt_count += 1,
            FileDisposition::Unmatched => unmatched_count += 1,
        }
    }

    assert_eq!(recovered_count, 1, "Exactly 1 scan recovered");
    assert_eq!(duplicate_count, 1, "Exactly 1 scan duplicate");
    assert_eq!(corrupt_count, 1, "Exactly 1 scan corrupt");
    assert_eq!(unmatched_count, 1, "Exactly 1 scan unmatched");

    // Verify folder locations on disk
    let recovered_folder = output_dir.join("CARD_A");
    assert!(recovered_folder.exists());
    let recovered_files: Vec<_> = fs::read_dir(&recovered_folder)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(recovered_files.len(), 1);

    let corrupt_file_dest = output_dir.join("corrupt").join("broken.dcm");
    assert!(corrupt_file_dest.exists(), "Corrupt file must be in corrupt/");

    let dup_in_duplicates = output_dir.join("duplicates").join("dup_valid1.dcm").exists()
        || output_dir.join("duplicates").join("valid1.dcm").exists();
    assert!(dup_in_duplicates, "One of the duplicate scans must be in duplicates/");

    let unmatched_file_dest = output_dir.join("unmatched").join("orphan.dcm");
    assert!(unmatched_file_dest.exists(), "Unmatched file must be in unmatched/");
}
