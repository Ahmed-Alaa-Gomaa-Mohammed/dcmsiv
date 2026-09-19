use crate::common::synthetic_dicom::{generate_synthetic_dicom, SyntheticDicomOptions};
use dcmsiv::dicom::hasher::compute_pixel_layer_hash;
use dcmsiv::dicom::header::read_dicom_header;
use dcmsiv::dicom::types::LayerCount;
use std::path::Path;
use tempfile::tempdir;

#[test]
fn test_hasher_single_layer() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("hasher_single.dcm");

    let opts = SyntheticDicomOptions {
        patient_id: Some("P1".to_string()),
        other_patient_ids: None,
        acquisition_date_time: Some("20230101120000".to_string()),
        number_of_frames: 1,
        rows: 32,
        columns: 32,
        bits_allocated: 16,
        pixel_fill_byte: Some(0xAB),
    };

    let info = generate_synthetic_dicom(&file_path, &opts).unwrap();
    let meta = read_dicom_header(&file_path).unwrap();
    let (computed_hash, layer_count) = compute_pixel_layer_hash(&file_path, &meta).unwrap();

    assert_eq!(layer_count, LayerCount::Single);
    assert_eq!(computed_hash, info.middle_layer_hash);
}

#[test]
fn test_hasher_volumetric_odd_and_even_frames() {
    let dir = tempdir().unwrap();

    // Test with 5 frames (odd): floor(5/2) = frame 2
    let file_odd = dir.path().join("vol_odd.dcm");
    let opts_odd = SyntheticDicomOptions {
        number_of_frames: 5,
        rows: 32,
        columns: 32,
        bits_allocated: 16,
        pixel_fill_byte: None,
        ..Default::default()
    };
    let info_odd = generate_synthetic_dicom(&file_odd, &opts_odd).unwrap();
    let meta_odd = read_dicom_header(&file_odd).unwrap();
    let (hash_odd, layer_odd) = compute_pixel_layer_hash(&file_odd, &meta_odd).unwrap();

    assert_eq!(layer_odd, LayerCount::Multi(5));
    assert_eq!(hash_odd, info_odd.middle_layer_hash);

    // Test with 6 frames (even): floor(6/2) = frame 3
    let file_even = dir.path().join("vol_even.dcm");
    let opts_even = SyntheticDicomOptions {
        number_of_frames: 6,
        rows: 32,
        columns: 32,
        bits_allocated: 16,
        pixel_fill_byte: None,
        ..Default::default()
    };
    let info_even = generate_synthetic_dicom(&file_even, &opts_even).unwrap();
    let meta_even = read_dicom_header(&file_even).unwrap();
    let (hash_even, layer_even) = compute_pixel_layer_hash(&file_even, &meta_even).unwrap();

    assert_eq!(layer_even, LayerCount::Multi(6));
    assert_eq!(hash_even, info_even.middle_layer_hash);
}

#[test]
fn test_hasher_real_testset_file_31() {
    let path = Path::new("testset/31.dcm");
    if !path.exists() {
        return;
    }
    let meta = read_dicom_header(path).expect("Failed to read header of 31.dcm");
    let (hash, layer_count) = compute_pixel_layer_hash(path, &meta).expect("Failed to hash 31.dcm");

    assert!(layer_count.is_multi());
    assert_eq!(hash, "b3cf7bdfe02dcfc751fe4d884d62d6ab43c78e6c");
}

#[test]
fn test_hasher_8bit_rgb_bgr_swap_and_dib_layout() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("rgb_test.dcm");

    let opts = crate::common::synthetic_dicom::SyntheticRgbOptions {
        patient_id: Some("RGB_P01".to_string()),
        other_patient_ids: None,
        acquisition_date_time: Some("20260420223801".to_string()),
        rows: 65,
        columns: 65,
        custom_pad_byte: Some(0x7F),
    };

    let info = crate::common::synthetic_dicom::generate_synthetic_rgb_dicom(&file_path, &opts).unwrap();
    let meta = read_dicom_header(&file_path).unwrap();
    let (computed_hash, layer_count) = compute_pixel_layer_hash(&file_path, &meta).unwrap();

    assert_eq!(layer_count, LayerCount::Single);
    assert_eq!(computed_hash, info.bgr_swapped_hash);
}

#[test]
fn test_hasher_8bit_rgb_various_row_strides() {
    let dir = tempdir().unwrap();

    // Test with columns % 4 padding variations:
    // col * 3 % 4:
    // col = 64 -> 192 % 4 = 0 (pad = 0)
    // col = 65 -> 195 % 4 = 3 (pad = 1)
    // col = 66 -> 198 % 4 = 2 (pad = 2)
    // col = 67 -> 201 % 4 = 1 (pad = 3)
    for cols in [64u16, 65, 66, 67] {
        let file_path = dir.path().join(format!("rgb_stride_{cols}.dcm"));
        let opts = crate::common::synthetic_dicom::SyntheticRgbOptions {
            rows: 32,
            columns: cols,
            custom_pad_byte: Some(0x00),
            ..Default::default()
        };

        let info = crate::common::synthetic_dicom::generate_synthetic_rgb_dicom(&file_path, &opts).unwrap();
        let meta = read_dicom_header(&file_path).unwrap();
        let (computed_hash, layer_count) = compute_pixel_layer_hash(&file_path, &meta).unwrap();

        assert_eq!(layer_count, LayerCount::Single);
        assert_eq!(computed_hash, info.bgr_swapped_hash, "Hash mismatch for cols={cols}");
    }
}
