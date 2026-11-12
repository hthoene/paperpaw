//! What Beta 3's reader kept in its own folder of a book,
//! `.paperpaw/plugins/reader/books/<key>/` (`docs/data-files.md`): the
//! position from before positions were shared (`progress.ini`, section
//! `[progress]`) and the annotations from before they were shared
//! (`annotations.txt`). The interface 6 reader read the first while a book had
//! no shared position and moved the second when it opened the book; the native
//! reader reads only the shared files, so the host moves both there the first
//! time a book's position or annotations are read or edited here. An owner who
//! upgrades keeps the place and the bookmarks.
//!
//! A move is a rename, never a copy or a rewrite (the SDK reads `[progress]`
//! as a position, `paperpaw_sdk::positions`), and only where the book has no
//! shared file yet: a move would replace it. Nothing fails the read that asked:
//! a move that cannot be made now (the card, no memory) is tried again at the
//! book's next read, as the interface 6 reader did.
//!
//! The reads ask for one book again and again (a page of annotations after
//! another, the position at each opening): [`Memo`] remembers the last book
//! looked at and which of its files are settled, so a read of it again looks
//! at nothing and holds nothing.
use super::kept_file;
use alloc::string::String;
use core::cell::Cell;
use paperpaw_platform::{Storage, StorageError};
use paperpaw_sdk::{digest::fnv1a64, library, positions};

/// The reader's folder of books (`Env::plugin_dir("reader")` + `/books`).
const BOOKS: &str = ".paperpaw/plugins/reader/books";
/// The folder of the shared annotation files (`paperpaw_sdk::annotations`).
const ANNOTATIONS: &str = ".paperpaw/annotations";

/// What a read is about to look at.
#[derive(Clone, Copy)]
pub(super) enum Kept {
    /// The position (op 13): `progress.ini`.
    Position,
    /// The annotations (ops 15 to 17): `annotations.txt`.
    Annotations,
}

impl Kept {
    /// Its bit in [`Memo`].
    const fn bit(self) -> u8 {
        match self {
            Self::Position => 1,
            Self::Annotations => 2,
        }
    }
}

/// The last book looked at (the hash of its path) and the bits of its
/// [`Kept`] files that are settled: moved, or with nothing to move. Nine
/// bytes; one book at a time (another book's read takes its place).
#[derive(Default)]
pub(crate) struct Memo(Cell<(u64, u8)>);

impl Memo {
    /// [`adopt`], unless the book's file of `kept` is settled already: then
    /// nothing is looked at and nothing is held. A move that could not be made
    /// leaves the bit clear, so the next read tries again.
    pub(super) fn adopt(&self, storage: &dyn Storage, path: &str, kept: Kept) {
        let hash = fnv1a64(path);
        let (last, settled) = self.0.get();
        let settled = if last == hash { settled } else { 0 };
        if settled & kept.bit() != 0 {
            return;
        }
        let settled = if adopt(storage, path, kept) {
            settled | kept.bit()
        } else {
            settled
        };
        self.0.set((hash, settled));
    }
}

/// Moves the reader's old file of `kept` of the book at `path` where it is
/// shared, if it is there and the book has no shared file yet. `false` where
/// a move is left for a later read (the card, no memory).
pub(super) fn adopt(storage: &dyn Storage, path: &str, kept: Kept) -> bool {
    let (name, dir) = match kept {
        Kept::Position => ("progress.ini", positions::DIR),
        Kept::Annotations => ("annotations.txt", ANNOTATIONS),
    };
    let Some(old) = old_file(path, name) else {
        return false;
    };
    match storage.size(&old) {
        Ok(_) => {}
        Err(StorageError::NotFound) => return true,
        Err(_) => return false,
    }
    let Ok(new) = kept_file(dir, path) else {
        return false;
    };
    match storage.size(&new) {
        Err(StorageError::NotFound) => {}
        // A shared file wins: the old one is left as it is.
        Ok(_) => return true,
        Err(_) => return false,
    }
    // A folder that is there already is no error; one that cannot be made
    // fails the rename below, which is tried again.
    let _ = storage.create_dir(dir);
    // Where it stays (the card), it is moved at the book's next read.
    matches!(
        storage.rename(&old, &new),
        Ok(()) | Err(StorageError::NotFound)
    )
}

/// `BOOKS/<key>/<name>` of the book at `path`, `None` without the memory.
fn old_file(path: &str, name: &str) -> Option<String> {
    let mut digits = [0_u8; library::KEY_LEN];
    let key = library::key_into(path, &mut digits);
    let mut file = String::new();
    file.try_reserve_exact(BOOKS.len() + key.len() + name.len() + 2)
        .ok()?;
    for part in [BOOKS, "/", key, "/", name] {
        file.push_str(part);
    }
    Some(file)
}

#[cfg(test)]
#[path = "old_reader_tests.rs"]
mod tests;
