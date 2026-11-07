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
/// 32 each). More are read when asked for, the earlier ones kept.
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

    /// The key of its name.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::All => "files.list.filter.all",
            Self::Reading => "files.list.filter.reading",
            Self::Unread => "files.list.filter.unread",
            Self::Finished => "files.list.filter.finished",
            Self::Shelves => "files.list.filter.shelves",
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
}

/// A scope's names as the screen's parameters carry them.
const KINDS: [(&str, u8); 6] = [
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

/// The books of a list read so far.
#[derive(Debug, Default)]
pub struct Window {
    pub books: Vec<BookLine>,
    /// The catalog generation the books were read in.
    pub generation: u32,
    /// How many books match in all.
    pub total: u32,
    /// Where the next reply continues; none after the last.
    pub next: Option<CatalogCursor>,
}

impl Window {
    /// Whether more books can be read and the window has room for them.
    #[must_use]
    pub fn can_grow(&self) -> bool {
        self.next.is_some() && self.books.len() < MOST_BOOKS
    }

    /// Whether books match that the window does not hold.
    #[must_use]
    pub fn has_more(&self) -> bool {
        usize::try_from(self.total).map_or(true, |total| total > self.books.len())
    }
}

/// The words a row is made of, in the reader's language.
pub trait Say {
    /// The catalog's text for `key`.
    ///
    /// # Errors
    /// The host's refusal or `NoMemory`.
    fn say(&mut self, key: &str) -> Result<String, Error>;
}

/// A book's row: its title, how far it is read (or *Finished*, or *New*)
/// and, below, its author, series with number and badges.
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
    } else if let Some(progress) = book.progress {
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
    for badge in &book.badges {
        part(badge)?;
    }
    Ok(Line {
        title: copy(&book.title)?,
        value,
        note,
    })
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
        assert_eq!(line(&mut Words, &b).unwrap().note, "A · Saga #2 · On loan");
        b.author = String::new();
        b.number = String::new();
        assert_eq!(line(&mut Words, &b).unwrap().note, "Saga · On loan");
    }

    #[test]
    fn a_window_grows_until_its_bound() {
        let mut window = Window::default();
        assert!(!window.can_grow());
        window.next = Some(CatalogCursor::start());
        window.total = 200;
        assert!(window.can_grow() && window.has_more());
        window.books = (0..MOST_BOOKS).map(|_| book()).collect();
        assert!(!window.can_grow() && window.has_more());
        window.total = 96;
        assert!(!window.has_more());
    }
}
