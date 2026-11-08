//! Acceptance of the interface-7 port, pending on the host and the SDK. The
//! interface-6 plugin had no pictures of its own (its tests read the screen's
//! description through a kernel), so every case is behavioural; none passes by
//! itself (`shells/desktop/tests/support/NATIVE-UI.md`).
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 16] = [
    Case {
        name: "a-plugin-installed-while-the-list-is-open-is-listed",
        golden: None,
        blocked_by: &["H1", "H3", "H4", "registry-through-loader"],
    },
    Case {
        name: "turned-the-list-stays-open-with-its-focus",
        golden: None,
        blocked_by: &["H1", "H3", "H4"],
    },
    Case {
        name: "opening-a-long-list-reads-the-rows-shown-not-all-descriptions",
        golden: None,
        blocked_by: &["H1", "H3", "H4", "registry-through-loader"],
    },
    Case {
        name: "every-page-of-a-long-list-is-in-the-language-of-the-card",
        golden: None,
        blocked_by: &["H1", "H3", "H4", "registry-through-loader"],
    },
    Case {
        name: "the-pages-of-a-long-list-stay-as-they-are-once-seen",
        golden: None,
        blocked_by: &["H1", "H3", "H4"],
    },
    Case {
        name: "a-new-fallback-is-not-offered-for-the-slot",
        golden: None,
        blocked_by: &["H2", "registry-through-loader"],
    },
    Case {
        name: "a-new-screen-bound-as-high-is-offered-once",
        golden: None,
        blocked_by: &["H2", "registry-through-loader"],
    },
    Case {
        name: "a-slot-offered-twice-is-chosen-and-kept",
        golden: None,
        blocked_by: &["H1", "H3", "H4", "H2", "registry-through-loader"],
    },
    Case {
        name: "safe-mode-and-not-installed-notices-at-start",
        golden: None,
        blocked_by: &["device-reads-through-loader"],
    },
    Case {
        name: "holding-a-plugin-shows-how-it-works",
        golden: None,
        blocked_by: &["plugin-how-text-read"],
    },
    Case {
        name: "holding-a-plugin-that-says-nothing-more-keeps-the-list",
        golden: None,
        blocked_by: &["plugin-how-text-read"],
    },
    Case {
        name: "a-plugin-needed-for-places-names-them",
        golden: None,
        blocked_by: &["reg-plugin-required-for"],
    },
    Case {
        name: "a-plugin-not-from-the-store-says-so",
        golden: None,
        blocked_by: &["plugin-origin-read"],
    },
    Case {
        name: "no-room-names-the-web-flasher-on-the-stock-layout",
        golden: None,
        blocked_by: &["code-room-larger-layout"],
    },
    Case {
        name: "restart-now-applies-a-switch",
        golden: None,
        blocked_by: &["restart-now-sender-342", "H1", "H3", "H4"],
    },
    Case {
        name: "the-reading-app-is-chosen-from-screens",
        golden: None,
        blocked_by: &["setting-editor-opener"],
    },
];

#[test]
fn the_cases_name_what_they_wait_for() {
    for case in &CASES {
        assert!(case.golden_exists(), "{}", case.name);
        let error = case.refuse().unwrap_err();
        assert!(error.starts_with("BLOCKED "), "{error}");
        assert!(!case.blocked_by.is_empty(), "{}", case.name);
    }
}

#[test]
#[ignore = "BLOCKED H1/H3/H4 and the registry through the loader: the screens are not qualified"]
fn the_list_and_the_places_behave_as_the_interface_6_screens_did() {
    for case in &CASES[..8] {
        case.refuse().unwrap();
    }
}

#[test]
#[ignore = "BLOCKED: the device reads through the loader"]
fn the_notices_at_a_start_are_said() {
    CASES[8].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: the registry gives no how-it-works text, no places a plugin is needed for, no origin and no larger code layout"]
fn the_lines_the_registry_cannot_tell_yet_are_said() {
    for case in &CASES[9..14] {
        case.refuse().unwrap();
    }
}

#[test]
#[ignore = "BLOCKED: `restart` has no SDK sender until the typed DEV commands (#342); the reading app's setting editor has no opener"]
fn the_restart_and_the_reading_app() {
    CASES[14].refuse().unwrap();
    CASES[15].refuse().unwrap();
}
