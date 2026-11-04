//! A page index that counts a page past the end of the book (made while a
//! font pack was missing): the turn that finds the book over throws it
//! away once and the pages are counted again, and not again at the turns
//! after. A turn that cannot lay the next page out (no memory, the card
//! out) leaves the index alone.

use super::*;
use paperpaw_plugin_format_txt::TxtDocument;
use paperpaw_sdk::document::{Block, Document, DocumentError, Locator, Metadata};
use paperpaw_sdk::{FormatSpec, Storage};
use std::borrow::Cow;
use std::cell::Cell;

/// Plain text that can fail: while the flag is set, no block can be read
/// (as when there is no memory for it).
struct Flaky {
    text: TxtDocument,
    fail: Rc<Cell<bool>>,
}

impl Document for Flaky {
    fn metadata(&self) -> &Metadata {
        self.text.metadata()
    }
    fn sections(&self) -> u32 {
        self.text.sections()
    }
    fn section_len(&self, section: u32) -> u32 {
        self.text.section_len(section)
    }
    fn progress(&self, at: Locator) -> f32 {
        self.text.progress(at)
    }
    fn block_at(&self, storage: &dyn Storage, at: Locator) -> Result<Option<Block>, DocumentError> {
        if self.fail.get() {
            return Err(DocumentError::OutOfMemory);
        }
        self.text.block_at(storage, at)
    }
}

/// The format of `.flaky` files, the text of a `.txt`.
struct FlakyFormat(Rc<Cell<bool>>);

impl Plugin for FlakyFormat {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("flaky"), "Flaky", Version::new(0, 1, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let fail = self.0.clone();
        r.add_format(FormatSpec::new(
            Id::new_static("flaky.text"),
            "flaky.title",
            vec![Cow::Borrowed("flaky")],
            Box::new(move |storage, path, _| {
                let text = TxtDocument::open(storage, path)?;
                Ok(Box::new(Flaky {
                    text,
                    fail: fail.clone(),
                }) as Box<dyn Document>)
            }),
        ))
    }
}

/// A short book of a few pages.
fn short_book() -> String {
    (0..60)
        .map(|n| format!("p{n} {}\n\n", "word ".repeat(14)))
        .collect()
}

/// Where the page shown starts, as the reader last said.
fn here(reading: &Reading) -> (u32, u32) {
    let heard = reading.heard.borrow();
    let (_, r) = heard
        .iter()
        .rev()
        .find(|(n, _)| *n == "opened" || *n == "turned")
        .unwrap();
    (r.section, r.offset)
}

/// Turns forward until the page stays.
fn to_the_end(reading: &mut Reading) -> (u32, u32) {
    let mut at = here(reading);
    for _ in 0..3000 {
        reading.run(core_actions::NEXT_PAGE);
        if here(reading) == at {
            return at;
        }
        at = here(reading);
    }
    panic!("no end to the book");
}

/// The index file of the book, and what a count of this layout makes of
/// it.
fn index_of(reading: &Reading) -> (String, Vec<u8>) {
    let names = indexes(&reading.storage, reading.book);
    assert_eq!(names.len(), 1, "{names:?}");
    let path = format!(
        ".paperpaw/plugins/reader/books/{:016x}/{}",
        paperpaw_sdk::digest::fnv1a64(reading.book),
        names[0]
    );
    let data = paperpaw_sdk::read_file(&reading.storage, &path, 1 << 20).unwrap();
    (path, data)
}

/// The book counted right, then its index made to count one page more:
/// the book opened again at its last page.
fn counting_one_too_many(book: &'static str, data: &[u8]) -> (Reading, String, Vec<u8>, Vec<u8>) {
    let storage = Shared::default();
    let mut first = Reading::with_book(storage.clone(), book, data);
    first.open();
    let last = to_the_end(&mut first);
    // Some more moments for the count to end.
    for _ in 0..30 {
        first.run(core_actions::NEXT_PAGE);
    }
    let (path, right) = index_of(&first);
    let mut wrong = right[..right.len() - 8].to_vec();
    wrong.extend_from_slice(&last.0.to_le_bytes());
    wrong.extend_from_slice(&(last.1 + 1).to_le_bytes());
    wrong.extend_from_slice(&right[right.len() - 8..]);
    storage.write(&path, &wrong).unwrap();
    let mut again = Reading::with_book(storage, book, data);
    again.open();
    assert_eq!(to_the_end(&mut again), last);
    (again, path, right, wrong)
}

