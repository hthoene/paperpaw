//! Plain text in every shape it comes in: UTF-8 with and without BOM,
//! Windows-1252, invalid UTF-8, UTF-16 (from its UTF-8 copy), line ends of
//! every system, very long lines, empty files, Project Gutenberg markers
//! in odd places. The text must come through, offsets must point back
//! into the file.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_plugin_format_txt::TxtDocument;
use paperpaw_plugin_format_txt::preparation::Preparation;
use paperpaw_sdk::document::{Block, Document, Locator};
use paperpaw_sdk::native::Error;
use paperpaw_sdk::{MemoryStorage, Storage};

const COPY: &str = "books/text.utf8";

/// The book as the provider opens it: a UTF-16 file is read from its UTF-8
/// copy, made first the way the host's preparation calls make it.
fn open(data: impl Into<Vec<u8>>) -> (MemoryStorage, TxtDocument) {
    let storage = MemoryStorage::new().with("books/text.txt", data);
    let mut bom = [0u8; 2];
    storage.read_at("books/text.txt", 0, &mut bom).unwrap();
    if matches!(bom, [0xff, 0xfe] | [0xfe, 0xff]) {
        let size = storage.size("books/text.txt").unwrap();
        let mut preparation = Preparation::new(size, bom == [0xfe, 0xff]).unwrap();
        while !preparation
            .advance(
                |offset, buf| {
                    storage
                        .read_at("books/text.txt", offset, buf)
                        .map_err(|_| Error::Io)
                },
                |first, bytes| {
                    let written = if first {
                        storage.write(COPY, bytes)
                    } else {
                        storage.append(COPY, bytes)
                    };
                    written.map_err(|_| Error::Io)
                },
            )
            .unwrap()
        {}
    }
    let doc = TxtDocument::open_converted(&storage, "books/text.txt", COPY).unwrap();
    (storage, doc)
}

fn blocks(doc: &TxtDocument, storage: &MemoryStorage) -> Vec<Block> {
    let mut at = Locator::START;
    let mut out = Vec::new();
    while let Some(block) = doc.block_at(storage, at).unwrap() {
        assert!(block.end > at.offset, "no progress at {at:?}");
        at = Locator::new(0, block.end);
        out.push(block);
        assert!(out.len() < 1_000_000);
    }
    out
}

