//! Acceptance of the interface-7 Wikipedia lookup, pending on the host and
//! the later slices; none of these cases passes by itself. The interface-6
//! tests that drove a kernel went with the interface-6 code.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 3] = [
    Case {
        name: "the-summary-screen-fetches-scrolls-and-closes",
        golden: None,
        blocked_by: &["H1", "H3", "H4"],
    },
    Case {
        name: "the-provider-answers-the-dictionary-panel-with-the-summary",
        golden: None,
        blocked_by: &["dictionary-panel-asker"],
    },
    Case {
        name: "the-about-line-names-the-source-and-the-licence",
        golden: None,
        blocked_by: &["about-lines-stage-reply"],
    },
];

#[test]
fn every_case_still_has_its_reference() {
    for case in &CASES {
        assert!(case.golden_exists(), "{}", case.name);
    }
}

#[test]
#[ignore = "BLOCKED: H1/H3/H4 (the host draws no native screen yet)"]
fn the_summary_screen() {
    CASES[0].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: the dictionary panel as the asker (the provider's flow: native_provider_flow.rs)"]
fn the_provider() {
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: the about.lines stage reply (a later slice)"]
fn the_licence_line() {
    CASES[2].refuse().unwrap();
}
