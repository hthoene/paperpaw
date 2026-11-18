//! The interface-6 text filters of the reading aids, kept only as the
//! reference that `reader_filter::apply` is tested against (parity tests in
//! `reader_filter/tests.rs`, property tests in `filters_tests.rs`). No plugin
//! registers them any more.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::Translator;
use paperpaw_sdk::document::{Align, Block, BlockKind, Run, TextStyle};
use paperpaw_sdk::filters::Verdict;

use crate::pictures::{CAPTION, CHOICES, HIDE, SHOW};
use crate::whole_word;

/// The pairs of a replacement list: `from = to` separated by `;`.
pub(crate) fn pairs(list: &str) -> Vec<(String, String)> {
    list.split(';')
        .filter_map(|pair| {
            let (from, to) = pair.split_once('=')?;
            let from = from.trim();
            (!from.is_empty()).then(|| (from.to_string(), to.trim().to_string()))
        })
        .collect()
}

/// Replaces the pairs' words in `block`, last first so earlier places
/// stay where they are.
pub(crate) fn replace(block: &mut Block, pairs: &[(String, String)]) {
    for (from, to) in pairs {
        let mut found: Vec<usize> = block
            .text
            .match_indices(from.as_str())
            .map(|(at, _)| at)
            .filter(|&at| whole_word(&block.text, at, at + from.len()))
            .collect();
        found.reverse();
        for at in found {
            block.replace(at..at + from.len(), to);
        }
    }
}

/// Bold runs for the first half of every word, where no other style is.
pub(crate) fn bold_beginnings(block: &mut Block) {
    if !matches!(block.kind, BlockKind::Paragraph) {
        return;
    }
    let mut runs: Vec<Run> = Vec::new();
    let mut word: Option<(usize, usize)> = None; // start, letters
    let end_word = |start: usize, letters: usize, text: &str, runs: &mut Vec<Run>| {
        if letters < 2 {
            return;
        }
        let bold = letters.div_ceil(2);
        let end = text[start..]
            .char_indices()
            .nth(bold)
            .map_or(text.len(), |(i, _)| start + i);
        runs.push(Run::new(start as u32, end as u32, TextStyle::BOLD));
    };
    for (i, c) in block.text.char_indices() {
        match (&mut word, c.is_alphabetic()) {
            (Some((_, letters)), true) => *letters += 1,
            (None, true) => word = Some((i, 1)),
            (Some((start, letters)), false) => {
                end_word(*start, *letters, &block.text, &mut runs);
                word = None;
            }
            (None, false) => {}
        }
    }
    if let Some((start, letters)) = word {
        end_word(start, letters, &block.text, &mut runs);
    }
    // Not where the book styles the text itself.
    runs.retain(|run| {
        !block
            .runs
            .iter()
            .any(|r| r.start < run.end && run.start < r.end)
    });
    block.runs.extend(runs);
    paperpaw_sdk::sort::sort_by_key(&mut block.runs, |run| run.start);
}

/// A signature for the page cache: what the setting does to a book.
pub(crate) fn signature(choice: &str) -> u64 {
    match choice {
        CAPTION => 1,
        HIDE => 2,
        _ => 0,
    }
}

/// Books that are nothing but pictures (comics, XTC): a page without its
/// picture is no page, so the choice does not apply to them.
fn all_pictures(path: &str) -> bool {
    let ext = path.rsplit_once('.').map_or("", |(_, ext)| ext);
    ["cbz", "cbr", "xtc", "xtch"]
        .iter()
        .any(|picture_book| ext.eq_ignore_ascii_case(picture_book))
}

/// Applies `choice` to `block` of the book at `path`.
pub(crate) fn apply(choice: &str, path: &str, i18n: &Translator, block: &mut Block) -> Verdict {
    if block.kind != BlockKind::Image || choice == SHOW || all_pictures(path) {
        return Verdict::Keep;
    }
    match choice {
        HIDE => Verdict::Drop,
        CAPTION => {
            caption(i18n, block);
            Verdict::Keep
        }
        _ => Verdict::Keep,
    }
}

/// `block` as a centred line in italics: the word for a picture and what
/// the book says about it.
fn caption(i18n: &Translator, block: &mut Block) {
    caption_word(i18n.tr("aids.picture"), block);
}

