//! The library as a service: what is known about books beyond their
//! files, for every plugin that shows or uses them (the library, the home
//! screen, the sleep screen, syncs). Data and the screens that show it
//! are apart: any plugin may offer another view of the same books.
//!
//! - A book's details: title, author, series, tags and cover, read from
//!   the book once ([`index`](fn@index)), its reading progress, and fields that
//!   plugins add (e.g. a rating or where a book came from), in
//!   `.paperpaw/library/details/<h>/<hash>.ini`, `<h>` the first two
//!   digits of the hash; its cover made small beside, in
//!   `.paperpaw/library/thumbs/<h>/<hash>.thumb`. (Earlier versions kept
//!   them in `books/<hash>.ini` and `covers/<hash>.thumb`: read there
//!   until the kernel moved them, `split`.)
//! - The books opened last, most recent first (`opened.txt`), and the
//!   favourites (`favourites.txt`).
//! - The catalog of all books under the library folder ([`catalog`]),
//!   and listings of a folder's books ([`listing`]).
//!
//! The kernel keeps them up to date from the standard reading events and
//! moves them with a book ([`record`], [`moved`]). Nothing here grows
//! with the size of the library: the lists are capped, and details are
//! read a book at a time.

#![deny(clippy::let_underscore_must_use)]

mod carry;
pub mod catalog;
mod details;
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub mod emptiness;
mod fallible;
mod fingerprint;
pub mod index;
mod indexed;
pub mod listing;
mod metadata;
pub mod service;
pub mod sorted;
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub mod split;

pub use carry::{try_moved, try_removed};
pub use fallible::{KEY_LEN, key_into, try_book, try_book_sized, try_key, try_status};
#[cfg(any(test, feature = "kernel"))]
pub use fallible::{ProvidedText, try_provide, try_set_field, try_set_status};
pub use fingerprint::Fingerprint;
pub use indexed::try_indexed;
#[cfg(any(test, feature = "kernel"))]
pub use metadata::keep_native_details;
pub use service::{Badge, BookBadge, BookField, Deleted, Library, is_book_path, service};

/// What changed of a book ([`events::BookChanged::what`](crate::builtin::events::BookChanged)):
/// bits, any together.
pub mod what {
    /// Its title, author, series, tags (read from the book, or provided).
    pub const DETAILS: u32 = 1;
    /// How far it is read.
    pub const PROGRESS: u32 = 1 << 1;
    /// Its cover.
    pub const COVER: u32 = 1 << 2;
    /// Whether it is a favourite.
    pub const FAVOURITE: u32 = 1 << 3;
    /// Marked finished or unread.
    pub const STATUS: u32 = 1 << 4;
    /// A plugin's field.
    pub const FIELDS: u32 = 1 << 5;
}

use alloc::borrow::Cow;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::config;
use crate::document::ImageRef;
use crate::{Env, Storage, StorageError};
use paperpaw_digest::fnv1a64;

/// Where the library keeps what it knows.
pub const DIR: &str = crate::data_path!("library");

/// The library folder, where books are kept (e.g. `books`): the kernel's
/// setting [`settings::LIBRARY_FOLDER`](crate::builtin::settings::LIBRARY_FOLDER),
/// which every plugin that puts books somewhere reads ([`folder`]).
pub const FOLDER: crate::setting::Setting<crate::setting::Text> =
    crate::builtin::settings::LIBRARY_FOLDER;

/// The library folder ([`FOLDER`]) as a path in the storage, without
/// slashes around it (`""` for the storage's top).
pub fn folder<'e>(env: &Env<'e>) -> &'e str {
    env.settings().read(&FOLDER).trim_matches('/')
}
/// Books remembered for "recently read" order.
const MAX_OPENED: usize = 100;
/// Favourites kept at most.
const MAX_FAVOURITES: usize = 500;
/// Longest details file of a book read; [`save`] keeps them shorter.
const MAX_DETAILS: usize = 16 * 1024;
/// Longest value of a book's details kept, in bytes.
const MAX_VALUE: usize = 1024;
/// Tags of a book kept at most.
const MAX_TAGS: usize = 32;
/// What the details hold; books read with another version are read
/// again (2 added the series, 3 the tags, 4 covers as large as the
/// screen shows them).
const DETAILS: u32 = 4;

/// What the library knows of a book.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Book {
    /// File size when the details were read; another size means another
    /// file.
    pub size: u64,
    /// Title, author and cover were read from the book.
    pub indexed: bool,
    pub title: Option<String>,
    pub author: Option<String>,
    /// The series and the book's number in it, if it names one.
    pub series: Option<String>,
    pub series_index: Option<String>,
    /// Its tags (subjects), if it names any.
    pub tags: Vec<String>,
    pub cover: Option<ImageRef>,
    /// Reading progress in percent, once opened.
    pub progress: Option<u8>,
    /// The day the library first read the book ([`index`](fn@index)), in days since
    /// 1970 (UTC); none where the device did not know the date then.
    pub added: Option<u32>,
    /// Fields plugins keep with the book: id (e.g. `myplugin.rating`)
    /// and value.
    pub fields: Vec<(String, String)>,
}

impl Book {
    pub fn size(mut self, size: u64) -> Self {
        self.size = size;
        self
    }

    pub fn indexed(mut self, indexed: bool) -> Self {
        self.indexed = indexed;
        self
    }

    pub fn title(mut self, title: impl Into<Option<String>>) -> Self {
        self.title = title.into();
        self
    }

    pub fn author(mut self, author: impl Into<Option<String>>) -> Self {
        self.author = author.into();
        self
    }

    pub fn series(mut self, series: impl Into<Option<String>>) -> Self {
        self.series = series.into();
        self
    }

    pub fn series_index(mut self, series_index: impl Into<Option<String>>) -> Self {
        self.series_index = series_index.into();
        self
    }

    pub fn tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    pub fn cover(mut self, cover: impl Into<Option<ImageRef>>) -> Self {
        self.cover = cover.into();
        self
    }

    pub fn progress(mut self, progress: impl Into<Option<u8>>) -> Self {
        self.progress = progress.into();
        self
    }

