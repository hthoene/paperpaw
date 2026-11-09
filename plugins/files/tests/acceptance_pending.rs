//! Acceptance of the interface-7 library port, pending on the host and the
//! SDK. The interface-6 plugin and its kernel tests are removed; the native
//! screens are accepted against what Beta 3 showed once a run through the
//! production loader exists. None of these cases passes by itself
//! (`shells/desktop/tests/support/NATIVE-UI.md`).
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 13] = [
    Case {
        name: "the-library-lists-the-books-with-how-far-each-is-read",
        golden: None,
        blocked_by: &["H1", "H3", "H4", "lib-op-2-host-semantics"],
    },
    Case {
        name: "a-filter-shows-reading-unread-or-finished-books",
        golden: None,
        blocked_by: &["H1", "H3", "H4", "lib-op-2-host-semantics"],
    },
    Case {
        name: "a-long-library-reads-more-books-when-asked",
        golden: None,
        blocked_by: &["H1", "H3", "H4", "lib-op-2-host-semantics"],
    },
    Case {
        name: "the-favourites-list-their-books",
        golden: None,
        blocked_by: &["H1", "H3", "H4", "lib-op-2-host-semantics"],
    },
    Case {
        name: "authors-series-tags-and-folders-list-their-names-and-books",
        golden: None,
        blocked_by: &["lib-facets-host-semantics", "H1", "H3", "H4"],
    },
    Case {
        name: "the-library-follows-the-catalog-while-it-is-open",
        golden: None,
        blocked_by: &["H1", "H3", "H4"],
    },
    Case {
        name: "covers-show-beside-the-titles-and-in-the-grid-view",
        golden: None,
        blocked_by: &["H3"],
    },
    Case {
        name: "the-folder-chooser-sets-the-archive-folder",
        golden: None,
        blocked_by: &["H1", "H4"],
    },
    Case {
        name: "a-held-book-offers-its-actions-and-the-archive-moves-it",
        golden: None,
        blocked_by: &["lib-set-favourite-facade", "H2", "H4"],
    },
    Case {
        name: "find-by-letter-and-search-by-words",
        golden: None,
        blocked_by: &[
            "letters-jump-cursor",
            "lib-search-host-semantics",
            "H3",
            "H4",
        ],
    },
    Case {
        name: "book-information-shows-the-details",
        golden: None,
        blocked_by: &["lib-op-4-text-facade", "documents-metadata-facade", "H3"],
    },
    Case {
        name: "the-next-book-of-each-series-begun",
        golden: None,
        blocked_by: &[
            "lib-facets-host-semantics",
            "lib-next-in-series-host",
            "H1",
            "H3",
        ],
    },
    Case {
        name: "menus-help-hints-and-the-home-search-gesture",
        golden: None,
        blocked_by: &["menu-place-944", "help-hint-admission", "home-swipe-binding", "H4"],
    },
];

#[test]
fn every_case_still_has_its_reference() {
    for case in &CASES {
        assert!(case.golden_exists(), "{}", case.name);
    }
}

#[test]
#[ignore = "BLOCKED H1/H3/H4 and the host's library semantics: the native screens are not qualified"]
fn the_library_and_its_lists_behave_as_the_interface_6_screens_did() {
    for case in &CASES[..4] {
        case.refuse().unwrap();
    }
}

#[test]
#[ignore = "BLOCKED: the host's facets and search (the catalog behind LIB ops 5 and 2), the letters' jump"]
fn the_shelves_and_the_live_catalog() {
    CASES[4].refuse().unwrap();
    CASES[5].refuse().unwrap();
    CASES[9].refuse().unwrap();
    CASES[12].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: covers wait for the host's drawing (H3), the folder chooser for the host's screens and input (H1, H4)"]
fn covers_and_the_folder_chooser() {
    CASES[6].refuse().unwrap();
    CASES[7].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: the sheet's dialogs and the favourites facade, the tags, the language and the description of the information screen, menus, help hints and the home swipe"]
fn the_rest_of_the_library() {
    CASES[8].refuse().unwrap();
    CASES[10].refuse().unwrap();
    CASES[11].refuse().unwrap();
}
