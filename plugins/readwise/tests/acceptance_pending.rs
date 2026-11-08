//! Acceptance of the interface-7 port, pending on the host. The interface-6
//! plugin is pinned by `send.rs` and `sending.rs` on a kernel; these cases are
//! the same behaviours through the production loader with the native plugin.
//! They never pass by themselves.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 3] = [
    // The notes of an export are read a line at a time, the request written to
    // a file and sent; what Readwise took leaves the waiting list.
    Case {
        name: "readwise-export",
        golden: None,
        blocked_by: &["FS", "NET", "EVENT-notes-exported"],
    },
    // The token is asked for on the keyboard and kept with the secrets; a token
    // Readwise refuses is let go.
    Case {
        name: "readwise-token",
        golden: None,
        blocked_by: &["SEC", "KEYBOARD-slot"],
    },
    // Exports that wait are sent again at the next connection, at the latest in
    // half an hour.
    Case {
        name: "readwise-retry",
        golden: None,
        blocked_by: &["WORK-declaration", "NET"],
    },
];

#[test]
fn the_cases_name_what_they_wait_for() {
    for case in &CASES {
        assert!(case.golden_exists());
        let error = case.refuse().unwrap_err();
        assert!(error.starts_with("BLOCKED "), "{error}");
        assert!(error.contains(case.blocked_by[0]), "{error}");
    }
}

#[test]
#[ignore = "BLOCKED FS/NET/EVENT-notes-exported: files, network and the export event are not on integration"]
fn readwise_export() {
    CASES[0].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED SEC/KEYBOARD-slot: secrets (PR 367) and the keyboard slot are not qualified for the native plugin"]
fn readwise_token() {
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED WORK-declaration/NET: declared work and the network are not qualified for the native plugin"]
fn readwise_retry() {
    CASES[2].refuse().unwrap();
}
