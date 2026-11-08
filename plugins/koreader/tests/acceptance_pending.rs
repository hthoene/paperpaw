//! Acceptance of the interface-7 KOReader exchange, pending on the host and
//! the SDK. The interface-6 tests drive a kernel (`App`) and stay as the
//! references (`tests/exchange.rs`, `tests/lua.rs`, `src/tests.rs`); none of
//! these cases passes by itself.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 4] = [
    Case {
        name: "a-book-opened-takes-over-what-koreader-knows",
        golden: None,
        blocked_by: &[
            "book-opened-event",
            "document-annotations-sdk",
            "files-facade-384",
        ],
    },
    Case {
        name: "annotations-changed-here-go-to-koreader",
        golden: None,
        blocked_by: &[
            "annotations-changed-event",
            "document-annotations-sdk",
            "files-facade-384",
        ],
    },
    Case {
        name: "removed-there-asks-the-reader-in-the-book",
        golden: None,
        blocked_by: &["ask-service-sdk", "H1", "H3", "H4"],
    },
    Case {
        name: "a-moved-book-takes-its-known-file-along",
        golden: None,
        blocked_by: &["book-moved-event", "book-data-declaration"],
    },
];

#[test]
fn every_case_still_has_its_reference() {
    for case in &CASES {
        assert!(case.golden_exists(), "{}", case.name);
    }
}

#[test]
#[ignore = "BLOCKED: the book-opened event, the document/annotations facade and the files facade (#384)"]
fn opening_a_book() {
    CASES[0].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: the annotations-changed event, the document/annotations facade and the files facade"]
fn changing_annotations() {
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: the ask service and H1/H3/H4"]
fn removal_asks_the_reader() {
    CASES[2].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: the book-moved event and a book-data declaration"]
fn moving_a_book() {
    CASES[3].refuse().unwrap();
}
