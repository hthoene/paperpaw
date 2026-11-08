//! Acceptance of the interface-7 port, pending on the reader. The interface-6
//! text filters are pinned by `filters_tests.rs`; these cases are the same
//! behaviours through the production reader with the `reader.filter` stage.
//! They never pass by themselves.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 2] = [
    // The reader asks the stage for the rules (bold beginnings, replaced words,
    // pictures kept, as a caption or left out) and applies them to each block:
    // the same pages as the text filters made.
    Case {
        name: "aids-reader-filter",
        golden: None,
        blocked_by: &["READER-filter-stage"],
    },
    // A configuration the payload cannot hold (more than 32 pairs, a string
    // over 96 bytes, a reply over 3,840 bytes) is refused visibly before it
    // is adopted: the settings editor needs a validation hook for it.
    Case {
        name: "aids-refuse-oversize",
        golden: None,
        blocked_by: &["SETTINGS-validation"],
    },
];

#[test]
fn the_cases_name_what_they_wait_for() {
    for case in &CASES {
        assert!(case.golden_exists());
        let error = case.refuse().unwrap_err();
        assert!(error.starts_with("BLOCKED "), "{error}");
        assert!(error.contains(case.blocked_by[0]), "{error}");
    }
}

#[test]
#[ignore = "BLOCKED READER-filter-stage: the reader port does not call the stage yet"]
fn aids_reader_filter() {
    CASES[0].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED SETTINGS-validation: no hook to refuse a setting value that does not fit a payload"]
fn aids_refuse_oversize() {
    CASES[1].refuse().unwrap();
}
