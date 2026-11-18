//! A folder's entries a page at a time, folders first, then by name (case
//! ignored), without ever holding more than a page: a folder of thousands of
//! files fits the X4's memory.
//!
//! Pages are found by keyset pagination: one pass over the folder keeps only
//! the `n` entries that follow (or precede) a known one in that order. Every
//! allocation is fallible.

use alloc::string::String;
use alloc::vec::Vec;
use core::cmp::Ordering;

use paperpaw_sdk::Storage;

use super::path::copy;
use super::{Error, error_of};

/// An entry of a folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
}

impl Entry {
    /// An entry; the name is copied with fallible allocation.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn new(name: &str, is_dir: bool, size: u64) -> Result<Self, Error> {
        Ok(Self {
            name: copy(name)?,
            is_dir,
            size,
        })
    }
}

/// Folders first, then names regardless of case, then exact names (so the
/// order is total).
fn order(a: (bool, &str), b: (bool, &str)) -> Ordering {
    b.0.cmp(&a.0)
        .then_with(|| {
            a.1.chars()
                .flat_map(char::to_lowercase)
                .cmp(b.1.chars().flat_map(char::to_lowercase))
        })
        .then_with(|| a.1.cmp(b.1))
}

fn cmp(entry: &Entry, is_dir: bool, name: &str) -> Ordering {
    order((entry.is_dir, &entry.name), (is_dir, name))
}

/// A page with room for `n` entries, taken at once.
fn room(n: usize) -> Result<Vec<Entry>, Error> {
    let mut page = Vec::new();
    page.try_reserve_exact(n).map_err(|_| Error::NoMemory)?;
    Ok(page)
}

/// The entries of folder `dir`; names starting with `.` only with `hidden`.
pub struct Folder<'a> {
    pub storage: &'a dyn Storage,
    pub dir: &'a str,
    pub hidden: bool,
}

impl core::fmt::Debug for Folder<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Folder")
            .field("dir", &self.dir)
            .field("hidden", &self.hidden)
            .finish()
    }
}

impl Folder<'_> {
    fn each(&self, f: &mut dyn FnMut(bool, &str, u64)) -> Result<(), Error> {
        self.storage
            .list(self.dir, &mut |e| {
                if self.hidden || !e.name.starts_with('.') {
                    f(e.is_dir, e.name, e.size);
                }
            })
            .map_err(error_of)
    }

    /// How many entries there are.
    ///
    /// # Errors
    /// The storage's refusal (a missing folder too).
    pub fn count(&self) -> Result<usize, Error> {
        let mut count = 0;
        self.each(&mut |_, _, _| count += 1)?;
        Ok(count)
    }

    /// How many entries come before `entry` (where it is, or would be).
    ///
    /// # Errors
    /// The storage's refusal.
    pub fn rank(&self, entry: &Entry) -> Result<usize, Error> {
        let mut rank = 0;
        self.each(&mut |is_dir, name, _| {
            if cmp(entry, is_dir, name) == Ordering::Greater {
                rank += 1;
            }
        })?;
        Ok(rank)
    }

    /// The first `n` entries after `after` (from the start if `None`).
    ///
    /// # Errors
    /// The storage's refusal; [`Error::NoMemory`].
    pub fn after(&self, after: Option<&Entry>, n: usize) -> Result<Vec<Entry>, Error> {
        // One more than a page: an entry is put in, then the last one leaves.
        let mut page = room(n.saturating_add(1))?;
        let mut failed = None;
        self.each(&mut |is_dir, name, size| {
            if failed.is_some()
                || n == 0
                || after.is_some_and(|a| cmp(a, is_dir, name) != Ordering::Less)
            {
                return;
            }
            if page.len() == n
                && page
                    .last()
                    .is_some_and(|l| cmp(l, is_dir, name) == Ordering::Less)
            {
                return;
            }
            match Entry::new(name, is_dir, size) {
                Ok(entry) => {
                    let at = page.partition_point(|e| cmp(e, is_dir, name) == Ordering::Less);
                    page.insert(at, entry);
                    page.truncate(n);
                }
                Err(e) => failed = Some(e),
            }
        })?;
        failed.map_or(Ok(page), Err)
    }

    /// The last `n` entries before `before` (up to the end if `None`).
    ///
    /// # Errors
    /// The storage's refusal; [`Error::NoMemory`].
    pub fn before(&self, before: Option<&Entry>, n: usize) -> Result<Vec<Entry>, Error> {
        let mut page = room(n.saturating_add(1))?;
        let mut failed = None;
        self.each(&mut |is_dir, name, size| {
            if failed.is_some()
                || n == 0
                || before.is_some_and(|b| cmp(b, is_dir, name) != Ordering::Greater)
            {
                return;
            }
            if page.len() == n
                && page
                    .first()
                    .is_some_and(|f| cmp(f, is_dir, name) == Ordering::Greater)
            {
                return;
            }
            match Entry::new(name, is_dir, size) {
                Ok(entry) => {
                    let at = page.partition_point(|e| cmp(e, is_dir, name) == Ordering::Less);
                    page.insert(at, entry);
                    if page.len() > n {
                        page.remove(0);
                    }
                }
                Err(e) => failed = Some(e),
            }
        })?;
        failed.map_or(Ok(page), Err)
    }

    /// Page number `page` of `rows` entries of the `len` there are, walking
    /// from the nearer end.
    ///
    /// # Errors
    /// As [`Folder::after`].
    pub fn page(&self, page: usize, rows: usize, len: usize) -> Result<Vec<Entry>, Error> {
        let rows = rows.max(1);
        let pages = len.div_ceil(rows);
        if len == 0 {
            return Ok(Vec::new());
        }
        if page * 2 < pages {
            let mut entries = self.after(None, rows)?;
            for _ in 0..page {
                entries = self.after(entries.last(), rows)?;
            }
            Ok(entries)
        } else {
            let last = len - (pages - 1) * rows;
            let mut entries = self.before(None, last)?;
            for _ in page + 1..pages {
                entries = self.before(entries.first(), rows)?;
            }
            Ok(entries)
        }
    }

    /// The page of `rows` entries where `entry` is (or would be), and the
    /// index of its first entry.
    ///
    /// # Errors
    /// As [`Folder::after`].
    pub fn page_of(&self, entry: &Entry, rows: usize) -> Result<(usize, Vec<Entry>), Error> {
        let rows = rows.max(1);
        let rank = self.rank(entry)?;
        let mut entries = self.before(Some(entry), rank % rows)?;
        let rest = self.after(entries.last(), rows - entries.len())?;
        entries
            .try_reserve(rest.len())
            .map_err(|_| Error::NoMemory)?;
        entries.extend(rest);
        Ok((rank - rank % rows, entries))
    }
}

#[cfg(test)]
mod tests;
