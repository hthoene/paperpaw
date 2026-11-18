use super::*;
use crate::fail_alloc::sweep;
use alloc::string::ToString;

fn entry(title: &str, level: u8, section: u32, offset: u32) -> Entry {
    Entry::new(title.to_string(), level, Place::new(section, offset))
}

fn sample() -> Vec<Entry> {
    alloc::vec![
        entry("Part", 1, 0, 0),
        entry("One", 2, 1, 0),
        entry("Two", 3, 2, 40),
        entry("Deep", 9, 3, 7),
    ]
}

#[test]
fn a_title_is_its_words_cut_at_the_limit() {
    assert_eq!(
        title_of("  A   long\n title ").unwrap().as_deref(),
        Some("A long title")
    );
    assert_eq!(title_of(" \n\t").unwrap(), None);
    let long = "ä".repeat(300);
    assert_eq!(title_of(&long).unwrap().unwrap().chars().count(), MAX_TITLE);
    let words = "abc ".repeat(100);
    assert!(title_of(&words).unwrap().unwrap().chars().count() <= MAX_TITLE);
}

#[test]
fn levels_count_from_the_highest_and_stop_at_four() {
    let mut found = sample();
    normalize(&mut found);
    let levels: Vec<u8> = found.iter().map(|e| e.level).collect();
    assert_eq!(levels, [0, 1, 2, 4]);
    let mut none: Vec<Entry> = Vec::new();
    normalize(&mut none);
}

#[test]
fn the_kept_copy_is_read_back_for_the_same_size_only() {
    let text = kept_text(1234, &sample()).unwrap();
    assert!(text.starts_with("size 1234\n0\t0\t1\tPart\n"));
    assert_eq!(kept_headings(&text, 1234).unwrap(), Some(sample()));
    assert_eq!(kept_headings(&text, 1235).unwrap(), None);
    assert_eq!(kept_headings("", 1).unwrap(), None);
    assert_eq!(kept_headings("size x\n", 1).unwrap(), None);
    // Lines that are not entries are left out; a title may hold spaces.
    let odd = "size 5\n1\t2\t0\tA title\nbad\n1\tx\t0\tno\n3\t4\t1\n";
    assert_eq!(
        kept_headings(odd, 5).unwrap(),
        Some(alloc::vec![entry("A title", 0, 1, 2)])
    );
}

#[test]
fn no_more_than_the_limit_are_read_back() {
    let mut text = String::from("size 1\n");
    for i in 0..(MAX_HEADINGS + 20) {
        text.push_str(&alloc::format!("0\t{i}\t0\tx\n"));
    }
    assert_eq!(
        kept_headings(&text, 1).unwrap().unwrap().len(),
        MAX_HEADINGS
    );
}

#[test]
fn headings_are_preferred_when_they_are_more() {
    assert!(wants_scan(0) && wants_scan(1) && !wants_scan(2));
    assert_eq!(prefer(Some(sample()), 4), None);
    assert_eq!(prefer(Some(sample()), 3).map(|h| h.len()), Some(4));
    assert_eq!(prefer(None, 0), None);
}

#[test]
fn the_focus_is_the_last_entry_at_or_before_the_place() {
    let found = sample();
    assert_eq!(current(&found, Place::new(0, 0)), 0);
    assert_eq!(current(&found, Place::new(2, 39)), 1);
    assert_eq!(current(&found, Place::new(2, 40)), 2);
    assert_eq!(current(&found, Place::new(9, 0)), 3);
    assert_eq!(current(&[], Place::new(1, 1)), 0);
    assert_eq!(current(&found[1..], Place::new(0, 0)), 0);
}

#[test]
fn a_books_own_table_is_followed_by_exact_targets_in_the_chapter() {
    let sections = [0u32, 1, 1, 2];
    let targets = [
        Place::new(0, 0),
        Place::new(1, 0),
        Place::new(1, 90),
        Place::new(2, 0),
    ];
    let focus = |at| current_of_table(sections.iter().copied(), at, |i| targets.get(i).copied());
    assert_eq!(focus(Place::new(0, 500)), 0);
    assert_eq!(focus(Place::new(1, 10)), 1);
    assert_eq!(focus(Place::new(1, 90)), 2);
    assert_eq!(focus(Place::new(2, 0)), 3);
    assert_eq!(focus(Place::new(1, 10)), 1);
    // An entry without a target in the chapter is not before the place.
    let none = current_of_table(sections.iter().copied(), Place::new(1, 100), |_| None);
    assert_eq!(none, 0);
}

#[test]
fn the_scan_goes_on_past_the_block() {
    assert_eq!(next_place(Place::new(2, 10), 50), Place::new(2, 50));
    assert_eq!(next_place(Place::new(2, 10), 10), Place::new(2, 11));
    assert_eq!(
        next_place(Place::new(2, u32::MAX), 0),
        Place::new(2, u32::MAX)
    );
}

#[test]
fn a_place_needs_both_numbers() {
    assert_eq!(place_of(Some("3"), Some("40")), Some(Place::new(3, 40)));
    assert_eq!(place_of(Some("3"), None), None);
    assert_eq!(place_of(Some("x"), Some("1")), None);
    assert_eq!(place_of(Some("-1"), Some("1")), None);
}

#[test]
fn headings_path_is_in_the_books_folder() {
    assert_eq!(headings_path("books/ab").unwrap(), "books/ab/headings.txt");
}

#[test]
fn every_allocation_may_be_refused() {
    let found = sample();
    let text = kept_text(7, &found).unwrap();
    sweep(|| {
        title_of("A  title with words")?;
        headings_path("b")?;
        kept_text(7, &found)?;
        kept_headings(&text, 7)?;
        Ok(())
    });
}

#[test]
fn a_kept_copy_over_the_cap_is_not_read() {
    let mut text = String::from("size 1\n");
    while text.len() <= MAX_KEPT {
        text.push_str("0\t0\t0\tx\n");
    }
    assert_eq!(kept_headings(&text, 1).unwrap(), None);
}
