#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
extern crate std;
use super::*;
use alloc::vec::Vec;

fn drain(table: &mut PendingAsks, provider: u64, hash: u64, now: u64) -> Vec<u64> {
    let (askers, count) = table.take(provider, hash, now);
    let mut got = Vec::from(&askers[..count]);
    got.sort_unstable();
    got
}

#[test]
fn the_hash_is_fnv1a_over_the_language_length_the_language_and_the_text() {
    // The empty language and text: the one byte 0 hashed.
    assert_eq!(ask_hash("", ""), 0xaf63_bd4c_8601_b7df);
    // The pair maps to one input: moving a byte between them changes it.
    assert_ne!(ask_hash("en", "xy"), ask_hash("enx", "y"));
    assert_ne!(ask_hash("en", "word"), ask_hash("de", "word"));
    assert_eq!(ask_hash("en", "word"), ask_hash("en", "word"));
}

#[test]
fn an_ask_is_pending_until_ready_then_it_is_gone_and_every_asker_is_told() {
    let mut table = PendingAsks::new();
    let hash = ask_hash("en", "tree");
    table.record(1, 100, hash, 1_000).unwrap();
    table.record(2, 100, hash, 1_500).unwrap();
    // Another provider's, and another text's, are not the ready command's.
    table.record(3, 101, hash, 1_500).unwrap();
    table.record(4, 100, ask_hash("en", "leaf"), 1_500).unwrap();
    assert_eq!(drain(&mut table, 100, hash, 2_000), [1, 2]);
    assert_eq!(table.len(), 2);
    // Gone: a second ready command matches nothing.
    assert_eq!(drain(&mut table, 100, hash, 2_000), Vec::<u64>::new());
    assert_eq!(table.unmatched(), 1);
}

#[test]
fn asking_again_refreshes_the_deadline_and_is_still_one_entry() {
    let mut table = PendingAsks::new();
    let hash = ask_hash("", "tree");
    table.record(1, 100, hash, 0).unwrap();
    table.record(1, 100, hash, 50_000).unwrap();
    assert_eq!(table.len(), 1);
    // The first deadline (60 s) has passed, the second (110 s) has not.
    assert_eq!(drain(&mut table, 100, hash, 70_000), [1]);
}

#[test]
fn a_deadline_ends_the_ask_without_an_event() {
    let mut table = PendingAsks::new();
    let hash = ask_hash("", "tree");
    table.record(1, 100, hash, 1_000).unwrap();
    // Just before 60 s on it is there; at 60 s on it is gone.
    let mut early = PendingAsks::new();
    early.record(1, 100, hash, 1_000).unwrap();
    assert_eq!(drain(&mut early, 100, hash, 60_999), [1]);
    assert_eq!(drain(&mut table, 100, hash, 61_000), Vec::<u64>::new());
    assert_eq!(table.len(), 0);
    // Expired places are free again.
    table.record(2, 100, hash, 61_000).unwrap();
    assert_eq!(table.len(), 1);
}

#[test]
fn a_stopped_asker_or_provider_ends_its_asks_without_an_event() {
    let mut table = PendingAsks::new();
    let hash = ask_hash("", "tree");
    table.record(1, 100, hash, 0).unwrap();
    table.record(2, 100, hash, 0).unwrap();
    table.record(2, 101, hash, 0).unwrap();
    table.stopped(1);
    assert_eq!(table.len(), 2);
    // The provider 101 stops: the ask to it ends, the one to 100 stays.
    table.stopped(101);
    assert_eq!(drain(&mut table, 101, hash, 1), Vec::<u64>::new());
    assert_eq!(drain(&mut table, 100, hash, 1), [2]);
    // The asker 2 stopped: nothing is left of it.
    table.record(2, 100, hash, 2).unwrap();
    table.stopped(2);
    assert_eq!(table.len(), 0);
}

#[test]
fn an_asker_holds_four_a_provider_its_share_and_the_table_its_size() {
    let mut table = PendingAsks::new();
    // One asker: four, the fifth is refused; a refresh of one of them is not.
    for n in 0..4 {
        table.record(1, 100, n, 0).unwrap();
    }
    assert_eq!(table.record(1, 100, 4, 0), Err(Refused::Full));
    assert_eq!(table.record(1, 100, 0, 1), Ok(()));
    assert_eq!(table.len(), 4);
    // One provider: askers one after another until its share is full.
    let mut table = PendingAsks::new();
    for asker in 0..PROVIDER_MAX as u64 {
        table.record(asker, 100, 7, 0).unwrap();
    }
    assert_eq!(table.record(99, 100, 7, 0), Err(Refused::Full));
    // The others' providers still have room in the rest of the table.
    table.record(99, 101, 7, 0).unwrap();
    // The whole table: other providers fill it, then the next is refused.
    let mut table = PendingAsks::new();
    let mut taken = 0;
    'fill: for provider in 0..CAPACITY as u64 {
        for slot in 0..PROVIDER_MAX as u64 {
            let asker = provider * 100 + slot / 4 * 1_000_000 + slot;
            if table.record(asker, provider, slot, 0).is_ok() {
                taken += 1;
            }
            if taken == CAPACITY {
                break 'fill;
            }
        }
    }
    assert_eq!(taken, CAPACITY);
    assert_eq!(table.record(10_000, 10_000, 1, 0), Err(Refused::Full));
    // Ends of asks make room again.
    table.stopped(0);
    assert!(table.len() < CAPACITY);
}

#[test]
fn unmatched_ready_commands_are_counted_and_logged_at_most_once_a_minute() {
    let mut table = PendingAsks::new();
    for _ in 0..3 {
        drain(&mut table, 1, 1, 0);
    }
    assert_eq!(table.unmatched(), 3);
    assert!(table.log_due(1_000));
    assert!(!table.log_due(30_000));
    assert!(!table.log_due(60_999));
    assert!(table.log_due(61_000));
}

#[test]
fn the_table_is_32_bytes_an_entry_and_holds_no_text() {
    assert_eq!(core::mem::size_of::<Entry>(), 32);
    assert!(core::mem::size_of::<PendingAsks>() <= CAPACITY * 32 + 40);
}
