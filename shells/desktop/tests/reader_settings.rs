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
    session_on("xteink-x4", card, shot, requests);
}

/// [`session`] on the device `profile`.
fn session_on(profile: &str, card: &Path, shot: &Path, requests: &[String]) {
    let path = paperpaw_core::sdk::json::string(&card.display().to_string());
    let target = paperpaw_core::sdk::json::string(&shot.display().to_string());
    let emu = std::env::var("PP_EMU").unwrap_or_else(|_| env!("CARGO_BIN_EXE_paperpaw-emu").into());
    let mut child = Command::new(emu)
        .arg("--stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut lines = format!(
        "{{\"op\": \"start\", \"profile\": \"{profile}\", \"library\": {path}}}\n\
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
    // comma that ends a line hangs out of the text's edge. The page is one
    // where a line ends in one: kerning moved the line breaks, and the page
    // after 14 turns has none now.
    let ini = |hanging: &str| {
        format!("[core]\nfont-size = 12\njustify = on\nhanging-punctuation = {hanging}\n")
    };
    let book = "alice-in-wonderland.epub";
    let off = shown("hang-off", book, &ini("off"), &turns(13));
    assert_eq!(
        off,
        shown("hang-off2", book, &ini("off"), &turns(13)),
        "the page is not steady"
    );
    let on = shown("hang-on", book, &ini("on"), &turns(13));
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

/// The gray rows of a shot above its status line, and the width of a row.
fn gray_page(path: &Path, rows: usize) -> (Vec<u8>, usize) {
    let decoder = png::Decoder::new(std::io::BufReader::new(fs::File::open(path).unwrap()));
    let mut reader = decoder.read_info().unwrap();
    let mut bytes = vec![0; reader.output_buffer_size().unwrap()];
    let frame = reader.next_frame(&mut bytes).unwrap();
    // One channel of each pixel is its gray: the shots are equal in all three.
    let step = frame.line_size / frame.width as usize;
    let gray = bytes[..rows * frame.line_size]
        .iter()
        .step_by(step)
        .copied()
        .collect();
    (gray, frame.width as usize)
}

/// The ink of each line of text, as (first, last) dark column: the rows of a shot
/// split at the empty rows (a band under 8 rows is a rule or a stray mark and is
/// left out).
fn line_ink(gray: &[u8], width: usize) -> Vec<(usize, usize)> {
    let dark = |row: &[u8]| {
        Some((
            row.iter().position(|&g| g < 140)?,
            row.iter().rposition(|&g| g < 140)?,
        ))
    };
    let mut lines = Vec::new();
    let mut band: Option<(usize, (usize, usize))> = None;
    for row in gray
        .chunks(width)
        .chain(std::iter::once(&vec![255u8; width][..]))
    {
        match (dark(row), band) {
            (Some((a, z)), Some((rows, (first, last)))) => {
                band = Some((rows + 1, (first.min(a), last.max(z))));
            }
            (Some(ink), None) => band = Some((1, ink)),
            (None, Some((rows, ink))) => {
                if rows >= 8 {
                    lines.push(ink);
                }
                band = None;
            }
            (None, None) => {}
        }
    }
    lines
}

/// The ink of the lines of the Lines book (regular, bold and italic text,
/// justified) on the device `profile` at `size` points, `turned` pages in.
fn lines_of(profile: &str, size: u32, turned: usize) -> Vec<(usize, usize)> {
    let rows = if profile == "xteink-x4" { 740 } else { 1400 };
    let ini = format!("[core]\nfont-size = {size}\njustify = on\n");
    let card = card("lines", "lines.html", &ini);
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures-lines/lines.html"),
        card.join("books/lines.html"),
    )
    .unwrap();
    let shot = card.join("page.png");
    session_on(profile, &card, &shot, &turns(turned));
    let (gray, width) = gray_page(&shot, rows);
    let lines = line_ink(&gray, width);
    assert!(lines.len() >= 6, "{profile} {size}: {lines:?}");
    lines
}

#[test]
fn no_line_is_drawn_past_the_margin() {
    // The layout measures the pairs the host draws kerned, so a line is never
    // drawn wider than its measure: ink past the right margin (2 px for the
    // side of a letter) would be clipped text. The X4 and a Boox, two sizes.
    for (profile, margin, size) in [
        ("xteink-x4", 444, 12),
        ("xteink-x4", 444, 10),
        ("boox-go-7", 1216, 12),
        ("boox-go-7", 1216, 16),
    ] {
        for turned in [0usize, 1] {
            let lines = lines_of(profile, size, turned);
            for (_, last) in &lines {
                assert!(
                    *last <= margin + 2,
                    "{profile} {size}: ink at {last}, past {margin}: {lines:?}"
                );
            }
        }
    }
}

#[test]
fn stretched_lines_of_a_boox_reach_the_margin() {
    // Not short of it either: where the layout measured more than was drawn the
    // edge was ragged (12 px short on a Boox before the pairs were measured).
    // A paragraph starts with an indent: the line before one is its last and is
    // not stretched. A stretched line ends at the margin within the room at the
    // side of its last letter (up to 6 px); a line of three words may keep the
    // most its gaps can grow and fall short, so one such line is left out.
    let margin = 1216;
    for size in [12, 10] {
        for turned in [0usize, 1] {
            let lines = lines_of("boox-go-7", size, turned);
            let left = lines.iter().map(|l| l.0).min().unwrap();
            let (mut stretched, mut short) = (0, 0);
            for (at, &(first, last)) in lines.iter().enumerate() {
                let ends_paragraph = lines.get(at + 1).is_none_or(|next| next.0 > left + 8);
                if !ends_paragraph && first <= left + 8 {
                    stretched += 1;
                    short += usize::from(last + 7 < margin);
                }
            }
            assert!(
                stretched >= 3 && short <= 1,
                "{size}: {short} of {stretched} stretched lines end short of {margin}: {lines:?}"
            );
        }
    }
}
