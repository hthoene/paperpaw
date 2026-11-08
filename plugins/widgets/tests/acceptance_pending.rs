//! Acceptance of the interface-7 port, pending on the host. The interface-6
//! pack is pinned by `tests/drawing.rs` on a kernel; each behaviour needs the
//! same run through the production loader with the native plugin. These cases
//! never pass by themselves.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 3] = [
    // The day widget on the home screen in each style, with the frame and
    // look settings applied, at the sizes a cell takes.
    Case {
        name: "widgets-day-draws-in-each-style",
        golden: None,
        blocked_by: &["H1", "H3"],
    },
    // A widget whose content changes at the minute tells its maker.
    Case {
        name: "widgets-live-minute-update",
        golden: None,
        blocked_by: &["H1", "H3"],
    },
    // With no time zone chosen the times follow the shell's UTC offset (the
    // snapshot's `utc_offset_minutes`, draft 25); until then they read as UTC.
    Case {
        name: "widgets-system-utc-offset",
        golden: None,
        blocked_by: &["H1", "H3"],
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
#[ignore = "BLOCKED H1/H3: native screen commands and composition are not qualified"]
fn widgets_day_draws_in_each_style() {
    CASES[0].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H3: native screen commands and composition are not qualified"]
fn widgets_live_minute_update() {
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H3 and draft 25: the snapshot has no UTC offset yet, so an unset zone reads as UTC"]
fn widgets_system_utc_offset() {
    CASES[2].refuse().unwrap();
}
