//! Plugins ordered by a key, more of them than memory holds at once (slice 4,
//! host-free): sorted in runs on the card, then merged. The interface-6 rules
//! of `catalog/runs.rs`, made fallible: memory refused is `NoMemory`, a card
//! that does not answer ends the merge, and nothing indexes in a way that can
//! panic.
use alloc::vec::Vec;
use paperpaw_sdk::Storage;
use paperpaw_sdk::native::Error;

/// A plugin with the key it is ordered by: (key, its place in the index).
pub type Keyed = (u64, u32);

/// The bytes of a [`Keyed`] on the card.
pub const KEYED: usize = 12;

/// Plugins ordered by a key in memory at once (16 bytes each): longer lists
/// are sorted in runs of this many on the card.
pub const RUN: usize = 512;

/// Entries of a run read at once while runs are merged.
const MERGE_READ: usize = 16;

/// The index and the hits are written in pieces of about this many bytes.
pub const WRITE_AT: usize = 2048;

/// Appends `bytes` to `to`, asking for the room first.
///
/// # Errors
/// `NoMemory`.
pub fn put(to: &mut Vec<u8>, bytes: &[u8]) -> Result<(), Error> {
    to.try_reserve(bytes.len()).map_err(|_| Error::NoMemory)?;
    to.extend_from_slice(bytes);
    Ok(())
}

/// The order of plugins by a key: the highest (newest) first; of one key, in
/// the list's order.
#[must_use]
pub fn order_of(a: Keyed, b: Keyed) -> core::cmp::Ordering {
    b.0.cmp(&a.0).then(a.1.cmp(&b.1))
}

#[must_use]
pub fn before(a: Keyed, b: Keyed) -> bool {
    order_of(a, b) == core::cmp::Ordering::Less
}

/// Where a sorted run on the card is read while merging.
#[derive(Debug)]
pub struct Cursor {
    /// The byte of the next entry not read yet, and how many are left.
    at: u64,
    left: u32,
    read: [Keyed; MERGE_READ],
    /// The next of those read, and how many were.
    next: usize,
    len: usize,
}

impl Cursor {
    #[must_use]
    pub fn new(at: u64, left: u32) -> Self {
        Self {
            at,
            left,
            read: [(0, 0); MERGE_READ],
            next: 0,
            len: 0,
        }
    }

    /// The run's first entry not taken yet; `None` at its end or where the
    /// card does not answer.
    pub fn head(&mut self, storage: &dyn Storage, path: &str) -> Option<Keyed> {
        if self.next == self.len {
            let want = usize::try_from(self.left).map_or(MERGE_READ, |l| l.min(MERGE_READ));
            if want == 0 {
                return None;
            }
            let mut bytes = [0u8; MERGE_READ * KEYED];
            let mut got = 0;
            while got < want * KEYED {
                let part = bytes.get_mut(got..want * KEYED)?;
                match storage.read_at(path, self.at + got as u64, part) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => got += n,
                }
            }
            let entries = got / KEYED;
            let filled = bytes.get(..entries * KEYED)?;
            let (chunks, _) = filled.as_chunks::<KEYED>();
            for (slot, chunk) in self.read.iter_mut().zip(chunks) {
                let mut key = [0u8; 8];
                let mut n = [0u8; 4];
                key.copy_from_slice(chunk.get(..8)?);
                n.copy_from_slice(chunk.get(8..)?);
                *slot = (u64::from_le_bytes(key), u32::from_le_bytes(n));
            }
            self.at += (entries * KEYED) as u64;
            self.left = if entries == want {
                self.left - u32::try_from(want).unwrap_or(0)
            } else {
                0
            };
            self.next = 0;
            self.len = entries;
            if entries == 0 {
                return None;
            }
        }
        self.read.get(self.next).copied()
    }

    pub fn next(&mut self) {
        self.next += 1;
    }
}

/// The plugins picked, written to the card a piece at a time.
pub struct Output<'a> {
    storage: &'a dyn Storage,
    path: &'a str,
    bytes: Vec<u8>,
    pub count: usize,
    written: bool,
}

impl core::fmt::Debug for Output<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Output")
            .field("path", &self.path)
            .field("count", &self.count)
            .finish_non_exhaustive()
    }
}

impl<'a> Output<'a> {
    /// Starts the file at `path` empty.
    #[must_use]
    pub fn new(storage: &'a dyn Storage, path: &'a str) -> Self {
        Self {
            written: storage.write(path, b"").is_ok(),
            storage,
            path,
            bytes: Vec::new(),
            count: 0,
        }
    }

    /// Notes plugin `n` as picked.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn push(&mut self, n: u32) -> Result<(), Error> {
        put(&mut self.bytes, &n.to_le_bytes())?;
        self.count += 1;
        if self.bytes.len() >= WRITE_AT {
            self.written &= self.storage.append(self.path, &self.bytes).is_ok();
            self.bytes.clear();
        }
        Ok(())
    }

    /// How many were picked; none where they could not be kept.
    #[must_use]
    pub fn finish(mut self) -> usize {
        self.written &= self.storage.append(self.path, &self.bytes).is_ok();
        if self.written { self.count } else { 0 }
    }
}
