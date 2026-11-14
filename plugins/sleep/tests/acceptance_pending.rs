//! Acceptance of the interface-7 port, pending on the host. The interface-6
//! sleep screen is pinned by `sleep_screen.rs` and `over_page.rs` on a kernel
//! and by the goldens; these cases are the same behaviours through the
//! production loader with the native plugin. They never pass by themselves.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 5] = [
    // The sleeping cat (decoded for the screen's size into the data folder), the
    // cover of the book read last, and a picture of the folder, one after the
    // other, draw as before.
    Case {
        name: "sleep-images",
        golden: None,
        blocked_by: &["H1"],
    },
    // The message with PaperPaw's logo, the page as it is, and a mode of
    // another plugin shown in place.
    Case {
        name: "sleep-screen",
        golden: None,
        blocked_by: &["H1"],
    },
    // The widgets (and a widget over the page) composed with `ui.compose`.
    Case {
        name: "sleep-widgets",
        golden: None,
        blocked_by: &["H1", "H3"],
    },
    // Asleep, the screen takes every button but Sleep and Home, and its hints
    // stay hidden (declared, `hints_hidden`; the host's input waits for H4).
    Case {
        name: "sleep-buttons-and-hints",
        golden: None,
        blocked_by: &["H4"],
    },
    // The screen closes when the device wakes; what opened as it woke stays.
    Case {
        name: "sleep-wake",
        golden: None,
        blocked_by: &["H1", "H4"],
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
#[ignore = "BLOCKED H1: the native plugin is not constructed by the production loader yet (the images are ported, not accepted)"]
fn sleep_images() {
    CASES[0].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1: native screen commands are not qualified"]
fn sleep_screen() {
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H3: screen commands and composition are not qualified for the native plugin"]
fn sleep_widgets() {
    CASES[2].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H4: input is not qualified"]
fn sleep_buttons_and_hints() {
    CASES[3].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H4: screen commands and input are not qualified"]
fn sleep_wake() {
    CASES[4].refuse().unwrap();
}