    pub fn added(mut self, added: impl Into<Option<u32>>) -> Self {
        self.added = added.into();
        self
    }

    pub fn fields(mut self, fields: Vec<(String, String)>) -> Self {
        self.fields = fields;
        self
    }
}

impl Book {
    /// The value of field `id`, if set.
    pub fn field(&self, id: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(k, _)| k == id)
            .map(|(_, v)| v.as_str())
    }

    /// The title, else the file's name without its extension.
    pub fn title_or<'a>(&'a self, path: &'a str) -> &'a str {
        match &self.title {
            Some(title) if !title.is_empty() => title,
            _ => {
                let name = crate::files::name(path);
                name.rsplit_once('.').map_or(name, |(stem, _)| stem)
            }
        }
    }

    /// The title to show: the book's own, else its file's name made
    /// readable ([`readable_name`]), e.g. "Alice In Wonderland" for
    /// `alice-in-wonderland.txt`.
    pub fn shown_title<'a>(&'a self, path: &'a str) -> Cow<'a, str> {
        match &self.title {
            Some(title) if !title.is_empty() => Cow::Borrowed(title),
            _ => readable_name(self.title_or(path)),
        }
    }
}

/// A file's name as a title: a name written as one word joined by `-`,
/// `_` or `.` and in small letters only (`alice-in-wonderland`) gets
/// spaces and capitals (`Alice In Wonderland`); any other is kept as it
/// is, since its writer chose it.
pub fn readable_name(stem: &str) -> Cow<'_, str> {
    let joined = |c: char| matches!(c, '-' | '_' | '.');
    let slug = stem.contains(joined)
        && !stem.contains(' ')
        && !stem.chars().any(char::is_uppercase)
        && stem.chars().any(char::is_alphabetic);
    if !slug {
        return Cow::Borrowed(stem);
    }
    let mut out = String::with_capacity(stem.len());
    let mut start = true;
    for c in stem.chars() {
        if joined(c) {
            if !out.ends_with(' ') && !out.is_empty() {
                out.push(' ');
            }
            start = true;
        } else if start {
            out.extend(c.to_uppercase());
            start = false;
        } else {
            out.push(c);
        }
    }
    Cow::Owned(out.trim_end().into())
}

/// The key that names what is kept for the book at `path`: 16
/// hexadecimal digits of its hash. Folders of [`BookData`] name their
/// entries by it.
pub fn key(path: &str) -> String {
    let mut digits = [0_u8; KEY_LEN];
    String::from(key_into(path, &mut digits))
}

/// A folder where a plugin keeps something for each book, named by the
/// book's [`key`] after `prefix`, alone or followed by `.` and more (a
/// file or a folder): `cache/0123456789abcdef`, `epub-0123…/`,
/// `0123….ini`; in the folder itself, or in the part of the folder of
/// the key's first two digits (`01/0123…`), where a folder would hold
/// thousands (a memory card looks through a folder one entry after the
/// other for a name). Registered with
/// [`Registrar::add_book_data`](crate::Registrar::add_book_data) or
/// [`Registrar::add_book_cache`](crate::Registrar::add_book_cache), the
/// kernel moves it with its book ([`move_data`]), removes it with it
/// ([`remove_data`]) and when its book is no longer there
/// ([`clean_up_unused`]): a cache by itself, the reader's data only when
/// the reader says so.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct BookData {
    pub folder: alloc::borrow::Cow<'static, str>,
    pub prefix: alloc::borrow::Cow<'static, str>,
    pub kind: BookDataKind,
}

impl BookData {
    /// Data of `kind` in `folder`, named `prefix` and the book's id.
    pub fn new(
        folder: impl Into<alloc::borrow::Cow<'static, str>>,
        prefix: impl Into<alloc::borrow::Cow<'static, str>>,
        kind: BookDataKind,
    ) -> Self {
        Self {
            folder: folder.into(),
            prefix: prefix.into(),
            kind,
        }
    }
}

/// What a folder of [`BookData`] holds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum BookDataKind {
    /// The reader's: a place, notes, settings for the book, a sync's
    /// state. A book not found may be on a card not inserted, or renamed
    /// on a computer: its data goes only when the reader agrees.
    #[default]
    UserData,
    /// Made again from the book when needed (pages laid out, a cover made
    /// small): it goes by itself once its book is not found.
    Cache,
}

impl BookData {
    /// The book key entry `name` of the folder is named by, if it is one
    /// of these, and what follows it.
    pub fn key_of<'n>(&self, name: &'n str) -> Option<(&'n str, &'n str)> {
        let rest = name.strip_prefix(&*self.prefix)?;
        let key = rest.get(..16)?;
        let tail = &rest[16..];
        let hex = key.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
        (hex && (tail.is_empty() || tail.starts_with('.'))).then_some((key, tail))
    }

    /// Where what is kept for books is in the folder: in the folder
    /// itself, as earlier plugins keep it, or split by the first two
    /// digits of the key ([`split`]), each in the subfolder of its own:
    /// `0123…`, `01/0123…`. With `only`, of that key alone (looked for
    /// where it goes, not among all), else of every key. `found` is given
    /// each one's path in the folder, its key and whether it is a folder,
    /// once the folder holding it was read.
    fn each(&self, storage: &dyn Storage, only: Option<&str>, found: &mut dyn FnMut(Kept<'_>)) {
        let mut parts = alloc::vec![String::new()];
        if let Some(key) = only {
            parts.push(String::from(key.get(..2).unwrap_or("00")));
        }
        let mut at = 0;
        while let Some(part) = parts.get(at).cloned() {
            at += 1;
            let folder = match part.is_empty() {
                true => String::from(&*self.folder),
                false => format!("{}/{part}", self.folder),
            };
            let mut entries: Vec<(String, bool)> = Vec::new();
            #[expect(
                clippy::let_underscore_must_use,
                reason = "a folder that cannot be listed keeps nothing for books"
            )]
            let _ = storage.list(&folder, &mut |entry| {
                let key = self.key_of(entry.name).map(|(key, _)| key);
                if key.is_some_and(|key| only.is_none_or(|only| key == only)) {
                    entries.push((String::from(entry.name), entry.is_dir));
                } else if only.is_none() && part.is_empty() && entry.is_dir && is_split(entry.name)
                {
                    parts.push(String::from(entry.name));
                }
            });
            for (name, is_dir) in &entries {
                if let Some((key, tail)) = self.key_of(name) {
                    let path = crate::files::join(&part, name);
                    found(Kept {
                        path: &path,
                        key,
                        tail,
                        is_dir: *is_dir,
                        split: !part.is_empty(),
                    });
                }
            }
        }
    }
}

