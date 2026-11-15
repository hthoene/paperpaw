//! The reading place and the marks of a book that was converted again (#265).
//!
//! A place saved for another conversion of the book (a Beta 3 card, another
//! picture size) is found again by its progress; one that cannot be found
//! opens the book at its start, and nothing is written to the card when the
//! book opens. Each test starts the emulator on a card with the sample book.
//! Marks a Beta 3 card holds are drawn where their text is, and only there.
//! The last test is for #270: a highlight saved in this conversion is drawn.

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
const HASH: &str = "b1348f1e1a4d5649";
/// The conversion a real Beta 3 card wrote for this book (`tests/beta3-card`).
const BETA3: &str = "af63bc4c8601b62c";
/// The rows of a shot above the status line (its clock changes by the minute).
const PAGE_ROWS: usize = 740;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// A card as a Beta 3 reader left it, with the book open as the last one but
/// no place saved yet.
fn new_card(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("paperpaw-place-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("books")).unwrap();
    fs::copy(
        root().join("samples/library/books").join(BOOK),
        dir.join("books").join(BOOK),
    )
    .unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/beta3-card");
    fs::copy(fixture.join("paperpaw.ini"), dir.join("paperpaw.ini")).unwrap();
    copy_tree(&fixture.join("data"), &dir.join(".paperpaw"));
    fs::remove_file(positions(&dir)).unwrap();
    let setup = dir.join(".paperpaw/plugins/setup");
    fs::create_dir_all(&setup).unwrap();
    fs::write(setup.join("done"), "").unwrap();
    dir
}

fn positions(card: &Path) -> PathBuf {
    card.join(format!(".paperpaw/positions/{HASH}.txt"))
}

/// Runs the emulator on `card` through `requests`, then shoots the page to
/// `shot` and quits.
fn session(card: &Path, shot: &Path, requests: &[String]) {
    let path = paperpaw_core::sdk::json::string(&card.display().to_string());
    let target = paperpaw_core::sdk::json::string(&shot.display().to_string());
    let mut child = Command::new(env!("CARGO_BIN_EXE_paperpaw-emu"))
        .arg("--stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut lines = format!(
        "{{\"op\": \"start\", \"profile\": \"xteink-x4\", \"library\": {path}}}\n\
         {{\"op\": \"wait\", \"ms\": 2500}}\n"
    );
    for request in requests {
        lines.push_str(request);
        lines.push('\n');
    }
    lines.push_str(&format!("{{\"op\": \"shot\", \"path\": {target}}}\n"));
    lines.push_str("{\"op\": \"quit\"}\n");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(lines.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "the emulator failed");
    assert!(shot.exists(), "no shot of the page");
}

/// Turns the page forward `times` times and waits for the place to be saved.
fn turns(times: usize) -> Vec<String> {
    let mut steps = Vec::new();
    for _ in 0..times {
        for pressed in ["true", "false"] {
            steps.push(format!(
                "{{\"op\": \"button\", \"name\": \"page-forward\", \"pressed\": {pressed}}}"
            ));
        }
        steps.push("{\"op\": \"wait\", \"ms\": 400}".to_string());
    }
    // Past the quiet time: the place is written.
    steps.push("{\"op\": \"wait\", \"ms\": 8000}".to_string());
    steps
}

/// The page of a shot: the rows above the status line, as gray values.
fn page(path: &Path) -> Vec<u8> {
    let decoder = png::Decoder::new(std::io::BufReader::new(fs::File::open(path).unwrap()));
    let mut reader = decoder.read_info().unwrap();
    let mut bytes = vec![0; reader.output_buffer_size().unwrap()];
    let frame = reader.next_frame(&mut bytes).unwrap();
    let row = frame.line_size;
    bytes[..PAGE_ROWS * row].to_vec()
}

/// The conversion line of a position file, the value.
fn conversion(file: &str) -> String {
    file.lines()
        .find_map(|l| l.strip_prefix("conversion = "))
        .unwrap()
        .to_string()
}

/// Reads a book on a fresh card for `times` turns: the page it ends on, the
/// position file saved, and the card.
fn read(name: &str, times: usize) -> (Vec<u8>, String, PathBuf) {
    let card = new_card(name);
    let shot = card.join("read.png");
    session(&card, &shot, &turns(times));
    let saved = fs::read_to_string(positions(&card)).unwrap();
    (page(&shot), saved, card)
}

/// The page the book opens at on `card` as it is.
fn opens_at(card: &Path, name: &str) -> Vec<u8> {
    let shot = card.join(format!("{name}.png"));
    session(card, &shot, &[]);
    page(&shot)
}

#[test]
fn a_place_saved_in_another_conversion_is_found_again_by_its_progress() {
    let (read_page, saved, card) = read("another", 3);
    let start = opens_at(&new_card("start"), "start");
    assert_ne!(read_page, start, "three turns left the first page");
    // The same place under another conversion: a Beta 3 value.
    let own = conversion(&saved);
    assert_ne!(own, BETA3);
    fs::write(
        positions(&card),
        saved.replace(
            &format!("conversion = {own}"),
            &format!("conversion = {BETA3}"),
        ),
    )
    .unwrap();
    let before = fs::read_to_string(positions(&card)).unwrap();
    assert_eq!(opens_at(&card, "again"), read_page);
    // Opening wrote nothing: the file is as it was.
    assert_eq!(fs::read_to_string(positions(&card)).unwrap(), before);
}

#[test]
fn a_place_a_beta3_card_wrote_opens_the_book_there_not_at_its_start() {
    // The position file is the real one `tests/beta3-card` holds: section 1,
    // offset 190, 0.6 %, a conversion of Beta 3.
    let card = new_card("beta3");
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("tests/beta3-card/data/positions/{HASH}.txt"));
    fs::copy(fixture, positions(&card)).unwrap();
    let before = fs::read_to_string(positions(&card)).unwrap();
    assert!(before.contains(&format!("conversion = {BETA3}")));
    let there = opens_at(&card, "there");
    let start = opens_at(&new_card("beta3-start"), "start");
    assert_ne!(there, start, "the book opened at its start");
    assert_eq!(fs::read_to_string(positions(&card)).unwrap(), before);
}

