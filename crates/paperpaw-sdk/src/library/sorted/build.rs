//! Making the sorted file, as the kernel does: a few steps of reading
//! the library's books into runs sorted in memory, then a few steps of
//! merging the runs into the file, written beside it and put in place
//! whole. It holds a run and the series counted (some 24 KB on a device
//! of little memory) whatever the library's size; paused, it writes the
//! run it holds (a shorter one) or lets go of what it read ahead of the
//! runs, and keeps the series counted.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::{
    BUILDING, CHECK_START, FOLDER, KEPT_FILES, RECORD, RUNS, Rec, TRAILER, Trailer, archive_of,
    checked, file_of, is_file_name, series_hash, trailer_of,
};
use crate::library::catalog::{self, Record};
use crate::library::index::{self, FAVOURITE, NONE, OPENED};
use crate::{Env, MemoryClass, Storage, StorageError, files};

/// Books sorted in memory at once (a run) on a device of little memory
/// (the X4), and on others.
const SMALL_RUN: usize = 256;
const LARGE_RUN: usize = 512;
/// Books written or read at once (a piece of a run, or of a step).
const PIECE: usize = 64;
/// Lines of the catalog read in one step.
const STEP_LINES: usize = 64;
/// Books written in one step of the merge.
const STEP_BOOKS: usize = 256;
/// Books read ahead of all the runs together, while merging.
const AHEAD: usize = 256;
/// Series counted at most on a device of little memory, and on others;
/// the books of others are not stacked.
const SMALL_SERIES: usize = 1024;
const LARGE_SERIES: usize = 16_384;

/// Removes what a making cut short left (the runs, the file being
/// written); the file made stays.
#[expect(
    clippy::let_underscore_must_use,
    reason = "files that are not there are removed already"
)]
pub fn clean_up(storage: &dyn Storage) {
    for path in [RUNS, BUILDING] {
        if storage.exists(path) {
            let _ = storage.remove(path);
        }
    }
}

/// The archive folders the files there leave out, of those whole
/// (two at most; others go): the kernel makes them anew for
/// the same views after the catalog changed.
#[expect(
    clippy::let_underscore_must_use,
    reason = "a file left over goes at the next start"
)]
pub fn kept_archives(storage: &dyn Storage) -> Vec<String> {
    let mut names = Vec::new();
    let _ = storage.list(FOLDER, &mut |entry| {
        if !entry.is_dir && is_file_name(entry.name) {
            names.push(files::join(FOLDER, entry.name));
        }
    });
    let mut kept = Vec::new();
    for path in names {
        let archive = trailer_of(storage, &path).and_then(|trailer| {
            archive_of(storage, &path, &trailer).filter(|a| index::hash(a) == trailer.archive)
        });
        match archive {
            Some(archive) if kept.len() < KEPT_FILES && file_of(&archive) == path => {
                kept.push(archive);
            }
            _ => {
                let _ = storage.remove(&path);
            }
        }
    }
    kept
}

/// Takes in that a view asked for the file leaving `archive` out:
/// `archives` (those asked for last first) begin with it, and the file
/// of one asked for before two others goes.
#[expect(
    clippy::let_underscore_must_use,
    reason = "a file left over goes at the next start"
)]
pub fn asked(storage: &dyn Storage, archives: &mut Vec<String>, archive: &str) {
    let archive = archive.trim_matches('/');
    archives.retain(|a| a != archive);
    archives.insert(0, archive.to_string());
    while archives.len() > KEPT_FILES {
        let Some(old) = archives.pop() else {
            break;
        };
        // Not where it is the file of one kept, too (alike hashes).
        let path = file_of(&old);
        if !archives.iter().any(|a| file_of(a) == path) && storage.exists(&path) {
            let _ = storage.remove(&path);
        }
    }
}

/// Whether the file is to be made (anew) for a view leaving `archive`
/// out: there is none whole, or it is of another library, version or
/// archive. Not where there are no books to ask for.
pub fn wanted(env: &Env<'_>, archive: &str) -> bool {
    let Some(version) = crate::library::service(env).version(env) else {
        return false;
    };
    let archive = archive.trim_matches('/');
    match trailer_of(env.storage(), &file_of(archive)) {
        Some(t) => t.version != version || t.archive != index::hash(archive),
        None => true,
    }
}

