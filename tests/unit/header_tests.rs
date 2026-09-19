use crate::common::synthetic_dicom::{
    generate_corrupt_dicom, generate_synthetic_dicom, SyntheticDicomOptions,
};
use dcmsiv::dicom::header::{read_dicom_header, DicomHeaderError};
use tempfile::tempdir;

#[test]
fn test_read_synthetic_dicom_header_single_layer() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("test_single.dcm");

    let opts = SyntheticDicomOptions {
        patient_id: Some("P12345".to_string()),
        other_patient_ids: None,
        acquisition_date_time: Some("20230515123045".to_string()),
        number_of_frames: 1,
        rows: 64,
        columns: 64,
        bits_allocated: 16,
        pixel_fill_byte: None,
    };

    let info = generate_synthetic_dicom(&file_path, &opts).expect("Failed to create synthetic");
    let meta = read_dicom_header(&file_path).expect("Failed to read header");

    assert_eq!(meta.patient_id.as_deref(), Some("P12345"));
    assert_eq!(meta.other_patient_ids, None);
    assert!(meta.acquisition_datetime.is_some());
    let dt = meta.acquisition_datetime.unwrap();
    assert_eq!(dt.format("%Y-%m-%d %H:%M:%S").to_string(), "2023-05-15 12:30:45");
    assert_eq!(meta.number_of_frames, 1);
    assert_eq!(meta.rows, 64);
    assert_eq!(meta.columns, 64);
    assert_eq!(meta.bits_allocated, 16);
    assert!(meta.pixel_data_offset > 132);
    assert_eq!(meta.pixel_data_length as usize, info.total_pixel_bytes);
}

#[test]
fn test_read_synthetic_dicom_header_volumetric_and_fallback_tag() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("test_volume.dcm");

    let opts = SyntheticDicomOptions {
        patient_id: None,
        other_patient_ids: Some("FALLBACK_CHART_99".to_string()),
        acquisition_date_time: Some("20220101091530.123456".to_string()),
        number_of_frames: 10,
        rows: 128,
        columns: 128,
        bits_allocated: 16,
        pixel_fill_byte: None,
    };

    let info = generate_synthetic_dicom(&file_path, &opts).expect("Failed to create synthetic");
    let meta = read_dicom_header(&file_path).expect("Failed to read header");

    assert_eq!(meta.patient_id, None);
    assert_eq!(meta.other_patient_ids.as_deref(), Some("FALLBACK_CHART_99"));
    assert_eq!(meta.number_of_frames, 10);
    assert_eq!(meta.rows, 128);
    assert_eq!(meta.columns, 128);
    assert_eq!(meta.pixel_data_length as usize, info.total_pixel_bytes);
}

#[test]
fn test_read_corrupt_magic_fails() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("corrupt_magic.dcm");
    generate_corrupt_dicom(&file_path, true).unwrap();

    let err = read_dicom_header(&file_path).unwrap_err();
    assert!(matches!(err, DicomHeaderError::InvalidMagic));
}

#[test]
fn test_read_truncated_dicom_fails() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("truncated.dcm");
    // Write only 50 bytes
    std::fs::write(&file_path, [0u8; 50]).unwrap();

    let err = read_dicom_header(&file_path).unwrap_err();
    assert!(matches!(err, DicomHeaderError::PrematureEof));
}
