//! Acceptance of the interface-7 port, pending on the host and the SDK. Each
//! case is the interface-6 screen (its pictures are kept in
//! `shells/desktop/tests/snapshots`) seen through the production loader with
//! the native plugin; it never passes by itself
//! (`shells/desktop/tests/support/NATIVE-UI.md`).
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 12] = [
    Case {
        name: "x4-endbook-next",
        golden: Some("x4-endbook-next"),
        blocked_by: &["H1", "H3", "H4", "book-field-declaration"],
    },
    Case {
        name: "x4-endbook-next-de",
        golden: Some("x4-endbook-next-de"),
        blocked_by: &["H1", "H3", "H4", "book-field-declaration"],
    },
    Case {
        name: "x4-endbook-lone",
        golden: Some("x4-endbook-lone"),
        blocked_by: &["H1", "H3", "H4", "book-field-declaration"],
    },
    Case {
        name: "x4-endbook-lone-de",
        golden: Some("x4-endbook-lone-de"),
        blocked_by: &["H1", "H3", "H4", "book-field-declaration"],
    },
    // The rating: read back (LIB op 22) and written.
    Case {
        name: "x4-endbook-rated",
        golden: Some("x4-endbook-rated"),
        blocked_by: &["H1", "H3", "H4", "lib-fields-read"],
    },
    Case {
        name: "x4-endbook-large",
        golden: Some("x4-endbook-large"),
        blocked_by: &["H1", "H3", "H4"],
    },
    Case {
        name: "x4-endbook-large-de",
        golden: Some("x4-endbook-large-de"),
        blocked_by: &["H1", "H3", "H4"],
    },
    // Select on the first row opens the next book in the reader.
    Case {
        name: "x4-endbook-opened-next",
        golden: Some("x4-endbook-opened-next"),
        blocked_by: &["H1", "H4"],
    },
    // The behaviours the interface-6 tests drove on a real kernel and library.
    Case {
        name: "next-book-skips-read-and-gone-books",
        golden: None,
        blocked_by: &["lib-op-6-host-semantics", "lib-file-present-draft-23"],
    },
    Case {
        name: "finished-mark-kept-and-said-again-at-close",
        golden: None,
        blocked_by: &["book-events-host"],
    },
    Case {
        name: "rating-kept-in-the-books-field-and-badge",
        golden: None,
        blocked_by: &[
            "book-field-declaration",
            "core.book_badge-stage",
            "lib-fields-read",
        ],
    },
    Case {
        name: "a-book-gone-from-the-card-has-no-screen",
        golden: None,
        blocked_by: &["book-events-host", "lib-file-present-draft-23"],
    },
];

#[test]
fn the_cases_name_what_they_wait_for() {
    for case in &CASES {
        assert!(case.golden_exists(), "{}", case.name);
        let error = case.refuse().unwrap_err();
        assert!(error.starts_with("BLOCKED "), "{error}");
    }
}

#[test]
#[ignore = "BLOCKED H1/H3/H4: native screen commands, composition and input are not qualified"]
fn the_screen_matches_its_interface_6_pictures() {
    for case in CASES.iter().filter(|case| case.golden.is_some()) {
        case.refuse().unwrap();
    }
}

#[test]
#[ignore = "BLOCKED: the SDK reads no book field back (LIB op 22) and has no book field or badge declaration"]
fn the_rating_is_kept_and_shown_as_a_badge() {
    CASES[10].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: the book events and the library need the host qualified through the loader"]
fn the_finished_mark_and_the_next_book_follow_the_library() {
    CASES[8].refuse().unwrap();
    CASES[9].refuse().unwrap();
    CASES[11].refuse().unwrap();
}
