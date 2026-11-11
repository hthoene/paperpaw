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
use super::kept_file;
use alloc::string::String;
use paperpaw_platform::{Storage, StorageError};
use paperpaw_sdk::{library, positions};

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

/// Moves the reader's old file of `kept` of the book at `path` where it is
/// shared, if it is there and the book has no shared file yet.
pub(super) fn adopt(storage: &dyn Storage, path: &str, kept: Kept) {
    let (name, dir) = match kept {
        Kept::Position => ("progress.ini", positions::DIR),
        Kept::Annotations => ("annotations.txt", ANNOTATIONS),
    };
    let Some(old) = old_file(path, name) else {
        return;
    };
    if !storage.exists(&old) {
        return;
    }
    let Ok(new) = kept_file(dir, path) else {
        return;
    };
    if !matches!(storage.size(&new), Err(StorageError::NotFound)) {
        return;
    }
    // A folder that is there already is no error; one that cannot be made
    // fails the rename below, which is tried again.
    let _ = storage.create_dir(dir);
    // Where it stays (the card), it is moved at the book's next read.
    let _ = storage.rename(&old, &new);
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
