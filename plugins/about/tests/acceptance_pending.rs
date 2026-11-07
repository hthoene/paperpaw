//! Acceptance of the interface-7 port, pending on the host. Each case is the
//! interface-6 page seen through the production loader with the native plugin;
//! it never passes by itself (`shells/desktop/tests/support/NATIVE-UI.md`).
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 3] = [
    // The page: version, source, plugins built in, code plugins, licences.
    Case {
        name: "about-page",
        golden: None,
        blocked_by: &["H1", "H4"],
    },
    // Licences and the plugins built in open their text in the reader
    // (`command.open` into `core.reader`), which needs draft 22's
    // `doc_open.root` 5 so the reader may read About's own Data.
    Case {
        name: "about-open-licences-in-reader",
        golden: None,
        blocked_by: &["H1", "H4", "draft-22-doc_open-root-5"],
    },
    // Other plugins' lines (the `about.lines` stage) after the licences: the
    // host refuses the stage spec until its stages are wired.
    Case {
        name: "about-lines-from-plugins",
        golden: None,
        blocked_by: &["stage-host-wiring", "H1"],
    },
];

#[test]
fn the_cases_name_what_they_wait_for() {
    for case in &CASES {
        assert!(case.golden_exists());
        let error = case.refuse().unwrap_err();
        assert!(error.starts_with("BLOCKED "), "{error}");
    }
}

#[test]
#[ignore = "BLOCKED H1/H4: native screen commands and input are not qualified"]
fn about_page() {
    CASES[0].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: draft 22 doc_open.root 5 (opening About's texts in the reader)"]
fn about_open_licences_in_reader() {
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: the host does not yet admit stage specs (about.lines)"]
fn about_lines_from_plugins() {
    CASES[2].refuse().unwrap();
}
