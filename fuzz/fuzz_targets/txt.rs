//! Any bytes as a plain text book, read block by block: a UTF-16 file is
//! converted first the way the host's preparation calls do it, then the book
//! is opened from the copy, as the provider opens it.

#![no_main]

use libfuzzer_sys::fuzz_target;
use paperpaw_fuzz::read_document;
use paperpaw_plugin_format_txt::TxtDocument;
use paperpaw_plugin_format_txt::preparation::Preparation;
use paperpaw_sdk::native::Error;
use paperpaw_sdk::{MemoryStorage, Storage};

fuzz_target!(|data: &[u8]| {
    let storage = MemoryStorage::new().with("book.txt", data.to_vec());
    let mut copy = "";
    if let Some(&[first, second]) = data.first_chunk::<2>() {
        if [first, second] == [0xff, 0xfe] || [first, second] == [0xfe, 0xff] {
            let Ok(mut preparation) = Preparation::new(data.len() as u64, first == 0xfe) else {
                return;
            };
            loop {
                let step = preparation.advance(
                    |offset, buf| {
                        storage
                            .read_at("book.txt", offset, buf)
                            .map_err(|_| Error::Io)
                    },
                    |first, bytes| {
                        let written = if first {
                            storage.write("book.utf8", bytes)
                        } else {
                            storage.append("book.utf8", bytes)
                        };
                        written.map_err(|_| Error::Io)
                    },
                );
                match step {
                    Ok(true) => break,
                    Ok(false) => {}
                    Err(_) => return,
                }
            }
            copy = "book.utf8";
        }
    }
    if let Ok(doc) = TxtDocument::open_converted(&storage, "book.txt", copy) {
        read_document(&doc, &storage);
    }
});
