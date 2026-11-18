//! Any bytes as an RTF document (as they are, and after a valid head so the
//! parser is reached at once), converted and read block by block.

#![no_main]

use libfuzzer_sys::fuzz_target;
use paperpaw_fuzz::read_document;
use paperpaw_plugin_format_rtf::RtfDocument;
use paperpaw_sdk::{MemoryStorage, OpenOptions};

fn read(bytes: Vec<u8>) {
    let storage = MemoryStorage::new().with("book.rtf", bytes);
    let options = OpenOptions::default();
    if let Ok(doc) = RtfDocument::open_converted(&storage, "book.rtf", ".cache/book", &options) {
        read_document(&doc, &storage);
    }
}

fuzz_target!(|data: &[u8]| {
    read(data.to_vec());
    let mut headed = b"{\\rtf1\\ansi ".to_vec();
    headed.extend_from_slice(data);
    read(headed);
});
