//! The reads of `core.library` over the catalog on the card: op 1 `state`, op
//! 3 `book`, op 4 `text`, op 13 `position`, ops 15 and 16 `annotations` and
//! `anchors`, and op 25 `book_key` (`docs/abi/library.md` 2.1, 2.2, 6).
//!
//! One implementation: `paperpaw_sdk::library`'s `try_` readers. This adapter
//! only maps them to the wire records, so nothing is read twice and no
//! reader lives in core. A memory shortage is `ERR_NO_MEMORY`, never an
//! abort and never "not found"; every buffer is reserved fallibly, and the
//! catalog is read a line at a time (no call holds the catalog or a line).
//!
//! The adapter is stateless: it holds the card and the three facts of the
//! device the card cannot tell ([`CatalogEnv`]).
//!
//! Memory of one call, peak heap (measured by `lib_reads_oom_tests`, with the
//! log of changes at two lines): `state` 57 bytes, `book` 2,005 (the six
//! text buffers, 1,952; the flag `indexed` scans the details a line at a
//! time and holds none of them), `text` 1,077, `book_key` 53, `position` 114 with
//! a 60-byte file (the file's text, 4 KiB at most, and its XPointer, 1 KiB at
//! most), `annotations` 1,130 (scratch for a row's text, note and key, 1,088), `anchors`
//! 2,090 (two XPointers of 1 KiB), `text` of an annotation 1,066; the annotation
//! file is read a line at a time and held nowhere. Every catalog read
//! holds the log of changes while it reads a line: 12 bytes a line, 3 KiB at
//! the 256 lines after which the kernel folds it, 12 KiB at most. Stack: the
//! partial MD5's 1 KiB sample for `book_key`, the SDK's catalog reads below
//! 1 KiB. Not measured on the X4 yet.
//!
//! Not here yet: the book key of a book whose path hashes like another's
//! (the writes round owns key allocation, so op 25 answers the plain
//! FNV-1a key). The `missing` flag of op 3 is the service's, from
//! [`LibQuery::book_file_present`]. The generation of an
//! annotation list is the file's own number, never 0 (`annotations::generation_of`).
use super::lib_service::{BookRef, LibQuery, One, Rows};
use crate::abi7::context::RowsEnd;
use crate::native_host::registrations::{self as wire, Record};
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
pub(crate) use importer::is_by as importer_is_by;
#[cfg(test)]
pub(crate) use importer::set as importer_set;
pub(crate) use jobs::CatalogJobs;
use paperpaw_native::host_glue::{
    ERR_INVALID, ERR_IO, ERR_NO_MEMORY, ERR_NO_SPACE, ERR_NOT_FOUND, ERR_STATE, ERR_UNSUPPORTED,
};
use paperpaw_platform::{Storage, StorageError};
use paperpaw_sdk::data;
use paperpaw_sdk::digest::{fnv1a64, md5};
use paperpaw_sdk::library::{self, catalog};

mod annotations;
mod book;
mod delete;
mod field;
mod importer;
mod jobs;
mod position;
mod provide;
mod removal;
mod status;
mod write;

const PPM: u32 = 1_000_000;
/// Counts of the books `state` takes while the catalog changes under it.
const STATE_TRIES: usize = 3;

/// What the card cannot tell the adapter.
pub trait CatalogEnv {
    /// `Some(progress in parts per million)` while the walk makes the
    /// catalog where there was none (its books are those found so far:
    /// `core.library-walking`), `None` otherwise.
    fn walking(&self) -> Option<u32>;
    /// Today as days since 1970, `None` for an unknown clock (no book is
    /// new then).
    fn today(&self) -> Option<u32>;
    /// The id of the format of the book at `path`, as the format index
    /// finds it by extension; nothing for none.
    fn format_id(&self, _path: &str, _visit: &mut dyn FnMut(&str)) {}
    /// The field `id` (`<plugin>.<name>`) a plugin declared with
    /// `book_field.spec`: `Some(true)` kept in the catalog too (short values),
    /// `Some(false)` kept with the book only, `None` for none declared.
    fn book_field(&self, _id: &str) -> Option<bool> {
        None
    }
    /// Whether any plugin keeps data for the book at `path` (`book_data.spec`
    /// folders and the interface 6 ones): `delete`'s fact bit 2. An error is
    /// the card's or the memory's.
    fn book_data_kept(&self, _path: &str) -> Result<bool, i32> {
        Ok(false)
    }
    /// The library folder setting (`library.folder`): a root-1 path, empty for
    /// the card's top. Where the host says nothing the library is the top.
    fn library_folder(&self, visit: &mut dyn FnMut(&str)) {
        visit("");
    }
}

/// The catalog reads over `storage`.
pub(crate) struct CatalogReads {
    storage: Rc<dyn Storage>,
    env: Rc<dyn CatalogEnv>,
}

