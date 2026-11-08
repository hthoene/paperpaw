//! Acceptance of the interface-7 port, pending on the host. The interface-6
//! plugin is pinned by `fetch.rs` and `answers.rs` on a kernel; these cases are
//! the same behaviours through the production loader with the native plugin.
//! They never pass by themselves.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 4] = [
    // The account is typed on the keyboard (or a phone) and kept: the server,
    // user and client ID as settings, the password and secret with the secrets.
    Case {
        name: "wallabag-setup",
        golden: None,
        blocked_by: &["H1", "H2", "SEC", "KEYBOARD-slot"],
    },
    // Unread articles are fetched as EPUB books into the Wallabag folder.
    Case {
        name: "wallabag-fetch",
        golden: None,
        blocked_by: &["NET", "FS", "JSON-stream"],
    },
    // Articles read to the end are archived with the next fetch and their books
    // removed unless they carry the reader's marks.
    Case {
        name: "wallabag-archive",
        golden: None,
        blocked_by: &["NET", "FS", "EVENT-page-turned", "ANNOTATIONS"],
    },
    // A fetch started by a rule runs as declared work at a cheap moment.
    Case {
        name: "wallabag-fetch-due",
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
#[ignore = "BLOCKED H1/H2/SEC/KEYBOARD-slot: screen commands, dialogs, secrets and the keyboard slot are not qualified"]
fn wallabag_setup() {
    CASES[0].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED NET/FS/JSON-stream: network, files and a fallible JSON stream reader are not on integration"]
fn wallabag_fetch() {
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED NET/FS/EVENT-page-turned/ANNOTATIONS: network, files, the reading event and annotations are not qualified"]
fn wallabag_archive() {
    CASES[2].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED WORK-declaration/NET: declared work and the network are not qualified for the native plugin"]
fn wallabag_fetch_due() {
    CASES[3].refuse().unwrap();
}
