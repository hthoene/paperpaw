//! Sorting files of fixed-size records bytewise with little memory: runs
//! that fit in memory are sorted in place and written to files, then
//! merged a few at a time. Records are big-endian, so bytewise order is
//! numeric order.
//!
//! Every allocation is fallible: a refused one ends the call with
//! [`StorageError::TooLarge`], and the files of the runs are removed as
//! after any other error.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::{Storage, StorageError};

use crate::room;

/// Bytes sorted in memory at once.
const RUN: usize = 16 * 1024;
/// Runs merged at once: memory stays at [`RUN`] plus this many
/// [`AHEAD`]s, however long the file; longer files take more passes.
const FAN_IN: usize = 16;
/// Bytes read ahead from each run while merging.
const AHEAD: usize = 512;
/// The longest record: the files of stays hold 20 bytes, their keys 16.
pub(crate) const MAX_RECORD: usize = 20;

/// A record read: its first `size` bytes are the record.
pub(crate) type Record = [u8; MAX_RECORD];

/// Records that are not 1 to [`MAX_RECORD`] bytes are not sorted.
fn check(size: usize) -> Result<(), StorageError> {
    if size == 0 || size > MAX_RECORD {
        return Err(StorageError::TooLarge);
    }
    Ok(())
}

/// Sorts the records of `size` bytes in `input` into `output` (which may
/// be `input`); `temp` names the files of the runs, which are gone
/// afterwards, also after an error.
pub(crate) fn sort(
    storage: &dyn Storage,
    input: &str,
    output: &str,
    size: usize,
    temp: &str,
) -> Result<(), StorageError> {
    check(size)?;
    let mut runs: Vec<String> = Vec::new();
    let result = sort_in_runs(storage, input, output, size, temp, &mut runs);
    for run in &runs {
        #[expect(
            clippy::let_underscore_must_use,
            reason = "a temporary file left behind is written over the next time"
        )]
        let _ = storage.remove(run);
    }
    result
}

/// A new name in `runs`; its place there.
fn next_run(runs: &mut Vec<String>, temp: &str) -> Result<usize, StorageError> {
    room::reserve(runs, 1)?;
    let name = room::numbered(temp, runs.len())?;
    runs.push(name);
    Ok(runs.len() - 1)
}

fn sort_in_runs(
    storage: &dyn Storage,
    input: &str,
    output: &str,
    size: usize,
    temp: &str,
    runs: &mut Vec<String>,
) -> Result<(), StorageError> {
    let per_run = (RUN / size).max(1) * size;
    let mut buffer = room::zeroed(per_run)?;
    let mut at = 0u64;
    // The places in `runs` of those not merged yet. Every run made, also
    // those merged into others, stays in `runs` to be removed.
    let mut pending: Vec<usize> = Vec::new();
    loop {
        let mut filled = 0;
        while filled < per_run {
            let Some(rest) = buffer.get_mut(filled..) else {
                break;
            };
            let n = match storage.read_at(input, at + filled as u64, rest) {
                Ok(n) => n,
                Err(StorageError::NotFound) if at == 0 && filled == 0 => 0,
                Err(e) => return Err(e),
            };
            if n == 0 {
                break;
            }
            filled += n;
        }
        let whole = filled - filled % size;
        if whole == 0 {
            break;
        }
        let Some(sorted) = buffer.get_mut(..whole) else {
            break;
        };
        sort_records(sorted, size);
        let run = next_run(runs, temp)?;
        storage.write(runs.get(run).map_or("", String::as_str), sorted)?;
        room::push(&mut pending, run)?;
        at += whole as u64;
        if filled < per_run {
            break;
        }
    }
    drop(buffer);
    // Merged a few at a time until few enough are left for the output.
    while pending.len() > FAN_IN {
        let mut merged: Vec<usize> = Vec::new();
        room::reserve(&mut merged, pending.len().div_ceil(FAN_IN))?;
        for group in pending.chunks(FAN_IN) {
            let run = next_run(runs, temp)?;
            merge(
                storage,
                runs,
                group,
                runs.get(run).map_or("", String::as_str),
                size,
            )?;
            for done in group {
                #[expect(
                    clippy::let_underscore_must_use,
                    reason = "a temporary file left behind is written over the next time"
                )]
                let _ = storage.remove(runs.get(*done).map_or("", String::as_str));
            }
            room::push(&mut merged, run)?;
        }
        pending = merged;
    }
    merge(storage, runs, &pending, output, size)
}