/// Whether `name` is a part of a folder of [`BookData`] split by keys
/// ([`split`]): two hexadecimal digits.
fn is_split(name: &str) -> bool {
    name.len() == 2 && name.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// What is kept for a book in a folder of [`BookData`] ([`BookData::each`]).
struct Kept<'a> {
    /// In the folder.
    path: &'a str,
    key: &'a str,
    /// What follows the key in its name.
    tail: &'a str,
    is_dir: bool,
    /// In the part of the folder of its key, not in the folder itself.
    split: bool,
}

/// Moves what is kept for the book `from` in folders `data` to `to`:
/// where it was, in the folder itself or in the part of its key
/// (`split`).
pub fn move_data(storage: &dyn Storage, data: &[BookData], from: &str, to: &str) {
    let (old, new) = (key(from), key(to));
    for folder in data {
        let mut moves = Vec::new();
        folder.each(storage, Some(&old), &mut |kept| {
            let named = format!("{}{new}{}", folder.prefix, kept.tail);
            let to = match kept.split {
                true => format!("{}/{named}", split(&folder.folder, &new)),
                false => crate::files::join(&folder.folder, &named),
            };
            moves.push((crate::files::join(&folder.folder, kept.path), to));
        });
        for (from, to) in moves {
            #[expect(
                clippy::let_underscore_must_use,
                reason = "what cannot be moved stays under the old name; the book itself is moved already"
            )]
            let _ = storage.rename(&from, &to);
        }
    }
}

/// Removes what is kept for the book at `path` in folders `data`.
pub fn remove_data(storage: &dyn Storage, data: &[BookData], path: &str) {
    let gone = key(path);
    clean(storage, data, Some(&gone), &|key| key != gone);
}

/// Removes what is kept in folders `data` for books whose key `keep` does
/// not keep; how many entries went.
pub fn clean_up(storage: &dyn Storage, data: &[BookData], keep: &dyn Fn(&str) -> bool) -> u32 {
    clean(storage, data, None, keep)
}

/// [`clean_up`], of key `only` alone where given.
fn clean(
    storage: &dyn Storage,
    data: &[BookData],
    only: Option<&str>,
    keep: &dyn Fn(&str) -> bool,
) -> u32 {
    let mut removed = 0;
    for folder in data {
        let mut gone = Vec::new();
        folder.each(storage, only, &mut |kept| {
            if !keep(kept.key) {
                gone.push((crate::files::join(&folder.folder, kept.path), kept.is_dir));
            }
        });
        for (path, is_dir) in gone {
            let done = if is_dir {
                storage.remove_dir_all(&path)
            } else {
                storage.remove(&path)
            };
            removed += u32::from(done.is_ok());
        }
    }
    removed
}

/// Removes what is kept in folders `data` of `kind` for files no longer
/// on the storage (e.g. books deleted on a computer); how many entries
/// went. What is kept for any file there stays, of any format, also of
/// formats whose plugin is switched off now, and by every spelling a
/// plugin may use for it: the storage's own and, where the storage
/// ignores case, with the library folder spelt as its setting has it
/// ([`folder`], `library`). Where a folder cannot be listed, nothing is
/// removed. The reader's data ([`BookDataKind::UserData`]) only when the
/// reader agreed: a book renamed on a computer is not found either
/// ([`unused_books`] says how many lose theirs).
pub fn clean_up_unused(
    storage: &dyn Storage,
    data: &[BookData],
    library: &str,
    kind: BookDataKind,
) -> Result<u32, StorageError> {
    let keys = file_keys(storage, library)?;
    let keep =
        |key: &str| u64::from_str_radix(key, 16).is_ok_and(|k| keys.binary_search(&k).is_ok());
    let data: Vec<BookData> = data.iter().filter(|d| d.kind == kind).cloned().collect();
    Ok(clean(storage, &data, None, &keep))
}

/// How many books [`clean_up_unused`] would remove data of `kind` for:
/// files not found, each counted once.
pub fn unused_books(
    storage: &dyn Storage,
    data: &[BookData],
    library: &str,
    kind: BookDataKind,
) -> Result<u32, StorageError> {
    let keys = file_keys(storage, library)?;
    let mut gone: Vec<u64> = Vec::new();
    let mut full = false;
    for folder in data.iter().filter(|d| d.kind == kind) {
        folder.each(storage, None, &mut |kept| {
            let Ok(key) = u64::from_str_radix(kept.key, 16) else {
                return;
            };
            if keys.binary_search(&key).is_err() {
                full |= gone.try_reserve(1).is_err();
                if !full {
                    gone.push(key);
                }
            }
        });
    }
    if full {
        return Err(StorageError::TooLarge);
    }
    gone.sort_unstable();
    gone.dedup();
    Ok(u32::try_from(gone.len()).unwrap_or(u32::MAX))
}

