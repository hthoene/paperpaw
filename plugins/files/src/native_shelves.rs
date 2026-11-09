//! What the shelves show, free of host calls: the lists of names (authors,
//! series, tags, folders) with their book counts, a name's row, and the
//! next book of each series begun. The host lists the names (`facets`, LIB
//! op 5) and the books; this holds the rules about them.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::{Error, library::Facet};
use paperpaw_ui_kit::{
    mem::push,
    text::{concat, copy},
};

use crate::native_model::{BookLine, Say, Scope};

/// One list of names the shelves hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shelf {
    Authors,
    Series,
    Tags,
    Folders,
}

impl Shelf {
    /// The lists in the order the shelves show them.
    pub const ALL: [Self; 4] = [Self::Authors, Self::Series, Self::Tags, Self::Folders];

    /// The host's list of these names.
    #[must_use]
    pub const fn facet(self) -> Facet {
        match self {
            Self::Authors => Facet::Authors,
            Self::Series => Facet::Series,
            Self::Tags => Facet::Tags,
            Self::Folders => Facet::Folders,
        }
    }

    /// The name of the list in a screen's parameters.
    #[must_use]
    pub const fn kind(self) -> &'static str {
        match self {
            Self::Authors => "authors",
            Self::Series => "series",
            Self::Tags => "tags",
            Self::Folders => "folders",
        }
    }

    /// The list a screen's parameter names; none for an unknown name.
    #[must_use]
    pub fn from_kind(kind: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|shelf| shelf.kind() == kind)
    }

    /// The key of the list's title (also its row on the shelves).
    #[must_use]
    pub const fn title_key(self) -> &'static str {
        match self {
            Self::Authors => "files.by-author",
            Self::Series => "files.by-series",
            Self::Tags => "files.by-tag",
            Self::Folders => "files.by-folder",
        }
    }

    /// What an empty list says.
    #[must_use]
    pub const fn none_key(self) -> &'static str {
        match self {
            Self::Series => "files.no-series",
            Self::Tags => "files.no-tags",
            Self::Authors | Self::Folders => "files.no-books",
        }
    }

    /// The books of one of its names.
    ///
    /// # Errors
    /// `NoMemory` when the name cannot be kept.
    pub fn scope(self, name: &str) -> Result<Scope, Error> {
        let name = copy(name)?;
        Ok(match self {
            Self::Authors => Scope::Author(name),
            Self::Series => Scope::Series(name),
            Self::Tags => Scope::Tag(name),
            Self::Folders => Scope::Folder(name),
        })
    }
}

/// The most bytes of a name a window holds (the host's names go to 256): a
/// longer one is cut for its row and read whole again when it is chosen.
pub const MOST_NAME: usize = 128;

/// `text` cut at a character boundary to `most` bytes at most, and whether
/// it was cut.
#[must_use]
pub fn short(text: &str, most: usize) -> (&str, bool) {
    if text.len() <= most {
        return (text, false);
    }
    let end = (0..=most)
        .rev()
        .find(|at| text.is_char_boundary(*at))
        .unwrap_or(0);
    (text.get(..end).unwrap_or(""), true)
}

/// A name of a list and how many books have it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NameLine {
    /// The name, cut to [`MOST_NAME`] bytes.
    pub text: String,
    /// Whether the name was cut: the host has it whole.
    pub cut: bool,
    pub count: u32,
}

/// A name's row: the name (a folder by its own name, no author as
/// *Unknown author*) and, below, how many books it has.
///
/// # Errors
/// `NoMemory` when a text cannot be kept, or the words' refusal.
pub fn name_row(
    say: &mut impl Say,
    shelf: Shelf,
    name: &NameLine,
) -> Result<(String, String), Error> {
    let title = if name.text.is_empty() {
        say.say("files.unknown-author")?
    } else if shelf == Shelf::Folders {
        copy(
            name.text
                .trim_end_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or(""),
        )?
    } else {
        copy(&name.text)?
    };
    let title = if name.cut {
        concat(&[&title, "…"])?
    } else {
        title
    };
    Ok((title, say.count("files.books", i64::from(name.count))?))
}

/// How many series the next books are gathered for, at most. A library
/// of more begun series shows the first of them.
pub const MOST_SERIES: usize = 48;

/// Pages of series names read for each state at most.
const MOST_PAGES: usize = 8;

/// Series looked at in all (two host calls each) before the list is
/// made of what was found.
pub const MOST_LOOKED: usize = 96;

/// How far a series is begun: a book of it is being read, or finished.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Begun {
    Reading,
    Finished,
}

/// One reply of series names.
#[derive(Debug)]
pub struct SeriesNames<C> {
    pub names: Vec<String>,
    /// Where the next reply continues; none after the last.
    pub next: Option<C>,
}

