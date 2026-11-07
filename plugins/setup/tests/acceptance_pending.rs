//! Acceptance of the interface-7 port, pending on the host. The interface-6
//! guide was pinned on a kernel by tests that went with it (card, guide walk,
//! switch step, wander); its logic (what an input means, the layout) is pinned
//! without the host in `src/native_guide.rs` and `src/native_layout.rs`. These
//! cases are the same behaviours through the production loader with the native
//! plugin; they never pass by themselves.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 5] = [
    // The first start opens the guide once; a start after it is done does not.
    Case {
        name: "setup-first-start",
        golden: None,
        blocked_by: &["H1"],
    },
    // The first page, the dots and the card of a choice of two.
    Case {
        name: "setup-first-page",
        golden: None,
        blocked_by: &["H1", "H3", "H4"],
    },
    // A number with a sample of the text at that size.
    Case {
        name: "setup-text-size",
        golden: None,
        blocked_by: &["H1", "H3", "H4"],
    },
    // Back on the first page asks whether to skip; skipping ends the guide for good.
    Case {
        name: "setup-skip",
        golden: None,
        blocked_by: &["H1", "H2", "H4"],
    },
    // A step another plugin owns (a screen) opens from the card.
    Case {
        name: "setup-screen-step",
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
#[ignore = "BLOCKED H1: native screen commands and events are not qualified"]
fn setup_first_start() {
    CASES[0].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H3/H4: native screen commands, composition and input are not qualified"]
fn setup_first_page() {
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H3/H4: native screen commands, composition and input are not qualified"]
fn setup_text_size() {
    CASES[2].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H2/H4: native screen commands, dialogs and input are not qualified"]
fn setup_skip() {
    CASES[3].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H3/H4: native screen commands, composition and input are not qualified"]
fn setup_screen_step() {
    CASES[4].refuse().unwrap();
}
