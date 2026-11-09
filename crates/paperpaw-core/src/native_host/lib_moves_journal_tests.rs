#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
//! The journal record: round trip, every way a record can be torn, and the
//! temp-sync-rename write.
extern crate std;
use super::*;
use paperpaw_sdk::MemoryStorage;
use std::{string::String, vec};

fn entry<'a>(from: &'a str, to: &'a str) -> Entry<'a> {
    Entry {
        from,
        to,
        replace: true,
        identity: 0x0123_4567_89ab_cdef,
    }
}

#[test]
fn an_entry_round_trips_and_a_missing_journal_is_no_entry() {
    let card = MemoryStorage::new();
    let journal = Journal(&card);
    assert_eq!(journal.open_entry(&mut |_| panic!("no entry")), Ok(false));
    journal
        .write_entry(&entry("Books/old", "Büchér/new"))
        .unwrap();
    let mut seen = None;
    let found = journal
        .open_entry(&mut |e| {
            seen = Some((
                String::from(e.from),
                String::from(e.to),
                e.replace,
                e.identity,
            ))
        })
        .unwrap();
    assert!(found);
    assert_eq!(
        seen,
        Some((
            "Books/old".into(),
            "Büchér/new".into(),
            true,
            0x0123_4567_89ab_cdef
        ))
    );
    assert!(!card.exists(TEMP), "the temp file is gone after the rename");
    // A second write replaces it whole; a flag off, an empty `to` is fine.
    journal
        .write_entry(&Entry {
            replace: false,
            ..entry("a", "")
        })
        .unwrap();
    let mut seen = None;
    journal
        .open_entry(&mut |e| seen = Some((String::from(e.from), e.replace)))
        .unwrap();
    assert_eq!(seen, Some(("a".into(), false)));
    journal.clear_entry().unwrap();
    journal.clear_entry().unwrap();
    assert_eq!(journal.open_entry(&mut |_| panic!("cleared")), Ok(false));
}

#[test]
fn a_record_that_is_not_whole_is_an_error_and_never_overwritten_blind() {
    let good = encode(&entry("Books/old", "Books/new")).unwrap();
    let mut torn = vec![];
    // Every truncation, and every single flipped byte, is refused.
    for cut in 0..good.len() {
        torn.push(good[..cut].to_vec());
    }
    for at in 0..good.len() {
        let mut bad = good.clone();
        bad[at] ^= 0x01;
        torn.push(bad);
    }
    let mut longer = good.clone();
    longer.push(0);
    torn.push(longer);
    for record in torn {
        assert_eq!(decode(&record).err(), Some(ERR_IO), "{record:?}");
        let card = MemoryStorage::new();
        card.write(JOURNAL, &record).unwrap();
        assert_eq!(Journal(&card).open_entry(&mut |_| ()).err(), Some(ERR_IO));
    }
    assert!(decode(&good).is_ok());
}

#[test]
fn a_huge_record_is_refused_without_reading_it() {
    let card = MemoryStorage::new();
    card.write(JOURNAL, &vec![0u8; 100_000]).unwrap();
    assert_eq!(Journal(&card).open_entry(&mut |_| ()).err(), Some(ERR_IO));
}

#[test]
fn a_path_past_the_limit_is_refused_and_the_old_journal_stays() {
    let card = MemoryStorage::new();
    let journal = Journal(&card);
    journal.write_entry(&entry("a", "b")).unwrap();
    let long = "x".repeat(MAX_PATH + 1);
    assert_eq!(journal.write_entry(&entry(&long, "b")), Err(ERR_INVALID));
    let mut seen = None;
    journal
        .open_entry(&mut |e| seen = Some(String::from(e.from)))
        .unwrap();
    assert_eq!(seen, Some("a".into()));
    // The longest path the library takes round-trips.
    let max = "y".repeat(MAX_PATH);
    journal.write_entry(&entry(&max, &max)).unwrap();
    assert!(
        journal
            .open_entry(&mut |e| assert_eq!(e.from.len(), MAX_PATH))
            .unwrap()
    );
}

#[test]
fn a_stale_temp_file_is_replaced_and_never_read_as_the_journal() {
    let card = MemoryStorage::new();
    // A cut before the rename left a temp file; no journal, so no entry.
    card.write(TEMP, b"PPLJ torn").unwrap();
    let journal = Journal(&card);
    assert_eq!(journal.open_entry(&mut |_| panic!("no entry")), Ok(false));
    journal.write_entry(&entry("a", "b")).unwrap();
    assert!(journal.open_entry(&mut |_| ()).unwrap());
    assert!(!card.exists(TEMP));
}

#[test]
fn a_kind_other_than_a_move_is_an_error_even_with_a_good_check() {
    let mut record = encode(&entry("a", "b")).unwrap();
    assert_eq!(record[5], 0, "a move");
    for kind in [1, 2, 3, 255] {
        record[5] = kind;
        let end = record.len() - 8;
        let sum = check(&record[..end]).to_le_bytes();
        record[end..].copy_from_slice(&sum);
        assert_eq!(decode(&record).err(), Some(ERR_IO), "kind {kind}");
    }
}
