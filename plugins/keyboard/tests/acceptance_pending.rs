//! Acceptance of the interface-7 port, pending on the host. The interface-6
//! keyboard is pinned by `typing.rs`, `presses.rs` and the other suites on a
//! kernel; these cases are the same behaviours through the production loader
//! with the native plugin. They never pass by themselves.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 5] = [
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
    // `type-text`) has no interface-7 input yet (draft 22: event.text_typed).
    Case {
        name: "keyboard-type-text",
        golden: None,
        blocked_by: &["H4"],
    },
    // Layouts other plugins add come by the stage `keyboard.layouts@1` (schema,
    // codec and the reading in `native_layouts`, compared with interface 6's
    // rules); the host does not run stages yet (slot 32), so a subscriber's
    // layout cannot be shown on the keyboard, and hiding layouts and variants
    // the fonts cannot draw needs the glyph query (`text.covers`, slot 39).
    Case {
        name: "keyboard-added-layouts",
        golden: None,
        blocked_by: &["stage-host-wiring", "text.covers"],
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
#[ignore = "BLOCKED stage-host-wiring, text.covers: the host does not run stages yet and has no glyph query facade"]
fn keyboard_added_layouts() {
    CASES[4].refuse().unwrap();
}
