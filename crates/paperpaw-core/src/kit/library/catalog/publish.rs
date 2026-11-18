//! What a first walk found so far, shown as it goes on: a walk that
//! begins where there is no catalog yet (books copied onto a new card,
//! thousands of them) puts the books it found into the catalog at
//! checkpoints, so that they are listed and can be read at once rather
//! than after the walk. The building file is written as always; the
//! catalog takes what was added to it since the last checkpoint, whole
//! lines a piece a step. At its end the walk adds the rest: the catalog
//! is what the building file was, byte for byte, as a walk that never
//! published puts it in place. Cut off (the power gone), the catalog is
//! the books found until then, its last line maybe cut short, and the
//! next walk compares with it and makes it whole as with any other.

use alloc::vec::Vec;

use super::{building_path, catalog_path};
use paperpaw_sdk::{Storage, StorageError};

/// Books found at the first checkpoint; then each time they are twice
/// as many.
const FIRST: usize = 20;
/// Between two checkpoints at most, once [`MIN_NEW`] books more were
/// found (and a [`GROWTH`]th more): a large library shows more of its
/// books every few minutes.
const EVERY_MS: u64 = 120_000;
/// Books more found that make a checkpoint after [`EVERY_MS`].
const MIN_NEW: usize = 50;
/// After [`EVERY_MS`], a checkpoint also wants the books shown grown by
/// this part of them: each sorts all books found, so in a library of
/// tens of thousands checkpoints every two minutes would sort them over
/// and over (as many sorts as the walk takes minutes); grown by a
/// quarter each time, all of them together sort the books about five
/// times.
const GROWTH: usize = 4;
/// Bytes taken into the catalog in a step.
const PIECE: usize = 2 * 1024;
/// A line of the catalog longer than a piece is taken whole, up to this
/// (no line written whole is this long, see `OLD_LINE`).
const LONGEST: usize = 8 * 1024;

/// The checkpoints of a first walk.
#[derive(Debug, Default)]
pub(super) struct Publish {
    /// Bytes of the building file in the catalog: its header and whole
    /// lines.
    done: u64,
    /// Books in the catalog.
    books: usize,
    /// When the last checkpoint was (or the walk began), in milliseconds.
    at: u64,
    /// The checkpoint under way: the building file's length then, and
    /// the books found.
    until: Option<(u64, usize)>,
    /// A checkpoint was done since [`Publish::take_news`] was asked.
    news: bool,
    /// The checkpoint under way is the walk's end (views hear of the
    /// catalog made, not of a checkpoint), however its steps are taken.
    ending: bool,
}

impl Publish {
    /// A first walk's, begun at `now` (milliseconds).
    pub(super) fn new(now: u64) -> Publish {
        Publish {
            at: now,
            ..Publish::default()
        }
    }

    /// Whether `found` books at `now` make a checkpoint.
    fn due(&self, found: usize, now: u64) -> bool {
        if self.books == 0 {
            return found >= FIRST;
        }
        let new = found.saturating_sub(self.books);
        found >= self.books.saturating_mul(2)
            || (new >= MIN_NEW.max(self.books / GROWTH) && now.saturating_sub(self.at) >= EVERY_MS)
    }

    /// Whether the catalog holds books of this walk (the end adds the
    /// rest to it, rather than putting the building file in its place).
    pub(super) fn begun(&self) -> bool {
        self.done > 0
    }

    /// A step of a checkpoint, with `found` books written at `now`: one
    /// begun where it is due (`all`: at the walk's end, whatever was
    /// added), and a piece of it taken into the catalog. `true` where the
    /// step was taken; `false` where there is nothing to take.
    pub(super) fn step(
        &mut self,
        storage: &dyn Storage,
        found: usize,
        now: u64,
        all: bool,
    ) -> Result<bool, StorageError> {
        let (until, books) = match self.until {
            Some(until) => until,
            None if all || self.due(found, now) => {
                let until = (storage.size(&building_path())?, found);
                if until.0 <= self.done {
                    return Ok(false);
                }
                self.until = Some(until);
                self.ending = all;
                until
            }
            None => return Ok(false),
        };
        if self.done < until {
            self.done += take_piece(storage, self.done, until)?;
        }
        if self.done >= until {
            self.until = None;
            self.books = books;
            self.at = now;
            // At the end, views hear of the catalog made.
            self.news = !self.ending;
        }
        Ok(true)
    }

    /// Whether a checkpoint was done since this was asked last.
    pub(super) fn take_news(&mut self) -> bool {
        core::mem::take(&mut self.news)
    }
}

/// Adds to the catalog the building file's bytes from `at` (where a line
/// starts) on, whole lines up to [`PIECE`] of them (a longer line whole,
/// up to [`LONGEST`]), not past `until`: how many.
fn take_piece(storage: &dyn Storage, at: u64, until: u64) -> Result<u64, StorageError> {
    let left = until - at;
    let mut piece = read(storage, at, left.min(PIECE as u64))?;
    let mut end = piece.iter().rposition(|&b| b == b'\n').map(|i| i + 1);
    if end.is_none() && (piece.len() as u64) < left {
        piece = read(storage, at, left.min(LONGEST as u64))?;
        end = piece.iter().rposition(|&b| b == b'\n').map(|i| i + 1);
    }
    let end = end.unwrap_or(piece.len());
    storage.append(&catalog_path(), &piece[..end])?;
    Ok(end as u64)
}

/// `len` bytes of the building file from `at`.
fn read(storage: &dyn Storage, at: u64, len: u64) -> Result<Vec<u8>, StorageError> {
    let len = usize::try_from(len).map_err(|_| StorageError::TooLarge)?;
    let mut bytes = alloc::vec![0; len];
    let mut read = 0;
    while read < len {
        let n = storage.read_at(&building_path(), at + read as u64, &mut bytes[read..])?;
        if n == 0 {
            // Shorter than when the checkpoint began: written by none but
            // the walk, so never.
            return Err(StorageError::Io);
        }
        read += n;
    }
    Ok(bytes)
}
