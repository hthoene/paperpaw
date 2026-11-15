//! Reading settings the native reader applies to a book: the publisher's
//! styles (the book opens with or without them, and again at its place when
//! the setting changes) and hanging punctuation. Each is seen on the page: it
//! differs when the setting does, and two runs of one setting do not.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The rows of a shot above the status line (its clock changes by the minute).
const PAGE_ROWS: usize = 740;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A card with `book` open, English, and `ini` as its settings.
fn card(name: &str, book: &str, ini: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("paperpaw-reading-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let books = dir.join("books");
    fs::create_dir_all(&books).unwrap();
    for source in [
        root().join("samples/library/books"),
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures"),
    ] {
        for entry in fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_file() {
                fs::copy(entry.path(), books.join(entry.file_name())).unwrap();
            }
        }
    }
    let setup = dir.join(".paperpaw/plugins/setup");
    fs::create_dir_all(&setup).unwrap();
    fs::write(setup.join("done"), "").unwrap();
    let state = dir.join(".paperpaw/plugins/reader");
    fs::create_dir_all(&state).unwrap();
    fs::write(
        state.join("state.ini"),
        format!("[reader]\nlast = books/{book}\n"),
    )
    .unwrap();
    // The first-run hints would cover the page.
    fs::write(
        dir.join("paperpaw.ini"),
        format!("{ini}[help]\nshow = off\n"),
    )
    .unwrap();
    dir
}

/// Runs the emulator on `card` through `requests`, then shoots the page.
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

/// Turns the page forward `times` times.
fn turns(times: usize) -> Vec<String> {
    let mut steps = Vec::new();
    for _ in 0..times {
        for pressed in ["true", "false"] {
            steps.push(format!(
                "{{\"op\": \"button\", \"name\": \"page-forward\", \"pressed\": {pressed}}}"
            ));
        }
        steps.push("{\"op\": \"wait\", \"ms\": 1200}".to_string());
    }
    steps.push("{\"op\": \"wait\", \"ms\": 1500}".to_string());
    steps
}

/// A setting changed while the book is open.
fn set(id: &str, value: &str) -> Vec<String> {
    vec![
        format!("{{\"op\": \"set\", \"id\": \"{id}\", \"value\": \"{value}\"}}"),
        "{\"op\": \"wait\", \"ms\": 3000}".to_string(),
    ]
}

/// The page of a shot: the rows above the status line, as gray values.
fn page(path: &Path) -> Vec<u8> {
    let decoder = png::Decoder::new(std::io::BufReader::new(fs::File::open(path).unwrap()));
    let mut reader = decoder.read_info().unwrap();
    let mut bytes = vec![0; reader.output_buffer_size().unwrap()];
    let frame = reader.next_frame(&mut bytes).unwrap();
    bytes[..PAGE_ROWS * frame.line_size].to_vec()
}

/// The page after `requests` on a card with `ini`.
fn shown(name: &str, book: &str, ini: &str, requests: &[String]) -> Vec<u8> {
    let card = card(name, book, ini);
    let shot = card.join("page.png");
    session(&card, &shot, requests);
    page(&shot)
}

#[test]
fn hanging_punctuation_moves_commas_and_quotes_into_the_margin() {
    // A page of the story (past the cover and the contents), justified: the
    // comma that ends a line hangs out of the text's edge.
    let ini = |hanging: &str| {
        format!("[core]\nfont-size = 12\njustify = on\nhanging-punctuation = {hanging}\n")
    };
    let book = "alice-in-wonderland.epub";
    let off = shown("hang-off", book, &ini("off"), &turns(14));
    assert_eq!(
        off,
        shown("hang-off2", book, &ini("off"), &turns(14)),
        "the page is not steady"
    );
    let on = shown("hang-on", book, &ini("on"), &turns(14));
    assert_ne!(off, on, "hanging punctuation changed nothing on the page");
}

#[test]
fn the_publishers_styles_are_off_when_the_setting_says_so() {
    let ini = |styles: &str| format!("[core]\nfont-size = 12\npublisher-styles = {styles}\n");
    let styled = shown("styles-on", "small-caps.html", &ini("on"), &[]);
    assert_eq!(
        styled,
        shown("styles-on2", "small-caps.html", &ini("on"), &[]),
        "not steady"
    );
    let plain = shown("styles-off", "small-caps.html", &ini("off"), &[]);
    assert_ne!(styled, plain, "the book looks the same without its styles");
}

#[test]
fn turning_the_styles_off_opens_the_book_again_where_it_was() {
    let ini = "[core]\nfont-size = 12\npublisher-styles = on\n";
    let book = "alice-in-wonderland.epub";
    let mut changed = turns(5);
    changed.extend(set("core.publisher-styles", "false"));
    let after = shown("reopen", book, ini, &changed);
    // Not the page of the first opening, and not the page with the styles.
    let first = shown(
        "reopen-first",
        book,
        "[core]\nfont-size = 12\npublisher-styles = off\n",
        &[],
    );
    let before = shown("reopen-before", book, ini, &turns(5));
    assert_ne!(after, first, "the book opened again at its beginning");
    assert_ne!(
        after, before,
        "the setting changed nothing on the open book"
    );
}