#[test]
fn a_place_that_cannot_be_checked_opens_at_the_start_and_stays_in_its_file() {
    // Another conversion and no progress stored with it: not sure to be
    // where it was, so not taken.
    let card = new_card("unsure");
    let file = format!(
        "[position]\nsection = 1\noffset = 463\nconversion = {BETA3}\nupdated = 1791967272\n"
    );
    fs::write(positions(&card), &file).unwrap();
    let start = opens_at(&new_card("unsure-start"), "start");
    assert_eq!(opens_at(&card, "unsure"), start);
    assert_eq!(fs::read_to_string(positions(&card)).unwrap(), file);
}

#[test]
fn the_first_page_turn_after_a_found_place_saves_it_in_this_conversion() {
    let (_, saved, card) = read("moved", 3);
    let own = conversion(&saved);
    fs::write(
        positions(&card),
        saved.replace(
            &format!("conversion = {own}"),
            &format!("conversion = {BETA3}"),
        ),
    )
    .unwrap();
    // One turn from the place found: written with this conversion, a page on.
    let shot = card.join("moved.png");
    session(&card, &shot, &turns(1));
    let after = fs::read_to_string(positions(&card)).unwrap();
    assert_eq!(conversion(&after), own);
    assert_ne!(after, saved);
}

/// Presses a button and waits for the screen to follow.
fn press(name: &str) -> Vec<String> {
    let mut steps = Vec::new();
    for pressed in ["true", "false"] {
        steps.push(format!(
            "{{\"op\": \"button\", \"name\": \"{name}\", \"pressed\": {pressed}}}"
        ));
    }
    steps.push("{\"op\": \"wait\", \"ms\": 400}".to_string());
    steps
}

#[test]
fn a_highlight_saved_in_this_conversion_is_drawn_on_its_page() {
    // Two turns on, one word selected and kept as a highlight (the keys the
    // reader's selection takes), as on a device.
    let card = new_card("drawn");
    let mut keys = turns(2);
    keys.push("{\"op\": \"run\", \"action\": \"reader.select-text\"}".to_string());
    keys.push("{\"op\": \"wait\", \"ms\": 500}".to_string());
    for name in ["confirm", "right", "right", "confirm", "back"] {
        keys.extend(press(name));
    }
    session(&card, &card.join("selecting.png"), &keys);
    let marks = card.join(".paperpaw/annotations");
    let saved = fs::read_dir(&marks).unwrap().count();
    assert_eq!(saved, 1, "the highlight was not saved");
    // The page as it opens with the highlight, and with the file taken away.
    let with = opens_at(&card, "with");
    let kept = card.join("kept");
    fs::rename(&marks, &kept).unwrap();
    let without = opens_at(&card, "without");
    assert_ne!(with, without, "the highlight is not drawn on its page");
}

