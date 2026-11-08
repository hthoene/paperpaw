//! Acceptance of the interface-7 wireless library, pending on the host, the
//! SDK and the later slices. The interface-6 tests drive a kernel (`App`) and
//! stay as the references (`tests/session.rs`, `tests/protocol.rs`,
//! `tests/hostile.rs`); none of these cases passes by itself.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 4] = [
    Case {
        name: "calibre-is-found-by-broadcast-or-by-its-address",
        golden: None,
        blocked_by: &["sock-host-draft", "sock-sdk-facade", "wifi-state"],
    },
    Case {
        name: "a-session-sends-the-book-list-and-answers-calibres-questions",
        golden: None,
        blocked_by: &["sock-host-draft", "sock-sdk-facade", "library-listing-sdk"],
    },
    Case {
        name: "a-book-calibre-sends-lands-in-the-library",
        golden: None,
        blocked_by: &[
            "sock-host-draft",
            "lib-import-sdk",
            "core-files-facade-384",
            "secrets-facade",
        ],
    },
    Case {
        name: "the-screen-shows-the-connection-and-the-progress",
        golden: None,
        blocked_by: &["H1", "H3", "H4", "menu-place-944"],
    },
];

#[test]
fn every_case_still_has_its_reference() {
    for case in &CASES {
        assert!(case.golden_exists(), "{}", case.name);
    }
}

#[test]
#[ignore = "BLOCKED: the sockets host and SDK facade (SOCK draft) and the Wi-Fi state"]
fn finding_and_talking_to_calibre() {
    CASES[0].refuse().unwrap();
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: sockets, library import (LIB), the files facade and secrets"]
fn receiving_books() {
    CASES[2].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: H1/H3/H4 and menu place (#944)"]
fn the_screen() {
    CASES[3].refuse().unwrap();
}
