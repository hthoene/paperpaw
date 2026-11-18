//! Acceptance of the interface-7 browser, pending on the host and the later
//! slices. The interface-6 tests that drove a kernel (`tests/browsing.rs`) went
//! with the interface-6 code; none of these cases passes by itself.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 5] = [
    // The start screen: Search or enter address, the bookmarks and the pages
    // read lately; holding an entry offers open / bookmark / delete; the
    // offer to go online in Offline mode; messages at the bottom.
    Case {
        name: "browser-start-screen",
        golden: None,
        blocked_by: &["H1", "H2", "H4"],
    },
    // Loading an address with `core.net` (GET, `save_to` the data folder),
    // telling the answer apart, keeping a page with its `<base>` and opening
    // it in the reader, Back through the pages of this time.
    Case {
        name: "browser-load-and-read",
        golden: None,
        blocked_by: &["core-net-host", "DOC-open-page"],
    },
    // Books to the library (`library.import`, free name), fonts to the fonts
    // folder, other files to Downloads (`core.files` move), and the offer to
    // read a book now.
    Case {
        name: "browser-downloads",
        golden: None,
        blocked_by: &["LIB-import", "FS-core-files", "H2"],
    },
    // A saved book renamed after its author and title (interface 6
    // `named_book`): the document service's metadata (`doc_meta`, op 3), which
    // the SDK has no call for yet.
    Case {
        name: "browser-book-named-by-metadata",
        golden: None,
        blocked_by: &["DOC-metadata"],
    },
    // The slot `core.browser`: the reader opens links out of books in the
    // browser's screen, with the address as a screen parameter.
    Case {
        name: "browser-link-from-a-book",
        golden: None,
        blocked_by: &["H1", "slot-core-browser-params"],
    },
];

#[test]
fn every_case_still_has_its_reference() {
    for case in &CASES {
        assert!(case.golden_exists(), "{}", case.name);
        let error = case.refuse().unwrap_err();
        assert!(error.starts_with("BLOCKED "), "{error}");
        assert!(error.contains(case.blocked_by[0]), "{error}");
    }
}

#[test]
#[ignore = "BLOCKED H1/H2/H4: native screen commands, dialogs and input are not qualified"]
fn the_start_screen() {
    CASES[0].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED core-net-host, DOC-open-page: no host for requests, no open-page command"]
fn loading_and_reading() {
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED LIB-import, FS-core-files, H2: downloads cannot be placed"]
fn downloads() {
    CASES[2].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED DOC-metadata: no call for a document's title and author"]
fn a_book_named_by_its_metadata() {
    CASES[3].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1, slot-core-browser-params: the screen cannot take a parameter"]
fn a_link_from_a_book() {
    CASES[4].refuse().unwrap();
}
