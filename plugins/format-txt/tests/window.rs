//! The window `TxtDocument` keeps between calls is a cache: a block found with the
//! window warm (one walk, block after block) is the block found cold (a new
//! document asked for that offset alone), for UTF-8 with multibyte letters and
//! CRLF line ends, UTF-8 with a byte order mark and Windows-1252.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_plugin_format_txt::TxtDocument;
use paperpaw_sdk::MemoryStorage;
use paperpaw_sdk::document::{Document, Locator};

fn blocks_of(book: Vec<u8>, warm: bool) -> Vec<(u32, u32, String)> {
    let storage = MemoryStorage::new().with("b.txt", book);
    let document = TxtDocument::open(&storage, "b.txt").unwrap();
    let mut found = Vec::new();
    let mut at = Locator::START;
    while let Some(block) = if warm {
        document.block_at(&storage, at).unwrap()
    } else {
        // Cold: a document of its own for this one offset.
        TxtDocument::open(&storage, "b.txt")
            .unwrap()
            .block_at(&storage, at)
            .unwrap()
    } {
        at = Locator::new(0, block.end);
        found.push((block.start, block.end, block.text.clone()));
        assert!(found.len() < 10_000);
    }
    found
}

/// `book` read warm and cold gives the same blocks, at least `least` of them.
fn check(book: Vec<u8>, least: usize) {
    let warm = blocks_of(book.clone(), true);
    assert!(warm.len() >= least, "{} blocks", warm.len());
    assert_eq!(warm, blocks_of(book, false));
}

#[test]
fn utf_8_with_multibyte_letters_and_crlf_reads_the_same_warm_and_cold() {
    let mut book = String::new();
    for i in 0..120 {
        book.push_str(&format!(
            "Kapitel {i}: Größe, café, „Zitat“ und Ärger.\r\nZweite Zeile ohne Ende, sehr lang genug.\r\n\r\n"
        ));
    }
    // One paragraph longer than a block, split on the grid.
    for i in 0..1500 {
        book.push_str(&format!("wörter{i} "));
    }
    book.push_str("\r\n\r\nEnde.\r\n");
    check(book.into_bytes(), 20);
}

#[test]
fn a_byte_order_mark_and_old_mac_line_ends_read_the_same_warm_and_cold() {
    // Line ends of old Macs are spaces: one long paragraph, split on the grid.
    let mut book = String::from("\u{feff}");
    for i in 0..600 {
        book.push_str(&format!("Zeile {i} mit Ümlaut\rnoch eine\r\r"));
    }
    check(book.into_bytes(), 4);
}

#[test]
fn windows_1252_reads_the_same_warm_and_cold() {
    let mut book = Vec::new();
    for _ in 0..100 {
        book.extend_from_slice(b"Gr\xf6\xdfe caf\xe9 \x93Zitat\x94\r\n\r\n");
    }
    check(book, 20);
}
