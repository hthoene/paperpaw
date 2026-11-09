//! The cells of the list of books and what an empty list says, free of host
//! calls: the filter row with its count, the books found so far while the
//! library is scanning, the way to the windows before and after, each book
//! with its bar and chips, and the words of an empty library, of a card that
//! is gone, of a folder that cannot be read and of a library whose books are
//! all in the archive.
use crate::{
    native_list::{Cell, Row},
    native_model::{BookLine, Filter, Order, Say, Scope, Selection, Window, chips, line},
    native_states::Condition,
};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::{Error, library::CatalogCursor};
use paperpaw_ui_kit::{
    mem::push,
    text::{concat, copy},
};

/// What the cells are made of.
#[derive(Clone, Copy, Debug)]
pub struct Facts<'a> {
    pub selection: &'a Selection,
    pub window: &'a Window<CatalogCursor, BookLine>,
    /// Where the windows before this one began.
    pub starts: &'a [CatalogCursor],
    pub condition: Condition,
    /// The library has books, all of them in the archive.
    pub archived: bool,
    /// The library folder, for the empty library's words.
    pub folder: &'a str,
}

/// The cells, and what an empty list says (both empty when it is not empty).
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct Built {
    pub cells: Vec<Cell>,
    pub empty: String,
    pub empty_text: String,
}

/// Whether the first book of a library's first window is shown as the card:
/// the library in the order of reading, its first book begun and not finished.
#[must_use]
pub fn card_first(
    selection: &Selection,
    starts: &[CatalogCursor],
    first: Option<&BookLine>,
) -> bool {
    selection.scope == Scope::Library
        && selection.filter == Filter::All
        && selection.order == Order::Recent
        && starts.is_empty()
        && first.is_some_and(|book| !book.finished && book.progress.is_some_and(|p| p > 0))
}

/// The bar under a book: how far it is read, shown from 1 to 99 per cent.
#[must_use]
pub const fn bar(book: &BookLine) -> Option<u8> {
    match book.progress {
        Some(percent) if !book.finished && percent > 0 && percent < 100 => Some(percent),
        _ => None,
    }
}

/// Makes the cells and the words of an empty list.
///
/// # Errors
/// `NoMemory`, or the words' refusal.
pub fn build(say: &mut impl Say, facts: &Facts<'_>) -> Result<Built, Error> {
    let Facts {
        selection,
        window,
        starts,
        condition,
        ..
    } = *facts;
    let library = selection.scope == Scope::Library;
    let mut cells = Vec::new();
    if library {
        let show = say.say("files.list.show")?;
        let name = say.say(selection.filter.key())?;
        let count = if selection.filter == Filter::Shelves || condition.counting() {
            String::new()
        } else {
            say.count("files.books", i64::from(window.total))?
        };
        let value = if count.is_empty() {
            name
        } else {
            concat(&[&name, " · ", &count])?
        };
        push(&mut cells, Cell::plain(Row::Filter, &show, &value)?)?;
    }
    if let (Condition::Scanning { books }, true) = (condition, library) {
        // The count leads: the books found so far.
        let note = say.count("files.list.so-far", i64::from(books))?;
        push(&mut cells, Cell::plain(Row::Note, &note, "")?)?;
    }
    if !starts.is_empty() {
        let previous = say.say("files.list.previous")?;
        push(&mut cells, Cell::plain(Row::Previous, &previous, "‹")?)?;
    }
    let card = card_first(selection, starts, window.books.first());
    for (at, book) in window.books.iter().enumerate() {
        let made = line(say, book)?;
        let first = card && at == 0;
        let value = if first {
            say.say("files.list.continue-reading")?
        } else {
            made.value
        };
        push(
            &mut cells,
            Cell {
                row: Row::Book(at),
                title: made.title,
                value,
                note: made.note,
                progress: bar(book),
                card: first,
                chips: chips(book)?,
            },
        )?;
    }
    if window.has_more() {
        let more = say.say("files.list.more")?;
        push(&mut cells, Cell::plain(Row::More, &more, "›")?)?;
    }
    let (empty, empty_text) = empty_words(say, facts)?;
    Ok(Built {
        cells,
        empty,
        empty_text,
    })
}

