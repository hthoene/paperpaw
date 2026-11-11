//! Any bytes as a comic book archive (CBZ): open, read the pages and the
//! table of contents.

#![no_main]

use libfuzzer_sys::fuzz_target;
use paperpaw_epub_family::CbzDocument;
use paperpaw_fuzz::read_document;
use paperpaw_sdk::MemoryStorage;

fuzz_target!(|data: &[u8]| {
    let storage = MemoryStorage::new().with("book.cbz", data.to_vec());
    if let Ok(doc) = CbzDocument::open(&storage, "book.cbz") {
        read_document(&doc, &storage);
    }
});
