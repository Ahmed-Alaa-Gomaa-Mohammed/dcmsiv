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
fn test_end_to_end_recovery_and_sorting_move_mode() {
    let temp_workspace = tempdir().unwrap();
    let input_dir = temp_workspace.path().join("input");
    let output_dir = temp_workspace.path().join("output");
    fs::create_dir_all(&input_dir).unwrap();
    fs::create_dir_all(&output_dir).unwrap();

    // 1. Generate synthetic DICOM files
    let single_path = input_dir.join("single.dcm");
    let single_opts = SyntheticDicomOptions {
        patient_id: Some("CARD_001".to_string()),
        other_patient_ids: None,
        acquisition_date_time: Some("20230515123045".to_string()),
        number_of_frames: 1,
        rows: 64,
        columns: 64,
        bits_allocated: 16,
        pixel_fill_byte: Some(0x11),
    };
    let single_info = generate_synthetic_dicom(&single_path, &single_opts).unwrap();

    let vol_path = input_dir.join("volume.dcm");
    let vol_opts = SyntheticDicomOptions {
        patient_id: Some("CARD_002".to_string()),
        other_patient_ids: None,
        acquisition_date_time: Some("20230620084510".to_string()),
        number_of_frames: 5,
        rows: 64,
        columns: 64,
        bits_allocated: 16,
        pixel_fill_byte: Some(0x22),
    };
    let vol_info = generate_synthetic_dicom(&vol_path, &vol_opts).unwrap();

    // 2. Generate matching Patient.csv
    let patient_csv_path = temp_workspace.path().join("Patient.csv");
    {
        let mut f = File::create(&patient_csv_path).unwrap();
        writeln!(f, "PatientId,InternalCardId").unwrap();
        writeln!(f, "1,CARD_001").unwrap();
        writeln!(f, "2,CARD_002").unwrap();
    }

    // 3. Generate matching MediaBase.csv
    let mediabase_csv_path = temp_workspace.path().join("MediaBase.csv");
    {
        let mut f = File::create(&mediabase_csv_path).unwrap();
        writeln!(f, "PatientId,RootNode,CreationDate,MediaHash").unwrap();
        writeln!(
            f,
            "1,ROOT_NODE_001,2023-05-15 12:30:45.000,{}",
            single_info.middle_layer_hash
        )
        .unwrap();
        writeln!(
            f,
            "2,ROOT_NODE_002,2023-06-20 08:45:10.000,{}",
            vol_info.middle_layer_hash
        )
        .unwrap();
    }

    // 4. Ingest indexes
    let patient_index = parse_patient_csv(&patient_csv_path).unwrap();
    let mediabase_index = parse_mediabase_csv(&mediabase_csv_path).unwrap();

    // 5. Run pipeline
    let candidates = discover_candidate_files(&input_dir);
    assert_eq!(candidates.len(), 2);

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

    assert_eq!(scans.len(), 2);
    for s in &scans {
        assert_eq!(s.disposition, FileDisposition::Recovered);
    }

    // 6. Verify filesystem state
    // Source files should be moved (no longer in input)
    assert!(!single_path.exists());
    assert!(!vol_path.exists());

    // Sorted destination files must exist
    let target_single = output_dir.join("CARD_001").join("RasterImage_2023-05-15_12-30-45.dcm");
    assert!(target_single.exists(), "Target single raster image must exist at {:?}", target_single);

    let target_vol = output_dir.join("CARD_002").join("Volume_2023-06-20_08-45-10.dcm");
    assert!(target_vol.exists(), "Target volumetric scan must exist at {:?}", target_vol);
}