/// What an empty list says: the condition's words, or what the filter or
/// scope has none of.
fn empty_words(say: &mut impl Say, facts: &Facts<'_>) -> Result<(String, String), Error> {
    let none = || -> Result<(String, String), Error> { Ok((copy("")?, copy("")?)) };
    let Facts {
        selection,
        window,
        condition,
        ..
    } = *facts;
    if !window.books.is_empty() || condition.counting() {
        return none();
    }
    if let Some((title, text)) = condition.words() {
        let text = if text.is_empty() {
            copy("")?
        } else {
            say.say(text)?
        };
        return Ok((say.say(title)?, text));
    }
    let library = selection.scope == Scope::Library;
    Ok(match selection.filter.none_key() {
        Some(key) if library => (say.say(key)?, copy("")?),
        _ if library && facts.archived => (say.say("files.list.none.all")?, copy("")?),
        _ if library => (
            say.say("files.empty.title")?,
            say.with("files.empty", "dir", facts.folder)?,
        ),
        _ if selection.scope == Scope::Next => (say.say("files.list.none.next")?, copy("")?),
        _ if selection.scope == Scope::Favourites => (
            say.say("files.no-favourites.title")?,
            say.say("files.no-favourites")?,
        ),
        _ => (say.say("files.search.none")?, copy("")?),
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixtures")]
    use super::*;
    use alloc::vec;

    struct Words;
    impl Say for Words {
        fn say(&mut self, key: &str) -> Result<String, Error> {
            copy(key)
        }
        fn count(&mut self, key: &str, count: i64) -> Result<String, Error> {
            Ok(alloc::format!("{key}:{count}"))
        }
        fn with(&mut self, key: &str, _: &str, value: &str) -> Result<String, Error> {
            Ok(alloc::format!("{key}:{value}"))
        }
    }

    fn book(key: u32, progress: Option<u8>) -> BookLine {
        BookLine {
            key,
            title: alloc::format!("Book {key}"),
            author: "Ann".into(),
            series: String::new(),
            number: String::new(),
            progress,
            finished: false,
            favourite: false,
            is_new: false,
            badges: vec!["On loan".into()],
        }
    }

    fn window(books: Vec<BookLine>, more: bool) -> Window<CatalogCursor, BookLine> {
        let mut window = Window::new(CatalogCursor::start());
        window.total = u32::try_from(books.len()).unwrap();
        window.books = books;
        window.next = more.then(CatalogCursor::start);
        window
    }

    fn made(
        selection: &Selection,
        window: &Window<CatalogCursor, BookLine>,
        starts: &[CatalogCursor],
        condition: Condition,
        archived: bool,
    ) -> Built {
        build(
            &mut Words,
            &Facts {
                selection,
                window,
                starts,
                condition,
                archived,
                folder: "books",
            },
        )
        .unwrap()
    }

    #[test]
    fn the_library_leads_with_its_filter_and_count_and_the_book_read_last_is_a_card() {
        let selection = Selection::default();
        let w = window(
            vec![book(1, Some(40)), book(2, None), book(3, Some(100))],
            true,
        );
        let built = made(&selection, &w, &[], Condition::Ready, false);
        let rows: Vec<_> = built.cells.iter().map(|c| c.row).collect();
        assert_eq!(
            rows,
            [
                Row::Filter,
                Row::Book(0),
                Row::Book(1),
                Row::Book(2),
                Row::More
            ]
        );
        assert_eq!(built.cells[0].value, "files.list.show.all · files.books:3");
        let card = &built.cells[1];
        assert!(card.card);
        assert_eq!(card.value, "files.list.continue-reading");
        assert_eq!(card.progress, Some(40));
        assert_eq!(card.chips, ["On loan"]);
        // Only the first book is the card; a book not begun has no bar, and
        // neither has one read to the end.
        assert!(!built.cells[2].card);
        assert_eq!(built.cells[2].progress, None);
        assert_eq!(built.cells[3].progress, None);
        assert!(built.empty.is_empty() && built.empty_text.is_empty());
    }

    #[test]
    fn no_card_after_the_first_window_in_the_order_of_titles_or_in_a_series() {
        let w = window(vec![book(1, Some(40))], false);
        let mut selection = Selection::default();
        let later = [CatalogCursor::start()];
        assert!(!made(&selection, &w, &later, Condition::Ready, false).cells[2].card);
        selection.order = Order::Name;
        assert!(!made(&selection, &w, &[], Condition::Ready, false).cells[1].card);
        selection.order = Order::Recent;
        selection.scope = Scope::Series("Saga".into());
        let built = made(&selection, &w, &[], Condition::Ready, false);
        // A series has no filter row.
        assert!(!built.cells[0].card && built.cells[0].row == Row::Book(0));
    }

    #[test]
    fn a_scan_leads_with_the_books_found_so_far() {
        let selection = Selection::default();
        let w = window(vec![book(1, None)], false);
        let built = made(
            &selection,
            &w,
            &[],
            Condition::Scanning { books: 123 },
            false,
        );
        let note = &built.cells[1];
        assert_eq!(note.row, Row::Note);
        assert_eq!(note.title, "files.list.so-far:123");
        // The filter row does not count books that are still being found.
        assert_eq!(built.cells[0].value, "files.list.show.all");
        // A scan with nothing found yet says nothing else.
        let empty = window(Vec::new(), false);
        let built = made(
            &selection,
            &empty,
            &[],
            Condition::Scanning { books: 0 },
            false,
        );
        assert!(built.empty.is_empty());
        assert_eq!(built.cells[1].title, "files.list.so-far:0");
    }

    #[test]
    fn an_empty_library_says_why() {
        let selection = Selection::default();
        let empty = window(Vec::new(), false);
        let said = |condition, archived| {
            let built = made(&selection, &empty, &[], condition, archived);
            (built.empty, built.empty_text)
        };
        assert_eq!(
            said(Condition::Ready, false),
            ("files.empty.title".into(), "files.empty:books".into())
        );
        assert_eq!(said(Condition::Ready, true).0, "files.list.none.all");
        assert_eq!(said(Condition::Preparing, false).0, "files.list.preparing");
        assert_eq!(
            said(Condition::Failed, false),
            ("files.list.failed.title".into(), "files.list.failed".into())
        );
        assert_eq!(
            said(Condition::CardGone, false),
            (
                "files.list.card-gone.title".into(),
                "files.list.card-gone".into()
            )
        );
    }

    #[test]
    fn a_filter_a_shelf_and_a_search_without_books_say_so() {
        let empty = window(Vec::new(), false);
        let selection = Selection {
            filter: Filter::Reading,
            ..Selection::default()
        };
        assert_eq!(
            made(&selection, &empty, &[], Condition::Ready, false).empty,
            "files.list.none.reading"
        );
        for (scope, title, text) in [
            (Scope::Next, "files.list.none.next", ""),
            (
                Scope::Favourites,
                "files.no-favourites.title",
                "files.no-favourites",
            ),
            (Scope::Search("x".into()), "files.search.none", ""),
        ] {
            let selection = Selection {
                scope,
                ..Selection::default()
            };
            let built = made(&selection, &empty, &[], Condition::Ready, false);
            assert_eq!(
                (built.empty.as_str(), built.empty_text.as_str()),
                (title, text)
            );
        }
    }

    #[test]
    fn the_way_to_the_window_before_leads_the_books() {
        let selection = Selection::default();
        let w = window(vec![book(1, None)], false);
        let built = made(
            &selection,
            &w,
            &[CatalogCursor::start()],
            Condition::Ready,
            false,
        );
        assert_eq!(built.cells[1].row, Row::Previous);
        assert_eq!(built.cells[2].row, Row::Book(0));
        assert_eq!(crate::native_model::MOST_BOOKS, 96);
    }
}
