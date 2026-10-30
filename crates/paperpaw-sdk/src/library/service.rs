//! The library as a service: one place every view of the books asks
//! ([`service`]), which a plugin may replace with its own (a library kept
//! by Calibre's database, say), and what plugins add to every view of a
//! book: fields kept with it ([`BookField`]) and badges on its row
//! ([`BookBadge`]).
//!
//! The kernel's library (the default) keeps what it knows in
//! `.paperpaw/library`: each book's details, the books read last, the
//! favourites and the catalog ([`catalog`], [`index`](mod@super::index)). A replacement
//! publishes itself as the service [`SERVICE`]
//! ([`Registrar::provide`](crate::Registrar::provide), a
//! [`LibraryService`] holding a `Box<dyn Library>`) and answers the same questions its own way; views
//! that ask [`service`] show its books.

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use super::catalog::{self, Query, Record, State};
use super::index::Index;
use super::{Book, what};
use crate::{Command, Env, Id, StorageError};

/// The service id of the library ([`service`]).
pub const SERVICE: &str = "core.library";

/// What every view of the library asks: books, their details, what is
/// found, the index of all, and changes to a book. Every method has the
/// kernel's library (its catalog) as its default. A library of one's own
/// overrides at least [`Library::each`], [`Library::records`] and
/// [`Library::version`] (and [`Library::catalog_state`]): the index,
/// queries, shelves, the letters and the widgets are then of its books.
pub trait Library {
    /// What the library knows of the book at `path`.
    fn book(&self, env: &Env<'_>, path: &str) -> Option<Book> {
        super::book(env.storage(), path)
    }

    /// Whether there are books to ask for ([`catalog::state`]).
    fn catalog_state(&self, env: &Env<'_>) -> State {
        catalog::state(env.storage())
    }

    /// Calls `f` with every book and its key (where its line starts in the
    /// catalog, [`catalog::each_at`]): a key fits 32 bits, and
    /// [`Library::records`] reads the book again by it.
    fn each(&self, env: &Env<'_>, f: &mut dyn FnMut(u64, Record)) {
        catalog::each_at(env.storage(), f);
    }

    /// The books of `keys` (a page of a view), each `None` where it is no
    /// longer there ([`catalog::read_page`]).
    fn records(&self, env: &Env<'_>, keys: &[u64]) -> Vec<Option<Record>> {
        catalog::read_page(env.storage(), keys)
    }

    /// Which version of the books there is: another once keys may mean
    /// other books ([`index::catalog_version`](super::index::catalog_version)),
    /// so that what a view keeps of them is known to hold; `None` without
    /// books.
    fn version(&self, env: &Env<'_>) -> Option<u64> {
        super::index::catalog_version(env.storage())
    }

    /// `n` books `query` finds, after `after` ([`catalog::query`]).
    fn query(&self, env: &Env<'_>, query: &Query, after: Option<&Record>, n: usize) -> Vec<Record> {
        catalog::query_in(&mut |f| self.each(env, f), query, after, n)
    }

    /// How many books `query` finds.
    fn found(&self, env: &Env<'_>, query: &Query) -> usize {
        catalog::found_in(&mut |f| self.each(env, f), query)
    }

    /// The index of every book ([`Index::collect`]): at most `cap`, those
    /// in the folder `archive` left out.
    fn index(&self, env: &Env<'_>, cap: usize, archive: &str) -> Index {
        let opened = super::index::opened(env.storage());
        let version = self.version(env);
        Index::collect(cap, archive, &opened, version, &mut |f| self.each(env, f))
    }

    /// Whether `index` is of the books there are now: its keys hold.
    fn is_current(&self, env: &Env<'_>, index: &Index) -> bool {
        index.version == self.version(env)
    }

    /// Takes what changed of single books into `index` (how far read,
    /// favourite, removed, the books read last; [`Index::refresh`]).
    fn refresh(&self, env: &Env<'_>, index: &mut Index) {
        index.refresh(env.storage());
    }