/// Where the series begun and their next books come from: the host.
pub trait SeriesSource<C> {
    /// The names of the series with a book `begun` this way, from `from`.
    ///
    /// # Errors
    /// The host's refusal (`State` for a changed catalog) or `NoMemory`.
    fn names(&mut self, begun: Begun, from: C) -> Result<SeriesNames<C>, Error>;

    /// The series' lowest unread book (the host's `next_in_series` from a
    /// book of it that is `begun` this way); none when every book of it was
    /// opened.
    ///
    /// # Errors
    /// The host's refusal (`State` for a changed catalog) or `NoMemory`.
    fn next_of(&mut self, begun: Begun, series: &str) -> Result<Option<BookLine>, Error>;
}

/// The next book of each series begun, the series being read first and
/// those of finished books after them (a series once, `most` books at
/// most). A book already begun or finished is not a next book (the host
/// gives the lowest unread one, so this only guards a stale answer).
///
/// # Errors
/// The source's refusal or `NoMemory`.
pub fn collect_next<C: Copy>(
    source: &mut impl SeriesSource<C>,
    first: C,
    most: usize,
) -> Result<Vec<BookLine>, Error> {
    let mut found: Vec<BookLine> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let mut looked = 0;
    'states: for begun in [Begun::Reading, Begun::Finished] {
        let mut from = first;
        for _ in 0..MOST_PAGES {
            let page = source.names(begun, from)?;
            let empty = page.names.is_empty();
            for name in page.names {
                if name.is_empty() || seen.contains(&name) {
                    continue;
                }
                if let Some(book) = source.next_of(begun, &name)?
                    && book.progress.is_none()
                    && !book.finished
                {
                    push(&mut found, book)?;
                }
                push(&mut seen, name)?;
                looked += 1;
                if found.len() >= most || looked >= MOST_LOOKED {
                    break 'states;
                }
            }
            match page.next {
                Some(next) if !empty => from = next,
                _ => break,
            }
        }
    }
    Ok(found)
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
        fn count(&mut self, key: &str, count: i64) -> Result<String, Error> {
            Ok(alloc::format!("{key}:{count}"))
        }
    }

    fn name(text: &str, count: u32) -> NameLine {
        NameLine {
            text: text.into(),
            cut: false,
            count,
        }
    }

    #[test]
    fn a_long_name_is_cut_at_a_character_and_says_so() {
        assert_eq!(short("abc", 5), ("abc", false));
        assert_eq!(short("abcdef", 3), ("abc", true));
        // Two-byte letters: never cut in the middle of one.
        assert_eq!(short("ééé", 3), ("é", true));
        assert_eq!(short("ééé", 0), ("", true));
        let cut = NameLine {
            cut: true,
            ..name("Long", 1)
        };
        assert_eq!(
            name_row(&mut Words, Shelf::Authors, &cut).unwrap().0,
            "Long…"
        );
    }

    #[test]
    fn a_shelf_survives_its_parameter_and_an_unknown_one_is_none() {
        for shelf in Shelf::ALL {
            assert_eq!(Shelf::from_kind(shelf.kind()), Some(shelf));
        }
        assert_eq!(Shelf::from_kind("letters"), None);
        assert_eq!(Shelf::Series.facet(), Facet::Series);
    }

    #[test]
    fn a_name_opens_the_books_of_its_kind() {
        assert_eq!(
            Shelf::Authors.scope("Ann").unwrap(),
            Scope::Author("Ann".into())
        );
        assert_eq!(
            Shelf::Folders.scope("books/A").unwrap(),
            Scope::Folder("books/A".into())
        );
        assert_eq!(Shelf::Tags.scope("sf").unwrap(), Scope::Tag("sf".into()));
        assert_eq!(
            Shelf::Series.scope("Saga").unwrap(),
            Scope::Series("Saga".into())
        );
    }

    #[test]
    fn a_row_names_the_name_and_counts_its_books() {
        let row = name_row(&mut Words, Shelf::Authors, &name("Ann", 2)).unwrap();
        assert_eq!(row, ("Ann".into(), "files.books:2".into()));
        // No author is the unknown author; a folder shows its own name.
        let row = name_row(&mut Words, Shelf::Authors, &name("", 1)).unwrap();
        assert_eq!(row.0, "files.unknown-author");
        let row = name_row(&mut Words, Shelf::Folders, &name("books/Crime/", 3)).unwrap();
        assert_eq!(row.0, "Crime");
        let row = name_row(&mut Words, Shelf::Tags, &name("a/b", 3)).unwrap();
        assert_eq!(row.0, "a/b");
    }

    fn line(title: &str, progress: Option<u8>, finished: bool) -> BookLine {
        BookLine {
            key: 1,
            title: title.into(),
            author: String::new(),
            series: String::new(),
            number: String::new(),
            progress,
            finished,
            favourite: false,
            is_new: false,
            badges: Vec::new(),
        }
    }

    /// Series by state, each with the book that follows its last begun one.
    struct Fake {
        reading: Vec<&'static str>,
        finished: Vec<&'static str>,
        /// Names per reply.
        per_page: usize,
        followers: Vec<(&'static str, BookLine)>,
        asked: Vec<(Begun, String)>,
    }

    impl SeriesSource<usize> for Fake {
        fn names(&mut self, begun: Begun, from: usize) -> Result<SeriesNames<usize>, Error> {
            let all = match begun {
                Begun::Reading => &self.reading,
                Begun::Finished => &self.finished,
            };
            let end = (from + self.per_page).min(all.len());
            let names = all[from.min(end)..end]
                .iter()
                .map(|n| (*n).into())
                .collect();
            Ok(SeriesNames {
                names,
                next: (end < all.len()).then_some(end),
            })
        }

        fn next_of(&mut self, begun: Begun, series: &str) -> Result<Option<BookLine>, Error> {
            self.asked.push((begun, series.into()));
            Ok(self
                .followers
                .iter()
                .find(|(name, _)| *name == series)
                .map(|(_, book)| book.clone()))
        }
    }

    fn fake() -> Fake {
        Fake {
            reading: alloc::vec!["Saga", "Tales"],
            finished: alloc::vec!["Done", "Saga", "Seen"],
            per_page: 2,
            followers: alloc::vec![
                ("Saga", line("Saga 2", None, false)),
                ("Tales", line("Tales 4", None, false)),
                ("Done", line("Done 9", None, false)),
                // The follower was read before: nothing is next.
                ("Seen", line("Seen 1", Some(10), false)),
            ],
            asked: Vec::new(),
        }
    }

    #[test]
    fn the_next_books_come_series_being_read_first_each_series_once() {
        let mut source = fake();
        let found = collect_next(&mut source, 0, 10).unwrap();
        let titles: Vec<&str> = found.iter().map(|b| b.title.as_str()).collect();
        assert_eq!(titles, ["Saga 2", "Tales 4", "Done 9"]);
        // Saga was asked once, as a series being read; Seen showed nothing.
        let sagas = source.asked.iter().filter(|(_, n)| n == "Saga").count();
        assert_eq!(sagas, 1);
        assert!(source.asked.contains(&(Begun::Finished, "Seen".into())));
    }

    #[test]
    fn the_next_books_stop_at_the_most_asked_and_unknown_series_are_skipped() {
        let mut source = fake();
        assert_eq!(collect_next(&mut source, 0, 1).unwrap().len(), 1);
        let mut none = fake();
        none.reading = alloc::vec![""];
        none.finished = Vec::new();
        assert!(collect_next(&mut none, 0, 5).unwrap().is_empty());
        assert!(none.asked.is_empty());
        let mut gone = fake();
        gone.followers = Vec::new();
        assert!(collect_next(&mut gone, 0, 5).unwrap().is_empty());
    }

    #[test]
    fn a_library_of_very_many_series_looks_at_a_bounded_number() {
        struct Many(usize);
        impl SeriesSource<usize> for Many {
            fn names(&mut self, _: Begun, from: usize) -> Result<SeriesNames<usize>, Error> {
                Ok(SeriesNames {
                    names: (from..from + 32).map(|n| alloc::format!("s{n}")).collect(),
                    next: Some(from + 32),
                })
            }
            fn next_of(&mut self, _: Begun, _: &str) -> Result<Option<BookLine>, Error> {
                self.0 += 1;
                Ok(None)
            }
        }
        let mut source = Many(0);
        assert!(collect_next(&mut source, 0, 5).unwrap().is_empty());
        assert_eq!(source.0, MOST_LOOKED);
    }

    #[test]
    fn a_refusal_of_the_source_is_the_refusal_of_the_collection() {
        struct Stale;
        impl SeriesSource<usize> for Stale {
            fn names(&mut self, _: Begun, _: usize) -> Result<SeriesNames<usize>, Error> {
                Err(Error::State)
            }
            fn next_of(&mut self, _: Begun, _: &str) -> Result<Option<BookLine>, Error> {
                Ok(None)
            }
        }
        assert_eq!(collect_next(&mut Stale, 0, 5).err(), Some(Error::State));
    }

    #[test]
    fn pages_that_never_end_are_read_a_bounded_number_of_times() {
        struct Endless(usize);
        impl SeriesSource<usize> for Endless {
            fn names(&mut self, _: Begun, from: usize) -> Result<SeriesNames<usize>, Error> {
                self.0 += 1;
                Ok(SeriesNames {
                    names: alloc::vec![alloc::format!("s{from}")],
                    next: Some(from + 1),
                })
            }
            fn next_of(&mut self, _: Begun, _: &str) -> Result<Option<BookLine>, Error> {
                Ok(None)
            }
        }
        let mut source = Endless(0);
        assert!(collect_next(&mut source, 0, 5).unwrap().is_empty());
        assert_eq!(source.0, 2 * MOST_PAGES);
    }
}
