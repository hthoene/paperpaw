//! What a book's first opening writes for a chapter (#250): no file made
//! empty ahead of its content, nothing an aborted try left kept, and a
//! conversion cut off anywhere never read as a complete one.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

use common::{blocks, book};
use paperpaw_epub_family::EpubDocument;
use paperpaw_sdk::document::{Block, Document, Locator};
use paperpaw_sdk::{DirEntry, MemoryStorage, Storage, StorageError};

const PATH: &str = "book.epub";

/// A card as a format's cache is: no tree of it can be removed (the call
/// is taken and does nothing, as the native cache root's). It notes the
/// whole writes, and after `left` changing calls its power is cut: every
/// later change is refused.
struct Card {
    inner: MemoryStorage,
    /// Path and length of each whole write.
    writes: RefCell<Vec<(String, usize)>>,
    appends: Cell<usize>,
    left: Cell<Option<usize>>,
}

impl Card {
    fn with(book: Vec<u8>) -> Self {
        Card {
            inner: MemoryStorage::new().with(PATH, book),
            writes: RefCell::new(Vec::new()),
            appends: Cell::new(0),
            left: Cell::new(None),
        }
    }

    /// One more change, unless the power is cut.
    fn change(&self) -> Result<(), StorageError> {
        match self.left.get() {
            Some(0) => Err(StorageError::Io),
            Some(left) => {
                self.left.set(Some(left - 1));
                Ok(())
            }
            None => Ok(()),
        }
    }

    /// Every file below `dir` with its content.
    fn files(&self, dir: &str) -> BTreeMap<String, Vec<u8>> {
        let mut out = BTreeMap::new();
        let mut entries = Vec::new();
        let _ = self.inner.list(dir, &mut |entry| {
            entries.push((entry.name.to_string(), entry.is_dir));
        });
        for (name, is_dir) in entries {
            let path = format!("{dir}/{name}");
            if is_dir {
                out.extend(self.files(&path));
            } else {
                out.insert(path.clone(), self.inner.get(&path).unwrap());
            }
        }
        out
    }

    /// The files of the chapters' conversions.
    fn sections(&self) -> BTreeMap<String, Vec<u8>> {
        let mut files = self.files(".paperpaw");
        files.retain(|path, _| path.contains("/sections/"));
        files
    }
}

impl Storage for Card {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.inner.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.inner.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.change()?;
        self.writes.borrow_mut().push((path.into(), data.len()));
        self.inner.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.change()?;
        self.appends.set(self.appends.get() + 1);
        self.inner.append(path, data)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.inner.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.change()?;
        self.inner.remove(path)
    }
    fn exists(&self, path: &str) -> bool {
        self.inner.exists(path)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.change()?;
        self.inner.rename(from, to)
    }
    fn remove_dir_all(&self, _: &str) -> Result<(), StorageError> {
        Ok(())
    }
}

/// Three chapters: plain text, one with a link and an anchor, and one
/// without anything in it.
fn chapters() -> Vec<String> {
    let plain: String = (0..60)
        .map(|p| format!("<p>Paragraph {p} of plain words, long enough to fill a line.</p>"))
        .collect();
    let linked = format!("<h1 id=\"top\">Linked</h1><p><a href=\"c0.xhtml\">back</a></p>{plain}");
    vec![plain, linked, String::new()]
}

/// As far as the book opens and reads on `card`: its blocks, or none of
/// it where anything fails.
fn read(card: &Card) -> Option<Vec<(u32, Block)>> {
    let doc = EpubDocument::open(card, PATH).ok()?;
    let mut out = Vec::new();
    for section in 0..doc.sections() {
        let mut at = Locator::new(section, 0);
        while let Some(block) = doc.block_at(card, at).ok()? {
            at = Locator::new(section, block.end);
            out.push((section, block));
        }
    }
    Some(out)
}

/// Each of a chapter's seven files was made empty before its content: a
/// whole write each, eight writes to the card on the X4, and 1.6 s of a
/// book's first opening for files of no bytes (#250).
#[test]
fn a_chapters_conversion_makes_no_file_empty_ahead_of_its_content() {
    let card = Card::with(book(&chapters(), &[]));
    let doc = EpubDocument::open(&card, PATH).unwrap();
    let all = blocks(&doc, &card);
    assert!(all.len() > 120);
    let empty: Vec<String> = card
        .writes
        .borrow()
        .iter()
        .filter(|(path, len)| *len == 0 && path.contains("/sections/"))
        .map(|(path, _)| path.clone())
        .collect();
    // The chapter without text: its text and index, which are asked for
    // their sizes.
    assert_eq!(empty.len(), 2, "{empty:?}");
    assert!(empty.iter().all(|path| path.contains("/sections/2/")));
    // A file without content is not there: the plain chapter has no links.
    let files = card.sections();
    let names = |section: &str| -> Vec<&str> {
        files
            .keys()
            .filter_map(|path| path.split_once(section).map(|(_, name)| name))
            .collect()
    };
    assert_eq!(names("/sections/0/"), ["end", "idx", "pos", "txt", "xpt"]);
    assert_eq!(
        names("/sections/1/"),
        ["anc", "end", "idx", "lnk", "pos", "txt", "xpt"]
    );
    assert_eq!(names("/sections/2/"), ["end", "idx", "txt"]);
}

#[test]
fn a_book_opened_again_converts_no_chapter_again_not_one_without_text() {
    let card = Card::with(book(&chapters(), &[]));
    let first = read(&card).unwrap();
    card.writes.borrow_mut().clear();
    card.appends.set(0);
    assert_eq!(read(&card).unwrap(), first);
    assert_eq!(*card.writes.borrow(), []);
    assert_eq!(card.appends.get(), 0);
}

/// What a try that was cut off left of a chapter (more than the chapter
/// makes, in every file) is neither kept nor appended to, on a card whose
/// folders the format cannot remove.
#[test]
fn what_an_aborted_try_left_of_a_chapter_does_not_stay() {
    let card = Card::with(book(&chapters(), &[]));
    let first = read(&card).unwrap();
    let whole = card.sections();
    for section in 0..3 {
        let folder = whole
            .keys()
            .find_map(|path| path.split_once(&format!("/sections/{section}/")))
            .map(|(dir, _)| format!("{dir}/sections/{section}"))
            .unwrap();
        card.inner.remove(&format!("{folder}/end")).unwrap();
        for name in ["txt", "idx", "anc", "lnk", "pos", "xpt", "pic"] {
            let left = format!("left\tof\tan\tearlier\ttry\n").repeat(200);
            card.inner
                .write(&format!("{folder}/{name}"), left.as_bytes())
                .unwrap();
        }
    }
    assert_eq!(read(&card).unwrap(), first);
    assert_eq!(card.sections(), whole);
}

/// The power cut after any number of changes to the card: opened again,
/// the book reads as it does converted in one go. No conversion that was
/// cut off is taken for a complete one.
#[test]
fn a_conversion_cut_off_anywhere_is_never_read_as_complete() {
    let book = book(&chapters(), &[]);
    let whole = read(&Card::with(book.clone())).unwrap();
    for changes in 0.. {
        assert!(changes < 5_000, "the conversion never ends");
        let card = Card::with(book.clone());
        card.left.set(Some(changes));
        let cut = read(&card);
        let ended = card.left.get() != Some(0);
        // Power again: the same card, read anew.
        card.left.set(None);
        assert_eq!(read(&card).as_ref(), Some(&whole), "cut after {changes}");
        if ended {
            assert_eq!(cut.as_ref(), Some(&whole));
            assert!(changes > 20, "a conversion changes the card ({changes})");
            break;
        }
    }
}