/// One of the runs, as the merge reads it: where its next book is in
/// the runs file, where it ends, and the books read ahead.
#[derive(Debug, Default)]
struct Cursor {
    next: u32,
    end: u32,
    ahead: Vec<Rec>,
}

/// What is being done.
#[derive(Debug)]
enum Phase {
    /// Reading the books into runs; the catalog from this place on (the
    /// kernel's library), or all in one step (another).
    Reading(u64),
    Merging,
    Done,
}

/// The making of the sorted file (see the module).
#[derive(Debug)]
pub struct Build {
    archive: String,
    /// The file made ([`file_of`]).
    path: String,
    version: u64,
    phase: Phase,
    run: Vec<Rec>,
    /// How many books each run written has.
    runs: Vec<u32>,
    /// Each series' hash, how many books it has, and how many were
    /// written (sorted by hash).
    series: Vec<(u32, u16, u16)>,
    max_series: usize,
    /// Books a run holds at most.
    run_len: usize,
    count: u32,
    archived: u32,
    cursors: Vec<Cursor>,
    /// The check of the books written.
    sum: u32,
    /// Where the books of the run being read begin in the catalog, and
    /// the books counted and archived before them: where a pause goes
    /// back to.
    mark: Option<(u64, u32, u32)>,
    /// Why a pause could not write the run it held: part of it may be in
    /// the runs file, so the next step fails (and the making begins
    /// anew).
    broken: Option<StorageError>,
}

impl Build {
    /// Begins making the file of the library there is now for a view
    /// leaving `archive` out; what an earlier one cut short left goes.
    pub fn start(env: &Env<'_>, archive: &str) -> Result<Build, StorageError> {
        let storage = env.storage();
        clean_up(storage);
        let version = crate::library::service(env)
            .version(env)
            .ok_or(StorageError::NotFound)?;
        storage.write(RUNS, b"")?;
        let (max_series, run_len) = if env.device().memory == MemoryClass::Small {
            (SMALL_SERIES, SMALL_RUN)
        } else {
            (LARGE_SERIES, LARGE_RUN)
        };
        let archive = archive.trim_matches('/');
        Ok(Build {
            archive: archive.to_string(),
            path: file_of(archive),
            version,
            phase: Phase::Reading(0),
            run: Vec::new(),
            runs: Vec::new(),
            series: Vec::new(),
            max_series,
            run_len,
            count: 0,
            archived: 0,
            cursors: Vec::new(),
            sum: CHECK_START,
            mark: None,
            broken: None,
        })
    }

    /// Writes the run being read (a shorter one; where that fails, it is
    /// read again from its first book), or lets go of the books read ahead
    /// of the runs; the series counted stay.
    pub fn pause(&mut self, storage: &dyn Storage) {
        if matches!(self.phase, Phase::Reading(_)) {
            match self.write_run(storage) {
                Ok(()) => self.run = Vec::new(),
                Err(e) => self.broken = Some(e),
            }
        }
        if let (Phase::Reading(_), Some((at, count, archived))) = (&self.phase, self.mark.take()) {
            for rec in &self.run {
                if let Ok(i) = self
                    .series
                    .binary_search_by_key(&rec.series, |&(s, _, _)| s)
                {
                    self.series[i].1 = self.series[i].1.saturating_sub(1);
                }
            }
            (self.count, self.archived) = (count, archived);
            self.phase = Phase::Reading(at);
        }
        self.run = Vec::new();
        for cursor in &mut self.cursors {
            cursor.next -= cursor.ahead.len() as u32;
            cursor.ahead = Vec::new();
        }
    }

    /// Gives up: the file there was stays.
    pub fn abort(self, storage: &dyn Storage) {
        clean_up(storage);
    }

    /// One step; `true` once the file is in place. On an error what was
    /// written goes, and the file there was stays.
    pub fn step(&mut self, env: &Env<'_>) -> Result<bool, StorageError> {
        let result = self.go_on(env);
        if result.is_err() {
            clean_up(env.storage());
        }
        result
    }

