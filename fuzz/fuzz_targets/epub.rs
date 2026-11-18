//! Any bytes as an EPUB: open, read the first chapters and the table of
//! contents.

#![no_main]

use libfuzzer_sys::fuzz_target;
use paperpaw_epub_family::EpubDocument;
use paperpaw_fuzz::read_document;
use paperpaw_sdk::MemoryStorage;

fuzz_target!(|data: &[u8]| {
    let storage = MemoryStorage::new().with("book.epub", data.to_vec());
    if let Ok(doc) = EpubDocument::open(&storage, "book.epub") {
        read_document(&doc, &storage);
    }
});
