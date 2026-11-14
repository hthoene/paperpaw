//! Acceptance of the transfer through the production loader with the native
//! plugin, pending on the host: serving with its key, files through the page,
//! installs, the cable. They never pass by themselves.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 5] = [
    // The page is served to the network, with its key; a few wrong keys lock it.
    // Native since slice 2; through the production loader it waits for the host.
    Case {
        name: "transfer-serving",
        golden: None,
        blocked_by: &["#944"],
    },
    // Files are put, fetched, renamed and deleted through the page; a book
    // goes into the library folder; a plugin is installed.
    Case {
        name: "transfer-files",
        golden: None,
        blocked_by: &["#944", "LIB-move"],
    },
    // The screen shows the address as text and as a QR code.
    Case {
        name: "transfer-screen",
        golden: None,
        blocked_by: &["#944", "H1", "H3"],
    },
    // The cable: an offer is asked about, allowed and taken once. Native since
    // slice 4 (`tests/cable_native.rs` pins the flow); through the production
    // loader it waits for the host's cable (Kernel #900).
    Case {
        name: "transfer-cable",
        golden: None,
        blocked_by: &["#900"],
    },
    // Typing on a phone for the keyboard.
    Case {
        name: "transfer-phone-keyboard",
        golden: None,
        blocked_by: &["#944"],
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
#[ignore = "BLOCKED #944: the host does not admit every declaration family (install_done, wifi_done, work_due)"]
fn transfer_serving() {
    CASES[0].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED #944/LIB-move: the host does not admit the plugin yet; the library does not follow a move"]
fn transfer_files() {
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED #944/H1/H3: the host does not admit the plugin yet; screen composition is not qualified for the native plugin"]
fn transfer_screen() {
    CASES[2].refuse().unwrap();
}

#[test]
#[ignore = "needs native activation (#900 cable host)"]
fn transfer_cable() {
    CASES[3].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED #944: the host does not admit the plugin yet (the phone's keyboard is native)"]
fn transfer_phone_keyboard() {
    CASES[4].refuse().unwrap();
}
