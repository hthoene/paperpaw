//! Acceptance of the interface-7 port, pending on the host. The interface-6
//! transfer is pinned by `serving.rs`, `cable.rs`, `plugins.rs` and
//! `upload_memory.rs` on a kernel; these cases are the same behaviours
//! through the production loader with the native plugin. They never pass by
//! themselves.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 5] = [
    // The page is served to the network, with its key; a few wrong keys lock it.
    Case {
        name: "transfer-serving",
        golden: None,
        blocked_by: &["SOCK", "WIFI"],
    },
    // Files are put, fetched, renamed and deleted through the page; a book
    // goes into the library folder, a plugin is installed.
    Case {
        name: "transfer-files",
        golden: None,
        blocked_by: &["FS", "SOCK", "INSTALL-design"],
    },
    // The screen shows the address as text and as a QR code.
    Case {
        name: "transfer-screen",
        golden: None,
        blocked_by: &["H1", "H3"],
    },
    // The cable: an offer is asked about, allowed and taken once.
    Case {
        name: "transfer-cable",
        golden: None,
        blocked_by: &["H2", "CABLE-stage", "INSTALL-design"],
    },
    // Typing on a phone for the keyboard.
    Case {
        name: "transfer-phone-keyboard",
        golden: None,
        blocked_by: &["SOCK", "H1", "H4"],
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
#[ignore = "BLOCKED SOCK/WIFI: core.sockets and the Wi-Fi state are not on integration"]
fn transfer_serving() {
    CASES[0].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED FS/SOCK/INSTALL-design: files, sockets and the native install path are not on integration"]
fn transfer_files() {
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H3: screen commands and composition are not qualified for the native plugin"]
fn transfer_screen() {
    CASES[2].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H2/CABLE-stage/INSTALL-design: dialogs, the cable stage and the install path are not on integration"]
fn transfer_cable() {
    CASES[3].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED SOCK/H1/H4: sockets, screen commands and input are not qualified"]
fn transfer_phone_keyboard() {
    CASES[4].refuse().unwrap();
}
