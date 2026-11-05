//! The bounded record format; these tests do not claim durable driver integration.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "record regression assertions"
)]
use hadris_fat::exfat::intent::{self, Change, EntryState, Identity, Operation, RecordError};
use proptest::prelude::*;

fn identity() -> Identity {
    Identity {
        serial: 42,
        volume_bytes: 8 << 20,
        heap_start: 1 << 20,
        heap_end: 8 << 20,
        journal_start: 2 << 20,
        journal_len: 8192,
        epoch: 17,
    }
}

fn record(sequence: u64, after: &[u8; 32]) -> Vec<u8> {
    let mut out = vec![0; intent::RECORD_BYTES];
    intent::encode(
        &mut out,
        identity(),
        sequence,
        Operation::Update,
        &[Change {
            offset: 1 << 20,
            before: &[0x81; 32],
            after,
        }],
    )
    .unwrap();
    out
}

#[test]
fn round_trip_checks_identity_and_each_entry_before_replay() {
    let bytes = record(1, &[0x82; 32]);
    let parsed = intent::decode(&bytes, identity()).unwrap().unwrap();
    assert_eq!(parsed.sequence, 1);
    assert_eq!(parsed.operation, Operation::Update);
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed.state(0, &[0x81; 32]), Ok(EntryState::Before));
    assert_eq!(parsed.state(0, &[0x82; 32]), Ok(EntryState::After));
    assert_eq!(parsed.state(0, &[0x83; 32]), Err(RecordError::Stale));
    assert_eq!(parsed.state(1, &[0; 32]), Err(RecordError::Malformed));
    let mut other = identity();
    other.epoch += 1;
    assert!(matches!(
        intent::decode(&bytes, other),
        Err(RecordError::WrongVolume)
    ));
}

#[test]
fn a_partial_publication_never_authorizes_an_unrecorded_after_image() {
    let old = record(1, &[0x82; 32]);
    let new = record(2, &[0x83; 32]);
    // Every byte cut in an overwrite of the payload, with the old header
    // still present. Only a complete, checksum-valid old/new image is usable.
    for cut in intent::HEADER_BYTES..=intent::RECORD_BYTES {
        let mut torn = old.clone();
        torn[intent::HEADER_BYTES..cut].copy_from_slice(&new[intent::HEADER_BYTES..cut]);
        if let Ok(Some(parsed)) = intent::decode(&torn, identity()) {
            assert_eq!(parsed.sequence, 1);
            assert_eq!(parsed.change(0).unwrap().after, &[0x82; 32]);
        }
    }
    // New body, then each byte cut of header publication.
    for cut in 0..=intent::HEADER_BYTES {
        let mut torn = new.clone();
        torn[cut..intent::HEADER_BYTES].copy_from_slice(&old[cut..intent::HEADER_BYTES]);
        if let Ok(Some(parsed)) = intent::decode(&torn, identity()) {
            assert_eq!(parsed.sequence, 2);
            assert_eq!(parsed.change(0).unwrap().after, &[0x83; 32]);
        }
    }
}

#[test]
fn bad_input_cannot_alias_the_journal_or_change_output_on_error() {
    for offset in [identity().journal_start, u64::MAX, 1, identity().heap_end] {
        let mut out = vec![0xa5; intent::RECORD_BYTES];
        assert_eq!(
            intent::encode(
                &mut out,
                identity(),
                1,
                Operation::Update,
                &[Change {
                    offset,
                    before: &[0; 32],
                    after: &[1; 32]
                },]
            ),
            Err(RecordError::Malformed)
        );
        assert!(out.iter().all(|b| *b == 0xa5));
    }
    let mut out = vec![0xa5; intent::RECORD_BYTES];
    let changes = [
        Change {
            offset: 1 << 20,
            before: &[0; 32],
            after: &[1; 32],
        },
        Change {
            offset: 1 << 20,
            before: &[0; 32],
            after: &[2; 32],
        },
    ];
    assert_eq!(
        intent::encode(&mut out, identity(), 1, Operation::Update, &changes),
        Err(RecordError::Malformed)
    );
    assert!(out.iter().all(|b| *b == 0xa5));
}

#[test]
fn corrupt_unknown_and_retired_records_are_not_replayed() {
    let bytes = record(1, &[0x82; 32]);
    for i in 0..bytes.len() {
        let mut corrupt = bytes.clone();
        corrupt[i] ^= 0x80;
        assert!(intent::decode(&corrupt, identity()).is_err(), "byte {i}");
    }
    let mut unknown = bytes.clone();
    unknown[8] = 9;
    assert!(matches!(
        intent::decode(&unknown, identity()),
        Err(RecordError::UnknownVersion)
    ));
    let mut retired = bytes;
    retired[..intent::HEADER_BYTES].fill(0);
    assert!(intent::decode(&retired, identity()).unwrap().is_none());
}

proptest! {
    #[test]
    fn arbitrary_records_are_bounded(bytes in prop::collection::vec(any::<u8>(), 0..4096)) {
        if let Ok(Some(record)) = intent::decode(&bytes, identity()) {
            prop_assert!(record.len() <= intent::MAX_CHANGES);
            for i in 0..record.len() { prop_assert!(record.change(i).is_some()); }
        }
    }
}
