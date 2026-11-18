//! Acceptance of the interface-7 port, pending on the host: the keyboard's
//! behaviours through the production loader with the native plugin (the
//! interface-6 suites that pinned them on a kernel are gone with that plugin).
//! They never pass by themselves. What the pure logic pins without the host
//! (keys, sizes, text limits, variants, focus moves, layouts) is in the
//! `native_model_*_tests` modules of the crate; `PORTING-7.md` lists which
//! interface-6 test is which.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 12] = [
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
    // `type-text`, now `event.text_typed`, 109): the plugin adds it to its
    // field like typed keys and `done` confirms; what needs the host is the
    // dialog on top and the dispatch of the event to it.
    Case {
        name: "keyboard-type-text",
        golden: None,
        blocked_by: &["H1", "H4"],
    },
    // Layouts other plugins add come by the stage `keyboard.layouts@1` (schema,
    // codec and the reading in `native_layouts`, tested against the rules of the
    // old line format); the host does not run stages yet (slot 32), so a subscriber's
    // layout cannot be shown on the keyboard. Hiding layouts and variants the
    // fonts cannot draw (`text.covers`, slot 39) is done and tested on what
    // the query answers; the host's own answer needs a font to ask.
    Case {
        name: "keyboard-added-layouts",
        golden: None,
        blocked_by: &["stage-host-wiring"],
    },
    // Finishing and cancelling the dialog with buttons: held Back keeps the
    // text and closes (on a letter too, with its capital shown, and typing a
    // password); a long press on Select off a letter, or before any key has the
    // focus, finishes; Back alone cancels without an answer; Back or a long
    // press while variants are shown closes them only.
    Case {
        name: "keyboard-finish-cancel",
        golden: None,
        blocked_by: &["H2", "H4"],
    },
    // The answer carries the question's context; a live question is answered
    // on each change (outcome typing) and finally once, one not live once; the
    // layout in use is kept for the next keyboard.
    Case {
        name: "keyboard-answers",
        golden: None,
        blocked_by: &["H2"],
    },
    // The field as drawn: lines as typed, a long line wraps, the field scrolls
    // to the end of the text, a plain field stays one line, the hint of the
    // letters held on the focused key ("Hold: ä") goes with the focus when a
    // tap takes it away (the field is drawn again), and the panel starts
    // under the status line on every screen.
    Case {
        name: "keyboard-field-lines",
        golden: None,
        blocked_by: &["H3", "H2"],
    },
    // Fingers: a long press on Delete clears, on a letter shows its capital and
    // variants (also at the screen's edges, where swipes begin, and in the
    // bottom row), in the text field it pastes; a tap on a variant types it,
    // anywhere else closes them.
    Case {
        name: "keyboard-touch-holds",
        golden: None,
        blocked_by: &["H4", "H2"],
    },
    // The layouts chooser: layouts are chosen in order (*On (2)* shows the
    // place in switching), *Automatic* stands alone and comes back when the last
    // layout is switched off, the choice is saved in `keyboard.layouts`, and a
    // built-in or repeated id is listed once.
    Case {
        name: "keyboard-layout-chooser",
        golden: None,
        blocked_by: &["H1", "H2"],
    },
    // Where the interface runs right to left, the keyboard's header is mirrored:
    // the title at the right, the hint of how typing ends at the left, and the
    // field's *Phone* button at the left end of the field (the interface-6
    // keyboard did this; the native paint and its field buttons do not mirror
    // yet: `Style::rtl` and `Style::flip` are not used by `native_paint`).
    Case {
        name: "keyboard-rtl",
        golden: None,
        blocked_by: &["native-rtl-mirroring", "H3"],
    },
    // On a touch screen, before any key has the focus, an action that is no move
    // (Home, another plugin's) passes the keyboard: it is not taken as the first
    // press that shows the focus.
    Case {
        name: "keyboard-other-actions",
        golden: None,
        blocked_by: &["H4"],
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
#[ignore = "BLOCKED H1/H4: the dialog on top and the dispatch of event 109 are not qualified"]
fn keyboard_type_text() {
    CASES[3].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED stage-host-wiring: the host does not run stages yet"]
fn keyboard_added_layouts() {
    CASES[4].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H2/H4: dialogs and input are not qualified"]
fn keyboard_finish_cancel() {
    CASES[5].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H2: dialogs are not qualified"]
fn keyboard_answers() {
    CASES[6].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H3/H2: composition and dialogs are not qualified"]
fn keyboard_field_lines() {
    CASES[7].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H4/H2: touch gestures and dialogs are not qualified"]
fn keyboard_touch_holds() {
    CASES[8].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H2: native screen commands and dialogs are not qualified"]
fn keyboard_layout_chooser() {
    CASES[9].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED native-rtl-mirroring: the native keyboard's paint does not mirror for right to left yet"]
fn keyboard_rtl() {
    CASES[10].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H4: input is not qualified"]
fn keyboard_other_actions() {
    CASES[11].refuse().unwrap();
}
