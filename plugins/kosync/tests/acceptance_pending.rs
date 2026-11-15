//! Remaining real-kernel reader acceptance. Account login/create/restart/sign-out
//! is covered by native_host; fetch/offer/close push and reply refusals by
//! native_server; the shown More-menu by native_menu.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 2] = [
    Case {
        name: "go-there-opens-the-book-at-the-other-devices-place",
        golden: None,
        blocked_by: &["native-reader-handover-acceptance-pending"],
    },
    // Beta 3 policy: only accepted navigation reaches the reader's normal save.
    // Unconfirmed remote positions are never written into the library by kosync.
    Case {
        name: "the-reader-persists-the-position-after-an-accepted-handover",
        golden: None,
        blocked_by: &["native-reader-persistence-acceptance-pending"],
    },
];

#[test]
fn every_case_still_has_its_reference() {
    for case in &CASES {
        assert!(case.golden_exists(), "{}", case.name);
    }
}

#[test]
#[ignore = "PENDING: real-reader handover test is unfinished; uncatalogued book_key needs Kernel #3124"]
fn going_there() {
    CASES[0].refuse().unwrap();
}

#[test]
#[ignore = "PENDING: prove reader persistence after accepted handover; no unconfirmed sync write required"]
fn stored_positions() {
    CASES[1].refuse().unwrap();
}
