//! Any bytes as a FictionBook (FB2) book, with and without its styles: open,
//! read the first sections and the table of contents.

#![no_main]

use libfuzzer_sys::fuzz_target;
use paperpaw_epub_family::Fb2Document;
use paperpaw_fuzz::read_document;
use paperpaw_sdk::MemoryStorage;

fuzz_target!(|data: &[u8]| {
    let Some((&flags, data)) = data.split_first() else {
        return;
    };
    let storage = MemoryStorage::new().with("book.fb2", data.to_vec());
    if let Ok(doc) = Fb2Document::open(&storage, "book.fb2", flags & 1 != 0) {
        read_document(&doc, &storage);
    }
});
