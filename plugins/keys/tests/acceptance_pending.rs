//! Acceptance of the interface-7 port, pending on the host. The interface-6
//! plugin had no desktop golden; its behaviour was pinned by kernel tests
//! (`keys.rs`, `remap.rs`) that went with it. Each case below is one of those
//! behaviours, to be run through the production loader with the native plugin;
//! the cases never pass by themselves. What needs no host (layer order,
//! sections, the buttons-only refusal, places, tap zones) is pinned by the
//! `model` unit tests.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 9] = [
    // Open the list, choose an input, pick an action, see the binding apply: a held button by buttons, a tap zone by touch with the search.
    Case {
        name: "keys-rebind-applies",
        golden: None,
        blocked_by: &["H1", "H2", "H3", "H4"],
    },
    // Buttons-only device: a change leaving no button to choose, go back or move is refused; select moves to another button first; touch screens reach everything without buttons.
    Case {
        name: "keys-buttons-only-refusal",
        golden: None,
        blocked_by: &["H1", "H3", "H4"],
    },
    // Reset buttons and gestures asks first, then takes every user binding back.
    Case {
        name: "keys-reset-all",
        golden: None,
        blocked_by: &["H1", "H2", "H4"],
    },
    // *As standard* takes the user binding away again, and the input does what plugins and the layout bind.
    Case {
        name: "keys-as-standard-removes",
        golden: None,
        blocked_by: &["H1", "H3", "H4"],
    },
    // *Nothing* switches an input off: it does nothing, and is listed as nothing.
    Case {
        name: "keys-nothing-switches-off",
        golden: None,
        blocked_by: &["H1", "H3", "H4"],
    },
    // The same input bound twice keeps the last binding; a binding for one screen wins on that screen only.
    Case {
        name: "keys-same-input-twice-keeps-last",
        golden: None,
        blocked_by: &["H1", "H3", "H4"],
    },
    // A place writes its own layer, which applies only there.
    Case {
        name: "keys-place-layer",
        golden: None,
        blocked_by: &["H1", "H3", "H4"],
    },
    // The gestures of a place switch round (as standard, own, none) on touch screens and are saved as the place policy.
    Case {
        name: "keys-gesture-policy",
        golden: None,
        blocked_by: &["H1", "H3", "H4"],
    },
    // Only the device buttons are listed; two presses of the same button are listed; a binding to an unknown action shows its id.
    Case {
        name: "keys-listed-inputs",
        golden: None,
        blocked_by: &["H1", "H3", "H4"],
    },
];

#[test]
fn the_cases_name_what_they_wait_for() {
    for case in &CASES {
        assert!(case.golden_exists());
        let error = case.refuse().unwrap_err();
        assert!(error.starts_with("BLOCKED H1"), "{error}");
    }
}

#[test]
#[ignore = "BLOCKED H1/H2/H3/H4: native screen commands, dialogs, composition and input are not qualified"]
fn keys_rebind_applies() {
    CASES[0].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H3/H4: native screen commands, composition and input are not qualified"]
fn keys_buttons_only_refusal() {
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H2/H4: native screen commands, dialogs and input are not qualified"]
fn keys_reset_all() {
    CASES[2].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H3/H4: native screen commands, composition and input are not qualified"]
fn keys_as_standard_removes() {
    CASES[3].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H3/H4: native screen commands, composition and input are not qualified"]
fn keys_nothing_switches_off() {
    CASES[4].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H3/H4: native screen commands, composition and input are not qualified"]
fn keys_same_input_twice_keeps_last() {
    CASES[5].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H3/H4: native screen commands, composition and input are not qualified"]
fn keys_place_layer() {
    CASES[6].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H3/H4: native screen commands, composition and input are not qualified"]
fn keys_gesture_policy() {
    CASES[7].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H3/H4: native screen commands, composition and input are not qualified"]
fn keys_listed_inputs() {
    CASES[8].refuse().unwrap();
}
