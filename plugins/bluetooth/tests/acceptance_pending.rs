//! Acceptance of the interface-7 port, pending on the host and the SDK. The
//! interface-6 plugin had no pictures of its own (its tests drove a kernel
//! with a radio), so every case is behavioural; none passes by itself
//! (`shells/desktop/tests/support/NATIVE-UI.md`).
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 13] = [
    Case {
        name: "remotes-connect-by-hand-never-by-themselves",
        golden: None,
        blocked_by: &[
            "bluetooth-radio-through-loader",
            "bluetooth-slot-bound-by-host",
        ],
    },
    Case {
        name: "remotes-connect-while-a-book-is-open",
        golden: None,
        blocked_by: &[
            "bluetooth-radio-through-loader",
            "bluetooth-slot-bound-by-host",
        ],
    },
    Case {
        name: "remotes-connect-always-at-start-and-on-waking",
        golden: None,
        blocked_by: &[
            "bluetooth-radio-through-loader",
            "bluetooth-slot-bound-by-host",
        ],
    },
    Case {
        name: "a-remote-switched-on-by-hand-is-wanted-again-at-start",
        golden: None,
        blocked_by: &[
            "bluetooth-radio-through-loader",
            "bluetooth-slot-bound-by-host",
        ],
    },
    Case {
        name: "the-page-lists-the-bonded-devices-and-switches-remotes",
        golden: None,
        blocked_by: &["H1", "H3", "H4", "bluetooth-radio-through-loader"],
    },
    Case {
        name: "holding-a-device-forgets-it",
        golden: None,
        blocked_by: &["H1", "H3", "H4", "bluetooth-radio-through-loader"],
    },
    Case {
        name: "a-sensor-is-given-to-a-plugin-and-taken-back",
        golden: None,
        blocked_by: &["H1", "H3", "H4", "bluetooth-radio-through-loader"],
    },
    Case {
        name: "adding-searches-connects-pairs-and-goes-back",
        golden: None,
        blocked_by: &["H1", "H3", "H4", "bluetooth-radio-through-loader"],
    },
    Case {
        name: "a-declined-pairing-says-so",
        golden: None,
        blocked_by: &["H1", "H3", "H4", "bluetooth-radio-through-loader"],
    },
    Case {
        name: "the-page-is-hidden-where-there-is-no-radio",
        golden: None,
        blocked_by: &["menu-need-host-admission-944"],
    },
    Case {
        name: "the-status-item-shows-the-sign-while-a-device-is-connected",
        golden: None,
        blocked_by: &[
            "status-item-stage-callback",
            "status-item-drawn-icon-widget-toolkit",
        ],
    },
    Case {
        name: "the-plugin-is-bound-to-the-bluetooth-slot",
        golden: None,
        blocked_by: &["slot-offer-host-admission-944"],
    },
    Case {
        name: "how-the-plugin-works-is-shown",
        golden: None,
        blocked_by: &["plugin-how-text-read"],
    },
];

#[test]
fn every_case_still_has_its_reference() {
    for case in &CASES {
        assert!(case.golden_exists(), "{}", case.name);
    }
}

#[test]
#[ignore = "BLOCKED: no Bluetooth radio through the production loader, and the host binds no plugin to core.bluetooth yet"]
fn remotes_connect_as_the_setting_says() {
    for case in &CASES[..4] {
        case.refuse().unwrap();
    }
}

#[test]
#[ignore = "BLOCKED H1/H3/H4 and the radio through the loader: the screens are not qualified"]
fn the_page_and_adding_a_device_behave_as_the_interface_6_screens_did() {
    for case in &CASES[4..9] {
        case.refuse().unwrap();
    }
}

#[test]
#[ignore = "BLOCKED #944: the host refuses menu needs and slot offers until its menus and slots take them"]
fn the_page_is_where_a_radio_is_and_fills_the_slot() {
    CASES[9].refuse().unwrap();
    CASES[11].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: the plugin has no callback for the host's status stage `core.status`, and drawing the sign needs the native widget toolkit"]
fn the_status_item_shows_a_connected_device() {
    CASES[10].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: the registry gives no how-it-works text"]
fn how_the_plugin_works_is_shown() {
    CASES[12].refuse().unwrap();
}
