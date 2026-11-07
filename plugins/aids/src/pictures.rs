//! Pictures of a book as it asks: shown, or a short line in their place
//! (a caption of their alternative text), or left out. A block filter
//! changes the picture block before the layout sees it, so pages hold what
//! is left and nothing is decoded for a picture that is not shown.

use alloc::format;
use alloc::string::String;

use paperpaw_sdk::Translator;
use paperpaw_sdk::document::{Align, Block, BlockKind, Run, TextStyle};
use paperpaw_sdk::filters::Verdict;

/// Pictures stay as the book has them.
pub const SHOW: &str = "aids.pictures.show";
/// A line in place of each picture: `[Picture: alternative text]`.
pub const CAPTION: &str = "aids.pictures.caption";
/// Pictures are left out.
pub const HIDE: &str = "aids.pictures.hide";

/// The choices of [`PICTURES`](crate::PICTURES).
pub const CHOICES: [&str; 3] = [SHOW, CAPTION, HIDE];

/// A signature for the page cache: what the setting does to a book.
pub fn signature(choice: &str) -> u64 {
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
pub fn apply(choice: &str, path: &str, i18n: &Translator, block: &mut Block) -> Verdict {
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

/// Reader-side application of the already localized policy.
#[cfg(any(test, feature = "reader-filter"))]
pub(crate) fn apply_word(policy: u32, path: &str, word: &str, block: &mut Block) -> Verdict {
    if block.kind != BlockKind::Image || all_pictures(path) {
        return Verdict::Keep;
    }
    // Drop is represented by the caller's verdict, not an empty paragraph.
    if policy == 1 {
        caption_word(word, block);
    }
    if policy == 2 {
        Verdict::Drop
    } else {
        Verdict::Keep
    }
}

#[cfg(test)]
mod tests {
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