/// The keys ([`key`]) of every file on the storage (not in PaperPaw's
/// own folder), by every spelling of [`clean_up_unused`], sorted.
fn file_keys(storage: &dyn Storage, library: &str) -> Result<Vec<u64>, StorageError> {
    let library = library.trim_matches('/');
    let depth = library.split('/').count();
    let mut keys: Vec<u64> = Vec::new();
    let mut add = |key: u64| -> Result<(), StorageError> {
        // A card full of files is not worth the device: told, not taken.
        keys.try_reserve(1).map_err(|_| StorageError::OutOfMemory)?;
        keys.push(key);
        Ok(())
    };
    let mut folders = alloc::vec![String::new()];
    while let Some(folder) = folders.pop() {
        let mut files = Vec::new();
        storage.list(&folder, &mut |entry| {
            // Hidden folders hold books too; PaperPaw's own none.
            let own = folder.is_empty() && entry.name.eq_ignore_ascii_case(crate::data::DIR);
            if !own {
                let path = crate::files::join(&folder, entry.name);
                if entry.is_dir {
                    folders.push(path);
                } else {
                    files.push(path);
                }
            }
        })?;
        for path in files {
            add(fnv1a64(&path))?;
            if !library.is_empty() && crate::files::is_within(storage, &path, library) {
                let rest = path.splitn(depth + 1, '/').nth(depth).unwrap_or_default();
                let spelt = crate::files::join(library, rest);
                if spelt != path {
                    add(fnv1a64(&spelt))?;
                }
            }
        }
    }
    keys.sort_unstable();
    keys.dedup();
    Ok(keys)
}

/// The keys ([`key`]) of every book on the storage (every file a format
/// plugin opens, in any folder but PaperPaw's own), sorted. Folders that
/// cannot be listed are left out, and books of formats switched off:
/// to clean up, [`clean_up_unused`] keeps more.
pub fn book_keys(storage: &dyn Storage, formats: &crate::Formats) -> Vec<u64> {
    let mut keys = Vec::new();
    books_in(storage, formats, "", &mut |path| keys.push(fnv1a64(path)));
    keys.sort_unstable();
    keys
}

/// Every book in `folder` and the folders in it (every file a format
/// plugin opens; not in hidden folders), by its path; the book itself if
/// `folder` is one. E.g. to say they are gone before a folder is deleted.
pub fn books_in(
    storage: &dyn Storage,
    formats: &crate::Formats,
    folder: &str,
    found: &mut dyn FnMut(&str),
) {
    let folder = folder.trim_matches('/');
    if !folder.is_empty() && formats.supports_file(storage, folder) && storage.size(folder).is_ok()
    {
        return found(folder);
    }
    let mut folders = alloc::vec![String::from(folder)];
    while let Some(folder) = folders.pop() {
        // Files without an extension, told by their first bytes once the
        // folder is listed.
        let mut unknown: Vec<String> = Vec::new();
        #[expect(
            clippy::let_underscore_must_use,
            reason = "a folder that cannot be listed is left out of the walk"
        )]
        let _ = storage.list(&folder, &mut |entry| {
            if entry.name.starts_with('.') {
                return;
            }
            let path = if folder.is_empty() {
                String::from(entry.name)
            } else {
                format!("{folder}/{}", entry.name)
            };
            if entry.is_dir {
                folders.push(path);
            } else if formats.supports(entry.name) {
                found(&path);
            } else if formats.may_sniff(entry.name) {
                unknown.push(path);
            }
        });
        for path in unknown {
            if formats.for_file(storage, &path).is_some() {
                found(&path);
            }
        }
    }
}

/// The folder of the books' details, in [`DIR`]: split by the first two
/// digits of their keys ([`split`]).
pub(crate) const DETAILS_DIR: &str = "details";

/// The folder of the books' covers made small, in [`DIR`], split as
/// [`DETAILS_DIR`].
pub(crate) const THUMBS_DIR: &str = "thumbs";

/// Where earlier versions kept the details and covers made small, in
/// [`DIR`], each in one folder: read until moved ([`split::Split`]).
pub(crate) const OLD_DETAILS_DIR: &str = "books";
pub(crate) const OLD_THUMBS_DIR: &str = "covers";

/// The part of folder `folder` that what is kept by `key` goes into:
/// the subfolder of its first two digits (of 256), so that no folder holds
/// thousands of entries, which a memory card looks through one after the
/// other for a name.
pub(crate) fn split(folder: &str, key: &str) -> String {
    format!("{folder}/{}", key.get(..2).unwrap_or("00"))
}

/// Where a plugin keeps what it keeps for the book at `book` in folder
/// `folder` of [`BookData`] (registered with
/// [`Registrar::add_book_cache`](crate::Registrar::add_book_cache) or
/// [`add_book_data`](crate::Registrar::add_book_data)), named `prefix` and
/// the book's [`key`]: in the part of the folder of the key's first two
/// digits, `<folder>/<2 digits>/<prefix><key>`, so that no folder holds
/// thousands of entries (a memory card looks through a folder one entry
/// after the other for a name). The kernel moves, removes and cleans up
/// what is there, and what earlier plugins kept in the folder itself.
#[must_use]
pub fn kept_path(folder: &str, prefix: &str, book: &str) -> String {
    let key = key(book);
    format!(
        "{}/{prefix}{key}",
        split(folder.trim_end_matches('/'), &key)
    )
}

fn book_file(path: &str) -> String {
    let key = key(path);
    format!("{}/{key}.ini", split(&format!("{DIR}/{DETAILS_DIR}"), &key))
}

/// Where earlier versions kept [`book_file`].
fn old_book_file(path: &str) -> String {
    format!("{DIR}/{OLD_DETAILS_DIR}/{}.ini", key(path))
}

fn cover_file(path: &str) -> String {
    cover_variant_file(path, "")
}

fn cover_variant_file(path: &str, variant: &str) -> String {
    let key = key(path);
    let dot = if variant.is_empty() { "" } else { "." };
    format!(
        "{}/{key}{dot}{variant}.thumb",
        split(&format!("{DIR}/{THUMBS_DIR}"), &key)
    )
}

/// Where earlier versions kept [`cover_variant_file`].
fn old_cover_variant_file(path: &str, variant: &str) -> String {
    let dot = if variant.is_empty() { "" } else { "." };
    format!("{DIR}/{OLD_THUMBS_DIR}/{}{dot}{variant}.thumb", key(path))
}

/// The details of the book at `path` as kept: where they are now, else
/// where an earlier version kept them and they were not moved yet.
fn load_details(storage: &dyn Storage, path: &str) -> Result<Option<String>, crate::data::Error> {
    fallible::try_load_details(storage, path)
}

