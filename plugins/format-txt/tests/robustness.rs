//! Any bytes are a plain text file of some sort: random data in various
//! shapes must be read block by block without panics or endless loops.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_plugin_format_txt::TxtDocument;
use paperpaw_sdk::document::{Document, Locator};
use paperpaw_sdk::{DirEntry, MemoryStorage, Storage, StorageError};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

const PIECES: [&[u8]; 10] = [
    b"\r\n",
    b"\n\n",
    b"\xef\xbb\xbf",
    b"\xff\xfe",
    "Größe ".as_bytes(),
    b"\xe4\xf6\xfc ",
    b"\t",
    b"CHAPTER I\n\n",
    b"\x00",
    b"\xc3",
];

#[test]
fn any_bytes_read_cleanly() {
    for seed in 1..=300u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15));
        let mut data = Vec::new();
        for _ in 0..rng.below(3000) {
            match rng.below(3) {
                0 => data.push(rng.next() as u8),
                1 => data.extend(std::iter::repeat_n(b'w', rng.below(9000))),
                _ => data.extend_from_slice(PIECES[rng.below(PIECES.len())]),
            }
        }
        let storage = MemoryStorage::new().with("book.txt", data);
        let Ok(doc) = TxtDocument::open_converted(&storage, "book.txt", "") else {
            continue;
        };
        let mut at = Locator::new(0, 0);
        let mut blocks = 0;
        while let Ok(Some(block)) = doc.block_at(&storage, at) {
            assert!(block.end > at.offset, "seed {seed}: no progress at {at:?}");
            let _ = (block.source_offset(block.text.len() / 2), doc.progress(at));
            at = Locator::new(0, block.end);
            blocks += 1;
            assert!(blocks < 100_000, "seed {seed}: too many blocks");
        }
    }
}

/// A file of almost 4 GiB, made up as it is read.
struct Huge;

impl Huge {
    const SIZE: u64 = u32::MAX as u64;
    const TEXT: &[u8] = b"Some words in a line.\n\n";
}

impl Storage for Huge {
    fn read_at(&self, _: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        let n = buf.len().min(Self::SIZE.saturating_sub(offset) as usize);
        for (i, byte) in buf[..n].iter_mut().enumerate() {
            *byte = Self::TEXT[((offset + i as u64) % Self::TEXT.len() as u64) as usize];
        }
        Ok(n)
    }

    fn size(&self, _: &str) -> Result<u64, StorageError> {
        Ok(Self::SIZE)
    }

    fn write(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }

    fn append(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }

    fn list(&self, _: &str, _: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }

    fn remove(&self, _: &str) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }
}

#[test]
fn files_of_almost_4_gib_read_to_the_end() {
    let doc = TxtDocument::open_converted(&Huge, "big.txt", "").unwrap();
    let mut at = Locator::new(0, u32::MAX - 10_000);
    let mut texts = Vec::new();
    while let Some(block) = doc.block_at(&Huge, at).unwrap() {
        assert!(block.end > at.offset, "no progress at {at:?}");
        at = Locator::new(0, block.end);
        texts.push(block.text);
    }
    assert!(texts.len() > 400, "{} blocks", texts.len());
    // The file ends inside the last line.
    let last = texts.pop().unwrap();
    assert!("Some words in a line.".starts_with(&last), "{last:?}");
    assert!(texts.iter().all(|t| t == "Some words in a line."));
}
