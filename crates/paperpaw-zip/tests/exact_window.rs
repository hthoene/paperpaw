//! A deflated entry whose window is its own size (up to 32 KiB) reads whole
//! at every size, also where the decoder meets the end of the stream only
//! after the window is full.

#![allow(
    clippy::unwrap_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_platform::{DirEntry, Storage, StorageError};
use paperpaw_zip::ZipArchive;
use paperpaw_zip::testing::ZipBuilder;
struct OneFile(Vec<u8>);
impl Storage for OneFile {
    fn read_at(&self, _: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        let start = (offset as usize).min(self.0.len());
        let n = buf.len().min(self.0.len() - start);
        buf[..n].copy_from_slice(&self.0[start..start + n]);
        Ok(n)
    }
    fn size(&self, _: &str) -> Result<u64, StorageError> {
        Ok(self.0.len() as u64)
    }
    fn list(&self, _: &str, _: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        Ok(())
    }
    fn write(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
        Err(StorageError::Io)
    }
    fn remove(&self, _: &str) -> Result<(), StorageError> {
        Err(StorageError::Io)
    }
    fn append(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
        Err(StorageError::Io)
    }
}
#[test]
fn a_window_filled_to_the_last_byte_is_not_too_long() {
    let words = [
        "pages",
        "Kafka",
        "it's",
        "€5",
        "Über",
        "reading",
        "x",
        "a",
        "and",
        "the",
        "<b>position</b>",
        "naïve",
    ];
    let mut bad = Vec::new();
    for seed in 0..3u64 {
        let mut s = seed * 7919 + 1;
        let mut text = String::new();
        while text.len() < 28400 {
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            text.push_str(words[(s >> 33) as usize % words.len()]);
            text.push(' ');
        }
        for len in (27500..28400).step_by(5) {
            if !text.is_char_boundary(len) {
                continue;
            }
            let t = text[..len].to_string();
            let storage = OneFile(ZipBuilder::new().deflated("a", t).build());
            let archive = ZipArchive::open(&storage, "x").unwrap();
            let entry = archive.entry(&storage, "a").unwrap().unwrap();
            let mut total = 0;
            let r = archive.stream(&storage, &entry, &mut |c| {
                total += c.len();
                true
            });
            if r.is_err() {
                bad.push((seed, len, format!("{r:?}")));
            }
        }
    }
    assert!(
        bad.is_empty(),
        "{:?} {}",
        &bad[..bad.len().min(10)],
        bad.len()
    );
}
