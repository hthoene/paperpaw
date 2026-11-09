//! What the native library shows, free of host calls: the filters, the
//! order, the scope of a list (the library, the favourites, an author, a
//! series, a tag, a folder), the query each makes, the books of a window
//! and the words of a row. The host's library service filters, sorts and
//! pages; the interface-6 index and name collection are not needed.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::{
    Error,
    library::{CatalogCursor, Entry, Query, Show, Sort},
};
use paperpaw_ui_kit::{
    mem::push,
    text::{concat, copy, decimal},
};

use crate::declarations::SORT_NAME;

/// Books held in a window at most: three replies of a query (`PAGE_MAX_ROWS`
/// 32 each). A longer list slides: "More books" starts the next window and
/// "Previous books" returns to the one before.
pub const MOST_BOOKS: usize = 96;

/// What the library shows, by how far books are read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Filter {
    #[default]
    All,
    Reading,
    Unread,
    Finished,
    /// Not a filter: the shelves are shown instead.
    Shelves,
}

/// The filters in the order the row steps through them.
const FILTERS: [Filter; 5] = [
    Filter::All,
    Filter::Reading,
    Filter::Unread,
    Filter::Finished,
    Filter::Shelves,
];

impl Filter {
    /// The query's `show`; none for the shelves.
    #[must_use]
    pub const fn show(self) -> Option<Show> {
        match self {
            Self::All => Some(Show::All),
            Self::Reading => Some(Show::Reading),
            Self::Unread => Some(Show::Unread),
            Self::Finished => Some(Show::Finished),
            Self::Shelves => None,
        }
    }

    /// The key of its name in the filter row (`show.*`, as interface 6 said it).
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::All => "files.list.show.all",
            Self::Reading => "files.list.show.reading",
            Self::Unread => "files.list.show.unread",
            Self::Finished => "files.list.show.finished",
            Self::Shelves => "files.list.show.shelves",
        }
    }

    /// What an empty list says, if it says anything.
    #[must_use]
    pub const fn none_key(self) -> Option<&'static str> {
        match self {
            Self::Reading => Some("files.list.none.reading"),
            Self::Unread => Some("files.list.none.unread"),
            Self::Finished => Some("files.list.none.finished"),
            _ => None,
        }
    }

    /// The filter after this one, the last going back to the first.
    #[must_use]
    pub fn next(self) -> Self {
        let at = FILTERS.iter().position(|f| *f == self).unwrap_or(0);
        FILTERS
            .get((at + 1) % FILTERS.len())
            .copied()
            .unwrap_or_default()
    }
}

/// The order of books (`files.sort`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Order {
    /// Recently read books first, then the others by title.
    #[default]
    Recent,
    Name,
}

impl Order {
    /// The order a setting value names; anything else is the default.
    #[must_use]
    pub fn from_key(key: &str) -> Self {
        if key == SORT_NAME {
            Self::Name
        } else {
            Self::Recent
        }
    }

    #[must_use]
    pub const fn sort(self) -> Sort {
        match self {
            Self::Recent => Sort::RecentlyOpened,
            Self::Name => Sort::Title,
        }
    }
}

/// Which books a list holds.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum Scope {
    /// Every book of the library.
    #[default]
    Library,
    Favourites,
    Author(String),
    Series(String),
    Tag(String),
    /// This folder and below.
    Folder(String),
    /// Books whose title, author, series, tag or file name has the words.
    Search(String),
    /// The next book of each series begun (not a host query: see
    /// [`crate::native_shelves::collect_next`]).
    Next,
}

/// A scope's names as the screen's parameters carry them.
const KINDS: [(&str, u8); 7] = [
    ("next", 6),
    ("favourites", 0),
    ("author", 1),
    ("series", 2),
    ("tag", 3),
    ("folder", 4),
    ("search", 5),
];