/// The book is over: the index is thrown away and counted again, once; the
/// turns after that do not count it again.
#[test]
fn the_end_of_the_text_mends_a_wrong_count_once() {
    let text = short_book();
    let (mut reading, path, right, wrong) = counting_one_too_many(BOOK, text.as_bytes());
    assert_ne!(right, wrong);
    reading.run(core_actions::NEXT_PAGE);
    for _ in 0..30 {
        reading.run(core_actions::NEXT_PAGE);
    }
    let mended = paperpaw_sdk::read_file(&reading.storage, &path, 1 << 20).unwrap();
    assert_eq!(mended, right);
    // The same index again at the end: not thrown away a second time (it
    // is the one in memory that counts, which is right now), and the
    // turns past the end go on doing nothing.
    let events = reading.heard.borrow().len();
    for _ in 0..5 {
        reading.run(core_actions::NEXT_PAGE);
    }
    assert_eq!(reading.heard.borrow().len(), events);
    let again = paperpaw_sdk::read_file(&reading.storage, &path, 1 << 20).unwrap();
    assert_eq!(again, right);
}

/// A turn that cannot lay the next page out (no memory) in the middle of a
/// book whose index is right: the index stays, not thrown away as if the
/// book were over, and the pages are not counted again at that turn or the
/// ones after.
#[test]
fn a_turn_that_cannot_read_leaves_the_index_alone() {
    const FLAKY: &str = "books/alice.flaky";
    let text = include_str!("../../../../samples/library/books/alice-in-wonderland.txt");
    let storage = Shared::default();
    storage.create_dir("books").unwrap();
    storage.write(FLAKY, text.as_bytes()).unwrap();
    let fail = Rc::new(Cell::new(false));
    let heard = Heard::default();
    let kernel = Kernel::builder(DEVICE)
        .storage(storage.clone())
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .plugin(FlakyFormat(fail.clone()))
        .plugin(Reader)
        .plugin(Listener(heard.clone()))
        .build()
        .unwrap();
    let mut reading = Reading {
        kernel,
        storage: storage.clone(),
        heard,
        now: 0,
        book: FLAKY,
    };
    let effects = reading.kernel.start(Instant(0));
    reading.settle(effects);
    reading.open();
    let last = to_the_end(&mut reading);
    for _ in 0..400 {
        reading.run(core_actions::NEXT_PAGE);
    }
    let (path, right) = index_of(&reading);
    assert!(right.ends_with(&[255; 8]), "the count is not complete");
    for _ in 0..3000 {
        let at = here(&reading);
        reading.run(core_actions::PREV_PAGE);
        if here(&reading) == at {
            break;
        }
    }
    for _ in 0..20 {
        reading.run(core_actions::NEXT_PAGE);
    }
    let at = here(&reading);
    assert!(at < last);
    // No memory from here on: the turn finds no page, and says so to
    // nobody; the index stays.
    fail.set(true);
    let heard = reading.heard.borrow().len();
    for _ in 0..5 {
        reading.run(core_actions::NEXT_PAGE);
    }
    assert_eq!(
        here(&reading),
        at,
        "a page turned that could not be laid out"
    );
    assert_eq!(reading.heard.borrow().len(), heard);
    let after = paperpaw_sdk::read_file(&reading.storage, &path, 1 << 20).unwrap();
    assert_eq!(after, right, "the index was thrown away on an error");
    // Memory again: the turns go on, with that index.
    fail.set(false);
    reading.run(core_actions::NEXT_PAGE);
    assert_ne!(here(&reading), at);
    let later = paperpaw_sdk::read_file(&reading.storage, &path, 1 << 20).unwrap();
    assert_eq!(later, right);
}
