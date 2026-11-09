//! `core.library` op 22 `fields` is paged (`docs/abi/library.md` section 4):
//! at most 3 rows at their caps fit one reply with its `service.end`, so a
//! book's up to 64 fields need a cursor (`lib_field_page`, pinned in
//! `tools/tests/test_abi_services.py`).
#![allow(clippy::unwrap_used, reason = "fixture failures must fail the test")]
use paperpaw_core::native_host::registrations::{
    Record, ServiceEnd, ServiceLibField, record_framed_len,
};

#[test]
fn three_field_rows_fit_a_page_and_four_do_not() {
    let (id, title, value) = ("i".repeat(128), "t".repeat(128), "v".repeat(1024));
    let row = record_framed_len(&Record::ServiceLibField(ServiceLibField {
        id: &id,
        title: &title,
        value: &value,
    }))
    .unwrap();
    let end = record_framed_len(&Record::ServiceEnd(ServiceEnd {
        flags: 1,
        total: 64,
        generation: u32::MAX,
        next: 3,
        cursor: None,
    }))
    .unwrap();
    assert_eq!(row, 1_291);
    assert!(3 * row + end <= 4_096);
    assert!(4 * row + end > 4_096);
}
