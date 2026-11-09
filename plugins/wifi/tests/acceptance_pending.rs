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
    // network, typing its password and forgetting one work. The screens are
    // `native_plugin`; `tests/scripted_networks.rs` runs what they ask the
    // device and the secrets. The scripted host does not answer
    // `i18n.copy` and `i18n.format` yet, so the rows' texts, the drawing and
    // the answers to the password question are not run.
    Case {
        name: "wifi-pages",
        golden: None,
        blocked_by: &["SCRIPTED-i18n"],
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
    // `wifi.ini` on the card read once at a start. What the plugin keeps and
    // asks for is `native_known`; the screens read the names kept (run in
    // `tests/scripted_networks.rs`); the `wifi.ini` is read at a start and its
    // networks kept (`native_ini`, compared with the old reading); keeping a
    // password once the device is in the network is wired (`native_plugin::events`); its typed-password
    // path is not run over the scripted dialogs yet.
    Case {
        name: "wifi-passwords",
        golden: None,
        blocked_by: &["SCRIPTED-typed-password"],
    },
    // The own network opened for the phone: its name and drawn password, the
    // token of its page, the join code (`native_own`), with the bytes from
    // `core.secrets` `random`. The setup screen prepares them from a fully
    // random draw only; opening the network and the served page wait.
    Case {
        name: "wifi-own-network",
        golden: None,
        blocked_by: &["NET-serve"],
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
#[ignore = "BLOCKED SCRIPTED-i18n: the scripted host does not answer i18n.copy and i18n.format"]
fn wifi_pages() {
    CASES[0].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED NET-serve: core.net serve/respond has no SDK side"]
fn wifi_phone() {
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED SCRIPTED-typed-password: the typed password kept once online is not run over the scripted dialogs"]
fn wifi_passwords() {
    CASES[2].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED NET-serve: opening the own network and the served page"]
fn wifi_own_network() {
    CASES[3].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1: status item callbacks are not qualified"]
fn wifi_status_item() {
    CASES[4].refuse().unwrap();
}
