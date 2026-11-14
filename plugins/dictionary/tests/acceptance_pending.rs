//! Acceptance of the interface-7 dictionary, pending on the host, the SDK and
//! the later slices. The interface-6 tests that drove a kernel (`App`) are
//! removed with the interface-6 plugin (#244); none of these cases passes by itself.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 4] = [
    Case {
        name: "the-provider-answers-from-the-stardict-dictionaries-of-the-folder",
        golden: None,
        blocked_by: &["lookup-broker-host"],
    },
    Case {
        name: "the-panel-shows-the-definitions-and-scrolls",
        golden: None,
        blocked_by: &["H1", "H3", "H4"],
    },
    Case {
        name: "first-lookup-prepares-the-indexes-in-the-background",
        golden: None,
        blocked_by: &["host-runs-work-due"],
    },
    Case {
        name: "other-providers-answering-later-and-going-online",
        golden: None,
        blocked_by: &["lookup-async-answers-todo-98"],
    },
];

#[test]
fn every_case_still_has_its_reference() {
    for case in &CASES {
        assert!(case.golden_exists(), "{}", case.name);
    }
}

#[test]
#[ignore = "BLOCKED: the host's lookup broker (the source over a card is tested in native_source.rs)"]
fn the_provider_reads_dictionaries() {
    CASES[0].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: H1/H3/H4: the host draws no native screen yet (the panel model is tested in native_panel/tests.rs)"]
fn the_panel() {
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: the host running work_due for the plugin (the builders are tested in native_source.rs)"]
fn background_indexing() {
    CASES[2].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: asynchronous lookup answers (todo 98)"]
fn providers_answering_later() {
    CASES[3].refuse().unwrap();
}