impl Scope {
    /// The parameters (`kind`, `value`) that open a list of this scope.
    #[must_use]
    pub fn params(&self) -> (&'static str, &str) {
        match self {
            Self::Library => ("", ""),
            Self::Favourites => ("favourites", ""),
            Self::Author(value) => ("author", value),
            Self::Series(value) => ("series", value),
            Self::Tag(value) => ("tag", value),
            Self::Folder(value) => ("folder", value),
            Self::Search(value) => ("search", value),
            Self::Next => ("next", ""),
        }
    }

    /// The scope the parameters of a screen name; an unknown kind is the library.
    ///
    /// # Errors
    /// `NoMemory` when the value cannot be kept.
    pub fn from_params(kind: &str, value: &str) -> Result<Self, Error> {
        let code = KINDS
            .iter()
            .find(|(name, _)| *name == kind)
            .map(|(_, c)| *c);
        Ok(match code {
            Some(0) => Self::Favourites,
            Some(1) => Self::Author(copy(value)?),
            Some(2) => Self::Series(copy(value)?),
            Some(3) => Self::Tag(copy(value)?),
            Some(4) => Self::Folder(copy(value)?),
            Some(5) => Self::Search(copy(value)?),
            Some(6) => Self::Next,
            _ => Self::Library,
        })
    }
}

/// What a list holds and in which order: the library's filter or a scope.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Selection {
    pub filter: Filter,
    pub order: Order,
    pub scope: Scope,
}

impl Selection {
    /// The host's query for this selection, continuing from `cursor`. The
    /// archive folder is left out of the library's own lists, not of a
    /// folder's or a search's.
    ///
    /// # Errors
    /// `NoSpace` for a name or words above the contract's cap (never shortened).
    pub fn query(&self, cursor: CatalogCursor) -> Result<Query<'_>, Error> {
        let mut query = Query::new().sort(self.order.sort()).cursor(cursor);
        match &self.scope {
            Scope::Library => {
                query = query
                    .show(self.filter.show().unwrap_or(Show::All))
                    .exclude_archive(true);
            }
            Scope::Favourites => query = query.show(Show::Favourites),
            Scope::Author(name) => query = query.author(name)?,
            Scope::Series(name) => query = query.series(name)?.sort(Sort::Series),
            Scope::Tag(name) => query = query.tag(name)?,
            Scope::Folder(path) => query = query.folder(path)?,
            Scope::Search(words) => query = query.words(words)?,
            // Read by `collect_next`, not by one query: the books of the
            // series begun, whichever way they are shown.
            Scope::Next => query = query.show(Show::Reading),
        }
        Ok(query)
    }
}

/// A book of a window, as its row says it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BookLine {
    /// The host's key, valid within the window's generation.
    pub key: u32,
    pub title: String,
    pub author: String,
    pub series: String,
    pub number: String,
    /// How far it is read; none when never opened.
    pub progress: Option<u8>,
    pub finished: bool,
    pub favourite: bool,
    pub is_new: bool,
    /// The badges plugins put on it (at most two).
    pub badges: Vec<String>,
}

impl BookLine {
    /// The line of a host row.
    ///
    /// # Errors
    /// `NoMemory` when a text cannot be kept.
    pub fn from_entry(entry: Entry<'_>) -> Result<Self, Error> {
        let mut badges = Vec::new();
        for badge in entry.badges() {
            push(&mut badges, copy(badge)?)?;
        }
        Ok(Self {
            key: entry.key(),
            title: copy(entry.title())?,
            author: copy(entry.author())?,
            series: copy(entry.series())?,
            number: copy(entry.number())?,
            progress: entry.progress(),
            finished: entry.finished(),
            favourite: entry.favourite(),
            is_new: entry.is_new(),
            badges,
        })
    }
}

/// The rows of a list (books, or names of a shelf) read so far. `C` is the host's cursor (the model is
/// generic over it so the reading is tested without a host).
#[derive(Debug)]
pub struct Window<C = CatalogCursor, T = BookLine> {
    pub books: Vec<T>,
    /// Where the window's first reply was read from.
    pub start: C,
    /// The catalog generation the books were read in.
    pub generation: u32,
    /// How many books match in all.
    pub total: u32,
    /// Where the next reply continues; none after the last.
    pub next: Option<C>,
}