impl CatalogReads {
    /// The library folder setting ([`CatalogEnv::library_folder`]).
    pub(crate) fn library_folder(&self, visit: &mut dyn FnMut(&str)) {
        self.env.library_folder(visit);
    }

    pub(crate) fn new(storage: Rc<dyn Storage>, env: Rc<dyn CatalogEnv>) -> Self {
        Self { storage, env }
    }

    /// The catalog generation of the wire: it changes whenever the
    /// catalog's own generation or its log of changes does (a book added,
    /// removed, its status changed); never 0.
    pub(super) fn generation(&self) -> Result<u32, i32> {
        Ok(self.snapshot()?.0)
    }

    /// The generation and whether the catalog is ready, from one read of its
    /// generation and one of its state.
    fn snapshot(&self) -> Result<(u32, bool), i32> {
        let catalog_generation = catalog::try_generation(&*self.storage).map_err(fail)?;
        let (stamp, ready) = match catalog::try_state(&*self.storage).map_err(fail)? {
            catalog::State::Ready { stamp } => (stamp, true),
            _ => (0, false),
        };
        let mut bytes = [0_u8; 17];
        bytes[0] = u8::from(catalog_generation.is_some());
        bytes[1..9].copy_from_slice(&catalog_generation.unwrap_or(0).to_le_bytes());
        bytes[9..].copy_from_slice(&stamp.to_le_bytes());
        let hash = fnv1a64(bytes);
        // Folded to 32 bits; 0 is no generation.
        let generation = u32::try_from((hash ^ (hash >> 32)) & 0xffff_ffff)
            .unwrap_or(1)
            .max(1);
        Ok((generation, ready))
    }

    /// Where the line of the book starts in the catalog (a handle): a
    /// handle of another generation is stale; one that is not the start of
    /// a line is no book.
    fn locate(&self, book: BookRef<'_>) -> Result<u64, i32> {
        match book {
            BookRef::Path(_) => {
                let changes = catalog::try_changes(&*self.storage).map_err(fail)?;
                self.locate_with(book, &changes)
            }
            BookRef::Key { .. } => self.locate_with(book, &catalog::Changes::default()),
        }
    }

    /// [`locate`](Self::locate) under the log of changes the caller read
    /// (a handle needs none: the line's own read checks the log).
    fn locate_with(&self, book: BookRef<'_>, changes: &catalog::Changes) -> Result<u64, i32> {
        match book {
            BookRef::Path(path) => catalog::try_offset_of_with(&*self.storage, changes, path)
                .map_err(fail)?
                .ok_or(ERR_NOT_FOUND),
            BookRef::Key { key, generation } => {
                if generation != self.generation()? {
                    return Err(ERR_STATE);
                }
                let offset = u64::from(key);
                if self.starts_a_line(offset)? {
                    Ok(offset)
                } else {
                    Err(ERR_NOT_FOUND)
                }
            }
        }
    }

    /// Whether `offset` follows a line break (the header at 0 is no book).
    fn starts_a_line(&self, offset: u64) -> Result<bool, i32> {
        let Some(before) = offset.checked_sub(1) else {
            return Ok(false);
        };
        let file = catalog_file()?;
        let mut byte = [0_u8; 1];
        match self.storage.read_at(&file, before, &mut byte) {
            Ok(1) => Ok(byte[0] == b'\n'),
            Ok(_) | Err(StorageError::NotFound) => Ok(false),
            Err(e) => Err(storage_status(e)),
        }
    }
}

impl LibQuery for CatalogReads {
    fn state(&self, visit: One<'_>) -> Result<(), i32> {
        // The books and the generation are one snapshot: the count is taken
        // between two reads of the generation and kept when they agree. A
        // catalog that changes under three tries in a row answers the
        // generation read before its count, which is never newer than the
        // books: a handle made of it is stale and the plugin asks again.
        let mut before = self.snapshot()?;
        let mut answer = None;
        for _ in 0..STATE_TRIES {
            let books = if before.1 {
                catalog::try_count(&*self.storage).map_err(fail)?
            } else {
                0
            };
            let after = self.snapshot()?;
            answer = Some((before, books));
            if after == before {
                break;
            }
            before = after;
        }
        let ((generation, ready), books) = answer.unwrap_or((before, 0));
        let (state, walk_ppm) = match self.env.walking() {
            // Never 100 % while walking: that is state 2.
            Some(ppm) => (1, ppm.min(PPM - 1)),
            None if ready => (2, PPM),
            None => (0, 0),
        };
        visit(Record::ServiceLibState(wire::ServiceLibState {
            state,
            books,
            generation,
            walk_ppm,
        }));
        Ok(())
    }

    fn book(&self, book: BookRef<'_>, visit: One<'_>) -> Result<(), i32> {
        book::book(self, book, visit)
    }

    fn book_file_present(&self, path: &str) -> Result<bool, i32> {
        book::file_present(self, path)
    }

