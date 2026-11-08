//! Acceptance of the interface-7 dictionary, pending on the host, the SDK and
//! the later slices. The interface-6 tests drive a kernel (`App`) and stay as
//! the references (`tests/lookup*.rs`, `tests/providers.rs`); none of these
//! cases passes by itself.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 4] = [
    Case {
        name: "the-provider-answers-from-the-stardict-dictionaries-of-the-folder",
        golden: None,
        blocked_by: &[
            "core-files-facade-384",
            "stardict-on-native-files",
            "lookup-broker-host",
        ],
    },
    Case {
        name: "the-panel-shows-the-definitions-and-scrolls",
        golden: None,
        blocked_by: &["slice-2-panel", "H1", "H3", "H4"],
    },
    Case {
        name: "first-lookup-prepares-the-indexes-in-the-background",
        golden: None,
        blocked_by: &["work-service-sdk", "stardict-on-native-files"],
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
#[ignore = "BLOCKED: core.files facade (#384), StarDict on native files, the host's lookup broker"]
fn the_provider_reads_dictionaries() {
    CASES[0].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: the panel (slice 2) and H1/H3/H4"]
fn the_panel() {
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: the work service SDK and StarDict on native files"]
fn background_indexing() {
    CASES[2].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: asynchronous lookup answers (todo 98)"]
fn providers_answering_later() {
    CASES[3].refuse().unwrap();
}
