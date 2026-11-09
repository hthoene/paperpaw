//! Acceptance of the interface-7 screen lock, pending on the host and the SDK.
//! The interface-6 tests drove a kernel (`App`) and are gone with the plugin
//! they tested; the rules they checked are tested without a host
//! (`tests/native_flow.rs`, `tests/native_input.rs`, `src/native_parity_tests.rs`).
//! These cases need a host that draws, takes input and answers the services
//! (the scripted host answers none of `core.secrets`, `core.ui` and storage
//! yet); none of them passes by itself.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 9] = [
    Case {
        name: "the-device-locks-on-start-and-on-waking",
        golden: None,
        blocked_by: &["scripted-host-secrets-ui-storage", "H1", "H3", "H4"],
    },
    Case {
        name: "the-lock-screen-takes-every-input-and-unlocks",
        golden: None,
        blocked_by: &[
            "input-interception-h4",
            "H1",
            "scripted-host-secrets-ui-storage",
        ],
    },
    Case {
        name: "the-kind-and-its-code-are-set-on-the-locks-page",
        golden: None,
        blocked_by: &["H1", "H3", "scripted-host-secrets-ui-storage"],
    },
    Case {
        name: "lock-now-locks-from-any-button-gesture-menu-or-rule",
        golden: None,
        blocked_by: &["sealed-action-declaration"],
    },
    Case {
        name: "the-touch-lock-takes-taps-and-swipes-and-has-a-way-out",
        golden: None,
        blocked_by: &[
            "input-filter-h4",
            "status-item-stage",
            "menu-entries-host",
            "key-maps-host",
        ],
    },
    Case {
        name: "the-lock-page-has-its-icon",
        golden: None,
        blocked_by: &["icon-records-host"],
    },
    Case {
        name: "the-first-run-setup-offers-the-lock",
        golden: None,
        blocked_by: &["core-registry-op-22-host-todo-99"],
    },
    Case {
        name: "a-kind-changed-without-its-code-goes-back-at-once",
        golden: None,
        blocked_by: &["event-setting-changed-subscription"],
    },
    Case {
        name: "the-wait-after-five-wrong-tries-ends-on-time-without-an-input",
        golden: None,
        blocked_by: &["native-work-sdk"],
    },
];

#[test]
fn every_case_still_has_its_reference() {
    for case in &CASES {
        assert!(case.golden_exists(), "{}", case.name);
    }
}

#[test]
#[ignore = "BLOCKED: a scripted host that answers secrets, ui and storage, and H1/H3/H4"]
fn locking_on_start_and_waking() {
    CASES[0].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: input interception (H4), screen commands (H1) and a scripted host that answers secrets, ui and storage"]
fn the_lock_screen() {
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: H1/H3 and a scripted host that answers secrets, ui (the keyboard question) and storage"]
fn setting_the_code() {
    CASES[2].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: the sealed-action declaration (ActionDecl has no seal)"]
fn lock_now() {
    CASES[3].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: the input filter (H4), the status-item stage, menu entries and key maps on the host"]
fn the_touch_lock() {
    CASES[4].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: the host takes icon records; declare icon `lock` on `lock.group` again"]
fn the_page_icon() {
    CASES[5].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: core.registry op 22 (Host todo 99); declare the setup step (order 70, page = the kind setting) again"]
fn the_setup_step() {
    CASES[6].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: the host refuses the subscription to a changed setting (Factory(-1)); the kind is repaired at start and wake until then"]
fn the_kind_goes_back_at_once() {
    CASES[7].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: native::work (core.work wrapper): the end of the wait redraws without an input"]
fn the_wait_ends_on_time() {
    CASES[8].refuse().unwrap();
}
