#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! Op 14 over a catalog on a card in memory: what is stored (the SDK's own
//! file, plus `insert` and `sync`), and what is answered where the book or the
//! card is not as it should be.
extern crate std;
use super::super::tests::card;
use super::*;
use crate::native_host::lib_service::{LibQuery, LibWrite};
use paperpaw_sdk::document::Locator;

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
