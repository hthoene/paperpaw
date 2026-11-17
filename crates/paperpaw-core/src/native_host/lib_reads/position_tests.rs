#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! Op 14 over a catalog on a card in memory: what is stored (the SDK's own
//! file, plus `insert` and `sync`), and what is answered where the book or the
//! card is not as it should be.
extern crate std;
use super::super::tests::card;
use super::*;
use crate::native_host::lib_service::{LibQuery, LibWrite};
use alloc::rc::Rc;
use paperpaw_sdk::document::Locator;
use std::format;

const A: BookRef<'static> = BookRef::Path("books/a.epub");

fn place(flags: u32, xpointer: &'static str) -> wire::ServiceLibPosition<'static> {
    wire::ServiceLibPosition {
        section: 7,
        offset: 1234,
        insert: 0,
        progress_ppm: 415_000,
        conversion: 255,
        updated: 1_760_000_000,
        xpointer,
        flags,
    }
}

fn read_back(reads: &CatalogReads, book: BookRef<'_>) -> [u64; 7] {
    let mut row = None;
    reads
        .position(book, &mut |record| {
            let Record::ServiceLibPosition(p) = record else {
                panic!("a position")
            };
            row = Some([
                u64::from(p.section),
                u64::from(p.offset),
                u64::from(p.progress_ppm),
                p.conversion,
                p.updated,
                u64::from(p.insert),
                u64::from(p.flags),
            ]);
        })
        .unwrap();
    row.unwrap()
}

#[test]
fn a_local_write_is_read_back_and_an_interface_6_reader_reads_it_too() {
    let card = card();
    let reads = card.reads();
    assert_eq!(reads.stored_position(A), Ok(None));
    reads
        .write_position(A, &place(0b011, "/body/p[3]\"x\""), false)
        .unwrap();
    // The op's own read: the flags are `has_xpointer` and `has_conversion`,
    // no `from_sync`.
    assert_eq!(
        read_back(&reads, A),
        [7, 1234, 415_000, 255, 1_760_000_000, 0, 0b011]
    );
    // The SDK's reader of interface 6 sees the same place.
    let kept = positions::load(&*card.storage, "books/a.epub").unwrap();
    assert_eq!(kept.at, Locator::new(7, 1234));
    assert!((kept.percent - 41.5).abs() < 0.01);
    assert_eq!(kept.conversion, Some(255));
    assert_eq!(kept.xpointer.as_deref(), Some("/body/p[3]\"x\""));
    assert_eq!(kept.updated, 1_760_000_000);
    assert_eq!(
        reads.stored_position(A),
        Ok(Some(StoredPosition {
            from_sync: false,
            updated: 1_760_000_000
        }))
    );
}

#[test]
fn a_position_without_conversion_xpointer_or_clock_keeps_none_of_them_and_the_percent_is_a_tenth() {
    let card = card();
    let reads = card.reads();
    let mut at = place(0, "");
    at.conversion = 0;
    at.updated = 0;
    at.progress_ppm = 999_999;
    reads.write_position(A, &at, false).unwrap();
    // 99.9999 % rounds to the tenth the file keeps: 100.0 %.
    assert_eq!(read_back(&reads, A), [7, 1234, 1_000_000, 0, 0, 0, 0]);
    at.progress_ppm = 12_345;
    reads.write_position(A, &at, false).unwrap();
    assert_eq!(read_back(&reads, A)[2], 12_000);
    let text = card
        .storage
        .read_at(&positions::file("books/a.epub"), 0, &mut [0; 512])
        .unwrap();
    assert!(text > 0);
}

#[test]
fn insert_and_the_sync_mark_are_kept_and_the_next_write_replaces_them() {
    let card = card();
    let reads = card.reads();
    let mut at = place(0b010, "");
    at.insert = 3;
    reads.write_position(A, &at, true).unwrap();
    // `insert` is read back; `from_sync` (bit 2) is set.
    assert_eq!(
        read_back(&reads, A),
        [7, 1234, 415_000, 255, 1_760_000_000, 3, 0b110]
    );
    assert_eq!(
        reads.stored_position(A),
        Ok(Some(StoredPosition {
            from_sync: true,
            updated: 1_760_000_000
        }))
    );
    // A local write removes both marks (`insert` kept only when not 0).
    at.insert = 0;
    reads.write_position(A, &at, false).unwrap();
    assert_eq!(
        read_back(&reads, A),
        [7, 1234, 415_000, 255, 1_760_000_000, 0, 0b010]
    );
    assert!(!reads.stored_position(A).unwrap().unwrap().from_sync);
}

#[test]
fn the_fingerprint_a_reader_keeps_in_the_file_stays_through_a_write() {
    let card = card();
    let reads = card.reads();
    let fingerprint =
        library::Fingerprint::parse("482133 0b6e8f2a6c1d4e5f8a9b0c1d2e3f4a5b").unwrap();
    positions::save_with_book(
        &*card.storage,
        "books/a.epub",
        &positions::Position::new(Locator::new(1, 1), 1.0),
        Some(&fingerprint),
    )
    .unwrap();
    reads.write_position(A, &place(0b010, ""), false).unwrap();
    assert_eq!(
        positions::book(&*card.storage, "books/a.epub").map(|f| f.text()),
        Some(fingerprint.text())
    );
}

#[test]
fn a_book_that_is_not_in_the_catalog_has_no_place_to_write_and_a_bad_file_is_the_cards_error() {
    let card = card();
    let reads = card.reads();
    let nowhere = BookRef::Path("books/zz.epub");
    assert_eq!(
        reads.write_position(nowhere, &place(0b010, ""), false),
        Err(ERR_NOT_FOUND)
    );
    assert_eq!(reads.stored_position(nowhere), Err(ERR_NOT_FOUND));
    assert!(
        card.storage
            .size(&positions::file("books/zz.epub"))
            .is_err(),
        "nothing was written"
    );
    // A file that is no position: the card's error, never "none" and never
    // written over by a sync write's check.
    card.storage
        .write(
            &positions::file("books/a.epub"),
            b"[position]\nsection = 1\n",
        )
        .unwrap();
    assert_eq!(reads.stored_position(A), Err(ERR_IO));
}

#[test]
fn a_record_the_file_cannot_hold_is_refused_before_anything_is_written() {
    let card = card();
    let reads = card.reads();
    // `has_xpointer` with none, and one over 1,024 bytes.
    let long = "x".repeat(1025);
    for (flags, xpointer) in [(0b001, ""), (0b001, long.as_str())] {
        let at = wire::ServiceLibPosition {
            xpointer: &long[..long.len().min(xpointer.len())],
            ..place(flags, "")
        };
        assert_eq!(reads.write_position(A, &at, false), Err(ERR_INVALID));
    }
    assert!(
        card.storage.size(&positions::file("books/a.epub")).is_err(),
        "nothing was written"
    );
}

/// A card whose position files cannot be read while `broken` is set.
struct Flaky(Rc<paperpaw_sdk::MemoryStorage>, core::cell::Cell<bool>);
impl Storage for Flaky {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        if self.1.get() && path.starts_with(positions::DIR) {
            return Err(StorageError::Io);
        }
        self.0.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        if self.1.get() && path.starts_with(positions::DIR) {
            return Err(StorageError::Io);
        }
        self.0.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.append(path, data)
    }
    fn list(
        &self,
        dir: &str,
        visit: &mut dyn FnMut(paperpaw_platform::DirEntry<'_>),
    ) -> Result<(), StorageError> {
        self.0.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.0.remove(path)
    }
}

#[test]
fn a_card_that_stumbles_on_the_file_writes_nothing_and_costs_no_fingerprint() {
    let card = card();
    let fingerprint =
        library::Fingerprint::parse("482133 0b6e8f2a6c1d4e5f8a9b0c1d2e3f4a5b").unwrap();
    positions::save_with_book(
        &*card.storage,
        "books/a.epub",
        &positions::Position::new(Locator::new(1, 1), 1.0),
        Some(&fingerprint),
    )
    .unwrap();
    let flaky = Rc::new(Flaky(card.storage.clone(), core::cell::Cell::new(true)));
    let reads = CatalogReads::new(flaky.clone(), card.env.clone());
    assert_eq!(
        reads.write_position(A, &place(0b010, ""), false),
        Err(ERR_IO)
    );
    // The file is as it was, fingerprint and all.
    flaky.1.set(false);
    assert_eq!(
        positions::book(&*card.storage, "books/a.epub").map(|f| f.text()),
        Some(fingerprint.text())
    );
    assert_eq!(
        positions::load(&*card.storage, "books/a.epub").map(|p| p.at),
        Some(Locator::new(1, 1))
    );
}

#[test]
fn a_file_that_is_no_text_is_replaced_by_the_new_place() {
    let card = card();
    let reads = card.reads();
    card.storage
        .write(&positions::file("books/a.epub"), &[0xff, 0xfe, 0x00, 0x80])
        .unwrap();
    reads.write_position(A, &place(0b010, ""), false).unwrap();
    assert_eq!(
        read_back(&reads, A),
        [7, 1234, 415_000, 255, 1_760_000_000, 0, 0b010]
    );
}

#[test]
fn the_xpointer_limit_counts_the_bytes_as_written_with_their_escapes() {
    let card = card();
    let reads = card.reads();
    let read_xpointer = |reads: &CatalogReads| {
        let mut got = None;
        reads
            .position(A, &mut |record| {
                let Record::ServiceLibPosition(p) = record else {
                    panic!("a position")
                };
                got = Some(String::from(p.xpointer));
            })
            .unwrap();
        got.unwrap()
    };
    // 512 quotes are written as 1,024 bytes: the limit, read back whole.
    let at_limit = "\"".repeat(512);
    let at = wire::ServiceLibPosition {
        xpointer: &at_limit,
        ..place(0b001, "")
    };
    reads.write_position(A, &at, false).unwrap();
    assert_eq!(read_xpointer(&reads), at_limit);
    // 1,024 plain bytes are the limit too. A value that starts with a quote
    // is quoted: a quote and 1,023 bytes are 1,025 once escaped, and 513
    // quotes are 1,026: refused, not written to be read back as none.
    let plain = "x".repeat(1024);
    let at = wire::ServiceLibPosition {
        xpointer: &plain,
        ..place(0b001, "")
    };
    reads.write_position(A, &at, false).unwrap();
    assert_eq!(read_xpointer(&reads), plain);
    let over = format!("\"{}", "x".repeat(1023));
    let at = wire::ServiceLibPosition {
        xpointer: &over,
        ..place(0b001, "")
    };
    assert_eq!(reads.write_position(A, &at, false), Err(ERR_INVALID));
    let over = "\"".repeat(513);
    let at = wire::ServiceLibPosition {
        xpointer: &over,
        ..place(0b001, "")
    };
    assert_eq!(reads.write_position(A, &at, false), Err(ERR_INVALID));
    // The refused writes left the last good place.
    assert_eq!(read_xpointer(&reads), plain);
}

#[test]
fn the_quoting_is_what_config_quote_writes() {
    for value in [
        "plain",
        " lead",
        "trail ",
        "\"q\"",
        "a\\b",
        "line\nbreak",
        "t\tab",
        "cr\r\n",
        "é€",
        "x\"y",
    ] {
        let (quoted, body) = quoted_size(value);
        let mut text = String::new();
        push_quoted(&mut text, value, quoted);
        assert_eq!(text, config::quote(value), "{value:?}");
        assert_eq!(text.len(), body + if quoted { 2 } else { 0 }, "{value:?}");
    }
}

/// A card that counts the files written whole and the writes in place.
struct Counted(
    Rc<paperpaw_sdk::MemoryStorage>,
    core::cell::Cell<(u32, u32)>,
    bool,
);
impl Storage for Counted {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.0.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.0.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        let (whole, in_place) = self.1.get();
        self.1.set((whole + 1, in_place));
        self.0.write(path, data)
    }
    fn write_at(&self, path: &str, offset: u64, data: &[u8]) -> Result<(), StorageError> {
        if !self.2 {
            return Err(StorageError::Unsupported);
        }
        let (whole, in_place) = self.1.get();
        self.1.set((whole, in_place + 1));
        assert_eq!((offset % 512, data.len()), (0, 512), "a slot, one block");
        self.0.write_at(path, offset, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.append(path, data)
    }
    fn list(
        &self,
        dir: &str,
        visit: &mut dyn FnMut(paperpaw_platform::DirEntry<'_>),
    ) -> Result<(), StorageError> {
        self.0.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.0.remove(path)
    }
    fn create_dir(&self, _: &str) -> Result<(), StorageError> {
        panic!("the folder is made by the write that makes the file");
    }
}

fn at_offset(offset: u32) -> wire::ServiceLibPosition<'static> {
    wire::ServiceLibPosition {
        offset,
        ..place(0b010, "")
    }
}

/// Saved again and again (a page turned), a place is one slot written in
/// place, by turns; only the first save writes a file. The SDK's reader and
/// the op's own read the newest, and a slot that is not whole (a save cut
/// off) leaves the place saved before.
#[test]
fn a_place_saved_again_is_one_slot_written_in_place_and_the_newest_is_read() {
    let card = card();
    let counted = Rc::new(Counted(card.storage.clone(), Default::default(), true));
    let reads = CatalogReads::new(counted.clone(), card.env.clone());
    let file = positions::file("books/a.epub");
    let newest = || positions::load(&*card.storage, "books/a.epub").map(|p| p.at.offset);
    reads.write_position(A, &at_offset(100), false).unwrap();
    assert_eq!(counted.1.get(), (1, 0));
    assert_eq!(card.storage.size(&file), Ok(1024));
    assert_eq!(newest(), Some(100));
    for turn in 1..=5u32 {
        reads
            .write_position(A, &at_offset(100 + turn), false)
            .unwrap();
        assert_eq!(counted.1.get(), (1, turn), "no file written again");
        assert_eq!(newest(), Some(100 + turn));
        assert_eq!(read_back(&reads, A)[1], u64::from(100 + turn));
        assert_eq!(card.storage.size(&file), Ok(1024));
    }
    // The slots, by turns: the sixth save went into the second.
    let mut data = paperpaw_sdk::read_file(&*card.storage, &file, 4096).unwrap();
    let text = core::str::from_utf8(&data).unwrap();
    assert!(text[..512].contains("offset = 104\n") && text[..512].contains("slot = 5 "));
    assert!(text[512..].contains("offset = 105\n") && text[512..].contains("slot = 6 "));
    assert!(text.ends_with(" \n"));
    // The newest slot not whole: the place saved before is read, and the
    // next save takes that slot again.
    data[512 + 40] ^= 0x10;
    card.storage.write(&file, &data).unwrap();
    assert_eq!(newest(), Some(104));
    assert_eq!(read_back(&reads, A)[1], 104);
    reads.write_position(A, &at_offset(200), false).unwrap();
    assert_eq!(counted.1.get(), (1, 6));
    let data = paperpaw_sdk::read_file(&*card.storage, &file, 4096).unwrap();
    let text = core::str::from_utf8(&data).unwrap();
    assert!(text[512..].contains("offset = 200\n") && text[512..].contains("slot = 6 "));
    assert_eq!(newest(), Some(200));
}

/// A file without slots (a reader's own, an earlier version's) is read as
/// it is; the next save gives it slots and keeps its fingerprint. A place
/// with an XPointer is too long for a slot at times: it is the whole file,
/// as before, and the save after it makes the slots anew.
#[test]
fn a_file_without_slots_is_read_and_gets_them_and_a_long_place_is_the_whole_file() {
    let card = card();
    let counted = Rc::new(Counted(card.storage.clone(), Default::default(), true));
    let reads = CatalogReads::new(counted.clone(), card.env.clone());
    let fingerprint =
        library::Fingerprint::parse("482133 0b6e8f2a6c1d4e5f8a9b0c1d2e3f4a5b").unwrap();
    positions::save_with_book(
        &*card.storage,
        "books/a.epub",
        &positions::Position::new(Locator::new(3, 30), 3.0),
        Some(&fingerprint),
    )
    .unwrap();
    assert_eq!(read_back(&reads, A)[..2], [3, 30]);
    reads.write_position(A, &at_offset(31), false).unwrap();
    reads.write_position(A, &at_offset(32), false).unwrap();
    assert_eq!(counted.1.get(), (1, 1));
    let kept = || positions::book(&*card.storage, "books/a.epub").map(|f| f.text());
    assert_eq!(kept(), Some(fingerprint.text()));
    // A place that does not fit a slot.
    let long: &'static str = std::boxed::Box::leak("/body/p[1]".repeat(60).into_boxed_str());
    reads.write_position(A, &place(0b011, long), false).unwrap();
    assert_eq!(counted.1.get(), (2, 1));
    let loaded = positions::load(&*card.storage, "books/a.epub").unwrap();
    assert_eq!(loaded.xpointer.as_deref(), Some(long));
    assert_eq!(kept(), Some(fingerprint.text()));
    reads.write_position(A, &at_offset(33), false).unwrap();
    reads.write_position(A, &at_offset(34), false).unwrap();
    assert_eq!(counted.1.get(), (3, 2));
    assert_eq!(read_back(&reads, A)[1], 34);
    assert_eq!(kept(), Some(fingerprint.text()));
}

/// On a storage that writes nothing in place the file is written whole
/// with its slots, and read the same.
#[test]
fn a_storage_without_writes_in_place_keeps_the_slots_in_a_file_written_whole() {
    let card = card();
    let counted = Rc::new(Counted(card.storage.clone(), Default::default(), false));
    let reads = CatalogReads::new(counted.clone(), card.env.clone());
    for turn in 0..4u32 {
        reads
            .write_position(A, &at_offset(50 + turn), false)
            .unwrap();
        assert_eq!(read_back(&reads, A)[1], u64::from(50 + turn));
    }
    assert_eq!(counted.1.get(), (4, 0));
    let file = positions::file("books/a.epub");
    assert_eq!(card.storage.size(&file), Ok(1024));
}

/// A book that is open is on the card: its place is saved without a search
/// of the catalog (#305).
#[test]
fn the_place_of_a_book_on_the_card_is_saved_without_reading_the_catalog() {
    /// Fails every read of the catalog and its log.
    struct NoCatalog(Rc<paperpaw_sdk::MemoryStorage>);
    impl Storage for NoCatalog {
        fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
            assert!(!path.starts_with(".paperpaw/library"), "{path} read");
            self.0.read_at(path, offset, buf)
        }
        fn size(&self, path: &str) -> Result<u64, StorageError> {
            assert!(!path.starts_with(".paperpaw/library"), "{path} asked for");
            self.0.size(path)
        }
        fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
            self.0.write(path, data)
        }
        fn write_at(&self, path: &str, offset: u64, data: &[u8]) -> Result<(), StorageError> {
            self.0.write_at(path, offset, data)
        }
        fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
            self.0.append(path, data)
        }
        fn list(
            &self,
            dir: &str,
            visit: &mut dyn FnMut(paperpaw_platform::DirEntry<'_>),
        ) -> Result<(), StorageError> {
            self.0.list(dir, visit)
        }
        fn remove(&self, path: &str) -> Result<(), StorageError> {
            self.0.remove(path)
        }
    }
    let card = card();
    let reads = CatalogReads::new(Rc::new(NoCatalog(card.storage.clone())), card.env.clone());
    reads.write_position(A, &at_offset(7), false).unwrap();
    reads.write_position(A, &at_offset(8), false).unwrap();
    assert_eq!(read_back(&reads, A)[1], 8);
}
