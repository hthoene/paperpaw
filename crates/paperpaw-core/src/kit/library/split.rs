//! The books' details and covers made small, kept by earlier versions in
//! one folder each (`books/<key>.ini`, `covers/<key>.thumb`), moved into
//! the folders split by keys that hold them now (`details/<k>/<key>.ini`,
//! `thumbs/<k>/<key>.thumb`, see [`kept_path`](super::kept_path)): once, a
//! few at a time, by the kernel. Until then they are read where they were.
//! Then what the folders of caches for books ([`BookData`], made again from
//! the book when needed) hold in the folder itself, as earlier versions of
//! their plugins kept it, goes: kept in the parts of their keys now, those
//! are out of date, and a folder of thousands is slow on a memory card.
//!
//! A step lists the next names of an old folder, or moves (or removes)
//! some of them: in the order the folder lists them, so that a memory card
//! finds each where it found the last (`paperpaw-fat`, noted folders), each
//! move whole or not made at all (cut off, the card's record of moves
//! settles it). Where what is moved is there already (written since), that
//! stays and the old one goes. What cannot be moved stays where it was,
//! read there; the folder goes once nothing is left of it.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::Storage;
use paperpaw_sdk::files;
use paperpaw_platform::storage::StorageError;

use super::{
    BookData, BookDataKind, DETAILS_DIR, DIR, OLD_DETAILS_DIR, OLD_THUMBS_DIR, THUMBS_DIR,
};

/// Bytes of names listed at a time (some 150 of a book's, `<key>.thumb`):
/// a step of their own, the old folder read whole each time; kept in one
/// piece of memory.
const LISTED: usize = 4 * 1024;

/// Names moved in a step.
const MOVED: usize = 16;

/// Old caches removed in a step (folders, of a few files each).
const CLEARED: usize = 4;

/// The old folders, each with the one its entries go to.
const FOLDERS: [(&str, &str); 2] = [(OLD_DETAILS_DIR, DETAILS_DIR), (OLD_THUMBS_DIR, THUMBS_DIR)];

/// Whether anything is left to move: an old folder is there.
pub fn wanted(storage: &dyn Storage) -> bool {
    FOLDERS
        .iter()
        .any(|(old, _)| files::is_dir(storage, &alloc::format!("{DIR}/{old}")))
}

/// The moving, under way: where it is.
#[derive(Debug, Default)]
pub struct Split {
    /// Of [`FOLDERS`], then of the folders of caches.
    folder: usize,
    /// The next names listed, to move or remove, a line each: `d` for a
    /// folder or `f`, then the name; from byte `taken` on.
    listed: String,
    taken: usize,
    /// Entries of the folder that could not be moved (first in its
    /// listing, as they stay): passed over.
    stuck: usize,
    /// Moved or removed so far.
    moved: u32,
}

impl Split {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// How many entries were moved or removed so far.
    #[must_use]
    pub fn moved(&self) -> u32 {
        self.moved
    }

    /// One step, with what plugins keep for books (`data`): `true` once
    /// all is moved; an error where an old folder could not be read
    /// (tried again later, from there on).
    pub fn step(&mut self, storage: &dyn Storage, data: &[BookData]) -> Result<bool, StorageError> {
        loop {
            let folder = self.folder;
            if let Some((old, new)) = FOLDERS.get(folder) {
                let (old, new) = (alloc::format!("{DIR}/{old}"), alloc::format!("{DIR}/{new}"));
                self.move_step(storage, &old, &new)?;
            } else {
                let cache = data
                    .iter()
                    .filter(|data| data.kind == BookDataKind::Cache)
                    .nth(folder - FOLDERS.len());
                match cache {
                    Some(cache) => self.clear_step(storage, cache)?,
                    None => return Ok(true),
                }
            }
            // A folder found done goes on to the next in the same step:
            // listing one with nothing (left) to do is quick.
            if self.folder == folder {
                return Ok(false);
            }
        }
    }

