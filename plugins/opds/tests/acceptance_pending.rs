//! Acceptance of the interface-7 catalogs through the host, pending on the
//! activation of native plugins and the shells' production services. The
//! engines are tested without the host (`native_browse_flow.rs`,
//! `native_sync_flow.rs`, `native_feeds.rs`, `native_catalogs.rs`,
//! `native_covers.rs`); no case here passes by itself, and none is faked.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 5] = [
    Case {
        name: "a-catalog-is-fetched-read-and-browsed",
        golden: None,
        blocked_by: &["native-activation-944", "host-417", "core-net-host-wiring"],
    },
    Case {
        name: "a-book-is-downloaded-into-the-library",
        golden: None,
        blocked_by: &["native-activation-944", "host-417", "lib-import-host"],
    },
    Case {
        name: "catalogs-are-added-edited-and-kept-with-their-passwords",
        golden: None,
        blocked_by: &["native-activation-944", "host-417", "H1", "H3", "H4"],
    },
    Case {
        name: "covers-show-in-lists",
        golden: None,
        blocked_by: &["native-activation-944", "host-417", "image-host-wiring"],
    },
    Case {
        name: "sync-fetches-new-books-of-marked-catalogs",
        golden: None,
        blocked_by: &["native-activation-944", "host-417", "work-host-wiring"],
    },
];

#[test]
fn every_case_still_has_its_reference() {
    for case in &CASES {
        assert!(case.golden_exists(), "{}", case.name);
    }
}

#[test]
#[ignore = "BLOCKED: activation of native plugins (#944, #417) and the shells' core.net wiring"]
fn browsing_a_catalog() {
    CASES[0].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: activation of native plugins (#944, #417) and library import on the host"]
fn downloading_books() {
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: activation of native plugins (#944, #417), H1/H3/H4 (screens on the host)"]
fn the_catalog_list_and_editor() {
    CASES[2].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: activation of native plugins (#944, #417) and the shells' image wiring"]
fn covers() {
    CASES[3].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: activation of native plugins (#944, #417) and the work service on the host"]
fn the_sync() {
    CASES[4].refuse().unwrap();
}
