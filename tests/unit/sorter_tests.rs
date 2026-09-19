use dcmsiv::engine::sorter::sanitize_folder_name;

#[test]
fn test_sanitize_folder_name_windows_illegal_characters() {
    assert_eq!(sanitize_folder_name("Patient:123/45"), "Patient_123_45");
    assert_eq!(sanitize_folder_name(r#"Name*With"Quotes?"#), "Name_With_Quotes_");
    assert_eq!(sanitize_folder_name("<tag>|pipe"), "_tag__pipe");
    assert_eq!(sanitize_folder_name("   "), "UNKNOWN_PATIENT");
    assert_eq!(sanitize_folder_name("Normal_Chart_123"), "Normal_Chart_123");
}
