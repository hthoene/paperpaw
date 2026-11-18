//! Catching up the books opened last (`opened.txt`) that a catalog already
//! there lacks, at the start of a library walk, so that they show at once
//! rather than when the walk ends, and without rewriting the catalog.
//!
//! A catalog is a line a book; its lines may stand in any order. So a book
//! missing from it is *appended* as one more line, a book a write, never
//! more than a hundred. The catalog before it is untouched: a cut off
//! (the power gone) leaves every line before the appended one whole and at
//! most a last line cut short, which the next catch-up cuts off first
//! (and appends the book again) and the next walk, which compares with the
//! catalog, makes whole. The walk
//! folds the lines appended into the catalog it puts in place anyway.
//!
//! Which of the opened books the catalog holds is told by one pass over it
//! a piece a step, never held (a hundred hashes are, 800 bytes, and the one
//! line being appended). The pass is made once for a catalog: what it
//! found is kept in `catalog.seen` (the catalog's generation and the books
//! it was looked for), so that a start in which only books known to be in
//! the catalog were opened reads nothing of it, and one that opened a new
//! book looks for that one only.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use super::fallible::path_of;
use super::making::header_end;
use super::{Record, catalog_path, each_head, generation, lines};
use crate::Place;
use crate::kit::library::{self, DIR};
use paperpaw_digest::fnv1a64;
use paperpaw_sdk::{Storage, StorageError, files};

/// Bytes of the catalog looked through in one step.
const STEP_BYTES: u64 = 16 * 1024;
/// How far back from its end a catalog is looked for the end of its last
/// whole line (a line is never this long, `OLD_LINE`).
const TAIL: usize = 8 * 1024;
/// Books looked for at most (what `opened.txt` holds).
const MAX_OPENED: usize = 100;

/// Where it is kept which books the catalog was looked through for.
pub(crate) fn seen_path() -> String {
    format!("{DIR}/catalog.seen")
}

/// The job: look through the catalog for the opened books it lacks, then
/// append them ([`CatchUp::step`]).
#[derive(Debug, Default)]
pub struct CatchUp {
    /// The generation of the catalog it looks through.
    generation: u64,
    /// The paths of `opened.txt`, hashed, sorted.
    all: Vec<u64>,
    /// Of them, those not found in the catalog yet (sorted).
    wanted: Vec<u64>,
    /// Where the next line of the catalog not looked at starts.
    at: u64,
    /// The catalog was looked through (or there is nothing to look for).
    scanned: bool,
    /// Books appended.
    appended: usize,
}

impl CatchUp {
    /// Begins: the opened books, less those a pass over this catalog
    /// found before; nothing to do where there is no catalog of this
    /// format (a walk makes it).
    pub fn start(env: &Place<'_>) -> Result<CatchUp, StorageError> {
        let storage = env.storage();
        let mut catch_up = CatchUp::default();
        let Some(head) = header_end(storage)? else {
            catch_up.scanned = true;
            return Ok(catch_up);
        };
        // What a cut off append left after the last line goes first: it
        // would be taken for the book it names.
        end_last_line(storage)?;
        catch_up.generation = generation(storage).unwrap_or(0);
        catch_up.at = head;
        let mut all = Vec::new();
        lines(storage, &library::opened_file(), &mut |_, line| {
            let line = line.trim_start_matches('\u{feff}');
            if !line.is_empty()
                && !line.starts_with(paperpaw_sdk::data::MARK)
                && all.len() < MAX_OPENED
                && all.try_reserve(1).is_ok()
            {
                all.push(fnv1a64(line));
            }
            true
        })?;
        paperpaw_sort::sort_unstable(&mut all);
        all.dedup();
        let seen = seen(storage, catch_up.generation)?;
        let mut wanted = Vec::new();
        wanted
            .try_reserve_exact(all.len())
            .map_err(|_| StorageError::OutOfMemory)?;
        wanted.extend(all.iter().filter(|h| seen.binary_search(h).is_err()));
        catch_up.all = all;
        catch_up.scanned = wanted.is_empty();
        catch_up.wanted = wanted;
        Ok(catch_up)
    }

    /// Whether the catalog was added to: views are to hear of it.
    pub fn appended(&self) -> bool {
        self.appended > 0
    }

    /// Lets go of nothing: it holds a few hundred bytes.
    pub fn pause(&mut self) {}

    /// One step: a piece of the catalog looked through, or the books
    /// missing appended; `true` when done. An error leaves the catalog as
    /// it is but for lines appended whole.
    pub fn step(&mut self, env: &Place<'_>) -> Result<bool, StorageError> {
        if self.scanned {
            self.append_missing(env)?;
            return Ok(true);
        }
        self.look(env.storage())?;
        Ok(false)
    }

    #[inline(never)]
    fn look(&mut self, storage: &dyn Storage) -> Result<(), StorageError> {
        let (from, mut stopped) = (self.at, None);
        let wanted = &mut self.wanted;
        each_head(storage, &catalog_path(), from, &mut |start, head| {
            if start - from >= STEP_BYTES {
                stopped = Some(start);
                return false;
            }
            if let Some(path) = path_of(head)
                && let Ok(at) = wanted.binary_search(&fnv1a64(path))
            {
                wanted.remove(at);
            }
            !wanted.is_empty()
        })?;
        match stopped {
            Some(start) => self.at = start,
            None => self.scanned = true,
        }
        Ok(())
    }