fn caption_word(word: &str, block: &mut Block) {
    let alt = block.text.trim();
    let line: String = if alt.is_empty() {
        format!("[{word}]")
    } else {
        format!("[{word}: {alt}]")
    };
    block.kind = BlockKind::Paragraph;
    block.image = None;
    block.align = Align::Center;
    block.text = line;
    block.runs.clear();
    block
        .runs
        .push(Run::new(0, block.text.len() as u32, TextStyle::ITALIC));
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_i18n::{Catalog, check};
    use paperpaw_sdk::document::{Align, OffsetMap};

    fn block(text: &str) -> Block {
        Block::new(BlockKind::Paragraph, 0, text.len() as u32, text.to_string())
            .map(OffsetMap::IDENTITY)
            .align(Align::default())
    }

    #[test]
    fn words_are_replaced_whole() {
        let mut b = block("Mr. Smith and Mr. Smithers met.");
        replace(&mut b, &pairs("Smith = Jones; Mr. = Mister"));
        assert_eq!(b.text, "Mister Jones and Mister Smithers met.");
        assert_eq!(
            pairs(" a = b ;; =x; c="),
            [
                (String::from("a"), String::from("b")),
                (String::from("c"), String::new())
            ]
        );
    }

    #[test]
    fn beginnings_are_bold() {
        let mut b = block("Read it, Alice!");
        bold_beginnings(&mut b);
        let bold: Vec<&str> = b
            .runs
            .iter()
            .map(|r| &b.text[r.start as usize..r.end as usize])
            .collect();
        assert_eq!(bold, ["Re", "i", "Ali"]);
    }

    #[test]
    fn translations_are_complete() {
        let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
        let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
        let issues = check::compare(&en, &de);
        assert!(issues.is_empty(), "{issues:?}");
    }
}

#[cfg(test)]
mod picture_tests {
    use super::*;
    use paperpaw_i18n::Catalog;
    use paperpaw_sdk::document::{ImageRef, OffsetMap};

    fn translator() -> Translator {
        let mut t = Translator::new();
        t.add(Catalog::parse(include_str!("../i18n/en.ini")).unwrap());
        t
    }

    fn picture(alt: &str) -> Block {
        Block::new(BlockKind::Image, 10, 80, String::from(alt))
            .map(OffsetMap::IDENTITY)
            .image(ImageRef {
                path: String::from("cache/img-0.gray"),
                width: 400,
                height: 300,
            })
    }

    #[test]
    fn shown_pictures_stay() {
        let mut b = picture("A ship");
        assert_eq!(apply(SHOW, "a.epub", &translator(), &mut b), Verdict::Keep);
        assert_eq!(b, picture("A ship"));
        // An unknown value (a setting from a later version) shows them.
        assert_eq!(apply("x", "a.epub", &translator(), &mut b), Verdict::Keep);
        assert_eq!(b.kind, BlockKind::Image);
    }

    #[test]
    fn a_caption_names_the_picture() {
        let mut b = picture("A ship at sea");
        assert_eq!(
            apply(CAPTION, "a.epub", &translator(), &mut b),
            Verdict::Keep
        );
        assert_eq!(b.kind, BlockKind::Paragraph);
        assert_eq!(b.text, "[Picture: A ship at sea]");
        assert!(b.image.is_none());
        assert_eq!(b.align, Align::Center);
        // Where the picture was in the book.
        assert_eq!((b.start, b.end), (10, 80));
        assert_eq!(
            b.runs,
            [Run::new(0, b.text.len() as u32, TextStyle::ITALIC)]
        );
        let mut none = picture("  ");
        apply(CAPTION, "a.epub", &translator(), &mut none);
        assert_eq!(none.text, "[Picture]");
    }

    #[test]
    fn hidden_pictures_are_left_out() {
        let mut b = picture("A ship");
        assert_eq!(apply(HIDE, "a.epub", &translator(), &mut b), Verdict::Drop);
    }

    #[test]
    fn text_and_picture_books_are_not_touched() {
        let mut text = Block::new(BlockKind::Paragraph, 0, 4, String::from("Text"));
        assert_eq!(
            apply(HIDE, "a.epub", &translator(), &mut text),
            Verdict::Keep
        );
        for path in ["c.cbz", "/books/C.CBZ", "x.xtc", "x.xtch"] {
            let mut page = picture("");
            assert_eq!(apply(HIDE, path, &translator(), &mut page), Verdict::Keep);
            assert_eq!(page.kind, BlockKind::Image, "{path}");
        }
    }

    #[test]
    fn the_signature_tells_the_choices_apart() {
        let all: alloc::vec::Vec<u64> = CHOICES.iter().map(|c| signature(c)).collect();
        assert_eq!(all, [0, 1, 2]);
    }
}
