//! The scan of the library (`native_scan`) over a scripted library: which
//! books are listed and counted, at any size of the pages, with stale pages,
//! with replies that do not add up, and with memory refused.
use crate::fail_alloc::sweep;
use crate::model::{Counted, Handle, Kind, MOST_BOOKS, TITLE_MAX};
use crate::native_scan::{Page, RESTARTS, Source, scan};
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use paperpaw_sdk::native::Error;

struct Book {
    path: String,
    title: String,
    notes: Vec<Kind>,
    gone: bool,
}

/// A library of books and what each holds, in pages of the given sizes.
struct Fake {
    books: Vec<Book>,
    book_page: usize,
    note_page: usize,
    /// Calls of `books` that say `State` first, then the rest work.
    stale_books: usize,
    stale_notes: usize,
    /// The total named by every page, if it is to be wrong.
    lie_total: Option<u32>,
    /// A last page that names a next one with no rows.
    endless: bool,
    note_calls: usize,
    book_calls: usize,
}

impl Fake {
    fn new(books: Vec<Book>) -> Self {
        Self {
            books,
            book_page: 3,
            note_page: 2,
            stale_books: 0,
            stale_notes: 0,
            lie_total: None,
            endless: false,
            note_calls: 0,
            book_calls: 0,
        }
    }
}

fn book(title: &str, notes: &[Kind]) -> Book {
    Book {
        path: format!("Books/{title}.epub"),
        title: title.into(),
        notes: notes.to_vec(),
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
        self.book_calls += 1;
        if self.stale_books > 0 {
            self.stale_books -= 1;
            return Err(Error::State);
        }
        let end = (cursor + self.book_page).min(self.books.len());
        for index in cursor..end {
            each(index)?;
        }
        let next = (end < self.books.len() || self.endless).then_some(end);
        Ok(Page {
            next,
            total: self.lie_total.unwrap_or(self.books.len() as u32),
        })
    }

    fn notes(
        &mut self,
        book: usize,
        cursor: usize,
        each: &mut dyn FnMut(Kind),
    ) -> Result<Page<usize>, Error> {
        self.note_calls += 1;
        if self.stale_notes > 0 {
            self.stale_notes -= 1;
            return Err(Error::State);
        }
        let book = &self.books[book];
        if book.gone {
            return Err(Error::NotFound);
        }
        let end = (cursor + self.note_page).min(book.notes.len());
        for kind in &book.notes[cursor..end] {
            each(*kind);
        }
        Ok(Page {
            next: (end < book.notes.len()).then_some(end),
            total: book.notes.len() as u32,
        })
    }

    fn handle(&self, book: usize) -> Handle {
        Handle {
            key: book as u32,
            generation: 1,
        }
    }

    fn title(
        &mut self,
        book: usize,
        put: &mut dyn FnMut(&str) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let book = &self.books[book];
        if book.gone {
            return Err(Error::NotFound);
        }
        put(&book.title)
    }
}

use Kind::{Bookmark, Highlight, Other};

fn library() -> Vec<Book> {
    vec![
        book("zebra", &[Highlight, Highlight, Bookmark]),
        book("Apple", &[Bookmark]),
        book("none", &[]),
        book("only other", &[Other, Other]),
        book(
            "banana",
            &[Highlight, Other, Bookmark, Highlight, Bookmark, Highlight],
        ),
        book("Apple", &[Highlight]),
    ]
}

/// A book listed: its path (found by its handle) and its highlights and bookmarks.
type Listed = (String, u16, u16);

fn summary(fake: &mut Fake) -> Result<(Vec<Listed>, bool), Error> {
    let listing = scan(fake)?;
    let books = listing
        .books()
        .iter()
        .map(|b| {
            let path = fake.books[b.handle.key as usize].path.clone();
            (path, b.highlights, b.bookmarks)
        })
        .collect();
    Ok((books, listing.more()))
}

#[test]
fn the_books_with_notes_are_listed_by_title_with_their_counts() {
    let (books, more) = summary(&mut Fake::new(library())).unwrap();
    assert!(!more);
    let want = [
        ("Books/Apple.epub", 0, 1),
        ("Books/Apple.epub", 1, 0),
        ("Books/banana.epub", 3, 2),
        ("Books/zebra.epub", 2, 1),
    ];
    let got: Vec<(&str, u16, u16)> = books.iter().map(|(p, h, b)| (p.as_str(), *h, *b)).collect();
    assert_eq!(got, want);
}

#[test]
fn the_size_of_the_pages_does_not_matter() {
    let want = summary(&mut Fake::new(library())).unwrap();
    for books in [1, 2, 5, 100] {
        for notes in [1, 3, 100] {
            let mut fake = Fake::new(library());
            fake.book_page = books;
            fake.note_page = notes;
            assert_eq!(summary(&mut fake).unwrap(), want, "{books} {notes}");
        }
    }
    assert_eq!(
        summary(&mut Fake::new(Vec::new())).unwrap(),
        (Vec::new(), false)
    );
}

#[test]
fn a_full_list_only_notes_that_there_are_more() {
    let many: Vec<Book> = (0..MOST_BOOKS + 30)
        .map(|n| {
            book(
                &format!("b{n:04}"),
                &[Highlight, Bookmark, Highlight, Bookmark],
            )
        })
        .collect();
    let mut fake = Fake::new(many);
    fake.note_page = 1;
    let (books, more) = summary(&mut fake).unwrap();
    assert_eq!(books.len(), MOST_BOOKS);
    assert!(more);
    // The 200 are counted whole (4 notes, 4 pages each, one more call for a
    // book that ends exactly); the others are looked at once.
    assert!(
        fake.note_calls <= MOST_BOOKS * 4 + 30,
        "{}",
        fake.note_calls
    );
}