    /// A step of moving the entries of folder `old` into `new`'s parts.
    fn move_step(
        &mut self,
        storage: &dyn Storage,
        old: &str,
        new: &str,
    ) -> Result<(), StorageError> {
        if self.taken >= self.listed.len() {
            self.list(storage, old, &|name, is_dir| !is_dir && is_kept_name(name))?;
            if self.listed.is_empty() {
                // Nothing left to move, but what was not: the folder goes
                // with what else is in it, the library's own.
                if self.stuck == 0 {
                    #[expect(
                        clippy::let_underscore_must_use,
                        reason = "an old folder left holds nothing to read: taken away at the next start"
                    )]
                    let _ = storage.remove_dir_all(old);
                }
                self.next();
            }
            return Ok(());
        }
        for (name, _) in self.take(MOVED) {
            let Some(key) = name.get(..16) else {
                continue;
            };
            let from = files::join(old, &name);
            let to = alloc::format!("{}/{name}", super::split(new, key));
            let done = if storage.exists(&to) {
                // Written since: the old one is out of date.
                storage.remove(&from)
            } else {
                storage.rename(&from, &to)
            };
            match done {
                Ok(()) => self.moved += 1,
                // Gone meanwhile (its book removed or moved).
                Err(StorageError::NotFound) => {}
                Err(_) => self.stuck += 1,
            }
        }
        Ok(())
    }

    /// A step of removing what folder of caches `cache` holds for books in
    /// the folder itself, not in the parts of their keys.
    fn clear_step(&mut self, storage: &dyn Storage, cache: &BookData) -> Result<(), StorageError> {
        if self.taken >= self.listed.len() {
            self.list(storage, &cache.folder, &|name, _| {
                cache.key_of(name).is_some()
            })?;
            if self.listed.is_empty() {
                self.next();
            }
            return Ok(());
        }
        for (name, is_dir) in self.take(CLEARED) {
            let path = files::join(&cache.folder, &name);
            let done = match is_dir {
                true => storage.remove_dir_all(&path),
                false => storage.remove(&path),
            };
            match done {
                Ok(()) => self.moved += 1,
                Err(StorageError::NotFound) => {}
                Err(_) => self.stuck += 1,
            }
        }
        Ok(())
    }

    /// The next `most` names listed, and whether each is a folder.
    fn take(&mut self, most: usize) -> Vec<(String, bool)> {
        let mut names = Vec::new();
        let rest = self.listed.get(self.taken..).unwrap_or_default();
        for line in rest.split_terminator('\n').take(most) {
            self.taken += line.len() + 1;
            if let Some(name) = line.get(1..) {
                names.push((String::from(name), line.starts_with('d')));
            }
        }
        names
    }

    /// On to the next folder.
    fn next(&mut self) {
        self.folder += 1;
        self.stuck = 0;
    }

    /// Lists the next names of folder `old` that `wanted` takes: after
    /// those that stay, at most [`LISTED`] bytes of them, in the order the
    /// folder lists them.
    fn list(
        &mut self,
        storage: &dyn Storage,
        old: &str,
        wanted: &dyn Fn(&str, bool) -> bool,
    ) -> Result<(), StorageError> {
        let mut seen = 0;
        self.listed.clear();
        self.taken = 0;
        let listed = &mut self.listed;
        let stuck = self.stuck;
        let mut full = false;
        match storage.list(old, &mut |entry| {
            if !wanted(entry.name, entry.is_dir) {
                return;
            }
            full |= listed.len() + entry.name.len() + 2 > LISTED;
            if seen >= stuck && !full {
                listed.push(if entry.is_dir { 'd' } else { 'f' });
                listed.push_str(entry.name);
                listed.push('\n');
            }
            seen += 1;
        }) {
            Err(StorageError::NotFound) => Ok(()),
            listed => listed,
        }
    }
}

/// Whether `name` is one the library keeps for a book: its key (16
/// hexadecimal digits) and `.` and more.
fn is_kept_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    let hex = |b: &u8| matches!(b, b'0'..=b'9' | b'a'..=b'f');
    bytes.len() > 17
        && bytes.get(..16).is_some_and(|key| key.iter().all(hex))
        && bytes.get(16) == Some(&b'.')
}

#[cfg(test)]
mod tests;
