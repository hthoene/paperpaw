//! Remaining full-host acceptance of the interface-7 progress sync. The interface-6 tests drove a kernel (`App`) and went with its code;
//! their scenarios are `tests/native_sync.rs` (the engine) and the unit tests of
//! `native_account`, `native_flow` and `native_store`. None of these cases passes
//! by itself.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 4] = [
    // A book opened is fetched by its fingerprint and, where another device is
    // further, offered; the position is sent while reading and when the book
    // closes, with the XPointer where the format has one.
    Case {
        name: "a-book-opened-fetches-and-offers-a-further-position",
        golden: None,
        blocked_by: &["native-reader-events-host"],
    },
    // *Go there* opens the book at the other device's place: the XPointer is
    // located in the book (or the percentage turned into a place) and the
    // reader is handed `path`, `section` and `offset`.
    Case {
        name: "go-there-opens-the-book-at-the-other-devices-place",
        golden: None,
        blocked_by: &["scripted-host-navigation-and-handover"],
    },
    // The account screen over a scripted host: typing the fields, signing in
    // and creating an account against the network service, the key kept among
    // the secrets, signing out.
    Case {
        name: "the-account-screen-signs-in-creates-and-signs-out",
        golden: None,
        blocked_by: &["ui-host-h1", "net-host"],
    },
    // Only after the user accepts *Go there*, the reader consumes the handover
    // and saves its reached position. Unaccepted remote positions are not stored.
    Case {
        name: "the-reader-persists-the-position-after-an-accepted-handover",
        golden: None,
        blocked_by: &["reader-confirmed-handover-persistence-host"],
    },
];

#[test]
fn every_case_still_has_its_reference() {
    for case in &CASES {
        assert!(case.golden_exists(), "{}", case.name);
    }
}

#[test]
#[ignore = "PENDING: real native reader events; native_server covers fingerprint fetch and offer"]
fn syncing_with_the_server() {
    CASES[0].refuse().unwrap();
}

#[test]
#[ignore = "PENDING: ScriptedHost has no navigation authority or handover service"]
fn going_there() {
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: core.ui and core.net hosts on the scripted host"]
fn the_account() {
    CASES[2].refuse().unwrap();
}

#[test]
#[ignore = "PENDING: reader persistence after accepted handover; no unconfirmed sync write required"]
fn stored_positions() {
    CASES[3].refuse().unwrap();
}