    fn go_on(&mut self, env: &Env<'_>) -> Result<bool, StorageError> {
        let storage = env.storage();
        if let Some(e) = self.broken {
            return Err(e);
        }
        match self.phase {
            Phase::Reading(at) => self.read_books(env, at)?,
            Phase::Merging => self.merge_step(storage)?,
            Phase::Done => return Ok(true),
        }
        Ok(matches!(self.phase, Phase::Done))
    }

    /// Reads a few lines of the catalog from `at` (all books of a library
    /// of a plugin's own at once) into the runs.
    fn read_books(&mut self, env: &Env<'_>, at: u64) -> Result<(), StorageError> {
        let storage = env.storage();
        let mut failed = None;
        if !crate::library::service::is_kept(env) {
            crate::library::service(env).each(env, &mut |key, record| {
                if failed.is_none()
                    && let Err(e) = self.take_book(storage, key, &record)
                {
                    failed = Some(e);
                }
            });
            if let Some(e) = failed {
                return Err(e);
            }
            return self.end_reading(storage);
        }
        let mut lines = 0;
        let next = catalog::lines_from(
            storage,
            &catalog::catalog_path(),
            at,
            &mut |offset, line| {
                lines += 1;
                if self.mark.is_none() {
                    self.mark = Some((offset, self.count, self.archived));
                }
                if let Some(record) = Record::parse(line)
                    && let Err(e) = self.take_book(storage, offset, &record)
                {
                    failed = Some(e);
                    return false;
                }
                lines < STEP_LINES
            },
        )?;
        if let Some(e) = failed {
            return Err(e);
        }
        if lines < STEP_LINES {
            return self.end_reading(storage);
        }
        self.phase = Phase::Reading(next);
        Ok(())
    }

    /// Takes the book of `record`, whose key is `key`.
    fn take_book(
        &mut self,
        storage: &dyn Storage,
        key: u64,
        record: &Record,
    ) -> Result<(), StorageError> {
        let archived = !self.archive.is_empty()
            && record
                .path
                .strip_prefix(self.archive.as_str())
                .is_some_and(|rest| rest.starts_with('/'));
        if archived {
            self.archived = self.archived.saturating_add(1);
            return Ok(());
        }
        // A line past 4 GiB of the catalog (some 10 million books) is left
        // out: a file keeps where a book's line starts in 32 bits.
        let Ok(offset) = u32::try_from(key) else {
            return Ok(());
        };
        let series = series_hash(&record.series);
        if series != 0 {
            match self.series.binary_search_by_key(&series, |&(s, _, _)| s) {
                Ok(at) => self.series[at].1 = self.series[at].1.saturating_add(1),
                Err(at) if self.series.len() < self.max_series => {
                    self.series.insert(at, (series, 1, 0));
                }
                Err(_) => {}
            }
        }
        let status = record.status;
        if self.run.capacity() == 0 {
            self.run
                .try_reserve_exact(self.run_len)
                .map_err(|_| StorageError::OutOfMemory)?;
        }
        self.run.push(Rec {
            offset,
            hash: index::hash(&record.path),
            key: index::sort_key(&record.title),
            series,
            number: index::number(&record.number),
            rank: 0,
            members: 0,
            progress: status.progress.unwrap_or(NONE),
            flags: (u8::from(status.favourite) * FAVOURITE) | (u8::from(status.opened) * OPENED),
        });
        self.count = self.count.saturating_add(1);
        if self.run.len() >= self.run_len {
            self.write_run(storage)?;
        }
        Ok(())
    }

    /// Sorts the run and writes it to the runs file.
    fn write_run(&mut self, storage: &dyn Storage) -> Result<(), StorageError> {
        if self.run.is_empty() {
            return Ok(());
        }
        super::sort_by(&mut self.run, |r| super::ranked(0, r.key, r.offset));
        // Written a piece at a time: never the run twice in memory.
        let mut bytes = Vec::with_capacity(self.run.len().min(PIECE) * RECORD);
        for piece in self.run.chunks(PIECE) {
            bytes.clear();
            for rec in piece {
                rec.encoded(&mut bytes);
            }
            storage.append(RUNS, &bytes)?;
        }
        self.runs.push(self.run.len() as u32);
        self.run.clear();
        self.mark = None;
        Ok(())
    }

