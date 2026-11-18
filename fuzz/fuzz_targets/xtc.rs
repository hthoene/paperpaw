//! Any bytes as an XTC book of prerendered pages: open, read the pages
//! and the table of contents.

#![no_main]

use libfuzzer_sys::fuzz_target;
use paperpaw_fuzz::read_document;
use paperpaw_plugin_format_xtc::XtcDocument;
use paperpaw_sdk::{DirEntry, MemoryStorage, Storage, StorageError};

/// Root 3 (the book, path `""`) and root 6 (any other path), as the host's
/// provider calls have them.
struct Roots(MemoryStorage);

impl Roots {
    fn map(path: &str) -> String {
        if path.is_empty() {
            String::from("book")
        } else {
            format!("cache/{path}")
        }
    }
}

impl Storage for Roots {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.0.read_at(&Self::map(path), offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.0.size(&Self::map(path))
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        if path.is_empty() {
            return Err(StorageError::Unsupported);
        }
        self.0.write(&Self::map(path), data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        if path.is_empty() {
            return Err(StorageError::Unsupported);
        }
        self.0.append(&Self::map(path), data)
    }
    fn list(&self, _: &str, _: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.0.remove(&Self::map(path))
    }
}

fuzz_target!(|data: &[u8]| {
    let storage = Roots(MemoryStorage::new().with("book", data.to_vec()));
    if let Ok(doc) = XtcDocument::open_native(&storage) {
        read_document(&doc, &storage);
    }
});
