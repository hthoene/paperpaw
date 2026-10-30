//! The reading aids on any text (property based) and on odd lists of
//! words to replace: no panic, styles stay on the text's character
//! boundaries and in order, the book's own styles are kept, and words
//! are replaced whole only.

extern crate std;

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::document::{Align, OffsetMap};
use proptest::prelude::*;

use super::*;

fn block(text: &str, runs: Vec<Run>) -> Block {
    Block::new(BlockKind::Paragraph, 0, text.len() as u32, text.to_string())
        .map(OffsetMap::IDENTITY)
        .runs(runs)
        .align(Align::default())
}

/// The runs are on the text: inside it, on character boundaries, not
/// empty, in order.
fn runs_fit(block: &Block) -> Result<(), String> {
    let mut last = 0;
    for run in &block.runs {
        let (start, end) = (run.start as usize, run.end as usize);
        if start >= end || end > block.text.len() {
            return Err(alloc::format!("{run:?} in {:?}", block.text));
        }
        if !block.text.is_char_boundary(start) || !block.text.is_char_boundary(end) {
            return Err(alloc::format!("{run:?} off a boundary in {:?}", block.text));
        }
        if start < last {
            return Err(alloc::format!("{run:?} out of order"));
        }
        last = start;
    }
    Ok(())
}

#[test]
fn bold_beginnings_count_letters_not_bytes() {
    let mut b = block("Größe ÄÖÜ zweiundzwanzig ab a 日本語", Vec::new());
    bold_beginnings(&mut b);
    let bold: Vec<&str> = b
        .runs
        .iter()
        .map(|r| &b.text[r.start as usize..r.end as usize])
        .collect();
    assert_eq!(bold, ["Grö", "ÄÖ", "zweiund", "a", "日本"]);
}

#[test]
fn bold_beginnings_leave_the_books_styles_and_other_blocks() {
    let italic = Run::new(0, 4, TextStyle::ITALIC);
    let mut b = block("Once upon", Vec::from([italic]));
    bold_beginnings(&mut b);
    assert_eq!(b.runs.len(), 2);
    assert_eq!(b.runs[0], italic);
    assert_eq!(
        &b.text[b.runs[1].start as usize..b.runs[1].end as usize],
        "up"
    );
    // Not in headings.
    let mut heading = block("Chapter One", Vec::new());
    heading.kind = BlockKind::Heading(1);
    bold_beginnings(&mut heading);
    assert!(heading.runs.is_empty());
}

#[test]
fn replacement_lists_odd_and_empty() {
    assert!(pairs("").is_empty());
    assert!(pairs(";;;").is_empty());
    assert!(pairs("no equals sign").is_empty());
    assert_eq!(
        pairs("a=b=c; ä = ü ;x =   "),
        [
            (String::from("a"), String::from("b=c")),
            (String::from("ä"), String::from("ü")),
            (String::from("x"), String::new()),
        ]
    );
}

#[test]
fn replaced_words_are_whole_and_may_be_removed() {
    let mut b = block("cat cats catcat concat cat.", Vec::new());
    replace(&mut b, &pairs("cat = dog"));
    assert_eq!(b.text, "dog cats catcat concat dog.");
    let mut b = block("um, well, um", Vec::new());
    replace(&mut b, &pairs("um ="));
    assert_eq!(b.text, ", well, ");
    // A replacement that holds the word is not replaced again.
    let mut b = block("Bob met Bob", Vec::new());
    replace(&mut b, &pairs("Bob = Bobby Bob"));
    assert_eq!(b.text, "Bobby Bob met Bobby Bob");
    // Unicode letters count as parts of words.
    let mut b = block("Straße Straßen", Vec::new());
    replace(&mut b, &pairs("Straße = Strasse"));
    assert_eq!(b.text, "Strasse Straßen");
}

#[test]
fn styles_move_with_replaced_words() {
    // `Alice` is bold; `Mr.` grows before it.
    let bold = Run::new(7, 12, TextStyle::BOLD);
    let mut b = block("Mr. X: Alice", Vec::from([bold]));
    replace(&mut b, &pairs("Mr. = Mister"));
    assert_eq!(b.text, "Mister X: Alice");
    let run = b.runs[0];
    assert_eq!(&b.text[run.start as usize..run.end as usize], "Alice");
}

fn words() -> impl Strategy<Value = String> {
    proptest::collection::vec(
        prop_oneof![
            Just("a"),
            Just("ab"),
            Just("Größe"),
            Just("日本"),
            Just(" "),
            Just(", "),
            Just("."),
            Just("x1"),
            Just("é"),
            Just("\u{301}"),
        ],
        0..30,
    )
    .prop_map(|parts| parts.concat())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256)))]

    /// Any text: bold runs fit the text and the book's runs stay.
    #[test]
    fn bold_beginnings_fit_any_text(text in any::<String>(), book in 0usize..3) {
        let mut runs = Vec::new();
        // A style of the book over its first characters, if any.
        let end = text.char_indices().nth(book).map_or(text.len(), |(i, _)| i);
        if end > 0 {
            runs.push(Run::new(0, end as u32, TextStyle::ITALIC));
        }
        let kept = runs.clone();
        let mut b = block(&text, runs);
        bold_beginnings(&mut b);
        prop_assert_eq!(&b.text, &text);
        runs_fit(&b).map_err(TestCaseError::fail)?;
        for run in &kept {
            prop_assert!(b.runs.contains(run));
        }
    }

    /// Any words replaced in any text: no panic, styles fit the text,
    /// and a word replaced by itself changes nothing.
    #[test]
    fn replacing_fits_any_text(text in words(), from in words(), to in words()) {
        let mut b = block(&text, Vec::new());
        bold_beginnings(&mut b);
        let list = Vec::from([(from.trim().to_string(), to.clone())]);
        if !list[0].0.is_empty() {
            replace(&mut b, &list);
        }
        runs_fit(&b).map_err(TestCaseError::fail)?;
        let mut same = block(&text, Vec::new());
        let itself = Vec::from([(from.clone(), from.clone())]);
        if !from.is_empty() {
            replace(&mut same, &itself);
        }
        prop_assert_eq!(same.text, text);
    }
}
