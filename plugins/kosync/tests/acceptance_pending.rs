//! Acceptance of the interface-7 progress sync, pending on the SDK and the
//! host. The interface-6 tests drove a kernel (`App`) and went with its code;
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
        blocked_by: &["library-book-key-facade", "document-xpointer-facade"],
    },
    // *Go there* opens the book at the other device's place: the XPointer is
    // located in the book (or the percentage turned into a place) and the
    // reader is handed `path`, `section` and `offset`.
    Case {
        name: "go-there-opens-the-book-at-the-other-devices-place",
        golden: None,
        blocked_by: &["document-locate-facade", "reader-opens-at-place-host"],
    },
    // The account screen over a scripted host: typing the fields, signing in
    // and creating an account against the network service, the key kept among
    // the secrets, signing out.
    Case {
        name: "the-account-screen-signs-in-creates-and-signs-out",
        golden: None,
        blocked_by: &["ui-host-h1", "net-host"],
    },
    // The position the sync wrote reads back from the library as a sync write
    // (`set_position` with `sync`, `confirmed` after *Go there*).
    Case {
        name: "a-synced-position-is-stored-as-a-sync-write",
        golden: None,
        blocked_by: &["library-position-facade"],
    },
];

#[test]
fn every_case_still_has_its_reference() {
    for case in &CASES {
        assert!(case.golden_exists(), "{}", case.name);
    }
}

#[test]
#[ignore = "BLOCKED: the SDK has no facade for a book's fingerprint (core.library op 25) or a place's XPointer (core.document)"]
fn syncing_with_the_server() {
    CASES[0].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: no core.document locate facade, no reader that reads section and offset"]
fn going_there() {
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: core.ui and core.net hosts on the scripted host"]
fn the_account() {
    CASES[2].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: no core.library position facade (ops 13 and 14)"]
fn stored_positions() {
    CASES[3].refuse().unwrap();
}
