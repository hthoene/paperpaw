//! The export (`native_export`) over a scripted library: the file it writes,
//! at any size of the pages, with books that are gone, stale pages, replies
//! that do not add up, and memory or the sink refused.
use crate::fail_alloc::sweep;
use crate::model::{Handle, Kind, Note, Sink};
use crate::native_export::{Exported, Texts, export};
use crate::native_scan::{Page, Source};
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use paperpaw_sdk::native::Error;

#[derive(Clone)]
struct Mark {
    kind: Kind,
    percent: u8,
    created: i64,
    text: &'static str,
    note: &'static str,
}

struct Book {
    title: &'static str,
    author: &'static str,
    notes: Vec<Mark>,
    gone: bool,
}

struct Fake {
    books: Vec<Book>,
    book_page: usize,
    note_page: usize,
    /// Calls of `texts` that say `State` before the rest work.
    stale: usize,
    lie_total: Option<u32>,
    endless: bool,
}

impl Fake {
    fn new(books: Vec<Book>) -> Self {
        Self {
            books,
            book_page: 2,
            note_page: 2,
            stale: 0,
            lie_total: None,
            endless: false,
        }
    }
}

fn mark(kind: Kind, percent: u8, text: &'static str, note: &'static str) -> Mark {
    Mark {
        kind,
        percent,
        created: 1_793_527_500,
        text,
        note,
    }
}

fn book(title: &'static str, author: &'static str, notes: Vec<Mark>) -> Book {
    Book {
        title,
        author,
        notes,
        gone: false,
    }
}

impl Source for Fake {
    type Book = usize;
    type Catalog = usize;
    type Notes = usize;

    fn books(
        &mut self,
        cursor: usize,
        each: &mut dyn FnMut(usize) -> Result<(), Error>,
    ) -> Result<Page<usize>, Error> {
        let end = (cursor + self.book_page).min(self.books.len());
        for index in cursor..end {
            each(index)?;
        }
        Ok(Page {
            next: (end < self.books.len()).then_some(end),
            total: self.lie_total.unwrap_or(self.books.len() as u32),
        })
    }

    fn notes(&mut self, _: usize, _: usize, _: &mut dyn FnMut(Kind)) -> Result<Page<usize>, Error> {
        Err(Error::Unsupported)
    }

    fn handle(&self, book: usize) -> Handle {
        Handle {
            key: book as u32,
            generation: 1,
        }
    }

    fn title(
        &mut self,
        _: usize,
        _: &mut dyn FnMut(&str) -> Result<(), Error>,
    ) -> Result<(), Error> {
        Err(Error::Unsupported)
    }
}

impl Texts for Fake {
    fn names(
        &mut self,
        book: usize,
        put: &mut dyn FnMut(&str, &str) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let book = &self.books[book];
        if book.gone {
            return Err(Error::NotFound);
        }
        put(book.title, book.author)
    }

