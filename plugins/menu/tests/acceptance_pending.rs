//! Acceptance of the interface-7 port, pending on the host. The interface-6
//! menu is pinned by `menus.rs`, `settings.rs`, `states.rs` and
//! `definitions.rs` on a kernel; these cases are the same behaviours through
//! the production loader with the native plugin. They never pass by
//! themselves.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 7] = [
    // The main menu and the tools: every entry opens what it says, back and
    // the menu button close, taps outside close, long menus are paged.
    Case {
        name: "menu-list",
        golden: Some("x4-main-menu"),
        blocked_by: &["H1", "H3", "H4"],
    },
    // Switches, numbers (by buttons and by touch) and choices change in place.
    Case {
        name: "menu-settings",
        golden: None,
        blocked_by: &["H1", "H4"],
    },
    // A submenu opens over the menu; an action closes every menu below it.
    Case {
        name: "menu-submenus",
        golden: None,
        blocked_by: &["H1", "H4"],
    },
    // An entry shown as a switch that is off: the row's flags carry only
    // "checked", so a switch off looks like a plain action (registry.md 3).
    Case {
        name: "menu-switch-off",
        golden: None,
        blocked_by: &["REG"],
    },
    // Tiles (the menu style) and the reader's bar draw icons. Both are
    // ported (tests/bar_mode.rs, tests/tiles_mode.rs); their golden waits for
    // the host.
    Case {
        name: "menu-tiles-and-bar",
        golden: Some("x4-reader-menu"),
        blocked_by: &["ICON", "H1"],
    },
    // The quick panel (core.panel): switches, sliders and tiles in a sheet.
    Case {
        name: "menu-panel",
        golden: Some("x4-quick-panel"),
        blocked_by: &["ICON", "SLIDER", "H1", "H4"],
    },
    // A menu follows registry changes while it is open.
    Case {
        name: "menu-live",
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
#[ignore = "BLOCKED H1/H3/H4: native screen commands, composition and input are not qualified"]
fn menu_list() {
    CASES[0].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H4: native screen commands and input are not qualified"]
fn menu_settings() {
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H4: native screen commands and input are not qualified"]
fn menu_submenus() {
    CASES[2].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED REG: reg_menu_row flags cannot tell a switch that is off from an action"]
fn menu_switch_off() {
    CASES[3].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED ICON: the canvas has no icon operation"]
fn menu_tiles_and_bar() {
    CASES[4].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED ICON/SLIDER: no icon operation, no slider in the kit"]
fn menu_panel() {
    CASES[5].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H4: native screen commands and input are not qualified"]
fn menu_live() {
    CASES[6].refuse().unwrap();
}
