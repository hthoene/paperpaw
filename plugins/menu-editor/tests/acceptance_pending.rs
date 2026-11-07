//! Acceptance against the interface-6 goldens, pending on the host.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

/// Opening the editor, the menu list and a finger dragging an entry.
const MENU_DRAG: Case = Case {
    golden: "boox-menu-drag",
    blocked_by: &["H1", "H3", "H4"],
};

#[test]
fn the_goldens_are_still_there() {
    assert!(MENU_DRAG.golden_exists());
}

#[test]
#[ignore = "BLOCKED H1/H3/H4: native screen commands, composition and input are not qualified"]
fn boox_menu_drag() {
    MENU_DRAG.refuse().unwrap();
}

#[test]
fn the_refusal_names_the_missing_hosts() {
    let error = MENU_DRAG.refuse().unwrap_err();
    assert!(error.starts_with("BLOCKED H1/H3/H4:"), "{error}");
}
