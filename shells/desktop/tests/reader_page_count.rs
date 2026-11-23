//! The reader's page count on the X4 Classic (#506).
//!
//! The status line shows "12 / 340" once the book's pages are counted, a
//! percentage before. The count runs in steps of background work, and the
//! host lets work declared in a run come due a second later at the soonest
//! (`work.md` 1.1): a step of 8 pages a second left a fresh book at "0 %" for
//! most of a minute, where Beta 3 showed its pages at once. A book opened on a
//! fresh card shows its page and pages within moments.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const BOOK: &str = "alice-in-wonderland.epub";

/// A card with the sample book as the one read last, past the setup.
fn card() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("paperpaw-page-count-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("books")).unwrap();
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../samples/library/books")
            .join(BOOK),
        dir.join("books").join(BOOK),
    )
    .unwrap();
    let reader = dir.join(".paperpaw/plugins/reader");
    fs::create_dir_all(&reader).unwrap();
    fs::write(
        reader.join("state.ini"),
        format!("[reader]\nlast = \"books/{BOOK}\"\n"),
    )
    .unwrap();
    let setup = dir.join(".paperpaw/plugins/setup");
    fs::create_dir_all(&setup).unwrap();
    fs::write(setup.join("done"), "").unwrap();
    // English is hyphenated as on a device (the firmware seeds the pack): the
    // reader would ask the store for it otherwise.
    for (path, bytes) in paperpaw_default_packs::english() {
        let target = dir
            .join("plugins")
            .join(paperpaw_default_packs::HYPHENATION)
            .join(path);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(target, bytes).unwrap();
    }
    dir
}

/// Whether `label` is a page of pages, as "1 / 343".
fn page_of_pages(label: &str) -> bool {
    label.split_once(" / ").is_some_and(|(page, pages)| {
        [page, pages]
            .iter()
            .all(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
    })
}

#[test]
fn a_fresh_book_shows_its_page_and_pages_within_moments() {
    let library = card();
    let path = paperpaw_core::sdk::json::string(&library.display().to_string());
    let mut child = Command::new(env!("CARGO_BIN_EXE_paperpaw-emu"))
        .arg("--stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    // The start opens the book read last (no hints over it); then 3 seconds
    // of reading.
    let requests = format!(
        "{{\"op\": \"start\", \"profile\": \"xteink-x4-classic\", \"library\": {path}, \
         \"settings\": \"[help]\\nshow = false\\n\"}}\n\
         {{\"op\": \"wait\", \"ms\": 2500}}\n\
         {{\"op\": \"wait\", \"ms\": 3000}}\n\
         {{\"op\": \"describe\"}}\n\
         {{\"op\": \"quit\"}}\n"
    );
    child
        .stdin
        .take()
        .unwrap()
        .write_all(requests.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let said = String::from_utf8_lossy(&out.stdout);
    let described = said
        .lines()
        .find(|l| l.contains("\"event\": \"described\""))
        .unwrap_or_else(|| panic!("no description in:\n{said}"));
    assert!(
        described.contains("\"screen\": \"reader.screen\""),
        "{described}"
    );
    let labels: Vec<&str> = described
        .split("\"label\": \"")
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
        .collect();
    assert!(
        labels.iter().any(|label| page_of_pages(label)),
        "no page of pages in the status line: {labels:?}"
    );
    let _ = fs::remove_dir_all(&library);
}
