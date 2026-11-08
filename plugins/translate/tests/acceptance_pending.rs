//! Acceptance of the interface-7 translation plugin, pending on the host and
//! the later slices. The interface-6 test (`tests/panel.rs`) drives a kernel
//! and stays as the reference; none of these cases passes by itself.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 4] = [
    Case {
        name: "the-panel-asks-deepl-and-shows-the-translation-scrolls-and-closes",
        golden: None,
        blocked_by: &["slice-3-panel-screen"],
    },
    Case {
        name: "without-a-key-the-panel-asks-for-it-and-keeps-it-with-the-secrets",
        golden: None,
        blocked_by: &["keyboard-slot-ask-text"],
    },
    Case {
        name: "offline-mode-offers-to-go-online-first",
        golden: None,
        blocked_by: &["slice-3-panel-screen", "offline-offer-widget"],
    },
    Case {
        name: "the-readers-translate-action-opens-the-panel-with-the-selected-text",
        golden: None,
        blocked_by: &["slice-3-panel-screen", "reader-translate-slot-host"],
    },
];

#[test]
fn every_case_still_has_its_reference() {
    for case in &CASES {
        assert!(case.golden_exists(), "{}", case.name);
    }
}

#[test]
#[ignore = "BLOCKED: slice 3 (the panel screen); the asking is run by tests/native_asker.rs"]
fn the_panel_translates() {
    CASES[0].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: the keyboard slot (the key is kept by tests/native_asker.rs)"]
fn the_key_is_asked_for_and_kept() {
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: slice 3 panel screen, offline offer"]
fn offline_mode_asks_first() {
    CASES[2].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: slice 3 panel screen, the reader's translate slot"]
fn the_reader_opens_the_panel() {
    CASES[3].refuse().unwrap();
}
