//! Acceptance of the KOReader exchange through the host, pending on the
//! activation of native plugins. None of these cases passes by itself.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 4] = [
    Case {
        name: "a-book-opened-takes-over-what-koreader-knows",
        golden: None,
        blocked_by: &["native-activation-944", "host-417", "book-opened-event"],
    },
    Case {
        name: "annotations-changed-here-go-to-koreader",
        golden: None,
        blocked_by: &["native-activation-944", "host-417", "library-changed-event"],
    },
    Case {
        name: "removed-there-asks-the-reader-in-the-book",
        golden: None,
        blocked_by: &["native-activation-944", "H1", "H3", "H4"],
    },
    Case {
        name: "a-moved-book-takes-its-known-file-along",
        golden: None,
        blocked_by: &["native-activation-944", "book-data-move-host"],
    },
];

#[test]
fn every_case_still_has_its_reference() {
    for case in &CASES {
        assert!(case.golden_exists(), "{}", case.name);
    }
}

#[test]
#[ignore = "BLOCKED: activation of native plugins (#944, #417) and the book-opened event"]
fn opening_a_book() {
    CASES[0].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: activation of native plugins (#944, #417) and the library-changed event"]
fn changing_annotations() {
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: activation of native plugins (#944) and H1/H3/H4"]
fn removal_asks_the_reader() {
    CASES[2].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: activation of native plugins (#944) and the host moving book data"]
fn moving_a_book() {
    CASES[3].refuse().unwrap();
}
