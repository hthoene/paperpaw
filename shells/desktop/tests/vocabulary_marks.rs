//! The words of the vocabulary list are marked in a book (`vocabulary.mark`):
//! the reader asks the stage `reader.decorations@1` and draws what the
//! vocabulary plugin answers. The emulator runs on a card with the sample book
//! open and a short list of its words; with the marks on the page differs.

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

/// A card with the sample book open as the last one, English, with the
/// words of its first pages in the vocabulary list; the marks on or off.
fn card(name: &str, marks: bool) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("paperpaw-marks-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("books")).unwrap();
    fs::copy(
        root().join("samples/library/books").join(BOOK),
        dir.join("books").join(BOOK),
    )
    .unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/beta3-card");
    copy_tree(&fixture.join("data"), &dir.join(".paperpaw"));
    let _ = fs::remove_file(dir.join(".paperpaw/positions").join(format!("{HASH}.txt")));
    let setup = dir.join(".paperpaw/plugins/setup");
    fs::create_dir_all(&setup).unwrap();
    fs::write(setup.join("done"), "").unwrap();
    // No hyphenation pack on the card: the reader would ask the store for it,
    // and the question would cover the page.
    let ini = format!(
        "[core]\nfont-size = 12\nhyphenate = off\n[vocabulary]\nmark = {}\n",
        if marks { "on" } else { "off" }
    );
    fs::write(dir.join("paperpaw.ini"), ini).unwrap();
    let words: String = ["the", "of", "and", "was", "she", "her", "alice", "rabbit"]
        .iter()
        .map(|w| format!("{w}\ten\t0\t0\t{BOOK}\tAlice\tA sentence with {w}.\n"))
        .collect();
    let folder = dir.join(".paperpaw/plugins/vocabulary");
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("words.txt"), words).unwrap();
    dir
}

/// Runs the emulator on `card` through `requests`, then shoots the page. What
/// the emulator told on its standard error (a plugin's refused call among it).
fn session(card: &Path, shot: &Path, requests: &[String]) -> String {
    let path = paperpaw_core::sdk::json::string(&card.display().to_string());
    let target = paperpaw_core::sdk::json::string(&shot.display().to_string());
    let mut child = Command::new(env!("CARGO_BIN_EXE_paperpaw-emu"))
        .arg("--stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
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
    String::from_utf8_lossy(&out.stderr).into_owned()
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

/// The page of a shot: the rows above the status line, as gray values.
fn page(path: &Path) -> Vec<u8> {
    let decoder = png::Decoder::new(std::io::BufReader::new(fs::File::open(path).unwrap()));
    let mut reader = decoder.read_info().unwrap();
    let mut bytes = vec![0; reader.output_buffer_size().unwrap()];
    let frame = reader.next_frame(&mut bytes).unwrap();
    bytes[..PAGE_ROWS * frame.line_size].to_vec()
}

fn text_page(name: &str, marks: bool) -> Vec<u8> {
    let card = card(name, marks);
    let shot = card.join("page.png");
    // Past the cover and the title page, to a page of the story.
    session(&card, &shot, &turns(5));
    page(&shot)
}

#[test]
fn words_of_the_vocabulary_list_are_marked_on_the_page_when_marks_are_on() {
    let plain = text_page("off", false);
    // The same page twice without marks: the comparison below is of one page.
    assert_eq!(
        plain,
        text_page("off-again", false),
        "the page is not steady"
    );
    let marked = text_page("on", true);
    assert_ne!(
        plain, marked,
        "no word of the list is marked on a page full of them"
    );
}

/// With nothing to mark the plugin answers with a batch of no records: bytes
/// of none are no batch, and the host refused them on every page, logging
/// "call 8 ended with -2" for the plugin.
#[test]
fn the_plugin_answers_every_page_without_a_refused_call() {
    for (name, marks) in [("quiet-off", false), ("quiet-on", true)] {
        let card = card(name, marks);
        let told = session(&card, &card.join("page.png"), &turns(5));
        let refused: Vec<&str> = told.lines().filter(|l| l.contains("ended with")).collect();
        assert!(refused.is_empty(), "marks {marks}: {refused:?}");
    }
}
