//! Every book of the library sorted by title, kept on the card and read a
//! page at a time, so that a list of any length takes a few kilobytes
//! (the X4 holds no more than 2,000 books of an [`Index`](super::index::Index)).
//!
//! The kernel makes the file of a view's archive folder (one for each,
//! for two at most) after the catalog was written anew, or when a
//! view asks for it ([`request`]): every book but those in the archive
//! folder, sorted by its title's first letters
//! ([`sort_key`](super::index::sort_key)) and where its line starts, each
//! with its series (a hash of the name), its place among the books of
//! its series and how many there are. It is written to a file beside it
//! and put in place whole, its end saying of which library it is
//! ([`Library::version`](super::service::Library::version)), how many
//! books it has and a check of them: a file of another version, cut
//! short or damaged is not read ([`Sorted::open`] gives `None`), and the
//! view says the library is being prepared until the kernel made it
//! anew.
//!
//! A view opens it once it shows, reads it through once (how many books
//! each filter shows in each block of it, the books read last, the
//! books being read) and then pages it ([`Sorted::page`]): what changed
//! since it was made (how far a book is read, favourites, books removed;
//! the catalog's log) is taken in as it is read.

use alloc::string::String;
use alloc::vec::Vec;

use super::catalog::Show;
use super::index::{self, Item, KEY};
use crate::Storage;

#[cfg(any(test, feature = "kernel"))]
mod build;
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub use build::{Build, asked, clean_up, kept_archives, wanted};

/// The folder of the files of the sorted books, one for each archive
/// folder a view leaves out ([`file_of`]).
const FOLDER: &str = ".paperpaw/library";
/// Files of the sorted books kept at most, for as many archive folders:
/// the views of all others ask for theirs anew.
const KEPT_FILES: usize = 2;
/// Where it is written before it is put in place.
const BUILDING: &str = ".paperpaw/library/sorted.tmp";
/// The runs sorted in memory, merged into it.
const RUNS: &str = ".paperpaw/library/sorted.runs";

/// What the file is, and its format: another is made anew.
const MAGIC: &[u8; 4] = b"PPS1";
/// Bytes of a book in the file.
const RECORD: usize = 28;
/// Bytes of the file's end (see [`Trailer`]).
const TRAILER: usize = 44;

/// The file of the sorted books for a view leaving `archive` out
/// (trimmed of `/`), named by its hash.
fn file_of(archive: &str) -> String {
    alloc::format!("{FOLDER}/sorted-{:08x}.idx", index::hash(archive))
}

/// Whether `name` (in the folder) is that of a file of the sorted books.
fn is_file_name(name: &str) -> bool {
    name.strip_prefix("sorted-")
        .and_then(|rest| rest.strip_suffix(".idx"))
        .is_some_and(|hex| hex.len() == 8 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// A book as the file keeps it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Rec {
    offset: u32,
    hash: u32,
    key: [u8; KEY],
    /// A hash of its series' name; 0 for none.
    series: u32,
    number: u16,
    /// Its place among the books of its series, by title, and how many
    /// there are (0 where the series was not counted).
    rank: u16,
    members: u16,
    progress: u8,
    flags: u8,
}

impl Rec {
    fn encoded(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.offset.to_le_bytes());
        out.extend_from_slice(&self.hash.to_le_bytes());
        out.extend_from_slice(&self.key);
        out.extend_from_slice(&self.series.to_le_bytes());
        out.extend_from_slice(&self.number.to_le_bytes());
        out.extend_from_slice(&self.rank.to_le_bytes());
        out.extend_from_slice(&self.members.to_le_bytes());
        out.push(self.progress);
        out.push(self.flags);
    }

    fn decoded(b: &[u8; RECORD]) -> Rec {
        let word = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
        let half = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
        let mut key = [0u8; KEY];
        key.copy_from_slice(&b[8..16]);
        Rec {
            offset: word(0),
            hash: word(4),
            key,
            series: word(16),
            number: half(20),
            rank: half(22),
            members: half(24),
            progress: b[26],
            flags: b[27],
        }
    }

    /// Ordered as the file is: by title, then where its line starts.
    fn order(&self) -> ([u8; KEY], u32) {
        (self.key, self.offset)
    }
}

/// Sorts `v` by `key`, a heapsort: a few hundred bytes of code where the
/// standard library's sort takes kilobytes for each type it sorts (the
/// order of things of the same key is any).
fn sort_by<T>(v: &mut [T], key: fn(&T) -> u128) {
    // Moves the thing at `root` down the heap of the first `end` until
    // none below is greater (`child` and `root` below `end` all along).
    let sift = |v: &mut [T], mut root: usize, end: usize| loop {
        let mut child = 2 * root + 1;
        if child >= end {
            break;
        }
        if child + 1 < end && key(&v[child]) < key(&v[child + 1]) {
            child += 1;
        }
        if key(&v[root]) >= key(&v[child]) {
            break;
        }
        v.swap(root, child);
        root = child;
    };
    let n = v.len();
    for root in (0..n / 2).rev() {
        sift(v, root, n);
    }
    for end in (1..n).rev() {
        v.swap(0, end);
        sift(v, 0, end);
    }
}