/// The cover of `book` (the one at `path`) made small to fit `width` ×
/// `height`, as a panel of `format` shows it: kept in the library
/// (`covers/<hash>.thumb`), so views of many books (a list, a grid) read a
/// few kilobytes for each instead of its picture. Made anew where the
/// book or the size asked changed.
pub fn cover(
    storage: &dyn Storage,
    path: &str,
    book: &Book,
    width: u16,
    height: u16,
    format: paperpaw_platform::PixelFormat,
) -> Option<crate::ui::Thumbnail> {
    cover_variant(storage, path, book, width, height, format, "")
}

/// [`cover`] kept apart for each `variant` (e.g. `row`, `card`, `grid`):
/// a view that shows a book's cover at several sizes keeps each, instead
/// of one size made anew over the other. Moved and removed with the book.
pub fn cover_variant(
    storage: &dyn Storage,
    path: &str,
    book: &Book,
    width: u16,
    height: u16,
    format: paperpaw_platform::PixelFormat,
    variant: &str,
) -> Option<crate::ui::Thumbnail> {
    let image = book.cover.as_ref()?;
    let (w, h) = crate::ui::fit_image(image, width, height);
    let stamp = fnv1a64(format!(
        "{} {} {}x{} {w}x{h} {format:?}",
        book.size, image.path, image.width, image.height
    ));
    let file = cover_variant_file(path, variant);
    let old = || old_cover_variant_file(path, variant);
    if let Some(thumb) = crate::ui::Thumbnail::kept(storage, &file, stamp, (w, h))
        .or_else(|| crate::ui::Thumbnail::kept(storage, &old(), stamp, (w, h)))
    {
        return Some(thumb);
    }
    let thumb = crate::ui::Thumbnail::new(storage, image, w, h, format)?;
    thumb.keep(storage, &file, stamp);
    Some(thumb)
}

pub(crate) fn opened_file() -> String {
    format!("{DIR}/opened.txt")
}

pub(crate) fn favourites_file() -> String {
    format!("{DIR}/favourites.txt")
}

/// The lines of a list file of at most `max` paths; none where there is
/// no such file, an error where it could not be read or a newer PaperPaw
/// wrote it ([`data::Format`](crate::data::Format)), so it is not written
/// over.
fn read_list(storage: &dyn Storage, path: &str, max: usize) -> Result<Vec<String>, StorageError> {
    let text = crate::data::Format::FIRST.load(storage, path, max * 512)?;
    Ok(text
        .as_deref()
        .unwrap_or_default()
        .lines()
        .filter(|line| !line.is_empty())
        .map(ToString::to_string)
        .collect())
}

/// Changes the list file at `path` of at most `max` paths, if it could
/// be read.
fn update_list(
    storage: &dyn Storage,
    path: &str,
    max: usize,
    change: impl FnOnce(&mut Vec<String>),
) {
    if let Ok(mut list) = read_list(storage, path, max) {
        change(&mut list);
        list.truncate(max);
        let mut text = list.join("\n");
        // Read without a byte order mark: a first path that begins with
        // one (U+FEFF) keeps it behind a mark of the file's own.
        if text.starts_with('\u{feff}') {
            text.insert(0, '\u{feff}');
        }
        #[expect(
            clippy::let_underscore_must_use,
            reason = "a list of books not written stays as it was: only this change is lost"
        )]
        let _ = storage.write(path, text.as_bytes());
    }
}

/// What the library knows of the book at `path`, if it belongs to this
/// file (not to another one of that name before).
pub fn book(storage: &dyn Storage, path: &str) -> Option<Book> {
    fallible::surviving(try_book(storage, path)).flatten()
}

/// [`book`] for a file whose size is known, e.g. from a listing
/// ([`listing::Entry::size`]): the file is not looked up again (on a card,
/// a search through its folder).
pub fn book_sized(storage: &dyn Storage, path: &str, size: u64) -> Option<Book> {
    fallible::surviving(try_book_sized(storage, path, size)).flatten()
}

/// What the library knows of the book at `path` to change it and
/// [`save`] it, or a new entry where it knows nothing of this file (of
/// another file there before: the day it came and the plugins' fields
/// stay, which it cannot read again); an error where its details are
/// there but could not be read, or the book's size, so they are not
/// written over.
fn book_for_update(storage: &dyn Storage, path: &str) -> Result<Book, StorageError> {
    let size = storage.size(path)?;
    let known = load_details(storage, path)?
        .map(|source| fallible::try_parse_details(&source))
        .transpose()
        .map_err(|_| StorageError::OutOfMemory)?;
    Ok(match known {
        Some(book) if book.size == size => book,
        Some(old) => Book {
            size,
            added: old.added,
            fields: old.fields,
            ..Book::default()
        },
        None => Book {
            size,
            ..Book::default()
        },
    })
}

/// Whether `key` of `section` in a book's details is one [`save`]
/// writes, from the book: others are kept as they are.
fn known_detail(section: &str, key: &str) -> bool {
    match section {
        "field" => true,
        "book" => {
            key.starts_with("tag.")
                || [
                    "size",
                    "indexed",
                    "version",
                    "title",
                    "author",
                    "series",
                    "series-index",
                    "cover",
                    "cover-width",
                    "cover-height",
                    "progress",
                    "added",
                ]
                .contains(&key)
        }
        _ => false,
    }
}

