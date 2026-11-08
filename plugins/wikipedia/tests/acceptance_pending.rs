//! Acceptance of the interface-7 Wikipedia lookup, pending on the host and
//! the later slices. The interface-6 tests (`tests/in_the_panel.rs`,
//! `tests/lookup.rs`) drive a kernel and stay as the references; none of
//! these cases passes by itself.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 3] = [
    Case {
        name: "the-summary-screen-fetches-scrolls-and-closes",
        golden: None,
        blocked_by: &["core-net-host", "H1", "H3", "H4"],
    },
    Case {
        name: "the-provider-answers-the-dictionary-panel-with-the-summary",
        golden: None,
        blocked_by: &["lookup-stage-host", "asynchronous-stage-reply"],
    },
    Case {
        name: "the-lookup-slot-opens-the-summary-and-about-names-the-licence",
        golden: None,
        blocked_by: &["slot-binding-command", "about-lines-stage-reply"],
    },
];

#[test]
fn every_case_still_has_its_reference() {
    for case in &CASES {
        assert!(case.golden_exists(), "{}", case.name);
    }
}

#[test]
#[ignore = "BLOCKED: core.net host, H1/H3/H4"]
fn the_summary_screen() {
    CASES[0].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: the lookup stage in the host and the asynchronous stage reply (ASY)"]
fn the_provider() {
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: the slot binding (a command in the first event) and the about.lines stage reply (a later slice)"]
fn the_slot_and_the_licence_line() {
    CASES[2].refuse().unwrap();
}
