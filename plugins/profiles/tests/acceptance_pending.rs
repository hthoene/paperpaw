//! Acceptance of the interface-7 port, pending on the host. The interface-6
//! plugin had no desktop golden; kernel tests (`apply.rs`, `names.rs`) pinned its
//! behaviour and went with it. These cases are those behaviours through the
//! production loader with the native plugin; they never pass by themselves.
//! What needs no host (the file, names, values) is pinned by the `store` unit tests.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 6] = [
    // Save as new profile asks for a name, refuses a name in use, keeps the settings.
    Case {
        name: "profiles-save-new",
        golden: None,
        blocked_by: &["H1", "H2", "H4"],
    },
    // Tapping a profile applies only the settings that differ and says so.
    Case {
        name: "profiles-apply",
        golden: None,
        blocked_by: &["H1", "H4"],
    },
    // Holding a profile offers update, rename and remove, each asked once more where it replaces.
    Case {
        name: "profiles-hold-options",
        golden: None,
        blocked_by: &["H1", "H2", "H4"],
    },
    // Next profile goes round them all.
    Case {
        name: "profiles-next",
        golden: None,
        blocked_by: &["H1"],
    },
    // Next follows a rename and starts at the first when the last is gone.
    Case {
        name: "profiles-next-follows-rename",
        golden: None,
        blocked_by: &["H1", "H2"],
    },
    // Hostile values apply as far as they are valid; a storage error is survived.
    Case {
        name: "profiles-hostile-values-and-storage-errors",
        golden: None,
        blocked_by: &["H1", "H4"],
    },
];

#[test]
fn the_cases_name_what_they_wait_for() {
    for case in &CASES {
        assert!(case.golden_exists());
        let error = case.refuse().unwrap_err();
        assert!(error.starts_with("BLOCKED H1"), "{error}");
    }
}

#[test]
#[ignore = "BLOCKED H1/H2/H4: native screen commands, dialogs and input are not qualified"]
fn profiles_save_new() {
    CASES[0].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H4: native screen commands and input are not qualified"]
fn profiles_apply() {
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H2/H4: native screen commands, dialogs and input are not qualified"]
fn profiles_hold_options() {
    CASES[2].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1: native screen commands are not qualified"]
fn profiles_next() {
    CASES[3].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H2: native screen commands and dialogs are not qualified"]
fn profiles_next_follows_rename() {
    CASES[4].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H4: native screen commands and input are not qualified"]
fn profiles_hostile_values_and_storage_errors() {
    CASES[5].refuse().unwrap();
}