/// `value` in at most [`MAX_VALUE`] bytes.
fn fit(value: &str) -> &str {
    let mut end = value.len().min(MAX_VALUE);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

/// Keeps `book` as what the library knows of the book at `path`: values
/// cut at a kilobyte, the first 32 tags, and what a newer version wrote
/// there that this one does not know; an error where it could not be
/// written (or would be too large to be read back,
/// [`StorageError::TooLarge`]), or the details there could not be read.
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub fn save(storage: &dyn Storage, path: &str, book: &Book) -> Result<(), crate::data::Error> {
    write_details(storage, path, book)
}

/// [`save`], for the library's own changes of a book: `false` where it
/// was not kept.
fn write_book(storage: &dyn Storage, path: &str, book: &Book) -> bool {
    write_details(storage, path, book).is_ok()
}

fn write_details(storage: &dyn Storage, path: &str, book: &Book) -> Result<(), crate::data::Error> {
    let before = load_details(storage, path)?;
    // Made without an infallible allocation, and not at all where it
    // would be over what is read back.
    let ini = details::try_details_text(book, before.as_deref().unwrap_or_default())?;
    let file = fallible::try_book_file(path).map_err(|_| crate::data::Error::OutOfMemory)?;
    Ok(storage.write(&file, ini.as_bytes())?)
}

/// Reads title, author, series, tags and cover from the book itself and
/// keeps them. Opening a book the first time may take a moment (e.g. an
/// EPUB's package is read), so views do this for one book at a time,
/// e.g. for the rows they show.
pub fn index(env: &Env<'_>, path: &str) -> Book {
    let storage = env.storage();
    // Details that could not be read are not written over.
    let (mut book, known) = match book_for_update(storage, path) {
        Ok(book) => (book, true),
        Err(_) => (Book::default(), false),
    };
    book.size = storage.size(path).unwrap_or_default();
    book.indexed = true;
    if book.added.is_none() {
        book.added = today(env);
    }
    match env.formats().open(storage, path) {
        Ok(document) => {
            // What the book names; else what its source said ([`provide`]).
            // Copied fallibly; with no memory the book is not written down
            // (it is read from the book again at the next walk).
            if fallible::try_merge_metadata(&mut book, document.metadata()).is_err() {
                // Not indexed: the book does not hold what it names, so a
                // walk that finds its line must make it again.
                book.indexed = false;
                return book;
            }
            book.cover = document.cover(storage).ok().flatten();
        }
        // A native format's book (the kernel's stand-in): nothing was read,
        // so nothing is written over or marked as indexed; the host's
        // details job reads it (`keep_native_details`).
        Err(crate::document::DocumentError::NotReady) => {
            book.indexed = false;
            return book;
        }
        Err(_) => {}
    }
    if known {
        write_book(storage, path, &book);
    }
    book
}

/// Today in days since 1970 (UTC), as [`Book::added`] counts, if the
/// device knows the date.
pub fn today(env: &Env<'_>) -> Option<u32> {
    crate::time::unix_seconds(env).and_then(|s| u32::try_from(s.div_euclid(86_400)).ok())
}

/// Books added this many days ago or less, and never opened, are new
/// ([`is_new`]).
pub const NEW_DAYS: u32 = 30;

/// Whether `book` counts as new: added in the last [`NEW_DAYS`] days and
/// never opened (no progress); `opened` says whether it was opened.
pub fn is_new(env: &Env<'_>, book: &Book, opened: bool) -> bool {
    is_new_on(book, opened, today(env))
}

/// [`is_new`] on the day `today` (none where the date is not known).
pub fn is_new_on(book: &Book, opened: bool, today: Option<u32>) -> bool {
    match (book.added, today) {
        (Some(added), Some(today)) => {
            !opened && book.progress.is_none() && today.saturating_sub(added) <= NEW_DAYS
        }
        _ => false,
    }
}

/// What the library knows of the book at `path`, read from the book
/// first if it is not yet ([`index`](fn@index)).
pub fn indexed(env: &Env<'_>, path: &str) -> Book {
    match book(env.storage(), path) {
        Some(book) if book.indexed => book,
        _ => index(env, path),
    }
}

/// What a source knows of a book it brought, e.g. a catalog or Calibre
/// ([`provide`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Provided {
    pub title: Option<String>,
    pub author: Option<String>,
    pub series: Option<String>,
    pub series_index: Option<String>,
    pub tags: Vec<String>,
}

impl Provided {
    pub fn title(mut self, title: impl Into<Option<String>>) -> Self {
        self.title = title.into();
        self
    }

    pub fn author(mut self, author: impl Into<Option<String>>) -> Self {
        self.author = author.into();
        self
    }

    pub fn series(mut self, series: impl Into<Option<String>>) -> Self {
        self.series = series.into();
        self
    }

    pub fn series_index(mut self, series_index: impl Into<Option<String>>) -> Self {
        self.series_index = series_index.into();
        self
    }

    pub fn tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }
}

/// Keeps what a source knows of the book at `path`, for books that name
/// nothing themselves (text files, PDFs): what the book names wins when
/// it is read ([`index`](fn@index)).
pub fn provide(storage: &dyn Storage, path: &str, provided: Provided) {
    let Ok(mut book) = book_for_update(storage, path) else {
        return;
    };
    let some = |s: Option<String>| s.filter(|s| !s.trim().is_empty());
    book.title = some(provided.title).or(book.title);
    book.author = some(provided.author).or(book.author);
    book.series = some(provided.series).or(book.series);
    book.series_index = some(provided.series_index).or(book.series_index);
    if !provided.tags.is_empty() {
        book.tags = provided.tags;
    }
    // Read from the book anew, keeping this where it names nothing.
    book.indexed = false;
    write_book(storage, path, &book);
}

/// Sets field `id` of the book at `path` (a plugin's, e.g.
/// `myplugin.rating`); an empty value removes it.
pub fn set_field(storage: &dyn Storage, path: &str, id: &str, value: &str) {
    // Not kept where there is no memory for it or the card fails: as the
    // field was before.
    #[expect(
        clippy::let_underscore_must_use,
        reason = "a field not kept is as it was; the host's form tells why"
    )]
    let _ = fallible::try_set_field(storage, path, id, value);
}