    fn text(
        &self,
        book: BookRef<'_>,
        at: &wire::ServiceLibTextAt,
        visit: One<'_>,
    ) -> Result<(), i32> {
        book::text(self, book, at, visit)
    }

    fn annotations(
        &self,
        book: BookRef<'_>,
        cursor: u32,
        generation: u32,
        visit: Rows<'_>,
    ) -> Result<RowsEnd, i32> {
        self.with_path(book, |path| {
            annotations::annotations(&*self.storage, path, cursor, generation, visit)
        })
    }

    fn anchors(
        &self,
        book: BookRef<'_>,
        cursor: u32,
        generation: u32,
        visit: Rows<'_>,
    ) -> Result<RowsEnd, i32> {
        self.with_path(book, |path| {
            annotations::anchors(&*self.storage, path, cursor, generation, visit)
        })
    }

    fn position(&self, book: BookRef<'_>, visit: One<'_>) -> Result<(), i32> {
        self.with_path(book, |path| position::read(&*self.storage, path, visit))
    }

    fn book_key(&self, book: BookRef<'_>, visit: One<'_>) -> Result<(), i32> {
        self.with_path(book, |path| self.key_of(path, visit))
    }
}

impl CatalogReads {
    /// Calls `f` with the root-1 path of the book, which is in the catalog.
    fn with_path<R>(
        &self,
        book: BookRef<'_>,
        f: impl FnOnce(&str) -> Result<R, i32>,
    ) -> Result<R, i32> {
        match book {
            BookRef::Path(path) => {
                self.locate(book)?;
                f(path)
            }
            BookRef::Key { .. } => {
                let offset = self.locate(book)?;
                let owned = catalog::try_path_at(&*self.storage, offset)
                    .map_err(fail)?
                    .ok_or(ERR_NOT_FOUND)?;
                f(&owned)
            }
        }
    }

    fn key_of(&self, path: &str, visit: One<'_>) -> Result<(), i32> {
        // The file may be gone from the card: then there is no key to give.
        let size = self.storage.size(path).map_err(storage_status)?;
        let mut key = [0_u8; library::KEY_LEN];
        let key = library::key_into(path, &mut key);
        let mut digits = [0_u8; 32];
        let fingerprint = md5::partial_into(&*self.storage, path, &mut digits).ok_or(ERR_IO)?;
        visit(Record::ServiceLibKey(wire::ServiceLibKey {
            key,
            fingerprint,
            size,
        }));
        Ok(())
    }
}

/// The status of a failed read of the library.
pub(super) fn fail(error: data::Error) -> i32 {
    match error {
        data::Error::OutOfMemory => ERR_NO_MEMORY,
        data::Error::Storage(e) => storage_status(e),
        _ => ERR_IO,
    }
}

/// The name of the file in `dir` that is kept for the book at `path` (its key
/// and `.txt`), built fallibly.
pub(super) fn kept_file(dir: &str, path: &str) -> Result<String, i32> {
    let mut digits = [0_u8; library::KEY_LEN];
    let key = library::key_into(path, &mut digits);
    let mut name = String::new();
    name.try_reserve_exact(dir.len() + 1 + key.len() + ".txt".len())
        .map_err(|_| ERR_NO_MEMORY)?;
    for part in [dir, "/", key, ".txt"] {
        name.push_str(part);
    }
    Ok(name)
}

fn storage_status(error: StorageError) -> i32 {
    match error {
        StorageError::NotFound => ERR_NOT_FOUND,
        StorageError::InvalidPath => ERR_INVALID,
        StorageError::Unsupported => ERR_UNSUPPORTED,
        StorageError::TooLarge | StorageError::Full => ERR_NO_SPACE,
        StorageError::OutOfMemory => ERR_NO_MEMORY,
        _ => ERR_IO,
    }
}

/// The catalog's file, reserved fallibly.
fn catalog_file() -> Result<String, i32> {
    const NAME: &str = "catalog.txt";
    let mut out = String::new();
    out.try_reserve_exact(library::DIR.len() + 1 + NAME.len())
        .map_err(|_| ERR_NO_MEMORY)?;
    out.push_str(library::DIR);
    out.push('/');
    out.push_str(NAME);
    Ok(out)
}

/// `size` zero bytes, reserved fallibly.
fn zeroed(size: usize) -> Result<Vec<u8>, i32> {
    let mut out = Vec::new();
    out.try_reserve_exact(size).map_err(|_| ERR_NO_MEMORY)?;
    out.resize(size, 0);
    Ok(out)
}

/// The longest prefix of `bytes` that is text (a cut character at its end
/// is left out).
fn valid_prefix(bytes: &[u8]) -> &str {
    match core::str::from_utf8(bytes) {
        Ok(text) => text,
        Err(e) => core::str::from_utf8(&bytes[..e.valid_up_to()]).unwrap_or_default(),
    }
}

#[cfg(test)]
#[path = "lib_reads_tests.rs"]
pub(crate) mod tests;
