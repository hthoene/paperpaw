//! Acceptance of the interface-7 port, pending on the host. The interface-6
//! plugin is pinned by `send.rs` and `setup.rs` on a kernel; these cases are
//! the same behaviours through the production loader with the native plugin.
//! They never pass by themselves.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 4] = [
    // The account is typed on the keyboard (or a phone) and kept: the service,
    // address, user and notebook as settings, the password with the secrets.
    Case {
        name: "joplin-setup",
        golden: None,
        blocked_by: &["H1", "H2", "SEC", "KEYBOARD-slot"],
    },
    // The notes of an export are written as a note in the notebook, over the
    // sync target's own locks, and an edited note is not written over.
    Case {
        name: "joplin-send",
        golden: None,
        blocked_by: &["NET", "FS", "EVENT-notes-exported", "XML"],
    },
    // Sync targets with end-to-end encryption get nothing.
    Case {
        name: "joplin-encrypted-target",
        golden: None,
        blocked_by: &["NET", "FS"],
    },
    // Notes that wait are sent again at the next connection, and by the action
    // *Send to Joplin* (also from a rule, as declared work).
    Case {
        name: "joplin-retry",
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
fn joplin_setup() {
    CASES[0].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED NET/FS/EVENT-notes-exported/XML: network, files, the export event and the XML reader are not qualified"]
fn joplin_send() {
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED NET/FS: network and files are not on integration"]
fn joplin_encrypted_target() {
    CASES[2].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED WORK-declaration/NET: declared work and the network are not qualified for the native plugin"]
fn joplin_retry() {
    CASES[3].refuse().unwrap();
}
