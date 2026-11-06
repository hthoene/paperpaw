use super::*;

#[test]
fn public_i18n_limits_match_the_generated_family_contract() {
    assert_eq!(MAX_KEY_BYTES, wire::I18N_FORMAT_MAX_KEY_BYTES);
    assert_eq!(MAX_ARGUMENT_BYTES, wire::I18N_FORMAT_MAX_ARGUMENT_BYTES);
    assert_eq!(MAX_DIGIT_BYTES, wire::I18N_DIGITS_MAX_BYTES);
    assert_eq!(wire::I18N_FORMAT_MAX_RECORDS, 8);
}

#[test]
fn i18n_arguments_do_not_borrow_registration_limits() {
    let source = include_str!("formatting.rs");
    assert!(!source.contains("REGISTRATION_MAX_RECORDS"));
    assert!(source.contains("I18N_FORMAT_MAX_RECORDS"));
}
