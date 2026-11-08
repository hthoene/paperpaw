//! Acceptance of the interface-7 progress sync, pending on the host, the SDK
//! and the later slices. The interface-6 tests drive a kernel (`App`) and stay
//! as the references (`src/tests.rs`, `tests/account.rs`, `tests/sync.rs`);
//! none of these cases passes by itself.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 4] = [
    Case {
        name: "a-book-opened-fetches-and-offers-a-further-position",
        golden: None,
        blocked_by: &[
            "core-net-host",
            "secrets-facade",
            "book-events-host",
            "document-position-sdk",
        ],
    },
    Case {
        name: "the-position-is-sent-while-reading-and-when-the-book-closes",
        golden: None,
        blocked_by: &[
            "core-net-host",
            "secrets-facade",
            "book-events-host",
            "work-service-sdk",
        ],
    },
    Case {
        name: "the-account-screen-signs-in-creates-and-signs-out",
        golden: None,
        blocked_by: &["core-net-host", "secrets-facade", "H1", "H3", "H4"],
    },
    Case {
        name: "the-confirmation-overlay-offers-and-reports",
        golden: None,
        blocked_by: &["H1", "H3", "H4", "menu-place-944"],
    },
];

#[test]
fn every_case_still_has_its_reference() {
    for case in &CASES {
        assert!(case.golden_exists(), "{}", case.name);
    }
}

#[test]
#[ignore = "BLOCKED: core.net host, the secrets facade, book events and the document position"]
fn syncing_with_the_server() {
    CASES[0].refuse().unwrap();
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: core.net host, the secrets facade and H1/H3/H4"]
fn the_account() {
    CASES[2].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: H1/H3/H4 and menu place (#944)"]
fn the_overlay() {
    CASES[3].refuse().unwrap();
}