impl<C: Copy, T> Window<C, T> {
    /// An empty window that will be read from `start`.
    #[must_use]
    pub const fn new(start: C) -> Self {
        Self {
            books: Vec::new(),
            start,
            generation: 0,
            total: 0,
            next: None,
        }
    }

    /// Whether more books can be read and the window has room for them.
    #[must_use]
    pub fn can_grow(&self) -> bool {
        self.next.is_some() && self.books.len() < MOST_BOOKS
    }

    /// Whether books follow the window's last one (to grow it or to slide to).
    #[must_use]
    pub const fn has_more(&self) -> bool {
        self.next.is_some()
    }
}

/// One reply of the host's query.
#[derive(Debug)]
pub struct Replied<C, T = BookLine> {
    pub books: Vec<T>,
    pub generation: u32,
    pub total: u32,
    pub next: Option<C>,
}

/// Where replies come from: the host's library service.
pub trait Source<C, T = BookLine> {
    /// The reply that continues at `from`; `Error::State` when the catalog
    /// changed since the cursor was given.
    ///
    /// # Errors
    /// The host's refusal or `NoMemory`.
    fn read(&mut self, from: C) -> Result<Replied<C, T>, Error>;
}

/// Reads replies into a window from `start` until it holds `want` books
/// (at most [`MOST_BOOKS`], the last reply read may go over) or the last one
/// was read. A reply holds as many rows as fit its buffer (32, but a few
/// with long titles), so the count of replies is not bounded by a constant
/// but by the books wanted; a reply without rows ends the reading. A stale cursor
/// starts the window over from `first` once; the flag is then true: the
/// windows before it were read in another generation.
///
/// # Errors
/// The source's refusal.
pub fn read_window<C: Copy, T>(
    source: &mut impl Source<C, T>,
    start: C,
    first: C,
    want: usize,
) -> Result<(Window<C, T>, bool), Error> {
    let mut window = Window::new(start);
    let mut restarted = false;
    let mut cursor = start;
    for _ in 0..=MOST_BOOKS {
        let reply = match source.read(cursor) {
            Ok(reply) => reply,
            Err(Error::State) if !restarted => {
                restarted = true;
                window = Window::new(first);
                cursor = first;
                continue;
            }
            Err(error) => return Err(error),
        };
        let empty = reply.books.is_empty();
        for book in reply.books {
            push(&mut window.books, book)?;
        }
        window.generation = reply.generation;
        window.total = reply.total;
        window.next = reply.next;
        match window.next {
            Some(next) if !empty && window.books.len() < want.min(MOST_BOOKS) => cursor = next,
            _ => break,
        }
    }
    Ok((window, restarted))
}

/// The words a row is made of, in the reader's language.
pub trait Say {
    /// The catalog's text for `key`.
    ///
    /// # Errors
    /// The host's refusal or `NoMemory`.
    fn say(&mut self, key: &str) -> Result<String, Error>;

    /// The catalog's plural text for `key` and `count` (`files.books`).
    ///
    /// # Errors
    /// The host's refusal or `NoMemory`.
    fn count(&mut self, key: &str, count: i64) -> Result<String, Error>;

    /// The catalog's text for `key` with `value` for its `{name}`.
    ///
    /// # Errors
    /// The host's refusal or `NoMemory`.
    fn with(&mut self, key: &str, name: &str, value: &str) -> Result<String, Error>;
}

/// A book's row: its title, how far it is read (or *Finished*, or *New*)
/// and, below, its author and series with number. The badges plugins put
/// on it are [`chips`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub title: String,
    pub value: String,
    pub note: String,
}

