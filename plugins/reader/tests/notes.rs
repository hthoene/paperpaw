//! Notes that link to notes: a wikilink or the path of a note's file opens
//! that note over this one, at its heading if the link names one, and
//! *Back* returns; a link to a note that is not there says so.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::*;
use paperpaw_plugin_reader::actions as reader;
use paperpaw_sdk::Storage;
use paperpaw_sdk::builtin::actions;

const PLAN: &str = "vault/Projects/Plan.md";

fn vault() -> Shared {
    let storage = Shared::default();
    let many = "A paragraph of the other note, with enough words to fill lines.\n\n".repeat(60);
    let notes = [
        (
            PLAN,
            "# Plan\n\nSee [[other note]] and [[Other Note#Later part]] and [[Gone]].\n\n\
             Or [by path](../Other%20note.md)."
                .to_string(),
        ),
        (
            "vault/Other note.md",
            format!("# Other note\n\nFirst part.\n\n{many}## Later part\n\nThe end.\n"),
        ),
        ("vault/.obsidian/app.json", "{}".to_string()),
    ];
    for (path, text) in notes {
        let dir = path.rsplit_once('/').unwrap().0;
        for part in ["vault", dir] {
            let _ = storage.0.create_dir(part);
        }
        storage.0.write(path, text.as_bytes()).unwrap();
    }
    storage
}

fn note() -> Reading {
    let mut reading = Reading::new(BUTTONS, vault(), PLAN, b"", "");
    reading.open();
    reading
}

/// Chooses the `n`th link of the page (from 0) and follows it.
fn follow(reading: &mut Reading, n: usize) {
    reading.press(reader::LINKS);
    for _ in 0..n {
        reading.press(actions::RIGHT);
    }
    reading.press(actions::CONFIRM);
}

#[test]
fn a_wikilink_opens_the_note_by_its_name_and_back_returns() {
    let mut reading = note();
    assert_eq!(reading.shown().path, PLAN);
    follow(&mut reading, 0);
    assert_eq!(reading.screen(), "reader.screen");
    assert_eq!(reading.shown().path, "vault/Other note.md");
    assert!(reading.shown().at().offset < 100, "from the top");
    reading.press(actions::BACK);
    assert_eq!(reading.screen(), "reader.screen");
    assert_eq!(reading.shown().path, PLAN, "back to the first note");
}

#[test]
fn a_link_with_a_heading_opens_there() {
    let mut reading = note();
    follow(&mut reading, 1);
    assert_eq!(reading.shown().path, "vault/Other note.md");
    assert!(
        reading.shown().at().offset > 1000,
        "at the heading, not the top"
    );
    // Back leads to the note it came from, and says so.
    assert!(
        reading.described().contains("Back"),
        "{}",
        reading.described()
    );
    reading.press(actions::BACK);
    assert_eq!(reading.shown().path, PLAN);
}

#[test]
fn the_path_of_a_note_opens_it_too() {
    let mut reading = note();
    follow(&mut reading, 3);
    assert_eq!(reading.shown().path, "vault/Other note.md");
}

#[test]
fn a_note_that_is_not_there_is_said_and_the_page_stays() {
    let mut reading = note();
    follow(&mut reading, 2);
    assert_eq!(reading.shown().path, PLAN);
    assert_eq!(reading.screen(), "reader.screen");
}

/// A note with a long front matter (hidden by the vault's Markdown, text
/// in the built-in one) and many paragraphs.
fn long_note() -> String {
    // 90 lines of properties (the vault's Markdown takes 100 at most).
    let properties: String = (0..90)
        .map(|n| {
            format!("key{n}: the value of the property number {n}, with some more words in it\n")
        })
        .collect();
    format!("---\n{properties}---\n\n{}", numbered_book(120, 12))
}

#[test]
#[ignore = "needs native activation (format removal)"]
fn a_place_kept_with_the_other_markdown_is_found_by_how_far_it_was() {
    let storage = Shared::default();
    let data = long_note();
    let path = "books/long.md";
    let mut with = Reading::new(BUTTONS, storage.clone(), path, data.as_bytes(), "");
    with.open();
    for _ in 0..25 {
        with.press(actions::NEXT_PAGE);
    }
    let before = with.shown().progress;
    assert!(before > 25.0, "{before}");
    with.kept();
    // The plugin off: the built-in Markdown, other text (the front matter
    // is shown): the place is where it was in the book, not its offset.
    let mut without = Reading::without_vault(BUTTONS, storage.clone(), path, "");
    without.open();
    let there = without.shown().progress;
    assert!((there - before).abs() < 3.0, "{before} -> {there}");
    without.kept();
    // And on again.
    let mut again = Reading::new(BUTTONS, storage, path, b"", "");
    again.open();
    let back = again.shown().progress;
    assert!((back - before).abs() < 3.0, "{before} -> {back}");
}
