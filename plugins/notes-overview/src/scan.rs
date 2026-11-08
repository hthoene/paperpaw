//! Which books have notes: read without opening a book. The folder of
//! annotation files is listed once (a book's file is named by the hash of
//! its path), then the library's books are passed one at a time, so nothing
//! of more than the books with notes is held, however many books there are.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::annotations::{self, Kind};
use paperpaw_sdk::digest::fnv1a64;
use paperpaw_sdk::library::{self, catalog::Record};
use paperpaw_sdk::{Env, Storage};

/// Books listed at most (the rest are not shown, but are exported).
pub(crate) const MOST_BOOKS: usize = 200;

/// Annotation files looked at at most: 8 bytes each are kept.
const MOST_FILES: usize = 4096;

/// A book with notes, and how many.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Counted {
    pub path: String,
    pub title: String,
    pub highlights: u16,
    pub bookmarks: u16,
}

/// The names (as numbers) of the annotation files that are not empty,
/// sorted.
pub(crate) fn kept(storage: &dyn Storage) -> Vec<u64> {
    let dir = format!("{}/annotations", paperpaw_sdk::data::DIR);
    let mut names = Vec::new();
    #[expect(
        clippy::let_underscore_must_use,
        reason = "no folder means no notes; what was listed so far stands"
    )]
    let _ = storage.list(&dir, &mut |entry| {
        if entry.is_dir || entry.size == 0 || names.len() >= MOST_FILES {
            return;
        }
        if let Some(name) = entry.name.strip_suffix(".txt")
            && name.len() == 16
            && let Ok(number) = u64::from_str_radix(name, 16)
        {
            names.push(number);
        }
    });
    paperpaw_sdk::sort::sort(&mut names);
    names
}

/// Whether `path` is the book of one of the `kept` files.
pub(crate) fn has_file(kept: &[u64], path: &str) -> bool {
    kept.binary_search(&fnv1a64(path)).is_ok()
}

/// Calls `f` with each book of the library that has a notes file and
/// what the library knows of it, the books one at a time.
pub(crate) fn each_noted(env: &Env<'_>, f: &mut dyn FnMut(&Record)) {
    let kept = kept(env.storage());
    if kept.is_empty() {
        return;
    }
    library::service(env).each(env, &mut |_, record| {
        if has_file(&kept, &record.path) {
            f(&record);
        }
    });
}

/// How many highlights and bookmarks the book at `path` has, read a line
/// at a time without their texts.
pub(crate) fn count(storage: &dyn Storage, path: &str) -> (u16, u16) {
    let (mut highlights, mut bookmarks) = (0u16, 0u16);
    for mark in annotations::marks(storage, path) {
        match mark.kind {
            Kind::Highlight => highlights = highlights.saturating_add(1),
            Kind::Bookmark => bookmarks = bookmarks.saturating_add(1),
            _ => {}
        }
    }
    (highlights, bookmarks)
}

/// The books with notes, by title, at most [`MOST_BOOKS`]; whether there
/// are more.
pub(crate) fn books(env: &Env<'_>) -> (Vec<Counted>, bool) {
    let mut found: Vec<Counted> = Vec::new();
    let mut more = false;
    each_noted(env, &mut |record| {
        let (highlights, bookmarks) = count(env.storage(), &record.path);
        if highlights + bookmarks == 0 {
            return;
        }
        if found.len() >= MOST_BOOKS {
            more = true;
            return;
        }
        found.push(Counted {
            path: record.path.clone(),
            title: record.title.clone(),
            highlights,
            bookmarks,
        });
    });
    paperpaw_sdk::sort::sort_by_cached_key(&mut found, |book| book.title.to_lowercase());
    (found, more)
}
