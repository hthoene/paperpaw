//! Acceptance of the wireless library through the host, pending on the
//! activation of native plugins and the host's sockets and library. Each case
//! is ignored and says what it waits for; none passes by itself.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 4] = [
    Case {
        name: "calibre-is-found-by-broadcast-or-by-its-address",
        golden: None,
        blocked_by: &["native-activation-944", "host-417", "sock-host-wiring"],
    },
    Case {
        name: "a-session-sends-the-book-list-and-answers-calibres-questions",
        golden: None,
        blocked_by: &["native-activation-944", "host-417", "sock-host-wiring"],
    },
    Case {
        name: "a-book-calibre-sends-lands-in-the-library",
        golden: None,
        blocked_by: &["native-activation-944", "host-417", "lib-import-host"],
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
#[ignore = "BLOCKED: activation of native plugins (#944, #417) and the host's sockets wiring"]
fn finding_and_talking_to_calibre() {
    CASES[0].refuse().unwrap();
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: activation of native plugins (#944, #417), sockets and library import on the host"]
fn receiving_books() {
    CASES[2].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: H1/H3/H4 and menu place (#944)"]
fn the_screen() {
    CASES[3].refuse().unwrap();
}
