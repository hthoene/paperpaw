//! Acceptance of the interface-7 port, pending on the host. The interface-6
//! settings plugin was pinned by `pages.rs`, `groups.rs` and the other suites
//! on a kernel (removed with it, #228); these cases are the same behaviours
//! through the production loader with the native plugin. They never pass by
//! themselves. What runs without the host is in `scripted_pages.rs`,
//! `scripted_secrets.rs` and `native_model` (the table in `PORTING-7.md`).
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 18] = [
    // The main page, a group's page and choosing a value draw as before.
    Case {
        name: "settings-pages",
        golden: None,
        blocked_by: &["H1", "H4"],
    },
    // A number changes with the buttons, shown at once through a preview and
    // kept or dropped; touch steps change it at once.
    Case {
        name: "settings-number-preview",
        golden: None,
        blocked_by: &["H1", "H4"],
    },
    // Resetting a page or all asks first and leaves language, folders and accounts.
    Case {
        name: "settings-reset",
        golden: None,
        blocked_by: &["H1", "H2", "H4"],
    },
    // Search asks for words and lists settings and pages that hold them.
    Case {
        name: "settings-search",
        golden: None,
        blocked_by: &["H1", "H2", "H4"],
    },
    // Passwords and keys: the list, typing one anew, removing one. The
    // screens are `native_secrets` and `native_plugin`; `tests/scripted_secrets.rs`
    // runs what they read from a scripted `core.secrets`, the questions asked
    // and the answers (typed again, removed, kept) with the scripted
    // `i18n` and `core.ui`. What stays is drawing the rows and moving the
    // focus and going back, which are screen commands.
    Case {
        name: "settings-secrets",
        golden: None,
        blocked_by: &["H1"],
    },
    // Restart PaperPaw once asked (`CommandSender::restart`, `uses.power`). Needs
    // the host's dialogs.
    Case {
        name: "settings-restart",
        golden: None,
        blocked_by: &["H2", "H4"],
    },
    // A widget placement's page (`widgets.clock@2`): its widget's settings with
    // the values that placement has (the widget's own where it sets none),
    // changed and reset on the placement alone.
    Case {
        name: "settings-placement-page",
        golden: None,
        blocked_by: &["H1", "H4"],
    },
    // The widget look row on the appearance page and its chooser: the looks
    // there are, *None*, the one in use marked, choosing one.
    Case {
        name: "settings-widget-look",
        golden: None,
        blocked_by: &["H1", "H4"],
    },
    // A widget drawn above a group's settings that it previews, redrawn as the
    // settings change. The host does not say which widget previews a group:
    // `reg_widget` has no `previews` field.
    Case {
        name: "settings-group-preview",
        golden: None,
        blocked_by: &["reg_widget.previews", "H3"],
    },
    // The size of the book text on a touch screen: a slider whose track is the
    // sizes the reading font draws differently, evenly spaced (a finger's
    // width moves the knob as far everywhere), and from the right in a
    // right-to-left language. The native plugin has no slider (numbers use
    // − and +); the kit draws none. Interface-6 `font_slider.rs`.
    Case {
        name: "settings-font-slider",
        golden: None,
        blocked_by: &["ui-kit.slider", "H1", "H4"],
    },
    // A page costs no more frames than with the pictures' look off: a group
    // opened or left draws once, without the second whole-screen frame
    // aside for pictures in grays (`core.pictures-in-grays`), and a frame of
    // the UI stays within 2,048 bytes on the X4. Needs the host's frame and
    // the screen's declaration that it has no pictures. Interface-6 `frames.rs`.
    Case {
        name: "settings-frames",
        golden: None,
        blocked_by: &["H1", "H3"],
    },
    // The pages turned or resized keep the row that has the focus and lay out
    // inside the new size (main page, a group's page, the search, the chooser);
    // the focus of a page shown again is kept in `scripted_pages.rs`, moving
    // it and laying out are the host's. Interface-6 `resize.rs`.
    Case {
        name: "settings-resize",
        golden: None,
        blocked_by: &["H1", "H4"],
    },
    // A group's page covered by another plugin's screen is not made anew for
    // each setting changed meanwhile, but once when it shows again. The host
    // does not say that a screen is covered (no event), so the native page
    // that is on top of its token is made anew for every change; only a page
    // below another of the same screen waits (`scripted_pages.rs`).
    // Interface-6 `covered.rs`.
    Case {
        name: "settings-covered-by-foreign-screen",
        golden: None,
        blocked_by: &["event.screen_covered"],
    },
    // An action a plugin adds to a group is its first row, with the state its
    // item shows at the right (a badge such as *New*). The native page shows
    // the row and runs the action (`scripted_pages.rs`) but not the item's
    // state: `Row::Action` takes no badge from the registry's menu row yet.
    // Interface-6 `groups.rs`.
    Case {
        name: "settings-group-action-state",
        golden: None,
        blocked_by: &["menu-row-badge", "H1"],
    },
    // The rows of a choice stand in the readable column on a wide screen, not
    // from edge to edge. Kit drawing. Interface-6 `pages.rs`.
    Case {
        name: "settings-wide-column",
        golden: None,
        blocked_by: &["H1"],
    },
    // The plugin manager and the plugin preferences are two rows with
    // distinct names and destinations, in English and German. Waits for the
    // plugin manager's place in the menu (#944). Interface-6 `pages.rs`
    // (`plugin_management_and_plugin_settings_have_distinct_names_and_destinations`,
    // ignored there for the same reason).
    Case {
        name: "settings-plugin-rows",
        golden: None,
        blocked_by: &["H1", "H3", "H4"],
    },
    // Resetting a book's values (the scope open on a screen of the book) sets
    // the book's values back, not the global ones: the reset goes through the
    // settings editor with "Only this book" cleared last. The keys that are
    // sent and their order are in `native_model` (`reset_plan`); that the
    // host's `reset_setting` honours the order, with the scope kept in the
    // book's settings file, and sends no event for a default that was not
    // changed, needs the host's settings editor and the confirmation dialog.
    // Interface-6 `pages.rs` (`resetting_a_books_values_does_not_fall_through_to_global_values`,
    // `reset_all_does_not_reload_unchanged_defaults_or_reread_the_card_for_them`).
    Case {
        name: "settings-reset-scope",
        golden: None,
        blocked_by: &["H1", "H2", "H4"],
    },
    // The value chosen in the chooser is kept at once and after a restart, the
    // language shows at once. The chooser's `close` and the `set_setting` that
    // follows it, and the registry of what was kept, are the host's: the
    // scripted host refuses the close, so the set is never sent
    // (`scripted_pages.rs` pins which row has the focus and that it closes).
    // Interface-6 `pages.rs`.
    Case {
        name: "settings-choose-value",
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
#[ignore = "BLOCKED H1/H4: native screen commands and input are not qualified"]
fn settings_pages() {
    CASES[0].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H4: native screen commands and input are not qualified"]
fn settings_number_preview() {
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H2/H4: native screen commands, dialogs and input are not qualified"]
fn settings_reset() {
    CASES[2].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H2/H4: native screen commands, dialogs and input are not qualified"]
fn settings_search() {
    CASES[3].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1: screen commands (focus redraw, back) and drawing are not qualified; the keep/remove answers run in scripted_secrets.rs"]
fn settings_secrets() {
    CASES[4].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H2/H4: dialogs and input are not qualified"]
fn settings_restart() {
    CASES[5].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H4: screen commands, drawing and input are not qualified; the id rules run in native_model tests"]
fn settings_placement_page() {
    CASES[6].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H4: screen commands, drawing and input are not qualified; the covered settings run in native_model tests"]
fn settings_widget_look() {
    CASES[7].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED reg_widget.previews, H3: the registry does not say which widget previews a group (asked of Kernel); composing needs the host's slot 35"]
fn settings_group_preview() {
    CASES[8].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED ui-kit.slider/H1/H4: the native plugin has no slider (numbers use − and +) and the kit draws none"]
fn settings_font_slider() {
    CASES[9].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H3: the host's frame and the screen's declaration of no pictures are not qualified"]
fn settings_frames() {
    CASES[10].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H4: turning, moving the focus and laying out are the host's; a page shown again keeps its focus in scripted_pages.rs"]
fn settings_resize() {
    CASES[11].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED event.screen_covered: the host does not say that a screen is covered; the page on top of its token is made anew for each change"]
fn settings_covered_by_foreign_screen() {
    CASES[12].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED menu-row-badge/H1: the group's action row shows no state (badge) of its item yet"]
fn settings_group_action_state() {
    CASES[13].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1: kit drawing of the readable column is not qualified"]
fn settings_wide_column() {
    CASES[14].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H3/H4: the plugin manager's menu place waits for the host (#944); its row is not in this app"]
fn settings_plugin_rows() {
    CASES[15].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H2/H4: settings editor, dialogs and input are not qualified; the keys and their order run in native_model tests"]
fn settings_reset_scope() {
    CASES[16].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H4: the chooser's close and the set that follows are screen commands the scripted host refuses"]
fn settings_choose_value() {
    CASES[17].refuse().unwrap();
}
