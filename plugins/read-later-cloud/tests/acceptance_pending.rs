//! Acceptance of the interface-7 port, pending on the host. The interface-6
//! plugin is pinned by `fetch.rs` on a kernel; these cases are the same
//! behaviours through the production loader with the native plugin. They never
//! pass by themselves.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 4] = [
    // The token is typed on the keyboard (or a phone) and kept with the
    // secrets; the account told apart by a number made of it.
    Case {
        name: "read-later-cloud-setup",
        golden: None,
        blocked_by: &["H1", "H2", "SEC", "KEYBOARD-slot"],
    },
    // The articles of the chosen place come as HTML books of their text, at
    // most nineteen requests a fetch.
    Case {
        name: "read-later-cloud-fetch",
        golden: None,
        blocked_by: &["NET", "FS", "JSON-parser-feed"],
    },
    // Articles read to the end are archived with the next fetch and their
    // books removed unless they carry the reader's marks.
    Case {
        name: "read-later-cloud-archive",
        golden: None,
        blocked_by: &["NET", "FS", "EVENT-page-turned", "ANNOTATIONS"],
    },
    // A fetch started by a rule runs as declared work at a cheap moment.
    Case {
        name: "read-later-cloud-fetch-due",
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
fn read_later_cloud_setup() {
    CASES[0].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED NET/FS/JSON-parser-feed: network, files and the fallible parser feed are not wired for the plugin"]
fn read_later_cloud_fetch() {
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED NET/FS/EVENT-page-turned/ANNOTATIONS: network, files, the reading event and annotations are not qualified"]
fn read_later_cloud_archive() {
    CASES[2].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED WORK-declaration/NET: declared work and the network are not qualified for the native plugin"]
fn read_later_cloud_fetch_due() {
    CASES[3].refuse().unwrap();
}