    /// Keeps `index` in the file `file` while the view lets it go
    /// (`Index::save`); `false` where it is not kept.
    fn keep_index(&self, env: &Env<'_>, index: &Index, file: &str, archive: &str) -> bool {
        index.write(env.storage(), file, archive).is_ok()
    }

    /// The index kept in `file`, where it is of the books there are now
    /// ([`Index::load`]).
    fn kept_index(&self, env: &Env<'_>, file: &str, archive: &str) -> Option<Index> {
        Index::load(env.storage(), file, archive, self.version(env)?)
    }

    /// The book of item `i` of `index`.
    fn record(&self, env: &Env<'_>, index: &Index, i: usize) -> Option<Record> {
        let key = u64::from(index.items.get(i)?.offset);
        self.records(env, &[key]).pop().flatten()
    }

    /// Marks the book at `path` finished, or unread; the command tells
    /// everyone ([`super::changed_book`]).
    fn set_finished(&self, env: &Env<'_>, path: &str, finished: bool) -> Command {
        super::set_finished(env.storage(), path, finished);
        super::changed_book(path, what::STATUS | what::PROGRESS)
    }

    /// Makes the book at `path` a favourite, or no longer one.
    fn set_favourite(&self, env: &Env<'_>, path: &str, favourite: bool) -> Command {
        super::set_favourite(env.storage(), path, favourite);
        super::changed_book(path, what::FAVOURITE)
    }

    /// Whether the book at `path` can be deleted (views offer it only
    /// then): the kernel's library, where its file is there and is a
    /// book ([`is_book_path`]).
    fn can_delete(&self, env: &Env<'_>, path: &str) -> bool {
        is_book_path(env, path) && env.storage().exists(path)
    }

    /// Deletes the books at `paths`: the kernel's library removes each
    /// file and says it is gone ([`BOOK_REMOVED`](crate::builtin::events::BOOK_REMOVED)),
    /// on which the kernel forgets it and removes what is kept for it
    /// (its place, notes, details, covers, what plugins keep); a book
    /// whose file is gone already is forgotten as well. What is not a
    /// book ([`is_book_path`]) is never deleted: it is refused
    /// ([`Deleted::refused`]). Asking first is the view's.
    fn delete(&self, env: &Env<'_>, paths: &[&str]) -> Deleted {
        let mut deleted = Deleted::default();
        for path in paths {
            if !is_book_path(env, path) {
                deleted.refused.push(String::from(*path));
                continue;
            }
            match env.storage().remove(path) {
                Ok(()) => {
                    let removed = crate::builtin::events::BookRemoved::new(String::from(*path));
                    let command = crate::builtin::events::BOOK_REMOVED.emit(removed);
                    deleted.commands.push(command);
                }
                Err(error) => deleted.failed.push((String::from(*path), error)),
            }
        }
        deleted
    }

    /// A question asked after this release, by its key and parameters:
    /// what PaperPaw asks of libraries later reaches libraries built now,
    /// which answer `None` to keys they do not know.
    fn extended(&self, env: &Env<'_>, key: &Id, params: &crate::Params) -> Option<crate::Params> {
        let _ = (env, key, params);
        None
    }
}

/// Whether `path` may be a book of the library: of a format known, by
/// its extension or, without one, by its first bytes
/// ([`Formats::supports_file`](crate::Formats::supports_file)), not in
/// PaperPaw's own folder ([`DIR`](crate::data::DIR), its settings and
/// what it keeps) and not climbing out of where it is (`..`).
pub fn is_book_path(env: &Env<'_>, path: &str) -> bool {
    let mut parts = path.trim_start_matches('/').split('/');
    let own = parts
        .clone()
        .next()
        .is_some_and(|first| first.eq_ignore_ascii_case(crate::data::DIR));
    !own && !parts.any(|part| part == "..") && env.formats().supports_file(env.storage(), path)
}