    /// Appends a line for each opened book still wanted, then notes that
    /// this catalog was looked through for the opened books.
    #[inline(never)]
    fn append_missing(&mut self, env: &Place<'_>) -> Result<(), StorageError> {
        let storage = env.storage();
        if !self.wanted.is_empty() {
            let mut failure = None;
            lines(storage, &library::opened_file(), &mut |_, line| {
                let path = line.trim_start_matches('\u{feff}');
                if self.wanted.binary_search(&fnv1a64(path)).is_err() {
                    return true;
                }
                match append_book(env, path) {
                    Ok(true) => self.appended += 1,
                    Ok(false) => {}
                    Err(e) => {
                        failure = Some(e);
                        return false;
                    }
                }
                true
            })?;
            if let Some(e) = failure {
                return Err(e);
            }
        }
        write_seen(storage, self.generation, &self.all)
    }
}

/// The books the catalog of `generation` was looked through for; none
/// where there is no note or it is of another catalog.
fn seen(storage: &dyn Storage, generation: u64) -> Result<Vec<u64>, StorageError> {
    let mut seen = Vec::new();
    let mut first = true;
    let mut ours = false;
    lines(storage, &seen_path(), &mut |_, line| {
        let line = line.trim();
        if first {
            first = false;
            ours = line.parse() == Ok(generation);
        } else if ours
            && let Ok(hash) = u64::from_str_radix(line, 16)
            && seen.len() < MAX_OPENED
            && seen.try_reserve(1).is_ok()
        {
            seen.push(hash);
        }
        true
    })?;
    paperpaw_sort::sort_unstable(&mut seen);
    Ok(seen)
}

fn write_seen(storage: &dyn Storage, generation: u64, all: &[u64]) -> Result<(), StorageError> {
    let mut text = String::new();
    text.try_reserve_exact(24 + all.len() * 17)
        .map_err(|_| StorageError::OutOfMemory)?;
    text.push_str(&format!("{generation}\n"));
    for hash in all {
        text.push_str(&format!("{hash:016x}\n"));
    }
    storage.write(&seen_path(), text.as_bytes())
}

/// Appends the line of the book at `path`, from its details, where it is
/// one the library walk would take (a book of a format in the library
/// folder, its details read from the book); `false` where it is not (the
/// walk makes up its mind).
fn append_book(env: &Place<'_>, path: &str) -> Result<bool, StorageError> {
    let storage = env.storage();
    let folder = library::folder(env);
    let inside = folder.is_empty()
        || path
            .strip_prefix(folder)
            .is_some_and(|rest| rest.starts_with('/'));
    if !inside || !library::listing::shown(env.formats(), files::name(path), false) {
        return Ok(false);
    }
    let size = match storage.size(path) {
        Ok(size) => size,
        Err(StorageError::NotFound) => return Ok(false),
        Err(e) => return Err(e),
    };
    let Some(book) = library::metadata::read_book(storage, path, size) else {
        return Ok(false);
    };
    let status = library::try_status(storage, path).map_err(|_| StorageError::Io)?;
    let line = Record::of(path, &book).with_status(status).text();
    storage.append(&catalog_path(), line.as_bytes())?;
    Ok(true)
}

/// Makes the catalog end after a whole line: what a cut off append (or a
/// first walk's checkpoint) left after the last line is cut off (a book
/// the walk makes whole, or the next catch-up appends); where the storage
/// cannot cut, the line is ended (the walk makes it whole).
fn end_last_line(storage: &dyn Storage) -> Result<(), StorageError> {
    let file = catalog_path();
    let size = storage.size(&file)?;
    let mut chunk = [0_u8; 256];
    let read = |at: u64, buf: &mut [u8]| -> Result<(), StorageError> {
        let mut done = 0;
        while done < buf.len() {
            match storage.read_at(&file, at + done as u64, &mut buf[done..])? {
                0 => return Err(StorageError::Io),
                n => done += n,
            }
        }
        Ok(())
    };
    let Some(last) = size.checked_sub(1) else {
        return Ok(());
    };
    read(last, &mut chunk[..1])?;
    if chunk[0] == b'\n' {
        return Ok(());
    }
    let (limit, mut end) = (size.saturating_sub(TAIL as u64), size);
    while end > limit {
        let from = end.saturating_sub(chunk.len() as u64).max(limit);
        let piece = &mut chunk[..usize::try_from(end - from).unwrap_or(0)];
        read(from, piece)?;
        if let Some(at) = piece.iter().rposition(|&b| b == b'\n') {
            return match storage.truncate(&file, from + at as u64 + 1) {
                Err(StorageError::Unsupported) => storage.append(&file, b"\n"),
                other => other,
            };
        }
        end = from;
    }
    storage.append(&file, b"\n")
}
