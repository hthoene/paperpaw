//! Acceptance of the interface-7 port, pending on the host. The interface-6
//! Wi-Fi plugin is pinned by `radio.rs`, `networks.rs` and `phone.rs` on a
//! kernel; these cases are the same behaviours through the production loader
//! with the native plugin. They never pass by themselves.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 6] = [
    // The Wi-Fi page and the networks in reach draw as before; choosing a
    // network, typing its password and forgetting one work.
    Case {
        name: "wifi-pages",
        golden: None,
        blocked_by: &["H1", "H2", "H4"],
    },
    // The radio follows the policy: on when a plugin wants the network, joins
    // the network joined before, off after the idle time. Needs the events
    // `wifi_scanned`, `wifi_done`, `network_wanted` and `network_idle`
    // (92-95), which have no subscription or decoder in the SDK yet.
    Case {
        name: "wifi-radio-events",
        golden: None,
        blocked_by: &["EVT-wifi-radio"],
    },
    // The setup with a phone: the device's own network and a page it serves
    // at port 80 (`core.net` `serve` / `respond`). Its decisions and pages are
    // `native_phone` (compared with the old screen); serving them, the codes
    // and the screen are what waits.
    Case {
        name: "wifi-phone",
        golden: None,
        blocked_by: &["NET-serve"],
    },
    // Networks and passwords kept and forgotten through `core.secrets`, and a
    // `wifi.ini` on the card read once at a start. The secrets facade is on
    // integration (PR 367): what the plugin keeps and asks for is
    // `native_known`; the calls that use the service are not run without a
    // native entry to load in a scripted host.
    Case {
        name: "wifi-passwords",
        golden: None,
        blocked_by: &["native-entry", "FS"],
    },
    // The own network opened for the phone: its name and drawn password, the
    // token of its page, the join code (`native_own`), with the bytes from
    // `core.secrets` `random`.
    Case {
        name: "wifi-own-network",
        golden: None,
        blocked_by: &["native-entry", "NET-serve"],
    },
    // The status item shows the sign while the device is in a network.
    Case {
        name: "wifi-status-item",
        golden: None,
        blocked_by: &["H1"],
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
#[ignore = "BLOCKED H1/H2/H4: native screen commands, dialogs and input are not qualified"]
fn wifi_pages() {
    CASES[0].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED EVT-wifi-radio: no SDK subscription or decoder for events 92-95"]
fn wifi_radio_events() {
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED NET-serve: core.net serve/respond has no SDK side"]
fn wifi_phone() {
    CASES[2].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED native-entry/FS: no native entry to run the secrets calls in a scripted host, core.files is not on integration"]
fn wifi_passwords() {
    CASES[3].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED native-entry, NET-serve: the own network needs the native screen and the served page"]
fn wifi_own_network() {
    CASES[4].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1: status item callbacks are not qualified"]
fn wifi_status_item() {
    CASES[5].refuse().unwrap();
}
