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
    assert!(view.targets.is_empty() && view.document.is_none());
}

#[test]
fn a_scan_given_up_says_why_and_lists_nothing() {
    let mut view = TocView::none("reader.preparing");
    view.table.push((entry("Only", 0), false));
    view.targets.push((Place::new(0, 0), false));
    view.give_up(Error::NoMemory);
    assert_eq!(view.message, Some("reader.no-memory"));
    assert!(view.targets.is_empty() && view.table.is_empty() && !view.scanning());
    view.give_up(Error::Io);
    assert_eq!(view.message, Some("reader.error"));
}

#[test]
fn the_rows_of_no_entries_say_there_is_no_table() {
    let mut view = TocView::none("reader.preparing");
    assert!(!view.fill(Vec::new(), |e| e, table_title).unwrap());
    assert_eq!(view.message, Some("reader.no-toc"));
    let two = alloc::vec![(entry("One", 0), false), (entry("Two", 1), true)];
    assert!(view.fill(two, |e| e, table_title).unwrap());
    assert_eq!(view.message, None);
    assert!(!view.cut);
    assert_eq!(
        view.targets,
        alloc::vec![(Place::new(0, 0), false), (Place::new(0, 0), true)]
    );
}

#[test]
fn a_list_past_its_bytes_keeps_the_first_rows_and_says_so() {
    let long = "A".repeat(native_toc::MAX_TITLE);
    let headings: Vec<Entry> = (0..MAX_HEADINGS).map(|_| entry(&long, 0)).collect();
    let fit = native_toc::TOC_BYTES / native_toc::row_cost(&long);
    let mut view = TocView::none("reader.preparing");
    assert!(view.fill(headings, |e| (e, false), heading_title).unwrap());
    assert!(view.cut);
    assert_eq!(view.targets.len(), fit);
    // Titles of 32 bytes all fit.
    let title = "C".repeat(32);
    let short: Vec<Entry> = (0..MAX_HEADINGS).map(|_| entry(&title, 0)).collect();
    assert!(view.fill(short, |e| (e, false), heading_title).unwrap());
    assert!(!view.cut);
    assert_eq!(view.targets.len(), MAX_HEADINGS);
}

#[test]
fn one_heading_past_the_bytes_is_still_listed_alone() {
    let mut view = TocView::none("reader.preparing");
    let huge = "B".repeat(native_toc::TOC_BYTES);
    assert!(
        view.fill(alloc::vec![entry(&huge, 0)], |e| (e, false), heading_title)
            .unwrap()
    );
    assert!(!view.cut);
    assert_eq!(view.targets.len(), 1);
}
