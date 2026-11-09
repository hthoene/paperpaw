//! Acceptance of the interface-7 port, pending on the host. The interface-6
//! settings plugin is pinned by `pages.rs`, `groups.rs` and the other suites
//! on a kernel; these cases are the same behaviours through the production
//! loader with the native plugin. They never pass by themselves.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 9] = [
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