/// What [`Library::delete`] did: the commands that tell everyone, the
/// books the storage could not delete with its error, and what was
/// refused as no book of the library ([`is_book_path`]). What failed or
/// was refused stays as it was.
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct Deleted {
    pub commands: Vec<Command>,
    pub failed: Vec<(String, StorageError)>,
    pub refused: Vec<String>,
}

/// The kernel's library.
#[derive(Debug, Default, Clone, Copy)]
pub struct Kept;

impl Library for Kept {
    // Its books are the catalog's: read once, not counted first.
    fn index(&self, env: &Env<'_>, cap: usize, archive: &str) -> Index {
        let opened = super::index::opened(env.storage());
        let version = self.version(env);
        Index::of_catalog(env.storage(), cap, archive, &opened, version)
    }
}

static KEPT: Kept = Kept;

/// A library of a plugin's own, provided as [`SERVICE`] in [`VERSION`]:
/// `r.provide(Id::new_static(SERVICE), LibraryService(Box::new(mine)), VERSION)`.
pub struct LibraryService(pub alloc::boxed::Box<dyn Library>);
crate::service_type!(LibraryService, "core.library/1");

impl core::fmt::Debug for LibraryService {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("LibraryService")
    }
}

/// The version of [`Library`] that [`LibraryService`] carries.
pub const VERSION: crate::Version = crate::Version::new(1, 0, 0);

/// The library to ask: one a plugin provided as [`SERVICE`], else the
/// kernel's.
pub fn service<'e>(env: &Env<'e>) -> &'e dyn Library {
    match env.services().get::<LibraryService>(SERVICE, VERSION) {
        Some(library) => library.0.as_ref(),
        None => &KEPT,
    }
}

/// A field plugins keep with books ([`super::set_field`]), e.g. a rating
/// or where a book came from: shown on its information page, and kept in
/// the catalog (to filter and sort by) where `in_catalog`. Added with
/// [`Registrar::add_book_field`](crate::Registrar::add_book_field).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct BookField {
    /// The field's id, e.g. `stars.rating`.
    pub id: Id,
    /// Translation key of its name.
    pub title: Cow<'static, str>,
    /// Kept in the catalog: short values only (a line a book).
    pub in_catalog: bool,
}

impl BookField {
    pub fn new(id: impl Into<Id>, title: impl Into<Cow<'static, str>>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            in_catalog: false,
        }
    }

    /// Kept in the catalog too.
    pub fn in_catalog(mut self) -> Self {
        self.in_catalog = true;
        self
    }
}

/// What a badge on a book's row shows.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Badge {
    /// A few letters ("4★", "Loaned").
    Text(alloc::string::String),
    /// An icon by its name.
    Icon(&'static str),
}

/// What a badge says of a book: asked for the books of a page when it is
/// read, never while drawing.
pub type BadgeFn = Box<dyn Fn(&Env<'_>, &Record) -> Option<Badge>>;

/// A badge plugins put on books' rows in views of the library, e.g. a
/// rating or "on loan"; views show two at most. Added with
/// [`Registrar::add_book_badge`](crate::Registrar::add_book_badge).
#[non_exhaustive]
pub struct BookBadge {
    pub id: Id,
    /// Translation key of its name (where views list them).
    pub title: Cow<'static, str>,
    pub badge: BadgeFn,
}

impl BookBadge {
    pub fn new(
        id: impl Into<Id>,
        title: impl Into<Cow<'static, str>>,
        badge: impl Fn(&Env<'_>, &Record) -> Option<Badge> + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            badge: Box::new(badge),
        }
    }
}

impl fmt::Debug for BookBadge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BookBadge")
            .field("id", &self.id)
            .field("title", &self.title)
            .finish_non_exhaustive()
    }
}

/// The badges of `record`, at most `n`, from every plugin.
pub fn badges(env: &Env<'_>, record: &Record, n: usize) -> Vec<Badge> {
    env.book_badges()
        .iter()
        .filter_map(|b| (b.badge)(env, record))
        .take(n)
        .collect()
}
