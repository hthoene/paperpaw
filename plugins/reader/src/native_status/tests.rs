//! The choice of a line through the state, the lines' kinds and the percent.
use alloc::string::String;

use paperpaw_sdk::MemoryStorage;

use super::*;

const LINES: &str = "core.status-line.items, reader.status-line.progress, reader.status-line.empty";

#[test]
fn a_line_is_one_of_ours_another_plugins_or_none() {
    assert!(matches!(kind("core.status-line.items"), Some(Line::Items)));
    assert!(matches!(
        kind("reader.status-line.items"),
        Some(Line::Items)
    ));
    assert!(matches!(kind(PROGRESS_LINE), Some(Line::Progress)));
    assert!(matches!(kind(EMPTY_LINE), Some(Line::Empty)));
    assert!(matches!(
        kind("home.status-line"),
        Some(Line::Other("home.status-line"))
    ));
    assert!(kind("").is_none());
}

#[test]
fn the_percent_is_whole_and_cut() {
    assert_eq!(percent(0), 0);
    assert_eq!(percent(9_999), 0);
    assert_eq!(percent(10_000), 1);
    assert_eq!(percent(999_999), 99);
    assert_eq!(percent(1_000_000), 100);
}

#[test]
fn a_tap_shows_the_next_line_and_it_is_kept() {
    let storage = MemoryStorage::new();
    let mut state = State::default();
    state.load(&storage).unwrap();
    assert_eq!(state.showing(false, LINES), "core.status-line.items");
    state.advance(false, LINES, &storage).unwrap();
    assert_eq!(state.showing(false, LINES), "reader.status-line.progress");
    // The top is its own place.
    assert_eq!(state.showing(true, LINES), "core.status-line.items");
    state
        .advance(true, "reader.status-line.empty", &storage)
        .unwrap();
    // Read again by the next screen.
    let mut again = State::default();
    again.load(&storage).unwrap();
    assert_eq!(again.showing(false, LINES), "reader.status-line.progress");
    assert_eq!(again.showing(true, LINES), "reader.status-line.empty");
    assert_eq!(
        choice::load(&storage).unwrap(),
        [
            String::from("reader.status-line.empty"),
            String::from("reader.status-line.progress")
        ]
    );
}

#[test]
fn a_choice_that_is_gone_from_the_lines_shows_the_first() {
    let mut state = State::default();
    state.chosen[1] = String::from("gone.line");
    assert_eq!(state.showing(false, LINES), "core.status-line.items");
}
