//! The library as a service: what is known about books beyond their
//! files, for every plugin that shows or uses them (the library, the home
//! screen, the sleep screen, syncs). Data and the screens that show it
//! are apart: any plugin may offer another view of the same books.
//!
//! - A book's details: title, author, series, tags and cover, read from
//!   the book once (`index`), its reading progress, and fields that
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
pub(crate) mod listing;
mod metadata;
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

/// What changed of a book ([`events::BookChanged::what`](crate::builtin::events::BookChanged)):
/// bits, any together.
pub mod what {
    /// How far it is read.
    pub const PROGRESS: u32 = 1 << 1;
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
    /// The day the library first read the book (`index`), in days since
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

    pub fn title(mut self, title: impl Into<Option<String>>) -> Self {
        self.title = title.into();
        self
    }

    pub fn author(mut self, author: impl Into<Option<String>>) -> Self {
        self.author = author.into();
        self
    }

    pub fn tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }
}

impl Book {
    /// The value of field `id`, if set.
    #[cfg(test)]
    pub fn field(&self, id: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(k, _)| k == id)
            .map(|(_, v)| v.as_str())
    }

    /// The title, else the file's name without its extension.
    pub(crate) fn title_or<'a>(&'a self, path: &'a str) -> &'a str {
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
    pub(crate) fn shown_title<'a>(&'a self, path: &'a str) -> Cow<'a, str> {
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
pub(crate) fn readable_name(stem: &str) -> Cow<'_, str> {
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
/// `Items::add_book_data` or
/// `Items::add_book_cache`, the
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
                if let Some((key, _)) = self.key_of(name) {
                    let path = crate::files::join(&part, name);
                    found(Kept {
                        path: &path,
                        key,
                        is_dir: *is_dir,
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
    is_dir: bool,
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
    paperpaw_sort::sort_unstable(&mut gone);
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
    paperpaw_sort::sort_unstable(&mut keys);
    keys.dedup();
    Ok(keys)
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
/// `Items::add_book_cache` or
/// `Items::add_book_data`), named `prefix` and
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
pub(crate) fn book_sized(storage: &dyn Storage, path: &str, size: u64) -> Option<Book> {
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

/// Today in days since 1970 (UTC), as [`Book::added`] counts, if the
/// device knows the date.
pub(crate) fn today(env: &Env<'_>) -> Option<u32> {
    crate::time::unix_seconds(env).and_then(|s| u32::try_from(s.div_euclid(86_400)).ok())
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

    pub fn tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }
}

/// Keeps what a source knows of the book at `path`, for books that name
/// nothing themselves (text files, PDFs): what the book names wins when
/// it is read (`index`).
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

/// The command that tells everyone something of the book at `path`
/// changed: `what`, bits of [`what`]
/// ([`events::BOOK_CHANGED`](crate::builtin::events::BOOK_CHANGED)).
pub fn changed_book(path: &str, what: u32) -> crate::Command {
    crate::builtin::events::BOOK_CHANGED.emit(crate::builtin::events::BookChanged {
        path: path.to_string(),
        what,
    })
}

#[cfg(test)]
mod fallible_tests;

#[cfg(test)]
#[allow(
    clippy::let_underscore_must_use,
    reason = "tests set up files in memory, which does not fail"
)]
mod tests;