    fn texts(
        &mut self,
        book: usize,
        cursor: usize,
        each: &mut dyn FnMut(&Note<'_>) -> Result<(), Error>,
    ) -> Result<Page<usize>, Error> {
        if self.stale > 0 {
            self.stale -= 1;
            return Err(Error::State);
        }
        let book = &self.books[book];
        if book.gone {
            return Err(Error::NotFound);
        }
        let end = (cursor + self.note_page).min(book.notes.len());
        for m in &book.notes[cursor..end] {
            each(&Note {
                kind: m.kind,
                percent: m.percent,
                created: m.created,
                updated: 0,
                text: m.text,
                note: m.note,
            })?;
        }
        Ok(Page {
            next: (end < book.notes.len() || self.endless).then_some(end),
            total: book.notes.len() as u32,
        })
    }
}

const WHEN: &str = "Sunday, 1 November 2026 10:05:00";

fn entry(book: &str, what: &str, place: &str, text: &str) -> String {
    format!(
        "{book}\r\n- Your {what} at location {place} | Added on {WHEN}\r\n\r\n{text}\r\n==========\r\n"
    )
}

fn run(fake: &mut Fake) -> Result<(String, Exported), Error> {
    let mut out = String::new();
    let done = export(fake, &mut out, &|seconds| seconds)?;
    Ok((out, done))
}

fn library() -> Vec<Book> {
    vec![
        book(
            "A Quiet Year",
            "Ann Reed",
            vec![
                mark(Kind::Highlight, 42, "The wind\ncame  over", "so true"),
                mark(Kind::Bookmark, 50, "p. 12", ""),
                mark(Kind::Other, 60, "x", ""),
            ],
        ),
        book("No notes", "", vec![]),
        book("Plain", "", vec![mark(Kind::Highlight, 7, "Short", "")]),
    ]
}

#[test]
fn every_note_is_written_in_the_form_of_my_clippings() {
    let (text, done) = run(&mut Fake::new(library())).unwrap();
    let quiet = "A Quiet Year (Ann Reed)";
    let expected = format!(
        "\u{feff}{}{}{}{}",
        entry(quiet, "Highlight", "42-42", "The wind came over"),
        entry(quiet, "Note", "42", "so true"),
        entry(quiet, "Bookmark", "50", ""),
        entry("Plain", "Highlight", "7-7", "Short"),
    );
    assert_eq!(text, expected);
    assert_eq!(done, Exported { notes: 4, books: 2 });
}

#[test]
fn the_same_file_comes_at_any_size_of_the_pages() {
    let (whole, _) = run(&mut Fake::new(library())).unwrap();
    for (books, notes) in [(1, 1), (1, 5), (7, 1), (7, 7), (2, 3)] {
        let mut fake = Fake::new(library());
        fake.book_page = books;
        fake.note_page = notes;
        assert_eq!(run(&mut fake).unwrap().0, whole, "{books} {notes}");
    }
}

#[test]
fn a_library_without_notes_has_only_the_mark() {
    let mut fake = Fake::new(vec![book("a", "", vec![]), book("b", "", vec![])]);
    let (text, done) = run(&mut fake).unwrap();
    assert_eq!(text, "\u{feff}");
    assert_eq!(done, Exported::default());
}

#[test]
fn a_book_that_is_gone_is_left_out() {
    let mut books = library();
    books[0].gone = true;
    let (text, done) = run(&mut Fake::new(books)).unwrap();
    assert!(!text.contains("Quiet"));
    assert_eq!(done, Exported { notes: 1, books: 1 });
}

#[test]
fn a_stale_page_ends_the_export_with_state() {
    let mut fake = Fake::new(library());
    fake.stale = 1;
    assert_eq!(run(&mut fake).unwrap_err(), Error::State);
    // The caller makes the file again: the next try works.
    assert!(run(&mut fake).is_ok());
}

#[test]
fn replies_that_do_not_add_up_end_the_export() {
    let mut fake = Fake::new(library());
    fake.lie_total = Some(1);
    assert_eq!(run(&mut fake).unwrap_err(), Error::Invalid);
    let mut fake = Fake::new(library());
    fake.endless = true;
    assert_eq!(run(&mut fake).unwrap_err(), Error::Invalid);
}

/// A sink that takes this many bytes and then refuses.
struct Small(usize, String);

impl Sink for Small {
    fn put(&mut self, text: &str) -> Result<(), Error> {
        if self.1.len() + text.len() > self.0 {
            return Err(Error::Full);
        }
        self.1.push_str(text);
        Ok(())
    }
}

#[test]
fn a_full_sink_ends_the_export_with_its_refusal() {
    let (whole, _) = run(&mut Fake::new(library())).unwrap();
    for limit in [0, 3, 40, whole.len() - 1] {
        let mut sink = Small(limit, String::new());
        let result = export(&mut Fake::new(library()), &mut sink, &|s| s);
        assert_eq!(result.unwrap_err(), Error::Full, "{limit}");
    }
    let mut sink = Small(whole.len(), String::new());
    assert!(export(&mut Fake::new(library()), &mut sink, &|s| s).is_ok());
}

#[test]
fn refused_allocations_end_in_no_memory() {
    let mut fake = Fake::new(library());
    sweep(|| {
        let mut out = String::new();
        Ok(export(&mut fake, &mut out, &|s| s)?.notes)
    });
}
