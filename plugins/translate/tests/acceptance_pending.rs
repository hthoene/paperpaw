//! Acceptance of the interface-7 translation plugin, pending on the host and
//! the later slices. The interface-6 test (`tests/panel.rs`) drives a kernel
//! and stays as the reference; none of these cases passes by itself.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 2] = [
    Case {
        name: "the-panel-is-drawn-over-the-lower-part-of-the-page",
        golden: None,
        blocked_by: &["kernel-backed-pixel-harness"],
    },
    Case {
        name: "the-readers-translate-action-opens-the-panel-with-the-selected-text",
        golden: None,
        blocked_by: &["reader-translate-slot-host"],
    },
];

#[test]
fn every_case_still_has_its_reference() {
    for case in &CASES {
        assert!(case.golden_exists(), "{}", case.name);
    }
}

#[test]
#[ignore = "BLOCKED: a pixel harness backed by the kernel (as Battery's), for the panel's drawing"]
fn the_panel_is_drawn() {
    CASES[0].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: the reader's slot host (the offer is submitted since #522; nothing yet reads it from the reader's side)"]
fn the_reader_opens_the_panel() {
    CASES[1].refuse().unwrap();
}