/// Sorts the records of `size` bytes in `data` in place (a heapsort: no
/// memory beyond the records themselves).
fn sort_records(data: &mut [u8], size: usize) {
    fn record(data: &[u8], size: usize, i: usize) -> &[u8] {
        &data[i * size..(i + 1) * size]
    }
    fn swap(data: &mut [u8], size: usize, a: usize, b: usize) {
        let (a, b) = (a.min(b), a.max(b));
        let (low, high) = data.split_at_mut(b * size);
        low[a * size..(a + 1) * size].swap_with_slice(&mut high[..size]);
    }
    /// Lets record `root` sink until the heap below it holds again.
    fn sift(data: &mut [u8], size: usize, mut root: usize, end: usize) {
        loop {
            let mut child = 2 * root + 1;
            if child >= end {
                return;
            }
            if child + 1 < end && record(data, size, child) < record(data, size, child + 1) {
                child += 1;
            }
            if record(data, size, root) >= record(data, size, child) {
                return;
            }
            swap(data, size, root, child);
            root = child;
        }
    }
    let n = data.len() / size;
    for root in (0..n / 2).rev() {
        sift(data, size, root, n);
    }
    for end in (1..n).rev() {
        swap(data, size, 0, end);
        sift(data, size, 0, end);
    }
}

/// A run being read.
struct Reader<'a> {
    path: &'a str,
    at: u64,
    /// Read ahead: `filled` bytes of it are records not taken yet from
    /// `start`.
    buffer: Vec<u8>,
    filled: usize,
    start: usize,
}

impl<'a> Reader<'a> {
    fn new(path: &'a str, size: usize) -> Result<Self, StorageError> {
        Ok(Self {
            path,
            at: 0,
            buffer: room::zeroed(AHEAD.max(size) / size * size)?,
            filled: 0,
            start: 0,
        })
    }

    fn head(&mut self, storage: &dyn Storage, size: usize) -> Result<Option<&[u8]>, StorageError> {
        if self.start + size > self.filled {
            let n = storage.read_at(self.path, self.at, &mut self.buffer)?;
            let whole = n - n % size;
            self.at += whole as u64;
            self.filled = whole;
            self.start = 0;
            if whole == 0 {
                return Ok(None);
            }
        }
        Ok(self.buffer.get(self.start..self.start + size))
    }
}

/// Merges the sorted runs `group` (places in `runs`) into `output`.
fn merge(
    storage: &dyn Storage,
    runs: &[String],
    group: &[usize],
    output: &str,
    size: usize,
) -> Result<(), StorageError> {
    storage.write(output, &[])?;
    let mut readers: Vec<Reader<'_>> = Vec::new();
    room::reserve(&mut readers, group.len())?;
    for run in group {
        let path = runs.get(*run).map_or("", String::as_str);
        readers.push(Reader::new(path, size)?);
    }
    // Room for a whole record beyond the limit: pushing never grows it.
    let mut out: Vec<u8> = Vec::new();
    out.try_reserve_exact(AHEAD * 4 + MAX_RECORD)
        .map_err(|_| StorageError::TooLarge)?;
    let mut record: Record = [0; MAX_RECORD];
    loop {
        // The run with the smallest head (few runs: a scan is fine).
        let mut best: Option<usize> = None;
        for (i, reader) in readers.iter_mut().enumerate() {
            if let Some(head) = reader.head(storage, size)?
                && best.is_none_or(|_| head < record.get(..size).unwrap_or_default())
            {
                record
                    .get_mut(..size)
                    .unwrap_or_default()
                    .copy_from_slice(head);
                best = Some(i);
            }
        }
        let Some(i) = best else {
            break;
        };
        if let Some(reader) = readers.get_mut(i) {
            reader.start += size;
        }
        out.extend_from_slice(record.get(..size).unwrap_or_default());
        if out.len() >= AHEAD * 4 {
            storage.append(output, &out)?;
            out.clear();
        }
    }
    storage.append(output, &out)
}

/// Calls `visit` with every record of `path`, in order; its error ends
/// the walk.
pub(crate) fn each(
    storage: &dyn Storage,
    path: &str,
    size: usize,
    mut visit: impl FnMut(&[u8]) -> Result<(), StorageError>,
) -> Result<(), StorageError> {
    check(size)?;
    let mut chunk = room::zeroed((AHEAD * 4 / size).max(1) * size)?;
    let mut at = 0u64;
    loop {
        let n = match storage.read_at(path, at, &mut chunk) {
            Ok(n) => n,
            Err(StorageError::NotFound) if at == 0 => 0,
            Err(e) => return Err(e),
        };
        let whole = n - n % size;
        for record in chunk.get(..whole).unwrap_or_default().chunks_exact(size) {
            visit(record)?;
        }
        // Only a read without a whole record is the end: a read may
        // return less than asked for.
        if whole == 0 {
            return Ok(());
        }
        at += whole as u64;
    }
}

/// The records of a file, read a chunk at a time. A read error ends them
/// early and is kept in [`Records::failed`].
pub(crate) struct Records<'s> {
    storage: &'s dyn Storage,
    path: String,
    size: usize,
    at: u64,
    buffer: Vec<u8>,
    filled: usize,
    start: usize,
    pub failed: Option<StorageError>,
}