/// Makes the row of `book`.
///
/// # Errors
/// `NoMemory` when a text cannot be kept, or the words' refusal.
pub fn line(say: &mut impl Say, book: &BookLine) -> Result<Line, Error> {
    let value = if book.finished {
        say.say("files.list.filter.finished")?
    } else if let Some(progress) = book.progress.filter(|p| *p > 0) {
        let mut digits = [0; 20];
        concat(&[decimal(usize::from(progress), &mut digits), " %"])?
    } else if book.is_new {
        say.say("files.list.new")?
    } else {
        String::new()
    };
    let mut note = String::new();
    let mut part = |text: &str| -> Result<(), Error> {
        if text.is_empty() {
            return Ok(());
        }
        note = if note.is_empty() {
            copy(text)?
        } else {
            concat(&[&note, " · ", text])?
        };
        Ok(())
    };
    part(&book.author)?;
    if !book.series.is_empty() {
        let numbered = if book.number.is_empty() {
            copy(&book.series)?
        } else {
            concat(&[&book.series, " #", &book.number])?
        };
        part(&numbered)?;
    }
    Ok(Line {
        title: copy(&book.title)?,
        value,
        note,
    })
}

/// The badges plugins put on `book`, copied for its row's chips.
///
/// # Errors
/// `NoMemory`.
pub fn chips(book: &BookLine) -> Result<Vec<String>, Error> {
    let mut chips = Vec::new();
    chips
        .try_reserve_exact(book.badges.len())
        .map_err(|_| Error::NoMemory)?;
    for badge in &book.badges {
        push(&mut chips, copy(badge)?)?;
    }
    Ok(chips)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixtures")]
    use super::*;

    struct Words;
    impl Say for Words {
        fn say(&mut self, key: &str) -> Result<String, Error> {
            copy(key)
        }
        fn count(&mut self, key: &str, _: i64) -> Result<String, Error> {
            copy(key)
        }
        fn with(&mut self, key: &str, _: &str, value: &str) -> Result<String, Error> {
            concat(&[key, ":", value])
        }
    }

    fn book() -> BookLine {
        BookLine {
            key: 1,
            title: "T".into(),
            author: "A".into(),
            series: String::new(),
            number: String::new(),
            progress: None,
            finished: false,
            favourite: false,
            is_new: false,
            badges: Vec::new(),
        }
    }

    #[test]
    fn filters_step_through_all_and_end_at_the_shelves() {
        let mut filter = Filter::All;
        let mut seen = Vec::new();
        for _ in 0..5 {
            seen.push(filter);
            filter = filter.next();
        }
        assert_eq!(seen, FILTERS);
        assert_eq!(filter, Filter::All);
        assert_eq!(Filter::Shelves.show(), None);
        assert_eq!(Filter::Reading.show(), Some(Show::Reading));
        assert!(Filter::All.none_key().is_none());
        assert!(Filter::Finished.none_key().is_some());
    }

    #[test]
    fn the_order_follows_the_setting_and_unknown_is_recent() {
        assert_eq!(Order::from_key(SORT_NAME), Order::Name);
        assert_eq!(Order::from_key("files.sort.recent"), Order::Recent);
        assert_eq!(Order::from_key("nonsense"), Order::Recent);
        assert_eq!(Order::Name.sort(), Sort::Title);
        assert_eq!(Order::Recent.sort(), Sort::RecentlyOpened);
    }

    #[test]
    fn a_scope_survives_its_parameters_and_an_unknown_kind_is_the_library() {
        for scope in [
            Scope::Favourites,
            Scope::Author("Ann".into()),
            Scope::Series("Saga".into()),
            Scope::Tag("sf".into()),
            Scope::Folder("books/Archive".into()),
            Scope::Search("two words".into()),
            Scope::Next,
        ] {
            let (kind, value) = scope.params();
            assert_eq!(Scope::from_params(kind, value).unwrap(), scope);
        }
        assert_eq!(Scope::from_params("x", "y").unwrap(), Scope::Library);
        assert_eq!(Scope::Library.params(), ("", ""));
    }

    #[test]
    fn every_scope_makes_a_query_and_a_too_long_name_is_refused() {
        let all = Selection::default();
        assert!(all.query(CatalogCursor::start()).is_ok());
        for scope in [
            Scope::Favourites,
            Scope::Author("Ann".into()),
            Scope::Series("Saga".into()),
            Scope::Tag("sf".into()),
            Scope::Folder("books".into()),
            Scope::Search("words".into()),
            Scope::Next,
        ] {
            let selection = Selection {
                scope,
                ..Selection::default()
            };
            assert!(selection.query(CatalogCursor::start()).is_ok());
        }
        let long = Selection {
            scope: Scope::Search("x".repeat(257)),
            ..Selection::default()
        };
        assert_eq!(
            long.query(CatalogCursor::start()).err(),
            Some(Error::NoSpace)
        );
    }

    #[test]
    fn a_row_says_progress_finished_or_new_and_joins_its_note() {
        let mut b = book();
        let row = line(&mut Words, &b).unwrap();
        assert_eq!((row.value.as_str(), row.note.as_str()), ("", "A"));
        b.is_new = true;
        assert_eq!(line(&mut Words, &b).unwrap().value, "files.list.new");
        b.progress = Some(0);
        assert_eq!(line(&mut Words, &b).unwrap().value, "files.list.new");
        b.is_new = false;
        assert_eq!(line(&mut Words, &b).unwrap().value, "");
        b.is_new = true;
        b.progress = Some(45);
        assert_eq!(line(&mut Words, &b).unwrap().value, "45 %");
        b.finished = true;
        assert_eq!(
            line(&mut Words, &b).unwrap().value,
            "files.list.filter.finished"
        );
        b.series = "Saga".into();
        b.number = "2".into();
        b.badges = alloc::vec!["On loan".into()];
        // The badges are chips of their own, not words of the note.
        assert_eq!(line(&mut Words, &b).unwrap().note, "A · Saga #2");
        assert_eq!(chips(&b).unwrap(), ["On loan"]);
        b.author = String::new();
        b.number = String::new();
        assert_eq!(line(&mut Words, &b).unwrap().note, "Saga");
    }

    /// A list of `len` books read 32 at a time; cursors are the offsets.
    struct Fake {
        len: u32,
        rows: u32,
        stale_once: bool,
        calls: Vec<u32>,
    }

    impl Source<u32> for Fake {
        fn read(&mut self, from: u32) -> Result<Replied<u32>, Error> {
            self.calls.push(from);
            if self.stale_once && from != 0 {
                self.stale_once = false;
                return Err(Error::State);
            }
            let end = (from + self.rows).min(self.len);
            let books = (from..end)
                .map(|n| {
                    let mut b = book();
                    b.key = n;
                    b
                })
                .collect();
            Ok(Replied {
                books,
                generation: 1,
                total: self.len,
                next: (end < self.len).then_some(end),
            })
        }
    }

    fn fake(len: u32) -> Fake {
        Fake {
            len,
            rows: 32,
            stale_once: false,
            calls: Vec::new(),
        }
    }

    #[test]
    fn a_window_grows_by_replies_until_its_bound() {
        let mut source = fake(200);
        let (window, _) = read_window(&mut source, 0, 0, 1).unwrap();
        assert_eq!(window.books.len(), 32);
        assert!(window.can_grow() && window.has_more());
        let (window, _) = read_window(&mut source, 0, 0, 64).unwrap();
        assert_eq!(window.books.len(), 64);
        let (window, _) = read_window(&mut source, 0, 0, 1_000).unwrap();
        assert_eq!(window.books.len(), MOST_BOOKS);
        assert!(!window.can_grow() && window.has_more());
        assert_eq!(window.next, Some(96));
    }

    #[test]
    fn the_97th_book_and_the_rest_are_reached_by_sliding_on_and_back() {
        let mut source = fake(250);
        let (first, _) = read_window(&mut source, 0, 0, MOST_BOOKS).unwrap();
        assert_eq!(first.books.len(), 96);
        // "More books" on a full window starts the next at `next`; the
        // window starts with one reply and grows by "More books" until full.
        let (second, _) = read_window(&mut source, first.next.unwrap(), 0, 0).unwrap();
        assert_eq!(second.books.first().map(|b| b.key), Some(96));
        let (second, _) = read_window(&mut source, second.start, 0, MOST_BOOKS).unwrap();
        assert_eq!(second.books.last().map(|b| b.key), Some(191));
        let (third, _) = read_window(&mut source, second.next.unwrap(), 0, MOST_BOOKS).unwrap();
        assert_eq!(third.books.len(), 250 - 192);
        assert_eq!(third.books.last().map(|b| b.key), Some(249));
        assert!(!third.has_more());
        // "Previous books" reads the window before in full, from its start.
        let (back, _) = read_window(&mut source, second.start, 0, MOST_BOOKS).unwrap();
        assert_eq!(back.books.len(), 96);
        assert_eq!(back.books.first().map(|b| b.key), Some(96));
    }

    #[test]
    fn short_replies_still_fill_a_window_and_the_list_slides_on() {
        // Long titles: a reply holds ten rows, so ten replies make the window.
        let mut source = fake(250);
        source.rows = 10;
        let (window, _) = read_window(&mut source, 0, 0, MOST_BOOKS).unwrap();
        assert!(window.books.len() >= MOST_BOOKS && !window.can_grow());
        assert!(window.has_more());
        // One reply is a start; growing by 32 reads on until the window is full.
        let (small, _) = read_window(&mut source, 0, 0, 1).unwrap();
        assert_eq!(small.books.len(), 10);
        let (grown, _) = read_window(&mut source, 0, 0, small.books.len() + 32).unwrap();
        assert!(grown.books.len() >= 42);
        // The next window starts after the last book read, the 97th included.
        let (next, _) = read_window(&mut source, window.next.unwrap(), 0, MOST_BOOKS).unwrap();
        assert_eq!(
            next.books.first().map(|b| usize::try_from(b.key).unwrap()),
            Some(window.books.len())
        );
        // The list ends: every book is reached once.
        let mut seen = window.books.len();
        let mut at = next;
        while let Some(more) = at.next {
            seen += at.books.len();
            at = read_window(&mut source, more, 0, MOST_BOOKS).unwrap().0;
        }
        assert_eq!(seen + at.books.len(), 250);
    }

    #[test]
    fn a_reply_without_rows_ends_the_reading() {
        struct Hollow;
        impl Source<u32> for Hollow {
            fn read(&mut self, from: u32) -> Result<Replied<u32>, Error> {
                Ok(Replied {
                    books: Vec::new(),
                    generation: 1,
                    total: 9,
                    next: Some(from + 1),
                })
            }
        }
        let (window, _) = read_window(&mut Hollow, 0, 0, MOST_BOOKS).unwrap();
        assert!(window.books.is_empty());
    }

    #[test]
    fn exactly_96_books_have_no_more_and_97_have_one() {
        let (all, _) = read_window(&mut fake(96), 0, 0, MOST_BOOKS).unwrap();
        assert!(!all.has_more());
        let (more, _) = read_window(&mut fake(97), 0, 0, MOST_BOOKS).unwrap();
        assert!(more.has_more());
        let (tail, _) = read_window(&mut fake(97), more.next.unwrap(), 0, 0).unwrap();
        assert_eq!(tail.books.len(), 1);
        assert!(!tail.has_more());
    }

    #[test]
    fn a_stale_cursor_starts_over_once_and_says_so() {
        let mut source = fake(200);
        source.stale_once = true;
        let (window, restarted) = read_window(&mut source, 96, 0, MOST_BOOKS).unwrap();
        assert!(restarted);
        assert_eq!(window.start, 0);
        assert_eq!(window.books.first().map(|b| b.key), Some(0));
        assert_eq!(source.calls.first(), Some(&96));
        // A second refusal is an error, not a loop.
        struct Always;
        impl Source<u32> for Always {
            fn read(&mut self, _: u32) -> Result<Replied<u32>, Error> {
                Err(Error::State)
            }
        }
        assert_eq!(read_window(&mut Always, 5, 0, 1).err(), Some(Error::State));
    }
}
