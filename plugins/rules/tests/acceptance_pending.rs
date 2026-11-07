//! Acceptance of the interface-7 port, pending on the host. The interface-6
//! plugin has its behaviour pinned by `tests/battery.rs`, `tests/timing.rs` and
//! `tests/picking.rs`, which run it on a kernel. These cases are the same
//! behaviours through the production loader with the native plugin; they never
//! pass by themselves.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 5] = [
    // A level rule applies once each time the level is reached, not while it wavers.
    Case {
        name: "rules-battery-level",
        golden: None,
        blocked_by: &["H1"],
    },
    // A time-of-day rule applies at its minute on its days, on the host's minute tick.
    Case {
        name: "rules-time-of-day",
        golden: None,
        blocked_by: &["H1"],
    },
    // Tapping a rule pauses it; the list says it is paused.
    Case {
        name: "rules-pause",
        golden: None,
        blocked_by: &["H1", "H4"],
    },
    // Adding a rule asks when, what and which action or setting.
    Case {
        name: "rules-add",
        golden: None,
        blocked_by: &["H1", "H2", "H4"],
    },
    // Holding a rule offers a condition or removal.
    Case {
        name: "rules-hold-options",
        golden: None,
        blocked_by: &["H1", "H2", "H4"],
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
#[ignore = "BLOCKED H1: native screen commands and events are not qualified"]
fn rules_battery_level() {
    CASES[0].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1: native screen commands and events are not qualified"]
fn rules_time_of_day() {
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H4: native screen commands and input are not qualified"]
fn rules_pause() {
    CASES[2].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H2/H4: native screen commands, dialogs and input are not qualified"]
fn rules_add() {
    CASES[3].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H2/H4: native screen commands, dialogs and input are not qualified"]
fn rules_hold_options() {
    CASES[4].refuse().unwrap();
}