/// The header and rows as Beta 3 wrote them for this book (`format 2`, its
/// conversion): a highlight on the first page, a bookmark a few pages on.
const BETA3_HEADER: &str = "# format 2\n# book 189249 59feb863c3a0652fe66a99c0978404f9\n";
const BETA3_HIGHLIGHT: &str = "h\t1\t97\t1\t123\t0\t1794701285\t0\taf63bc4c8601b62c\t\
    /body/DocFragment[2]/body/header[1]/div[1]/text()[1].18\t\
    /body/DocFragment[2]/body/header[1]/div[1]/text()[1].44\t\
    the use of anyone any- where\t\n";
const BETA3_BOOKMARK: &str = "b\t1\t1004\t1\t1004\t2\t1794701285\t0\taf63bc4c8601b62c\t\
    /body/DocFragment[2]/body/h2[1]/text()[1].0\t\t\
    by Lewis Carroll THE MILLENNIUM FULCRUM EDITION 3.0 * * *\t\n";

/// The pages from the first on, `count` of them, each as gray rows.
fn walk(card: &Path, name: &str, count: usize) -> Vec<Vec<u8>> {
    let mut pages = Vec::new();
    for upto in 0..count {
        let shot = card.join(format!("{name}-{upto}.png"));
        let mut keys = Vec::new();
        for _ in 0..upto {
            keys.extend(press("page-forward"));
        }
        // Each walk starts at the first page, not where the last one left.
        let _ = fs::remove_file(positions(card));
        session(card, &shot, &keys);
        pages.push(page(&shot));
    }
    pages
}

fn beta3_marks(name: &str, rows: &str) -> (PathBuf, Vec<Vec<u8>>, String) {
    let card = new_card(name);
    let dir = card.join(".paperpaw/annotations");
    fs::create_dir_all(&dir).unwrap();
    let file = format!("{BETA3_HEADER}{rows}");
    fs::write(dir.join(format!("{HASH}.txt")), &file).unwrap();
    let marked = walk(&card, "marked", 8);
    (card, marked, file)
}

fn plain_pages(name: &str) -> Vec<Vec<u8>> {
    walk(&new_card(name), "plain", 8)
}

#[test]
fn a_highlight_a_beta3_card_wrote_is_drawn_on_the_page_with_its_text() {
    let (card, marked, file) = beta3_marks("hl", BETA3_HIGHLIGHT);
    let plain = plain_pages("hl-plain");
    // The page after the cover holds "the use of anyone anywhere".
    assert_ne!(marked[1], plain[1], "the highlight is not drawn");
    let other = (0..8).filter(|k| marked[*k] != plain[*k]).count();
    assert_eq!(other, 1, "the highlight is drawn on pages without its text");
    let path = card.join(format!(".paperpaw/annotations/{HASH}.txt"));
    assert_eq!(
        fs::read_to_string(path).unwrap(),
        file,
        "the file was changed"
    );
}

#[test]
fn a_highlight_whose_text_is_not_on_the_page_is_not_drawn_and_stays_in_its_file() {
    let rows = BETA3_HIGHLIGHT.replace("the use of anyone any- where", "nothing like this text");
    let (card, marked, file) = beta3_marks("text", &rows);
    assert_eq!(
        marked,
        plain_pages("text-plain"),
        "a mark was drawn where its text is not"
    );
    let path = card.join(format!(".paperpaw/annotations/{HASH}.txt"));
    assert_eq!(fs::read_to_string(path).unwrap(), file);
}

#[test]
fn a_bookmark_a_beta3_card_wrote_shows_its_ribbon_on_a_page() {
    let (_, marked, _) = beta3_marks("bm", BETA3_BOOKMARK);
    let plain = plain_pages("bm-plain");
    let shown: Vec<_> = (0..8).filter(|k| marked[*k] != plain[*k]).collect();
    assert_eq!(shown.len(), 1, "the bookmark shows on pages {shown:?}");
}
