//! The native decisions (`native_joplin`) refuse instead of aborting when
//! memory runs short (their results are pinned by `native_golden_tests`).
use crate::fail_alloc::sweep;
use crate::native_joplin as native;

/// As Joplin 3.7 wrote it to a sync target.
const JOPLIN_NOTE: &str = "Hello\n\nLine one\n\n> quote\n\n\
    id: 1ae79351dd744c2794eebb334b1629a7\n\
    parent_id: 7db793874c624138b3b4520ae5cdf20c\n\
    created_time: 2026-10-19T22:24:10.090Z\n\
    updated_time: 2026-10-19T22:24:11.708Z\n\
    is_conflict: 0\nauthor: \nsource_url: \n\
    user_created_time: 2026-10-19T22:24:10.090Z\n\
    user_updated_time: 2026-10-19T22:24:10.090Z\n\
    order: 1792448650089\n\
    encryption_cipher_text: \nencryption_applied: 0\n\
    deleted_time: 0\nis_locked: 0\ntype_: 1";

#[test]
fn refused_allocations_end_in_no_memory() {
    sweep(|| native::new_id("a book 1", Some("old")).map(|id| id.len()));
    sweep(|| {
        native::Service::Server
            .url("https://j.example", "info.json")
            .map(|u| u.len())
    });
    sweep(|| {
        native::Service::WebDav
            .locks_url("https://j.example")
            .map(|u| u.len())
    });
    sweep(|| native::parse_props(JOPLIN_NOTE).map(|p| p.kind));
    sweep(|| native::notebook("id", " A  B ", "now").map(|t| t.len()));
    let note = native::Note {
        id: "i",
        parent: "p",
        title: "T",
        author: "a\nb",
        created: "c",
        user_created: "u",
        updated: "x",
        order: 7,
    };
    sweep(|| note.props().map(|t| t.len()));
    sweep(|| native::parse_head("# Title\n\nAuthor\n").map(|h| h.title.len()));
}

#[test]
fn times_are_written_as_joplin_writes_them() {
    for ms in [
        0,
        999,
        1_792_448_650_090,
        1_792_483_200_000,
        4_102_444_799_999,
    ] {
        let old = paperpaw_sdk::time::UnixTime::from_millis(ms).iso8601();
        assert_eq!(native::iso8601(ms).unwrap(), old);
    }
    assert_eq!(
        native::iso8601(1_792_448_650_090).unwrap(),
        "2026-10-19T22:24:10.090Z"
    );
    sweep(|| native::iso8601(1_792_448_650_090).map(|t| t.len()));
}
