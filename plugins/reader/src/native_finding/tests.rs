use super::*;

#[test]
fn the_number_moves_in_steps_and_only_a_move_asks_for_a_redraw() {
    let mut finding = Finding::new();
    assert_eq!(finding.shown(), None);
    assert_eq!(finding.key(), FINDING);
    assert!(finding.update(0));
    assert_eq!(finding.key(), FINDING_PERCENT);
    assert!(!finding.update(4));
    assert!(finding.update(5));
    assert!(!finding.update(9));
    assert_eq!(finding.shown(), Some(5));
    assert!(finding.update(99));
    assert_eq!(finding.shown(), Some(95));
    assert!(finding.update(100));
    assert!(!finding.update(250));
    assert_eq!(finding.shown(), Some(100));
}

#[test]
fn both_texts_are_in_both_languages() {
    for catalog in [
        include_str!("../../i18n/en.ini"),
        include_str!("../../i18n/de.ini"),
    ] {
        for key in [FINDING, FINDING_PERCENT, TITLE] {
            assert!(
                catalog
                    .lines()
                    .any(|l| l.starts_with(key) && l[key.len()..].trim_start().starts_with('=')),
                "{key}"
            );
        }
    }
    let en = include_str!("../../i18n/en.ini");
    let de = include_str!("../../i18n/de.ini");
    for catalog in [en, de] {
        let line = catalog
            .lines()
            .find(|l| l.starts_with("reader.toc.finding-percent"))
            .unwrap_or_default();
        assert!(line.contains("{percent}"));
    }
}
