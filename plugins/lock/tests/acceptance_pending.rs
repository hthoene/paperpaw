//! Acceptance of the interface-7 screen lock, pending on the host and the SDK.
//! The interface-6 tests drive a kernel (`App`) and stay as the references
//! (`tests/lock.rs`, `tests/locking.rs`, `tests/touch.rs`); none of these cases
//! passes by itself.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 7] = [
    Case {
        name: "the-device-locks-on-start-and-on-waking",
        golden: None,
        blocked_by: &["started-resume-events", "secrets-facade", "H1", "H3", "H4"],
    },
    Case {
        name: "the-lock-screen-takes-every-input-and-unlocks",
        golden: None,
        blocked_by: &["input-interception-h4", "ui-compose-h3", "secrets-facade"],
    },
    Case {
        name: "the-kind-and-its-code-are-set-on-the-locks-page",
        golden: None,
        blocked_by: &[
            "setting-editor-screen",
            "ask-service-sdk",
            "secrets-facade",
            "H1",
            "H3",
        ],
    },
    Case {
        name: "lock-now-locks-from-any-button-gesture-menu-or-rule",
        golden: None,
        blocked_by: &["sealed-action-declaration", "command-senders-342"],
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
];

#[test]
fn every_case_still_has_its_reference() {
    for case in &CASES {
        assert!(case.golden_exists(), "{}", case.name);
    }
}

#[test]
#[ignore = "BLOCKED: the started and resume events, the secrets facade and H1/H3/H4"]
fn locking_on_start_and_waking() {
    CASES[0].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: input interception (H4), composition (H3) and the secrets facade"]
fn the_lock_screen() {
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: the setting editor screen, the ask service, the secrets facade and H1/H3"]
fn setting_the_code() {
    CASES[2].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED: the sealed-action declaration and the DEV command senders (#342)"]
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
