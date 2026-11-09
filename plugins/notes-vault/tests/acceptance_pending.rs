//! Acceptance of the interface-7 notes vault, pending on the hosts and the
//! later slices. The interface-6 tests drive a kernel (`tests/vault.rs`,
//! `tests/markdown.rs`, `src/*` tests) and stay as the references; none of
//! these cases passes by itself. The Markdown provider's own behaviour runs
//! in `tests/provider.rs`, `tests/markdown_allocation.rs` and
//! `tests/native_ui_declarations.rs`.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 6] = [
    Case {
        name: "the-page-indexes-the-vault-in-the-background-and-says-how-many-notes",
        golden: None,
        blocked_by: &["H1", "H4"],
    },
    Case {
        name: "folders-tags-and-search-list-the-notes",
        golden: None,
        blocked_by: &["H1", "H4"],
    },
    Case {
        name: "a-note-opens-in-the-reader-and-its-links-lead-on-and-back",
        golden: None,
        blocked_by: &["DOC-format-declaration", "DOC-format-card-path", "H3", "H4"],
    },
    Case {
        name: "markdown-notes-are-a-format-for-md-and-markdown",
        golden: None,
        blocked_by: &["DOC-format-declaration", "DOC-format-card-path"],
    },
    Case {
        name: "the-widgets-show-recent-pinned-daily-random-tags-and-search",
        golden: None,
        blocked_by: &["widget-host-call", "book-field-declaration", "H4"],
    },
    Case {
        name: "the-tools-entry-and-icon-open-the-page",
        golden: None,
        blocked_by: &["registry-through-loader", "menu-place-944"],
    },
];

#[test]
fn every_case_still_has_its_reference() {
    for case in &CASES {
        assert!(case.golden_exists(), "{}", case.name);
    }
}

#[test]
#[ignore = "BLOCKED: H1/H4: the screens are native (native_home.rs, native_list.rs, native_tags.rs) but nothing is drawn or taken through a host yet; the scripted host has no core.files"]
fn the_page_and_its_lists() {
    CASES[0].refuse().unwrap();
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED DOC-format-declaration, DOC-format-card-path: the host takes no format.spec yet (it answers ERR_UNSUPPORTED) and puts no card path in fmt.call.text (Kernel todo 336, Host todos 329/109); the provider and its parity with interface 6 run in tests/provider.rs"]
fn notes_and_the_markdown_format() {
    CASES[2].refuse().unwrap();
    CASES[3].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: the widget host call and the book fields"]
fn the_widgets() {
    CASES[4].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: the registry through the loader and the menu place (#944)"]
fn the_entry() {
    CASES[5].refuse().unwrap();
}
