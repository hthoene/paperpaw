//! A chapter that begins a file is shown from its first page when it is
//! chosen in the contents: the pages of a file start at its first block,
//! after offset 0, which is where the contents point.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::*;
use paperpaw_plugin_reader::actions as reader;
use paperpaw_sdk::builtin::actions;

/// Three chapters of a few pages each, a file each, the contents naming
/// the files, no anchors.
fn chapters() -> Vec<u8> {
    let files: Vec<(String, String)> = ["One", "Two", "Three"]
        .iter()
        .enumerate()
        .map(|(i, title)| {
            (
                format!("ch{i}.xhtml"),
                format!("<h1>{title}</h1>{}", filler(title, 3)),
            )
        })
        .collect();
    let files: Vec<(&str, String)> = files.iter().map(|(n, b)| (n.as_str(), b.clone())).collect();
    epub(
        &files,
        &[
            ("One", "ch0.xhtml"),
            ("Two", "ch1.xhtml"),
            ("Three", "ch2.xhtml"),
        ],
    )
}

/// The book opened, its pages counted (the index is complete).
fn counted() -> Reading {
    let mut reading = Reading::new(
        BUTTONS,
        Shared::default(),
        "books/chapters.epub",
        &chapters(),
        "",
    );
    reading.open();
    reading.pass_until(reading.now + 600_000);
    reading.run(actions::NEXT_PAGE);
    reading.run(actions::PREV_PAGE);
    assert!(reading.shown().pages.is_some(), "the pages are counted");
    reading
}

#[test]
fn a_chapter_chosen_in_the_contents_opens_on_its_first_page() {
    let mut reading = counted();
    for (chapter, section) in [("Two", 1), ("Three", 2)] {
        reading.press(reader::TOC);
        assert_eq!(reading.screen(), "reader.toc");
        reading.press(actions::DOWN);
        reading.press(actions::CONFIRM);
        assert_eq!(reading.screen(), "reader.screen");
        let shown = reading.shown();
        assert_eq!(shown.chapter, chapter);
        assert_eq!(shown.at().section, section, "{chapter}'s first page");
        // Its first page: the page before is the last of the chapter before.
        reading.run(actions::PREV_PAGE);
        assert_eq!(reading.shown().at().section, section - 1);
        reading.run(actions::NEXT_PAGE);
        assert_eq!(reading.shown().at(), shown.at());
    }
}
