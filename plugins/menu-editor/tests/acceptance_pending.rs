//! Acceptance of the interface-7 port, pending on the host. The first case has
//! its interface-6 golden; the others were pinned by kernel tests (`editing.rs`,
//! `circles.rs`, `sequences.rs` and the app's `raw_ids.rs` / `menu_entries.rs`)
//! that went with the interface-6 plugin. The cases never pass by themselves.
//! What needs no host (the menu graph, ordering, reachability, edits, navigation)
//! is pinned by the `native_menu` unit tests.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 7] = [
    // Opening the editor, the menu list and a finger dragging an entry.
    Case {
        name: "boox-menu-drag",
        golden: Some("boox-menu-drag"),
        blocked_by: &["H1", "H3", "H4"],
    },
    // The list of menus has submenus after their menu; a submenu is edited as its menu.
    Case {
        name: "menu-editor-submenus-after-their-menu",
        golden: None,
        blocked_by: &["H1", "H3", "H4"],
    },
    // An entry moved, removed or added is kept and read back by a new kernel; moving past either end changes nothing; a reset gives back what the plugins define, for one menu and for all.
    Case {
        name: "menu-editor-edits-are-kept",
        golden: None,
        blocked_by: &["H1", "H3", "H4"],
    },
    // The way to the settings cannot be removed; once another way is there it may go.
    Case {
        name: "menu-editor-protects-the-way-to-settings",
        golden: None,
        blocked_by: &["H1", "H3", "H4"],
    },
    // A menu is not offered inside its own submenu; menus caught in a circle by an edited file are listed and reset.
    Case {
        name: "menu-editor-circles",
        golden: None,
        blocked_by: &["H1", "H3", "H4"],
    },
    // A hand-written unknown entry shows by its id and can be removed.
    Case {
        name: "menu-editor-unknown-entry",
        golden: None,
        blocked_by: &["H1", "H3", "H4"],
    },
    // In every language shipped, every menu and entry of the editor, page by page, shows a name, never a raw id; the add screen says who took an entry out.
    Case {
        name: "menu-editor-names-every-menu-and-entry",
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
#[ignore = "BLOCKED H1/H3/H4: native screen commands, composition and input are not qualified"]
fn boox_menu_drag() {
    CASES[0].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H3/H4: native screen commands, composition and input are not qualified"]
fn menu_editor_submenus_after_their_menu() {
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H3/H4: native screen commands, composition and input are not qualified"]
fn menu_editor_edits_are_kept() {
    CASES[2].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H3/H4: native screen commands, composition and input are not qualified"]
fn menu_editor_protects_the_way_to_settings() {
    CASES[3].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H3/H4: native screen commands, composition and input are not qualified"]
fn menu_editor_circles() {
    CASES[4].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H3/H4: native screen commands, composition and input are not qualified"]
fn menu_editor_unknown_entry() {
    CASES[5].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H3/H4: native screen commands, composition and input are not qualified"]
fn menu_editor_names_every_menu_and_entry() {
    CASES[6].refuse().unwrap();
}
