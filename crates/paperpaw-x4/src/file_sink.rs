//! A download's file as the firmware's HTTP client writes it (`http.rs`,
//! #216): pieces gathered into chunks, one append per chunk, and no sync
//! until the file is whole.
//!
//! The body goes to a `.part` file beside the target; the target is
//! replaced only once the body is whole (the part synced once, the old file
//! removed, the part renamed), so a cut at any moment leaves the old file or
//! the new one, never half of it. The first chunk is an append to a part
//! that was removed first, not a whole-file write (which the card's storage
//! makes through a temporary file and a sync of its own for each file).
//! Core's staged file (`staged`) exists already: only appended to, and
//! renamed by core.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_platform::Storage;

/// Why a piece could not be taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SinkError {
    /// The card refused a write.
    Write,
    /// No memory for the chunk's buffer, not even the base one.
    Memory,
}

impl SinkError {
    pub fn reason(self) -> &'static str {
        match self {
            SinkError::Write => "could not write the file",
            SinkError::Memory => "not enough memory to write the file",
        }
    }
}

/// The part file being written.
#[derive(Debug)]
pub struct FileSink {
    part: String,
    buf: Vec<u8>,
    chunk: usize,
    /// The part exists (written to, or core's staged file).
    started: bool,
    staged: bool,
}

impl FileSink {
    /// A sink for `part`, in chunks of `chunk` bytes (at least the base
    /// chunk); `staged` for core's file, which exists.
    pub fn new(part: String, staged: bool, chunk: usize) -> Self {
        Self {
            part,
            buf: Vec::new(),
            chunk: chunk.max(crate::download_boost::MIN_CHUNK),
            started: staged,
            staged,
        }
    }

    /// A larger (or the base) chunk, before anything was taken.
    pub fn set_chunk(&mut self, chunk: usize) {
        if self.buf.is_empty() && !self.started {
            self.chunk = chunk.max(crate::download_boost::MIN_CHUNK);
        }
    }

    /// Whether anything was written to the part (so that it is removed
    /// where the download fails).
    pub fn started(&self) -> bool {
        self.started
    }

    pub fn part(&self) -> &str {
        &self.part
    }

    /// Writes `bytes` to the part's end; a first write starts it new.
    fn write(&mut self, storage: &dyn Storage, bytes: &[u8]) -> Result<(), SinkError> {
        if !self.started {
            // A part an earlier cut or failure left: not appended to.
            let _ = storage.remove(&self.part);
            self.started = true;
        }
        storage
            .append(&self.part, bytes)
            .map_err(|_| SinkError::Write)
    }

    /// Takes a piece: gathered in the buffer, written in one append each
    /// time it holds a whole chunk. Every append but the last is a chunk, a
    /// multiple of the card's block: the FAT layer then writes whole blocks
    /// in one command, where a piece of any length would leave a part of a
    /// block to read and write again with each append (#419).
    pub fn push(&mut self, storage: &dyn Storage, mut piece: &[u8]) -> Result<(), SinkError> {
        self.reserve()?;
        while !piece.is_empty() {
            let take = (self.chunk - self.buf.len()).min(piece.len());
            self.buf.extend_from_slice(&piece[..take]);
            piece = &piece[take..];
            if self.buf.len() == self.chunk {
                let buf = core::mem::take(&mut self.buf);
                let written = self.write(storage, &buf);
                // The buffer stays for the next chunk.
                self.buf = buf;
                self.buf.clear();
                written?;
            }
        }
        Ok(())
    }

    /// Room for a chunk in the buffer: the chunk asked for, or the smallest
    /// where the heap has no block for it.
    fn reserve(&mut self) -> Result<(), SinkError> {
        if self.buf.capacity() >= self.chunk {
            return Ok(());
        }
        let missing = self.chunk - self.buf.len();
        if self.buf.try_reserve_exact(missing).is_ok() {
            return Ok(());
        }
        self.chunk = crate::download_boost::MIN_CHUNK;
        if self.buf.capacity() >= self.chunk || self.buf.len() > self.chunk {
            return Ok(());
        }
        self.buf
            .try_reserve_exact(self.chunk - self.buf.len())
            .map_err(|_| SinkError::Memory)
    }

    /// The body is whole. Core's staged file: what is gathered is appended
    /// (core renames it). Otherwise the part is synced once, and the target
    /// replaced by it; on failure the part is removed.
    pub fn finish(mut self, storage: &dyn Storage, target: &str) -> Result<(), SinkError> {
        let buf = core::mem::take(&mut self.buf);
        if self.staged {
            return if buf.is_empty() {
                Ok(())
            } else {
                storage
                    .append(&self.part, &buf)
                    .map_err(|_| SinkError::Write)
            };
        }
        let replaced = (if buf.is_empty() && self.started {
            Ok(())
        } else {
            self.write(storage, &buf)
        })
        .and_then(|()| {
            storage.sync().map_err(|_| SinkError::Write)?;
            let _ = storage.remove(target);
            storage
                .rename(&self.part, target)
                .map_err(|_| SinkError::Write)
        });
        if replaced.is_err() {
            let _ = storage.remove(&self.part);
        }
        replaced
    }

