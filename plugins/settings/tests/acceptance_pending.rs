//! Acceptance of the interface-7 port, pending on the host. The interface-6
//! settings plugin is pinned by `pages.rs`, `groups.rs` and the other suites
//! on a kernel; these cases are the same behaviours through the production
//! loader with the native plugin. They never pass by themselves.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 6] = [
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
    // Passwords and keys: the list, typing one anew, removing one. Needs the
    // core.secrets SDK facade (SEC) and the host service.
    Case {
        name: "settings-secrets",
        golden: None,
        blocked_by: &["SEC", "H1", "H2", "H4"],
    },
    // Restart PaperPaw once asked. Needs the DEV restart command facade.
    Case {
        name: "settings-restart",
        golden: None,
        blocked_by: &["DEV", "H2"],
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
#[ignore = "BLOCKED SEC: no core.secrets SDK facade or host service yet"]
fn settings_secrets() {
    CASES[4].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED DEV: no restart command facade yet (H10)"]
fn settings_restart() {
    CASES[5].refuse().unwrap();
}
