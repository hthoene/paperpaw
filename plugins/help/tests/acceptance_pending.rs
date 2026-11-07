//! Acceptance of the interface-7 port, pending on the host. The interface-6
//! plugin has no desktop golden; `tests/help.rs` pins its behaviour on a
//! kernel with the interface-6 plugin. Each case needs the same run through
//! the production loader with the native plugin; these never pass by themselves.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 3] = [
    // A screen with hints opens: the hints show over it once, a tap closes them.
    Case {
        name: "help-first-open",
        golden: None,
        blocked_by: &["H1", "H3", "H4"],
    },
    // No more hints switches them all off; Show hints again has each shown once more.
    Case {
        name: "help-switch-off-and-again",
        golden: None,
        blocked_by: &["H1", "H4"],
    },
    // A plugin's question opened over the hints closes them; they show next time.
    Case {
        name: "help-covered",
        golden: None,
        blocked_by: &["H1", "H2", "H3"],
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
#[ignore = "BLOCKED H1/H3/H4: native screen commands, composition and input are not qualified"]
fn help_first_open() {
    CASES[0].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H4: native screen commands and input are not qualified"]
fn help_switch_off_and_again() {
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H2/H3: native screen commands, dialogs and composition are not qualified"]
fn help_covered() {
    CASES[2].refuse().unwrap();
}