/// A sort key of a book's title key and its place in the catalog, after
/// `first`: as `(first, key, offset)` compares.
fn ranked(first: u8, key: [u8; KEY], offset: u32) -> u128 {
    (u128::from(first) << 96) | (u128::from(u64::from_be_bytes(key)) << 32) | u128::from(offset)
}

/// The hash of a series' name as the file keeps it (never 0).
fn series_hash(name: &str) -> u32 {
    if name.is_empty() {
        return 0;
    }
    (paperpaw_digest::fnv1a64(name) as u32).max(1)
}

/// FNV-1a, 32 bits, over `bytes` from `hash` on: the check of the books.
fn checked(mut hash: u32, bytes: &[u8]) -> u32 {
    for &b in bytes {
        hash ^= u32::from(b);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}
const CHECK_START: u32 = 0x811c_9dc5;

/// The file's end: what it is, of which library, how many books and a
/// check of them; then a check of itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Trailer {
    version: u64,
    archive: u32,
    count: u32,
    archived: u32,
    archive_len: u16,
    length: u64,
    records: u32,
}

impl Trailer {
    fn encoded(&self) -> [u8; TRAILER] {
        let mut out = Vec::with_capacity(TRAILER);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&self.version.to_le_bytes());
        out.extend_from_slice(&self.archive.to_le_bytes());
        out.extend_from_slice(&self.count.to_le_bytes());
        out.extend_from_slice(&self.archived.to_le_bytes());
        out.extend_from_slice(&self.archive_len.to_le_bytes());
        out.extend_from_slice(&[0, 0]);
        out.extend_from_slice(&self.length.to_le_bytes());
        out.extend_from_slice(&self.records.to_le_bytes());
        let own = checked(CHECK_START, &out);
        out.extend_from_slice(&own.to_le_bytes());
        let mut bytes = [0u8; TRAILER];
        bytes.copy_from_slice(&out);
        bytes
    }

    /// The end of a file of `size` bytes, where it is one of this format
    /// that holds together.
    fn decoded(b: &[u8; TRAILER], size: u64) -> Option<Trailer> {
        let word = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
        let long = |i: usize| u64::from(word(i)) | (u64::from(word(i + 4)) << 32);
        if &b[..4] != MAGIC || checked(CHECK_START, &b[..TRAILER - 4]) != word(TRAILER - 4) {
            return None;
        }
        let trailer = Trailer {
            version: long(4),
            archive: word(12),
            count: word(16),
            archived: word(20),
            archive_len: u16::from_le_bytes([b[24], b[25]]),
            length: long(28),
            records: word(36),
        };
        let wanted = u64::from(trailer.count) * RECORD as u64
            + u64::from(trailer.archive_len)
            + TRAILER as u64;
        (trailer.length == size && wanted == size).then_some(trailer)
    }
}

/// The trailer of the file at `path`, where it is one of this format
/// that holds together.
fn trailer_of(storage: &dyn Storage, path: &str) -> Option<Trailer> {
    let size = storage.size(path).ok()?;
    let start = size.checked_sub(TRAILER as u64)?;
    let mut b = [0u8; TRAILER];
    if storage.read_at(path, start, &mut b).ok()? != TRAILER {
        return None;
    }
    Trailer::decoded(&b, size)
}

/// The archive folder the file of `path` left out (as a view names it).
fn archive_of(storage: &dyn Storage, path: &str, trailer: &Trailer) -> Option<String> {
    let mut bytes = alloc::vec![0u8; usize::from(trailer.archive_len)];
    let at = u64::from(trailer.count) * RECORD as u64;
    if !bytes.is_empty() && storage.read_at(path, at, &mut bytes).ok()? != bytes.len() {
        return None;
    }
    String::from_utf8(bytes).ok()
}

/// How a view lists the books: those a filter shows, the books read
/// last first or all by title, the books of a series as one stack (as
/// covers, of all books).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct View {
    pub show: Show,
    pub by_recent: bool,
    pub stacked: bool,
}

impl View {
    pub fn new(show: Show, by_recent: bool, stacked: bool) -> View {
        View {
            show,
            by_recent,
            stacked: stacked && show_index(show) == 0,
        }
    }
}

/// A book of a view, as [`Sorted::page`] gives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Entry {
    /// What the index of the library knows of it ([`Item`]); its series
    /// is [`NO_SERIES`](super::index::NO_SERIES) (the file keeps no names).
    pub item: Item,
    /// How many books its stack holds (1 for a book on its own).
    pub stack: u16,
}

fn show_index(show: Show) -> usize {
    match show {
        Show::Unread => 1,
        Show::Reading => 2,
        Show::Finished => 3,
        Show::Favourites => 4,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_heapsort_sorts_as_the_standard_library_does() {
        let mut seed = 0x2545_f491_u32;
        for len in (0..40).chain([255, 256, 257, 1000]) {
            let v: Vec<(u8, [u8; KEY], u32)> = (0..len)
                .map(|n| {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    let key = (seed % 7).to_be_bytes();
                    (
                        (seed % 3) as u8,
                        [0, 0, 0, 0, key[0], key[1], key[2], key[3]],
                        n,
                    )
                })
                .collect();
            let mut ours = v.clone();
            sort_by(&mut ours, |&(first, key, offset)| {
                ranked(first, key, offset)
            });
            let mut theirs = v;
            theirs.sort_unstable();
            assert_eq!(ours, theirs, "{len}");
        }
    }
}
