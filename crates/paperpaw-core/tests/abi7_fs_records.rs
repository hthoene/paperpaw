//! The records of `core.files` (FS, `docs/abi/files.md` section 2.7) and the
//! optional 64-bit `cursor` of `service.end`.
#![allow(clippy::unwrap_used, reason = "fixture failures must fail the test")]
use paperpaw_core::native_host::registrations::{
    Record, SERVICE_FS_ENTRY_NAME_MAX_BYTES, SERVICE_FS_MOVE_FROM_MAX_BYTES,
    SERVICE_FS_MOVE_TO_MAX_BYTES, ServiceEnd, ServiceFsEntry, ServiceFsList, ServiceFsMove,
    ServiceFsPath, ServiceFsProgress, ServiceFsSpace, record_decode_one, record_encode_one,
    record_framed_len,
};

fn round_trip(record: Record<'_>) -> usize {
    let mut out = [0u8; 2048];
    let len = record_encode_one(&record, &mut out).unwrap();
    assert_eq!(len, record_framed_len(&record).unwrap());
    assert_eq!(record_decode_one(&out[..len]), Ok(record));
    len
}

fn end(cursor: Option<u64>) -> ServiceEnd {
    ServiceEnd {
        flags: 0,
        total: 0,
        generation: 0,
        next: 0,
        cursor,
    }
}

#[test]
fn the_end_cursor_is_optional_and_carries_a_64_bit_place() {
    // Without the cursor the record keeps its earlier size (19 framed); with it, 28.
    assert_eq!(round_trip(Record::ServiceEnd(end(None))), 19);
    assert_eq!(round_trip(Record::ServiceEnd(end(Some(u64::MAX)))), 28);
    assert_eq!(round_trip(Record::ServiceEnd(end(Some(0)))), 28);
}

#[test]
fn a_list_row_fits_the_contracts_bound() {
    // Section 2.1: a row is at most 793 bytes framed, so at least 5 fit a 4,096-byte page.
    let name = "n".repeat(SERVICE_FS_ENTRY_NAME_MAX_BYTES);
    let row = |modified| {
        Record::ServiceFsEntry(ServiceFsEntry {
            name: &name,
            flags: 2,
            size: u64::MAX,
            modified,
        })
    };
    let longest = round_trip(row(Some(u64::MAX)));
    assert_eq!(longest, 793);
    assert!(round_trip(row(None)) < longest);
    assert!(5 * longest + 28 <= 4096);
}

#[test]
fn inputs_and_replies_round_trip() {
    let path = "books/a.epub";
    assert!(round_trip(Record::ServiceFsPath(ServiceFsPath { root: 1, path })) > 0);
    let list = ServiceFsList {
        root: 0,
        path: "",
        cursor: u64::MAX,
        flags: 0,
    };
    assert!(round_trip(Record::ServiceFsList(list)) > 0);
    let space = ServiceFsSpace {
        flags: 0b111,
        free: u64::MAX,
        total: u64::MAX,
    };
    assert!(round_trip(Record::ServiceFsSpace(space)) > 0);
    let progress = ServiceFsProgress {
        removed: 64,
        flags: 1,
        next: u64::MAX,
    };
    assert!(round_trip(Record::ServiceFsProgress(progress)) > 0);
}

#[test]
fn the_longest_move_fits_its_ops_input_bound() {
    // `core.files` op 6 takes at most 2,048 bytes: one counted batch of one move.
    let from = "f".repeat(SERVICE_FS_MOVE_FROM_MAX_BYTES);
    let to = "t".repeat(SERVICE_FS_MOVE_TO_MAX_BYTES);
    let longest = round_trip(Record::ServiceFsMove(ServiceFsMove {
        from_root: 1,
        from: &from,
        to_root: 0,
        to: &to,
        flags: 1,
    }));
    assert!(longest + 8 <= 2048, "{longest}");
}

#[test]
fn an_overlong_name_is_refused() {
    let name = "n".repeat(SERVICE_FS_ENTRY_NAME_MAX_BYTES + 1);
    let row = Record::ServiceFsEntry(ServiceFsEntry {
        name: &name,
        flags: 0,
        size: 0,
        modified: None,
    });
    assert!(record_framed_len(&row).is_err());
}