/// Records a reading event (`core.book-opened`, `core.page-turned`): the
/// book's progress, its title until it is read from the book, and for an
/// opened book its place in the "recently read" order.
pub fn record(env: &Env<'_>, reading: &crate::builtin::events::Reading, opened: bool) {
    let path = reading.path.as_str();
    let storage = env.storage();
    if opened {
        update_list(storage, &opened_file(), MAX_OPENED, |list| {
            list.retain(|p| p != path);
            list.insert(0, path.to_string());
        });
    }
    let progress = Some((reading.progress + 0.5).clamp(0.0, 100.0) as u8);
    let Ok(mut book) = book_for_update(storage, path) else {
        return;
    };
    let mut changed = false;
    let title = reading.title.as_str();
    if !title.is_empty() && !book.indexed && book.title.as_deref() != Some(title) {
        book.title = Some(title.to_string());
        changed = true;
    }
    // Written only when the shown percentage changes.
    if progress.is_some() && book.progress != progress {
        book.progress = progress;
        changed = true;
    }
    if changed {
        write_book(storage, path, &book);
    }
    // Opened: the catalog knows it at once (a page turned does not write
    // there; the book's closing does, [`note_status`]).
    if opened {
        noted_status(storage, path);
    }
}

/// Notes the status of the book at `path` (how far it is read, whether it
/// is a favourite, whether it was opened) beside the catalog
/// (the catalog's log), so views of the library show it without reading
/// each book's details. The kernel calls it when a book is opened,
/// closed or finished; [`set_finished`] and [`set_favourite`] call it.
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub fn note_status(storage: &dyn Storage, path: &str) {
    noted_status(storage, path);
}

pub(crate) fn noted_status(storage: &dyn Storage, path: &str) {
    // What could not be read is not noted: a status of nothing would
    // stand for the book until the catalog is made anew.
    if let Ok(status) = status_of(storage, path) {
        #[expect(
            clippy::let_underscore_must_use,
            reason = "the catalog only keeps what the books' files say: a status not noted is read again when it is made anew"
        )]
        let _ = catalog::noted(storage, path, status);
    }
}

/// The status of the book at `path` as its details and the lists say;
/// an error where one of them cannot be read, or the book is gone.
fn status_of(storage: &dyn Storage, path: &str) -> Result<catalog::Status, StorageError> {
    Ok(try_status(storage, path)?)
}

/// Moves what the library knows of a book from `from` to `to`: its
/// details, its place among the recently opened and the favourites.
#[expect(
    clippy::let_underscore_must_use,
    reason = "details and a cover not moved are made again from the book"
)]
pub fn moved(storage: &dyn Storage, from: &str, to: &str) {
    // From where an earlier version kept them, where not moved yet; what
    // is left there would be moved back to `from` later.
    for (file, old) in [
        (
            book_file as fn(&str) -> String,
            old_book_file as fn(&str) -> String,
        ),
        (cover_file, |path| old_cover_variant_file(path, "")),
    ] {
        let moved = storage.rename(&file(from), &file(to)).is_ok()
            || storage.rename(&old(from), &file(to)).is_ok();
        // Not where neither could be moved (a full card, say) and nothing
        // is at `to`: the only copy, read there still.
        if moved || storage.exists(&file(to)) {
            let _ = storage.remove(&old(from));
        }
    }
    // A cover kept by a format's data for the book moves with it too
    // ([`BookData`]): the details name it where it is now.
    if let Some(mut book) = book(storage, to)
        && let Some(cover) = book.cover.as_mut()
        && cover.path.contains(&key(from))
    {
        cover.path = cover.path.replace(&key(from), &key(to));
        write_book(storage, to, &book);
    }
    let replace = |list: &mut Vec<String>| {
        for p in list.iter_mut().filter(|p| *p == from) {
            *p = to.to_string();
        }
    };
    update_list(storage, &opened_file(), MAX_OPENED, replace);
    update_list(storage, &favourites_file(), MAX_FAVOURITES, replace);
}

/// Tells the plugins that what was at `from` (a file or a folder) is at
/// `to` now, moved or renamed on the storage, so that the data they keep
/// by a book's path (where it is read, notes, statistics) goes along
/// ([`BOOK_MOVED`](crate::builtin::events::BOOK_MOVED)): for a folder, of
/// each book in it (each file a format opens), up to `most`; for a file,
/// whatever its name now (`a.epub.bak` may be named back). The reading
/// apps chosen for the folder go along, and settings that hold the folder
/// moved, or one in it (the library folder, say), follow it. Whether
/// there were more books than `most`, whose data stays where it was.
pub fn tell_moved(
    env: &Env<'_>,
    from: &str,
    to: &str,
    most: usize,
    out: &mut dyn FnMut(crate::Command),
) -> bool {
    use crate::builtin::events;
    follow_folders(env, from, to, out);
    #[expect(
        clippy::let_underscore_must_use,
        reason = "a reading app chosen for a folder is a convenience: chosen again where it did not move"
    )]
    let _ = crate::readers::folder_moved(env.storage(), from, to);
    let (from, to) = (from.trim_matches('/'), to.trim_matches('/'));
    if !crate::files::is_dir(env.storage(), to) {
        out(events::BOOK_MOVED.emit(events::BookMoved::new(from.into(), to.into())));
        return false;
    }
    let mut told = 0;
    let mut more = false;
    books_in(env.storage(), env.formats(), to, &mut |book| {
        let Some(rest) = book.strip_prefix(to) else {
            return;
        };
        if told == most {
            more = true;
            return;
        }
        told += 1;
        out(events::BOOK_MOVED.emit(events::BookMoved::new(
            format!("{from}{rest}"),
            book.to_string(),
        )));
    });
    more
}

/// Folder `value` (a setting's), after folder `from` moved to `to`: where
/// it is `from` or in it, as the storage names entries (`Books` is `books`
/// on the memory card), the same folder at `to`.
pub fn moved_folder(storage: &dyn Storage, value: &str, from: &str, to: &str) -> Option<String> {
    let (value, from, to) = (
        value.trim_matches('/'),
        from.trim_matches('/'),
        to.trim_matches('/'),
    );
    if from.is_empty() || !crate::files::is_within(storage, value, from) {
        return None;
    }
    let rest = value.split('/').skip(from.split('/').count());
    Some(
        core::iter::once(to)
            .chain(rest)
            .collect::<Vec<_>>()
            .join("/"),
    )
}