    /// The download failed: what was written is removed (core's staged file
    /// is core's to remove).
    pub fn discard(&self, storage: &dyn Storage) {
        if self.started && !self.staged {
            let _ = storage.remove(&self.part);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::cell::Cell;
    use paperpaw_platform::{DirEntry, StorageError};
    use paperpaw_sdk::MemoryStorage;

    /// A card that counts what a download asks of it.
    #[derive(Default)]
    struct Counting {
        card: MemoryStorage,
        writes: Cell<u32>,
        appends: Cell<u32>,
        sizes: core::cell::RefCell<Vec<usize>>,
        syncs: Cell<u32>,
        fail_appends: Cell<bool>,
    }

    impl Storage for Counting {
        fn read_at(&self, p: &str, o: u64, b: &mut [u8]) -> Result<usize, StorageError> {
            self.card.read_at(p, o, b)
        }
        fn size(&self, p: &str) -> Result<u64, StorageError> {
            self.card.size(p)
        }
        fn write(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
            self.writes.set(self.writes.get() + 1);
            self.card.write(p, d)
        }
        fn append(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
            if self.fail_appends.get() {
                return Err(StorageError::Io);
            }
            self.appends.set(self.appends.get() + 1);
            self.sizes.borrow_mut().push(d.len());
            self.card.append(p, d)
        }
        fn sync(&self) -> Result<(), StorageError> {
            self.syncs.set(self.syncs.get() + 1);
            self.card.sync()
        }
        fn list(&self, d: &str, v: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
            self.card.list(d, v)
        }
        fn remove(&self, p: &str) -> Result<(), StorageError> {
            self.card.remove(p)
        }
        fn rename(&self, a: &str, b: &str) -> Result<(), StorageError> {
            self.card.rename(a, b)
        }
    }

    fn read(card: &MemoryStorage, path: &str) -> Vec<u8> {
        paperpaw_sdk::read_file(card, path, 1 << 24).unwrap()
    }

    fn body(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i % 251) as u8).collect()
    }

    fn download(card: &Counting, chunk: usize, piece: usize, len: usize) -> Vec<u8> {
        let data = body(len);
        let mut sink = FileSink::new("a.bin.part".into(), false, chunk);
        for p in data.chunks(piece) {
            sink.push(card, p).unwrap();
        }
        sink.finish(card, "a.bin").unwrap();
        data
    }

    #[test]
    fn a_file_is_one_sync_and_a_write_per_chunk_not_per_piece() {
        let card = Counting::default();
        let data = download(&card, 16 * 1024, 1024, 200 * 1024);
        assert_eq!(read(&card.card, "a.bin"), data);
        assert!(!card.card.exists("a.bin.part"));
        // 200 KiB in chunks of 16 KiB: 13 appends (12 whole, one rest).
        assert_eq!(card.appends.get(), 13);
        assert_eq!(card.syncs.get(), 1);
        // The first chunk is no whole-file write (a temporary file, a sync).
        assert_eq!(card.writes.get(), 0);
    }

    #[test]
    fn every_append_but_the_last_is_a_whole_chunk_whatever_the_pieces() {
        // Pieces as a socket hands them out: of any length. The card's
        // blocks are 512 bytes; an append that ends inside one is read and
        // written again by the next (#419).
        for piece in [1, 511, 1460, 3000, 8191, 8192, 9000, 20_000] {
            let card = Counting::default();
            let data = download(&card, 8192, piece, 100_001);
            assert_eq!(read(&card.card, "a.bin"), data);
            let sizes = card.sizes.borrow();
            let (last, whole) = sizes.split_last().unwrap();
            assert!(whole.iter().all(|&n| n == 8192), "piece {piece}: {sizes:?}");
            assert_eq!(*last, 100_001 % 8192);
        }
    }

    #[test]
    fn small_chunks_write_more_but_still_sync_once() {
        let card = Counting::default();
        download(&card, 4096, 1024, 200 * 1024);
        assert_eq!(card.appends.get(), 50);
        assert_eq!(card.syncs.get(), 1);
    }

    #[test]
    fn the_old_file_stays_until_the_new_one_is_whole() {
        let card = Counting::default();
        card.card.write("a.bin", b"old").unwrap();
        let mut sink = FileSink::new("a.bin.part".into(), false, 4096);
        sink.push(&card, &body(10_000)).unwrap();
        assert_eq!(read(&card.card, "a.bin"), b"old");
        sink.finish(&card, "a.bin").unwrap();
        assert_eq!(read(&card.card, "a.bin"), body(10_000));
    }

    #[test]
    fn a_part_an_earlier_cut_left_is_not_appended_to() {
        let card = Counting::default();
        card.card.write("a.bin.part", b"stale bytes").unwrap();
        let data = download(&card, 4096, 1024, 5000);
        assert_eq!(read(&card.card, "a.bin"), data);
    }

    #[test]
    fn a_failed_write_removes_the_part_and_keeps_the_old_file() {
        let card = Counting::default();
        card.card.write("a.bin", b"old").unwrap();
        let mut sink = FileSink::new("a.bin.part".into(), false, 4096);
        sink.push(&card, &body(3000)).unwrap();
        card.fail_appends.set(true);
        assert_eq!(sink.finish(&card, "a.bin"), Err(SinkError::Write));
        assert!(!card.card.exists("a.bin.part"));
        assert_eq!(read(&card.card, "a.bin"), b"old");
    }

    #[test]
    fn a_staged_file_is_only_appended_to() {
        let card = Counting::default();
        card.card.write("stage/x", b"").unwrap();
        let mut sink = FileSink::new("stage/x".into(), true, 4096);
        sink.push(&card, &body(6000)).unwrap();
        sink.finish(&card, "ignored").unwrap();
        assert_eq!(read(&card.card, "stage/x"), body(6000));
        assert_eq!(card.syncs.get(), 0);
        assert_eq!(card.writes.get(), 0);
    }
}
