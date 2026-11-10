//! Acceptance of the interface-7 port, pending on the host. `native_fetch_flow.rs`
//! pins the fetch against scripted services; these cases are the same
//! behaviours through the production loader with the native plugin. They
//! never pass by themselves.
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
        blocked_by: &["HOST-activation", "NET-shell", "FS"],
    },
    // Articles read to the end are archived with the next fetch and their books
    // removed unless they carry the reader's marks.
    Case {
        name: "wallabag-archive",
        golden: None,
        blocked_by: &["HOST-activation", "NET-shell", "FS"],
    },
    // A fetch started by a rule runs as declared work at a cheap moment.
    Case {
        name: "wallabag-fetch-due",
        golden: None,
        blocked_by: &["WORK-declaration", "HOST-activation"],
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
#[ignore = "BLOCKED HOST-activation/NET-shell/FS: the native plugin is not linked into the app yet and the shells have no core.net and storage production wiring"]
fn wallabag_fetch() {
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED HOST-activation/NET-shell/FS: the native plugin is not linked into the app yet and the shells have no core.net and storage production wiring"]
fn wallabag_archive() {
    CASES[2].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED WORK-declaration/HOST-activation: there is no native work declaration, and the plugin is not linked into the app yet"]
fn wallabag_fetch_due() {
    CASES[3].refuse().unwrap();
}