/// Settings that name folder `from` or one in it, chosen as folders (with
/// the folder chooser or the folder editor), name it at `to` now.
fn follow_folders(env: &Env<'_>, from: &str, to: &str, out: &mut dyn FnMut(crate::Command)) {
    use crate::builtin::{screens, slots};
    for spec in env.settings().specs() {
        let folder = spec
            .extra()
            .editor
            .as_ref()
            .is_some_and(|editor| match editor {
                crate::Editor::Slot(slot) => *slot == slots::CHOOSE_FOLDER,
                crate::Editor::Screen(screen) => *screen == screens::FOLDER_EDITOR,
            });
        if !folder {
            continue;
        }
        let value = env.settings().text(spec.id.as_str());
        if let Some(moved) = moved_folder(env.storage(), value, from, to) {
            out(crate::Command::SetSetting(
                spec.id.clone(),
                crate::SettingValue::Text(moved.into()),
            ));
        }
    }
}

/// Forgets the book at `path`, deleted: its details, its cover made
/// small, its place among the recently opened and the favourites.
#[expect(
    clippy::let_underscore_must_use,
    reason = "details or a cover left behind belong to no book and are never shown"
)]
pub fn removed(storage: &dyn Storage, path: &str) {
    let _ = storage.remove(&book_file(path));
    let _ = storage.remove(&cover_file(path));
    let _ = storage.remove(&old_book_file(path));
    let _ = storage.remove(&old_cover_variant_file(path, ""));
    let without = |list: &mut Vec<String>| list.retain(|p| p != path);
    update_list(storage, &opened_file(), MAX_OPENED, without);
    update_list(storage, &favourites_file(), MAX_FAVOURITES, without);
}

/// Marks the book at `path` as read to the end, or as not read at all.
pub fn set_finished(storage: &dyn Storage, path: &str, finished: bool) {
    let Ok(mut book) = book_for_update(storage, path) else {
        return;
    };
    book.progress = finished.then_some(100);
    write_book(storage, path, &book);
    noted_status(storage, path);
}

/// Paths of the books opened last, most recent first (also of books no
/// longer there).
pub fn opened_books(storage: &dyn Storage) -> Vec<String> {
    read_opened(storage).unwrap_or_default()
}

/// [`opened_books`], or the error reading them.
pub(crate) fn read_opened(storage: &dyn Storage) -> Result<Vec<String>, StorageError> {
    read_list(storage, &opened_file(), MAX_OPENED)
}

/// [`favourites`], or the error reading them.
pub(crate) fn read_favourites(storage: &dyn Storage) -> Result<Vec<String>, StorageError> {
    read_list(storage, &favourites_file(), MAX_FAVOURITES)
}

/// The paths of the favourite books, the newest first.
pub fn favourites(storage: &dyn Storage) -> Vec<String> {
    read_favourites(storage).unwrap_or_default()
}

/// Makes the book at `path` a favourite, or no longer one.
pub fn set_favourite(storage: &dyn Storage, path: &str, favourite: bool) {
    update_list(storage, &favourites_file(), MAX_FAVOURITES, |list| {
        list.retain(|p| p != path);
        if favourite {
            list.insert(0, path.to_string());
        }
    });
    noted_status(storage, path);
}

/// Takes over what the library plugin kept before the library was a
/// service (`.paperpaw/plugins/files`), once.
#[expect(
    clippy::let_underscore_must_use,
    reason = "old files left where they were are taken over the next time"
)]
pub fn take_over_old(storage: &dyn Storage) {
    if storage.exists(&opened_file()) || storage.exists(&favourites_file()) {
        return;
    }
    let old = crate::Env::plugin_dir("files");
    for name in ["opened.txt", "favourites.txt"] {
        let _ = storage.rename(&format!("{old}/{name}"), &format!("{DIR}/{name}"));
    }
    let _ = storage.rename(&format!("{old}/books"), &format!("{DIR}/{OLD_DETAILS_DIR}"));
    // The catalog is made anew where it is now.
    let _ = storage.remove(&format!("{old}/catalog.txt"));
}

/// The command that tells everyone books came into the library or left
/// it ([`events::LIBRARY_CHANGED`](crate::builtin::events::LIBRARY_CHANGED)).
pub fn changed() -> crate::Command {
    crate::builtin::events::LIBRARY_CHANGED.emit(())
}

/// The command that tells everyone something of the book at `path`
/// changed: `what`, bits of [`what`]
/// ([`events::BOOK_CHANGED`](crate::builtin::events::BOOK_CHANGED)).
pub fn changed_book(path: &str, what: u32) -> crate::Command {
    crate::builtin::events::BOOK_CHANGED.emit(crate::builtin::events::BookChanged {
        path: path.to_string(),
        what,
    })
}

/// The command that tells everyone the book at `path` came into the
/// library, brought by the plugin `source`
/// ([`events::BOOK_ADDED`](crate::builtin::events::BOOK_ADDED)).
pub fn added(path: &str, source: &str) -> crate::Command {
    crate::builtin::events::BOOK_ADDED.emit(crate::builtin::events::BookAdded {
        path: path.to_string(),
        source: source.to_string(),
    })
}

/// The command a view of the library sends as it shows: the kernel makes
/// the catalog anew in the background where it was not made since
/// PaperPaw started or woke (books may have been copied meanwhile) or is not
/// there ([`events::CATALOG_WANTED`](crate::builtin::events::CATALOG_WANTED));
/// [`events::CATALOG_CHANGED`](crate::builtin::events::CATALOG_CHANGED)
/// follows where it was.
pub fn request_catalog() -> crate::Command {
    let wanted = crate::builtin::events::CatalogWanted::new(false);
    crate::builtin::events::CATALOG_WANTED.emit(wanted)
}

/// The command that has the kernel make the catalog anew in any case,
/// e.g. where a book it lists is gone without a word.
pub fn request_catalog_anew() -> crate::Command {
    let wanted = crate::builtin::events::CatalogWanted::new(true);
    crate::builtin::events::CATALOG_WANTED.emit(wanted)
}

#[cfg(test)]
mod fallible_tests;

#[cfg(test)]
#[allow(
    clippy::let_underscore_must_use,
    reason = "tests set up files in memory, which does not fail"
)]
mod tests;