fn words(doc: &TxtDocument, storage: &MemoryStorage) -> Vec<String> {
    blocks(doc, storage)
        .iter()
        .flat_map(|b| {
            b.text
                .split_whitespace()
                .map(String::from)
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Checks that each word's source offset holds the word's first byte
/// in `file` (for text that is UTF-8 in the file).
fn offsets_point_back(blocks: &[Block], file: &[u8]) {
    for block in blocks {
        for (i, c) in block.text.char_indices() {
            if c.is_whitespace() || c == '\u{fffd}' {
                continue;
            }
            let at = block.source_offset(i) as usize;
            let mut buf = [0u8; 4];
            let bytes = c.encode_utf8(&mut buf).as_bytes();
            assert_eq!(&file[at..at + bytes.len()], bytes, "{c:?} at {at}");
        }
    }
}

#[test]
fn utf8_with_and_without_bom() {
    for bom in [&b""[..], b"\xef\xbb\xbf"] {
        let mut file = bom.to_vec();
        file.extend_from_slice("Grüße aus Köln\n\nZweiter Absatz \u{2013} mit Strich\n".as_bytes());
        let (storage, doc) = open(file.clone());
        let read = blocks(&doc, &storage);
        let texts: Vec<&str> = read.iter().map(|b| b.text.as_str()).collect();
        assert_eq!(texts[0], "Grüße aus Köln");
        assert!(!texts.iter().any(|t| t.contains('\u{feff}')));
        assert!(read[0].start >= bom.len() as u32);
        offsets_point_back(&read, &file);
        assert_eq!(doc.progress(Locator::new(0, read[0].start)), 0.0);
    }
}

#[test]
fn a_bom_keeps_utf8_even_with_invalid_bytes() {
    // A byte that starts no character is read as Windows-1252; the UTF-8
    // around it stays UTF-8.
    let (storage, doc) = open(b"\xef\xbb\xbfGr\xf6\xdfe \xc3\xbc \xff ok\n".to_vec());
    let words = words(&doc, &storage);
    assert_eq!(words, ["Größe", "ü", "ÿ", "ok"]);
}

#[test]
fn invalid_utf8_at_the_start_means_windows_1252() {
    let (storage, doc) = open(b"Caf\xe9 \x93quoted\x94 \x80 \x81\x8d\x8f\x90\x9d end\n".to_vec());
    let words = words(&doc, &storage);
    assert_eq!(
        words,
        [
            "Café",
            "“quoted”",
            "€",
            "\u{fffd}\u{fffd}\u{fffd}\u{fffd}\u{fffd}",
            "end"
        ]
    );
}

#[test]
fn invalid_utf8_after_the_sniffed_start_is_read_as_windows_1252() {
    // 16 KB of valid UTF-8 decide the encoding; a bad byte later is one
    // Windows-1252 letter, not a switch of encoding.
    let mut file = "ä ".repeat(10_000).into_bytes();
    file.extend_from_slice(b"bad \xff byte\n");
    let (storage, doc) = open(file);
    let words = words(&doc, &storage);
    assert_eq!(words[words.len() - 3..], ["bad", "ÿ", "byte"]);
    assert_eq!(words.iter().filter(|w| *w == "ä").count(), 10_000);
}

#[test]
fn a_character_cut_by_the_sniffed_start_is_still_utf8() {
    // The first 16 KB end inside a two-byte character.
    let mut file = "x".repeat(16 * 1024 - 1).into_bytes();
    file.extend_from_slice("é and more\n".as_bytes());
    let (storage, doc) = open(file.clone());
    let read = blocks(&doc, &storage);
    let shown: String = read
        .iter()
        .flat_map(|b| b.text.chars())
        .filter(|c| !c.is_whitespace())
        .collect();
    let expected: String = String::from_utf8(file.clone())
        .unwrap()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    assert_eq!(shown, expected);
    offsets_point_back(&read, &file);
}

#[test]
fn utf16_is_read_from_its_converted_copy() {
    // A UTF-16 file is read from the UTF-8 copy the host's preparation made.
    for little in [true, false] {
        let mut file = if little {
            vec![0xff, 0xfe]
        } else {
            vec![0xfe, 0xff]
        };
        for unit in "Hello\nWorld\n".encode_utf16() {
            file.extend_from_slice(&if little {
                unit.to_le_bytes()
            } else {
                unit.to_be_bytes()
            });
        }
        let (storage, doc) = open(file);
        let text: String = blocks(&doc, &storage)
            .iter()
            .map(|b| b.text.clone())
            .collect();
        let letters: String = text.chars().filter(char::is_ascii_alphabetic).collect();
        assert_eq!(letters, "HelloWorld");
    }
}

#[test]
fn windows_line_ends_are_line_ends() {
    let file = b"First line\r\nsecond line\r\n\r\nNext paragraph\r\n\r\n";
    let (storage, doc) = open(file.to_vec());
    let read = blocks(&doc, &storage);
    let texts: Vec<&str> = read.iter().map(|b| b.text.as_str()).collect();
    assert_eq!(texts, ["First line second line", "Next paragraph"]);
    offsets_point_back(&read, file);
}

#[test]
fn old_mac_line_ends_part_words() {
    let (storage, doc) = open(b"First line\rsecond line\r\rNext\r".to_vec());
    let words = words(&doc, &storage);
    assert_eq!(words, ["First", "line", "second", "line", "Next"]);
}

#[test]
fn very_long_lines_are_split_into_blocks_and_keep_every_word() {
    let line: Vec<String> = (0..120_000).map(|i| format!("w{i}")).collect();
    let file = line.join(" ");
    let (storage, doc) = open(file.clone());
    let read = blocks(&doc, &storage);
    assert!(read.len() > 100);
    assert!(read.iter().all(|b| b.text.len() <= 4096 + 256));
    let words: Vec<&str> = read
        .iter()
        .flat_map(|b| b.text.split_whitespace())
        .collect();
    assert_eq!(words, line.iter().map(String::as_str).collect::<Vec<_>>());
    offsets_point_back(&read, file.as_bytes());
}

#[test]
fn very_long_lines_without_spaces_split_between_characters() {
    let file = "€ü".repeat(200_000);
    let (storage, doc) = open(file.clone());
    let read = blocks(&doc, &storage);
    let all: String = read.iter().map(|b| b.text.as_str()).collect();
    assert_eq!(all, file);
    assert!(read.iter().all(|b| !b.text.contains('\u{fffd}')));
    offsets_point_back(&read, file.as_bytes());
}

#[test]
fn empty_and_blank_files_have_no_blocks() {
    for file in [&b""[..], b"\xef\xbb\xbf", b"\n\n\n", b"   \t  \r\n  "] {
        let (storage, doc) = open(file.to_vec());
        assert_eq!(
            doc.block_at(&storage, Locator::START).unwrap(),
            None,
            "{file:?}"
        );
        assert_eq!(doc.metadata().title.as_deref(), Some("text"));
        assert!(doc.progress(Locator::START) <= 1.0);
    }
}

#[test]
fn control_characters_become_spaces() {
    let (storage, doc) = open(b"a\x00b\x01c\x1bd\x7fe\tf\x0cg\n".to_vec());
    assert_eq!(words(&doc, &storage), ["a", "b", "c", "d", "e", "f", "g"]);
}

#[test]
fn gutenberg_markers_in_odd_places() {
    // An end marker before the start one, a start marker at the very end,
    // markers in the text: never a panic, never text outside the file.
    for file in [
        "*** END OF THE BOOK ***\ntext\n*** START OF THE BOOK ***".to_string(),
        "Title: T\n*** START OF THE BOOK ***".to_string(),
        "*** START OF X ***\n\n*** START OF Y ***\nbody\n*** END OF Y ***\n*** END OF X ***\n"
            .to_string(),
        format!(
            "*** START OF X ***\n{}\n*** END OF X ***\nlicence\n",
            "word ".repeat(30_000)
        ),
        format!("{}*** END OF X ***", "a".repeat(9000)),
    ] {
        let (storage, doc) = open(file.clone());
        let read = blocks(&doc, &storage);
        assert!(read.iter().all(|b| b.end as usize <= file.len()));
        assert!(
            !read.iter().any(|b| b.text.contains("licence")),
            "{:.40}",
            file
        );
        assert!(doc.progress(Locator::new(0, file.len() as u32)) <= 1.0);
    }
}

#[test]
fn positions_past_the_end_find_nothing() {
    let (storage, doc) = open("one\n\ntwo\n");
    assert_eq!(
        doc.block_at(&storage, Locator::new(0, u32::MAX)).unwrap(),
        None
    );
    assert_eq!(doc.block_at(&storage, Locator::new(1, 0)).unwrap(), None);
    assert_eq!(doc.progress(Locator::new(0, u32::MAX)), 1.0);
}
