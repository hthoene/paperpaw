//! Acceptance of the interface-7 plugin store, pending on the host and the SDK.
//! The interface-6 tests drive a kernel (`App`) and stay as the references
//! (`tests/browse.rs`, `tests/code.rs`, `tests/store.rs`, …); none of these
//! cases passes by itself.
//!
//! Owner decisions that apply when the install is wired: a plugin-triggered
//! install or update needs the host's confirmation (132), and a change of the
//! firmware always confirms (134). Kernel todo 145 (the install design for
//! native plugins) is open: the install is modelled as pending host.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 6] = [
    Case {
        name: "the-library-is-fetched-and-indexed",
        golden: None,
        blocked_by: &["core-net-host", "core-files-facade-384", "H1", "H3", "H4"],
    },
    Case {
        name: "a-plugin-is-downloaded-checked-and-installed",
        golden: None,
        blocked_by: &[
            "install-design-todo-145",
            "core-net-host",
            "host-confirmation-132",
        ],
    },
    Case {
        name: "a-code-plugin-is-checked-signed-and-its-key-remembered",
        golden: None,
        blocked_by: &["install-design-todo-145", "author-keys-facade"],
    },
    Case {
        name: "revoked-keys-are-handed-to-the-kernel",
        golden: None,
        blocked_by: &["install-design-todo-145", "author-keys-facade"],
    },
    Case {
        name: "updates-are-queued-and-a-firmware-change-confirms",
        golden: None,
        blocked_by: &[
            "host-confirmation-132",
            "firmware-confirmation-134",
            "work-service-sdk",
        ],
    },
    Case {
        name: "the-first-run-setup-offers-the-plugins-most-readers-add",
        golden: None,
        blocked_by: &[
            "core-registry-op-22-host-todo-99",
            "install-design-todo-145",
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
fn fetching_the_library() {
    CASES[0].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: the install design for native plugins (Kernel todo 145), core.net and the host's confirmation (132)"]
fn installing_a_plugin() {
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: the install design (Kernel todo 145) and the author-keys facade"]
fn code_plugins_and_their_keys() {
    CASES[2].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: the install design (Kernel todo 145) and the author-keys facade"]
fn revoked_keys() {
    CASES[3].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: the host's confirmation (132 and 134) and the work SDK"]
fn updates_and_the_firmware() {
    CASES[4].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: core.registry op 22 (Host todo 99) and the install design"]
fn the_first_run_offer() {
    CASES[5].refuse().unwrap();
}
