//! Acceptance of the interface-7 port, pending on the host. The interface-6
//! plugin is pinned by its tests on a kernel; these cases are the same
//! behaviours through the production loader with the native plugin. They never
//! pass by themselves.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 4] = [
    // The place is found by its name (Open-Meteo's search), chosen from the
    // places found and kept with its coordinates.
    Case {
        name: "weather-place",
        golden: None,
        blocked_by: &["H1", "H2", "KEYBOARD-slot", "NET"],
    },
    // The weather is fetched while a weather widget is placed, at most each
    // hour or as set, and kept in a small file for the next start.
    Case {
        name: "weather-fetch",
        golden: None,
        blocked_by: &["NET", "FS", "TIMER"],
    },
    // The weather now and the forecast widgets draw the symbols, the
    // temperatures and the days in the units set.
    Case {
        name: "weather-widgets",
        golden: None,
        blocked_by: &["H1", "H3"],
    },
    // A tap on a widget fetches at once, or opens the place's screen where
    // none is set.
    Case {
        name: "weather-tap",
        golden: None,
        blocked_by: &["H3", "H4"],
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
#[ignore = "BLOCKED H1/H2/KEYBOARD-slot/NET: screen commands, dialogs, the keyboard slot and the network are not qualified"]
fn weather_place() {
    CASES[0].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED NET/FS/TIMER: network, files and timers are not qualified for the native plugin"]
fn weather_fetch() {
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H3: screen commands and composition are not qualified for the native plugin"]
fn weather_widgets() {
    CASES[2].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H3/H4: composition and input are not qualified for the native plugin"]
fn weather_tap() {
    CASES[3].refuse().unwrap();
}
