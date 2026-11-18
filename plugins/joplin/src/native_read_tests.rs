//! What the native plugin reads of an item's end and of a notes file's start
//! (`item_props`, `head_of`), with the caps of interface 6 (2,048 bytes of an
//! item's end, 1,024 of a notes file's start; the transcripts of
//! `native_golden_tests` pin what they read), refusing instead of aborting
//! when memory runs short.
use crate::fail_alloc::sweep;
use crate::native_joplin as native;
use alloc::format;

const PROPS: &str = "id: 0123456789abcdef0123456789abcdef\nparent_id: fedcba9876543210fedcba9876543210\ncreated_time: 2026-01-02T03:04:05.000Z\nupdated_time: 2026-02-03T04:05:06.000Z\nuser_created_time: 2026-01-02T03:04:05.000Z\norder: 5\ndeleted_time: 0\nencryption_applied: 0\ntype_: 1";

fn same_props(old: &native::Props, new: &native::Props) -> bool {
    (
        old.kind,
        &old.parent,
        &old.created,
        old.updated,
        &old.user_created,
    ) == (
        new.kind,
        &new.parent,
        &new.created,
        new.updated,
        &new.user_created,
    ) && (old.order, old.deleted, old.encrypted, old.text_end)
        == (new.order, new.deleted, new.encrypted, new.text_end)
}

#[test]
fn refused_allocations_end_in_no_memory() {
    let file = format!("Title\n\n{PROPS}\n");
    sweep(|| native::item_props(file.len() as u64, file.as_bytes()).map(|p| p.parent.len()));
    sweep(|| native::head_of(b"# A \xff Book\nJane Author\n").map(|h| h.title.len()));
}

#[test]
fn the_end_of_an_item_counts_from_where_the_tail_really_starts() {
    // A big file of which the caller read less than the 2,048 bytes that
    // count: the text still ends at the same place in the file.
    let file = format!("{}\n\nBody text\n\n{PROPS}", "y".repeat(5000));
    // Read with the whole file in hand (and so counted from its real start).
    let old = native::item_props(file.len() as u64, file.as_bytes()).unwrap();
    for have in [PROPS.len() + 20, 700, 1500, native::TAIL, 3000, file.len()] {
        let tail = &file.as_bytes()[file.len() - have..];
        let new = native::item_props(file.len() as u64, tail).unwrap();
        assert!(same_props(&old, &new), "{have}");
    }
    // A file shorter than its tail: counted from its start.
    let small = format!("T\n\n{PROPS}");
    let new = native::item_props(small.len() as u64, small.as_bytes()).unwrap();
    assert_eq!(new.text_end, 1);
    // A tail longer than the file says is cut to what the file has.
    let new = native::item_props(small.len() as u64, format!("zzzz{small}").as_bytes()).unwrap();
    assert_eq!(new.text_end, 1);
}
