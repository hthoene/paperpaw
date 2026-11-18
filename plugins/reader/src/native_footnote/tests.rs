use super::*;

#[test]
fn a_long_note_is_cut_at_a_character_boundary() {
    let long: String = "é".repeat(MAX_TEXT);
    let kept = clip(&long);
    assert!(kept.len() <= MAX_TEXT);
    assert!(long.starts_with(kept));
    assert_eq!(clip("short"), "short");
}

#[test]
fn the_buttons_have_words_in_both_languages() {
    for file in [
        include_str!("../../i18n/en.ini"),
        include_str!("../../i18n/de.ini"),
    ] {
        for key in BUTTONS {
            assert!(file.lines().any(|l| l.starts_with(key)), "{key}");
        }
    }
}
