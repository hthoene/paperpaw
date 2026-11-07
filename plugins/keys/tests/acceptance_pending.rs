//! Acceptance of the interface-7 port, pending on the host. The interface-6
//! plugin has no desktop golden; its behaviour is pinned by `tests/keys.rs` and
//! `tests/remap.rs`, which drive a kernel with the interface-6 plugin. Each
//! behaviour needs the same run through the production loader with the native
//! plugin; these cases never pass by themselves.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 3] = [
    // Open the list, choose an input, pick an action, see the binding apply.
    Case {
        name: "keys-rebind-applies",
        golden: None,
        blocked_by: &["H1", "H2", "H3", "H4"],
    },
    // Buttons-only device: a change leaving no button to choose, go back or move is refused.
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
];

#[test]
fn the_cases_name_what_they_wait_for() {
    for case in &CASES {
        assert!(case.golden_exists());
        let error = case.refuse().unwrap_err();
        assert!(error.starts_with("BLOCKED H1/"), "{error}");
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
