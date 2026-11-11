use super::*;

fn entry(title: &str, level: u8) -> Entry {
    Entry::new(String::from(title), level, Place::new(0, 0))
}

#[test]
fn a_title_is_indented_two_spaces_a_level_down_to_the_deepest_shown() {
    assert_eq!(row_title(&entry("One", 0)).unwrap(), "One");
    assert_eq!(row_title(&entry("Two", 1)).unwrap(), "  Two");
    assert_eq!(
        row_title(&entry("Deep", MAX_LEVEL)).unwrap(),
        "        Deep"
    );
    // Deeper than shown is as deep as shown.
    assert_eq!(row_title(&entry("Deeper", 15)).unwrap(), "        Deeper");
}

#[test]
fn the_screen_without_a_book_says_so_and_has_nothing_to_choose() {
    let view = TocView::none("reader.no-toc");
    assert_eq!(view.message, Some("reader.no-toc"));
    assert!(view.entries.is_empty() && view.document.is_none());
}
