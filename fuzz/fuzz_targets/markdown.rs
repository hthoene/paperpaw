//! Any bytes as a Markdown file book, with and without its styles: open,
//! read the first sections and the table of contents.

#![no_main]

use libfuzzer_sys::fuzz_target;
use paperpaw_epub_family::MarkdownDocument;
use paperpaw_fuzz::read_document;
use paperpaw_sdk::MemoryStorage;

fuzz_target!(|data: &[u8]| {
    let Some((&flags, data)) = data.split_first() else {
        return;
    };
    let storage = MemoryStorage::new().with("book.md", data.to_vec());
    if let Ok(doc) = MarkdownDocument::open(&storage, "book.md", flags & 1 != 0) {
        read_document(&doc, &storage);
    }
});
