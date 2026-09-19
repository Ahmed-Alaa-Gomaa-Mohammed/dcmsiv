use dcmsiv::db::csv_reader::{parse_mediabase_csv, parse_patient_csv};
use std::path::Path;

#[test]
fn test_parse_real_patient_csv() {
    let path = Path::new("testset/Patient.csv");
    if !path.exists() {
        return;
    }
    let index = parse_patient_csv(path).expect("Failed to parse Patient.csv");
    assert!(!index.by_patient_id.is_empty());
    assert!(!index.by_internal_card_id.is_empty());

    // Check known record from sample: PatientId=1, InternalCardId=1
    let patient1 = index.get_by_patient_id(1).expect("Patient 1 not found");
    assert_eq!(patient1.internal_card_id, "1");
}

#[test]
fn test_parse_real_mediabase_csv() {
    let path = Path::new("testset/MediaBase.csv");
    if !path.exists() {
        return;
    }
    let index = parse_mediabase_csv(path).expect("Failed to parse MediaBase.csv");
    assert!(!index.by_hash.is_empty());
    assert!(index.total_distinct_root_nodes() > 0);

    // Check known record from sample:
    // Hash: b9a02a474250aec61f6de835c52d378d75743c2c
    let hash = "b9a02a474250aec61f6de835c52d378d75743c2c";
    let records = index.get_by_hash(hash).expect("Hash not found");
    assert!(!records.is_empty());
    assert_eq!(records[0].patient_id, 1);
}