#[test]
fn a_stale_page_is_read_again_a_few_times_then_it_is_a_state_error() {
    let want = summary(&mut Fake::new(library())).unwrap();
    // A book's notes are read again by themselves, then the whole scan again.
    for stale in 0..=RESTARTS {
        let mut fake = Fake::new(library());
        fake.stale_books = stale;
        assert_eq!(summary(&mut fake).unwrap(), want, "{stale}");
        let mut fake = Fake::new(library());
        fake.stale_notes = stale;
        assert_eq!(summary(&mut fake).unwrap(), want, "{stale}");
    }
    let mut fake = Fake::new(library());
    fake.stale_books = 1000;
    assert_eq!(summary(&mut fake).err(), Some(Error::State));
    let mut fake = Fake::new(library());
    fake.stale_notes = 1000;
    assert_eq!(summary(&mut fake).err(), Some(Error::State));
}

#[test]
fn a_reply_that_does_not_add_up_ends_the_scan() {
    let mut fake = Fake::new(library());
    fake.lie_total = Some(2);
    assert_eq!(summary(&mut fake).err(), Some(Error::Invalid));
    let mut fake = Fake::new(library());
    fake.endless = true;
    assert_eq!(summary(&mut fake).err(), Some(Error::Invalid));
}

#[test]
fn a_book_that_is_gone_is_left_out() {
    let mut books = library();
    books[0].gone = true;
    books[4].gone = true;
    let (listed, _) = summary(&mut Fake::new(books)).unwrap();
    assert_eq!(listed.len(), 2);
    assert!(listed.iter().all(|(path, _, _)| path == "Books/Apple.epub"));
}

#[test]
fn refused_allocations_end_in_no_memory() {
    // The script is built once: what the scan allocates is what is counted.
    let mut fake = Fake::new(library());
    sweep(|| Ok(scan(&mut fake)?.books().len()));
    let many: Vec<Book> = (0..MOST_BOOKS + 3)
        .map(|n| book(&format!("b{n:04}"), &[Highlight]))
        .collect();
    let mut fake = Fake::new(many);
    fake.book_page = 40;
    sweep(|| {
        let listing = scan(&mut fake)?;
        Ok(listing.books().len())
    });
}

#[test]
fn the_walk_ends_at_the_first_book_past_the_list() {
    let many: Vec<Book> = (0..1000)
        .map(|n| book(&format!("b{n:04}"), &[Highlight]))
        .collect();
    let mut fake = Fake::new(many);
    fake.book_page = 40;
    let (books, more) = summary(&mut fake).unwrap();
    assert!(more);
    // The first 200 in the catalog's title order, and no page after the one
    // that held the 201st (`201 / 40` rounded up).
    assert_eq!(books.len(), MOST_BOOKS);
    assert_eq!(books[0].0, "Books/b0000.epub");
    assert_eq!(books[MOST_BOOKS - 1].0, "Books/b0199.epub");
    assert_eq!(fake.book_calls, 6);
    assert_eq!(fake.note_calls, MOST_BOOKS + 1);
}

#[test]
fn a_title_is_cut_and_a_book_is_kept_by_handle_not_by_path() {
    // Letters of two bytes: a careless cut falls inside one.
    let long = "\u{e4}".repeat(500);
    let mut books = vec![book(&long, &[Highlight]), book("Fine", &[Highlight])];
    // However long a path is, the list does not hold it.
    books.push(Book {
        path: format!("Books/{}.epub", "x".repeat(2000)),
        title: "Deep".into(),
        notes: vec![Highlight],
        gone: false,
    });
    let listing = scan(&mut Fake::new(books)).unwrap();
    assert_eq!(listing.books().len(), 3);
    for book in listing.books() {
        assert!(book.title.len() <= TITLE_MAX);
    }
    let cut = &listing.books()[2].title;
    assert_eq!(cut.len(), TITLE_MAX);
    assert!(cut.chars().all(|c| c == '\u{e4}'));
    // Deep, Fine, then the cut title: each by its handle (the index).
    let keys: Vec<_> = listing.books().iter().map(|b| b.handle.key).collect();
    assert_eq!(keys, [2, 1, 0]);
    assert!(listing.books().iter().all(|b| b.handle.generation == 1));
    // The most the list holds: 200 books of the longest title and a handle,
    // about 30 KiB with the headers of the strings.
    let most = MOST_BOOKS * (TITLE_MAX + core::mem::size_of::<Counted>());
    assert!(most < 64 * 1024, "{most}");
}

#[test]
fn counts_that_fill_a_u16_do_not_overflow_when_the_list_is_full() {
    // A full list is looked at only until the first note; one page of
    // 65,536 notes holds the most a count can take.
    let mut books: Vec<Book> = (0..MOST_BOOKS)
        .map(|n| book(&format!("b{n:04}"), &[Highlight]))
        .collect();
    let mut notes = vec![Highlight; usize::from(u16::MAX)];
    notes.push(Bookmark);
    books.push(book("zzz", &notes));
    let mut fake = Fake::new(books);
    fake.note_page = 70_000;
    let (listed, more) = summary(&mut fake).unwrap();
    assert!(more);
    assert_eq!(listed.len(), MOST_BOOKS);
    // And listed, the counts saturate.
    let mut books = vec![book("a", &notes)];
    books.push(book("b", &notes));
    let mut fake = Fake::new(books);
    fake.note_page = 70_000;
    let (listed, _) = summary(&mut fake).unwrap();
    assert_eq!((listed[0].1, listed[0].2), (u16::MAX, 1));
}
