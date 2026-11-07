//! Acceptance of the interface-7 port, pending on the host. The interface-6
//! keyboard is pinned by `typing.rs`, `presses.rs` and the other suites on a
//! kernel; these cases are the same behaviours through the production loader
//! with the native plugin. They never pass by themselves.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 6] = [
    // Typing on the keys, shift and its lock, variants, delete, finish and
    // cancel, answering through core.ui.
    Case {
        name: "keyboard-typing",
        golden: None,
        blocked_by: &["H1", "H2", "H4"],
    },
    // A password shows dots, is never put on the clipboard or sent to the
    // phone, and keeps the edges and the buttons to itself.
    Case {
        name: "keyboard-secret",
        golden: None,
        blocked_by: &["H2", "H4"],
    },
    // Paste from the clipboard and type on the phone (the nested question).
    Case {
        name: "keyboard-paste-phone",
        golden: None,
        blocked_by: &["H2", "H4"],
    },
    // Text from a hardware keyboard or a remote control (interface 6
    // `type-text`) has no interface-7 input yet.
    Case {
        name: "keyboard-type-text",
        golden: None,
        blocked_by: &["H4"],
    },
    // Layouts other plugins add (point `keyboard.layouts`) and hiding layouts
    // the fonts cannot draw need an extension point and a glyph query.
    Case {
        name: "keyboard-added-layouts",
        golden: None,
        blocked_by: &["EXT", "H1"],
    },
    // Showing *Phone* only where there is a network needs the DEV capability
    // facade.
    Case {
        name: "keyboard-phone-network",
        golden: None,
        blocked_by: &["DEV", "H2"],
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
#[ignore = "BLOCKED H1/H2/H4: native screen commands, dialogs and input are not qualified"]
fn keyboard_typing() {
    CASES[0].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H2/H4: dialogs and input are not qualified"]
fn keyboard_secret() {
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H2/H4: dialogs and input are not qualified"]
fn keyboard_paste_phone() {
    CASES[2].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H4: no interface-7 type-text input yet"]
fn keyboard_type_text() {
    CASES[3].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED EXT: no keyboard.layouts extension point or glyph query yet"]
fn keyboard_added_layouts() {
    CASES[4].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED DEV: no network capability facade yet"]
fn keyboard_phone_network() {
    CASES[5].refuse().unwrap();
}