    /// All books read: the last run written, the merge begun.
    fn end_reading(&mut self, storage: &dyn Storage) -> Result<(), StorageError> {
        self.write_run(storage)?;
        self.run = Vec::new();
        let mut start = 0u32;
        self.cursors = self
            .runs
            .iter()
            .map(|&n| {
                let cursor = Cursor {
                    next: start,
                    end: start + n,
                    ahead: Vec::new(),
                };
                start += n;
                cursor
            })
            .collect();
        storage.write(BUILDING, b"")?;
        self.phase = Phase::Merging;
        Ok(())
    }

    /// Books read ahead of each run at once.
    fn each_ahead(&self) -> usize {
        (AHEAD / self.cursors.len().max(1)).clamp(1, PIECE)
    }

    /// Reads ahead the books of each run that has none read, into
    /// `bytes` ([`Build::each_ahead`] books long).
    fn read_ahead(&mut self, storage: &dyn Storage, bytes: &mut [u8]) -> Result<(), StorageError> {
        let each = bytes.len() / RECORD;
        for cursor in &mut self.cursors {
            if !cursor.ahead.is_empty() || cursor.next >= cursor.end {
                continue;
            }
            let n = (cursor.end - cursor.next).min(each as u32) as usize;
            let want = &mut bytes[..n * RECORD];
            let at = u64::from(cursor.next) * RECORD as u64;
            if storage.read_at(RUNS, at, want)? != want.len() {
                return Err(StorageError::InvalidData);
            }
            // Read backwards, taken from the end.
            for chunk in want.as_chunks::<RECORD>().0.iter().rev() {
                cursor.ahead.push(Rec::decoded(chunk));
            }
            cursor.next += n as u32;
        }
        Ok(())
    }

    /// A few books merged into the file; the file put in place after the
    /// last.
    fn merge_step(&mut self, storage: &dyn Storage) -> Result<(), StorageError> {
        let mut bytes = Vec::with_capacity(PIECE * RECORD);
        let mut ahead = alloc::vec![0u8; self.each_ahead() * RECORD];
        for _ in 0..STEP_BOOKS {
            self.read_ahead(storage, &mut ahead)?;
            let first = self
                .cursors
                .iter()
                .enumerate()
                .filter_map(|(i, c)| c.ahead.last().map(|r| (r.order(), i)))
                .min();
            let Some((_, i)) = first else {
                break;
            };
            let Some(mut rec) = self.cursors[i].ahead.pop() else {
                break;
            };
            if let Ok(at) = self
                .series
                .binary_search_by_key(&rec.series, |&(s, _, _)| s)
            {
                let (_, members, written) = &mut self.series[at];
                rec.members = *members;
                rec.rank = *written;
                *written = written.saturating_add(1);
            }
            rec.encoded(&mut bytes);
            if bytes.len() >= PIECE * RECORD {
                self.sum = checked(self.sum, &bytes);
                storage.append(BUILDING, &bytes)?;
                bytes.clear();
            }
        }
        self.sum = checked(self.sum, &bytes);
        storage.append(BUILDING, &bytes)?;
        if self
            .cursors
            .iter()
            .any(|c| !c.ahead.is_empty() || c.next < c.end)
        {
            return Ok(());
        }
        self.put_in_place(storage)
    }

    /// The archive and the trailer written, the file put in place.
    fn put_in_place(&mut self, storage: &dyn Storage) -> Result<(), StorageError> {
        let archive = self.archive.as_bytes();
        let archive = &archive[..archive.len().min(usize::from(u16::MAX))];
        let length = u64::from(self.count) * RECORD as u64 + archive.len() as u64 + TRAILER as u64;
        let trailer = Trailer {
            version: self.version,
            archive: index::hash(&self.archive),
            count: self.count,
            archived: self.archived,
            archive_len: archive.len() as u16,
            length,
            records: self.sum,
        };
        let mut end = archive.to_vec();
        end.extend_from_slice(&trailer.encoded());
        storage.append(BUILDING, &end)?;
        if storage.size(BUILDING)? != length {
            return Err(StorageError::InvalidData);
        }
        files::move_to(storage, BUILDING, &self.path)?;
        #[expect(
            clippy::let_underscore_must_use,
            reason = "runs left over are removed by the next making"
        )]
        let _ = storage.remove(RUNS);
        self.cursors = Vec::new();
        self.series = Vec::new();
        self.phase = Phase::Done;
        Ok(())
    }
}
