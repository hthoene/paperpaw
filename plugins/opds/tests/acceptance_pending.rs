//! Acceptance of the interface-7 catalogs, pending on the host, the SDK and
//! the later slices. The interface-6 tests drive a kernel (`App`) and stay as
//! the references (`tests/browse*.rs`, `tests/catalogs.rs`, `tests/covers.rs`,
//! `tests/sync*.rs`); none of these cases passes by itself.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 5] = [
    Case {
        name: "a-catalog-is-fetched-read-and-browsed",
        golden: None,
        blocked_by: &["core-net-host", "core-files-facade-384", "H1", "H3", "H4"],
    },
    Case {
        name: "a-book-is-downloaded-into-the-library",
        golden: None,
        blocked_by: &["core-net-host", "lib-import-sdk", "core-files-facade-384"],
    },
    Case {
        name: "catalogs-are-added-edited-and-kept-with-their-passwords",
        golden: None,
        blocked_by: &["secrets-facade", "storage-files-sdk", "H1", "H3", "H4"],
    },
    Case {
        name: "covers-show-in-lists",
        golden: None,
        blocked_by: &["core-net-host", "image-slots-385"],
    },
    Case {
        name: "sync-fetches-new-books-of-marked-catalogs",
        golden: None,
        blocked_by: &[
            "core-net-host",
            "lib-import-sdk",
            "work-service-sdk",
            "secrets-facade",
        ],
    },
];

#[test]
fn every_case_still_has_its_reference() {
    for case in &CASES {
        assert!(case.golden_exists(), "{}", case.name);
    }
}

#[test]
#[ignore = "BLOCKED: core.net host, the files facade (#384) and H1/H3/H4"]
fn browsing_a_catalog() {
    CASES[0].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: core.net host, library import (LIB) and the files facade"]
fn downloading_books() {
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: the secrets facade, storage/files SDK and H1/H3/H4"]
fn the_catalog_list() {
    CASES[2].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: core.net host and image slots (#385)"]
fn covers() {
    CASES[3].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: core.net host, library import, the work SDK and secrets"]
fn the_sync() {
    CASES[4].refuse().unwrap();
}
