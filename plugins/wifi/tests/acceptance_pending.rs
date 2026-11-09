//! Acceptance of the interface-7 port, pending on the host. The interface-6
//! Wi-Fi plugin is pinned by `radio.rs`, `networks.rs` and `phone.rs` on a
//! kernel; these cases are the same behaviours through the production loader
//! with the native plugin. They never pass by themselves.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 5] = [
    // The Wi-Fi page and the networks in reach draw as before; choosing a
    // network, typing its password and forgetting one work.
    Case {
        name: "wifi-pages",
        golden: None,
        blocked_by: &["H1", "H2", "H4"],
    },
    // The radio follows the policy through the production loader: on when a
    // plugin wants the network, joins the network joined before, off after the
    // idle time. The events 92-95 are subscribed and the decisions ported
    // (`native_radio`); what is missing is the timers (the idle-off and retry
    // `work` entries) and the callbacks that read the facts and send the
    // commands.
    Case {
        name: "wifi-radio-events",
        golden: None,
        blocked_by: &["WORK-timers", "H4"],
    },
    // The setup with a phone: the device's own network and a page it serves
    // at port 80 (`core.net` `serve` / `respond`).
    Case {
        name: "wifi-phone",
        golden: None,
        blocked_by: &["NET-serve"],
    },
    // Networks and passwords kept and forgotten through `core.secrets`, and a
    // `wifi.ini` on the card read once at a start.
    Case {
        name: "wifi-passwords",
        golden: None,
        blocked_by: &["SEC", "FS"],
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
#[ignore = "BLOCKED WORK-timers/H4: the idle-off and retry timers and the native callbacks are not wired"]
fn wifi_radio_events() {
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED NET-serve: core.net serve/respond has no SDK side"]
fn wifi_phone() {
    CASES[2].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED SEC/FS: secrets facade (PR 367) and core.files are not on integration"]
fn wifi_passwords() {
    CASES[3].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1: status item callbacks are not qualified"]
fn wifi_status_item() {
    CASES[4].refuse().unwrap();
}