impl<'s> Records<'s> {
    /// The records of `size` bytes in `path`; the memory they take is
    /// taken now.
    pub(crate) fn new(
        storage: &'s dyn Storage,
        path: &str,
        size: usize,
    ) -> Result<Self, StorageError> {
        check(size)?;
        Ok(Self {
            storage,
            path: room::string(&[path])?,
            size,
            at: 0,
            buffer: room::zeroed((AHEAD * 4 / size).max(1) * size)?,
            filled: 0,
            start: 0,
            failed: None,
        })
    }
}

impl Iterator for Records<'_> {
    type Item = Record;

    fn next(&mut self) -> Option<Record> {
        if self.start + self.size > self.filled {
            let n = match self.storage.read_at(&self.path, self.at, &mut self.buffer) {
                Ok(n) => n,
                Err(StorageError::NotFound) if self.at == 0 => 0,
                Err(e) => {
                    self.failed = Some(e);
                    0
                }
            };
            let whole = n - n % self.size;
            self.filled = whole;
            self.at += whole as u64;
            self.start = 0;
            if whole == 0 {
                return None;
            }
        }
        let mut record: Record = [0; MAX_RECORD];
        record
            .get_mut(..self.size)?
            .copy_from_slice(self.buffer.get(self.start..self.start + self.size)?);
        self.start += self.size;
        Some(record)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;
    use paperpaw_sdk::MemoryStorage;

    /// `count` records of 8 bytes in a jumbled order.
    fn jumbled(count: u32) -> Vec<u8> {
        let mut data = Vec::new();
        let mut x: u32 = 12_345;
        for i in 0..count {
            x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            data.extend_from_slice(&(x % 5000).to_be_bytes());
            data.extend_from_slice(&i.to_be_bytes());
        }
        data
    }

    #[test]
    fn sorts_large_files_in_runs() {
        let storage = MemoryStorage::new();
        // 10 000 records of 8 bytes: five runs.
        storage.write("in", &jumbled(10_000)).unwrap();
        sort(&storage, "in", "in", 8, "tmp").unwrap();
        let mut records = Vec::new();
        each(&storage, "in", 8, |r| {
            records.push(r.to_vec());
            Ok(())
        })
        .unwrap();
        assert_eq!(records.len(), 10_000);
        assert!(records.windows(2).all(|w| w[0] <= w[1]));
        assert!(storage.size("tmp.0").is_err(), "runs are removed");
        let streamed: Vec<Vec<u8>> = Records::new(&storage, "in", 8)
            .unwrap()
            .map(|r| r[..8].to_vec())
            .collect();
        assert_eq!(streamed, records);
        // Nothing to sort.
        sort(&storage, "none", "out", 8, "tmp").unwrap();
        assert_eq!(storage.size("out").unwrap(), 0);
    }

    /// More runs than are merged at once: merged in passes, every run
    /// removed after.
    #[test]
    fn merges_many_runs_in_passes() {
        let storage = MemoryStorage::new();
        let count = (RUN / 8 * (FAN_IN * 2 + 3)) as u32;
        let mut expected: Vec<[u8; 8]> = jumbled(count).as_chunks::<8>().0.to_vec();
        storage.write("in", &jumbled(count)).unwrap();
        sort(&storage, "in", "out", 8, "tmp").unwrap();
        expected.sort_unstable();
        let mut records = Vec::new();
        each(&storage, "out", 8, |r| {
            records.push(r.to_vec());
            Ok(())
        })
        .unwrap();
        assert_eq!(
            records,
            expected.iter().map(|r| r.to_vec()).collect::<Vec<_>>()
        );
        for i in 0..64 {
            assert!(!storage.exists(&format!("tmp.{i}")), "tmp.{i} is left");
        }
    }

    /// Runs are written to a card that fills up: the error comes back and
    /// no run is left behind.
    #[test]
    fn runs_go_after_an_error() {
        let storage = crate::testing::Faulty::new(MemoryStorage::new());
        storage.inner.write("in", &jumbled(10_000)).unwrap();
        storage.fill_after(3);
        assert_eq!(
            sort(&storage, "in", "out", 8, "tmp"),
            Err(StorageError::Full)
        );
        for i in 0..8 {
            assert!(!storage.exists(&format!("tmp.{i}")), "tmp.{i} is left");
        }
    }

    #[test]
    fn records_are_sorted_in_place() {
        let mut data = jumbled(1000);
        let mut expected: Vec<[u8; 8]> = data.as_chunks::<8>().0.to_vec();
        expected.sort_unstable();
        sort_records(&mut data, 8);
        assert_eq!(data, expected.concat());
        // Odd sizes, one record and none.
        let mut three = [5, 0, 1, 2, 9, 9, 1, 1, 1];
        sort_records(&mut three, 3);
        assert_eq!(three, [1, 1, 1, 2, 9, 9, 5, 0, 1]);
        sort_records(&mut three[..3], 3);
        sort_records(&mut [], 3);
    }
}
