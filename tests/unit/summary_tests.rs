use chrono::{Duration, Utc};
use dcmsiv::dicom::types::{DicomScan, FileDisposition, LayerCount};
use dcmsiv::report::summary::RecoveryReport;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

#[test]
fn test_recovery_report_calculations_and_formatting() {
    let temp_workspace = tempdir().unwrap();
    let out_dir = temp_workspace.path().join("output");
    std::fs::create_dir_all(&out_dir).unwrap();

    let start = Utc::now();
    let end = start + Duration::seconds(10);

    let scans = vec![
        DicomScan {
            source_path: PathBuf::from("/in/1.dcm"),
            file_size: 1000,
            mtime: 0,
            metadata: None,
            layer_count: LayerCount::Single,
            computed_hash: Some("hash1".to_string()),
            disposition: FileDisposition::Recovered,
            destination_path: Some(out_dir.join("P1/RasterImage_2023.dcm")),
            error_reason: None,
            matched_patient_id: Some(1),
            matched_internal_card_id: Some("P1".to_string()),
            matched_root_node: Some("RN_001".to_string()),
        },
        DicomScan {
            source_path: PathBuf::from("/in/2.dcm"),
            file_size: 2000,
            mtime: 0,
            metadata: None,
            layer_count: LayerCount::Multi(5),
            computed_hash: Some("hash2".to_string()),
            disposition: FileDisposition::Recovered,
            destination_path: Some(out_dir.join("P2/Volume_2023.dcm")),
            error_reason: None,
            matched_patient_id: Some(2),
            matched_internal_card_id: Some("P2".to_string()),
            matched_root_node: Some("RN_002".to_string()),
        },
        DicomScan {
            source_path: PathBuf::from("/in/3.dcm"),
            file_size: 50,
            mtime: 0,
            metadata: None,
            layer_count: LayerCount::Single,
            computed_hash: None,
            disposition: FileDisposition::Corrupt,
            destination_path: Some(out_dir.join("corrupt/3.dcm")),
            error_reason: Some("Invalid DICM magic".to_string()),
            matched_patient_id: None,
            matched_internal_card_id: None,
            matched_root_node: None,
        },
        DicomScan {
            source_path: PathBuf::from("/in/4.dcm"),
            file_size: 1000,
            mtime: 0,
            metadata: None,
            layer_count: LayerCount::Single,
            computed_hash: Some("hash1".to_string()),
            disposition: FileDisposition::Duplicate,
            destination_path: Some(out_dir.join("duplicates/4.dcm")),
            error_reason: None,
            matched_patient_id: Some(1),
            matched_internal_card_id: Some("P1".to_string()),
            matched_root_node: Some("RN_001".to_string()),
        },
    ];

    let total_unique_root_nodes = 10;
    let recovered_unique_root_nodes = 2; // RN_001, RN_002

    let report = RecoveryReport::build(
        "TEST_SESSION_01".to_string(),
        start,
        end,
        Path::new("/in"),
        &out_dir,
        false,
        false,
        &scans,
        total_unique_root_nodes,
        recovered_unique_root_nodes,
    );

    assert_eq!(report.counts.total_scanned, 4);
    assert_eq!(report.counts.recovered, 2);
    assert_eq!(report.counts.corrupt, 1);
    assert_eq!(report.counts.duplicate, 1);
    assert_eq!(report.counts.unmatched, 0);

    // Rate: 2 / 10 * 100 = 20.0%
    assert_eq!(
        report.database_reconciliation.recovery_rate_percentage,
        20.0
    );
    // Carving loss: 10 - 2 = 8
    assert_eq!(
        report.database_reconciliation.estimated_carving_loss_files,
        8
    );

    // Verify persistent report writing
    report.write_persistent_reports(&out_dir).unwrap();

    let json_file = out_dir.join("recovery_report.json");
    let txt_file = out_dir.join("recovery_report.txt");
    assert!(json_file.exists());
    assert!(txt_file.exists());

    // Verify JSON content deserialization
    let json_str = std::fs::read_to_string(json_file).unwrap();
    let deserialized: RecoveryReport = serde_json::from_str(&json_str).unwrap();
    assert_eq!(deserialized.session.id, "TEST_SESSION_01");
    assert_eq!(deserialized.counts.recovered, 2);

    // Verify formatted text output includes essential sections
    let text = report.format_text_summary();
    assert!(text.contains("DCMSIV RECOVERY & SORTING REPORT"));
    assert!(text.contains("FILE RECOVERY COUNTS"));
    assert!(text.contains("SIDEXIS DATABASE RECONCILIATION"));
    assert!(text.contains("20.00%"));
}
