//! `command.reset_setting` through the stage and the kernel
//! (`docs/abi/registry.md` 4.1): one setting, what a placement set, a group
//! and the submitter's own settings go back to their defaults; what is kept
//! on reset and what the submitter may not write stay.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
use super::preview_tests::submit;
use super::*;
use paperpaw_sdk::{SettingGroupSpec, builtin::groups};

static LOOK: SettingGroupSpec = SettingGroupSpec::new(Id::new_static("probe.look"), "look");
static SECRET: paperpaw_sdk::SettingExtra = paperpaw_sdk::SettingExtra::new().secret();

/// A kernel with two plugins: `probe` (a group with a number and a number
/// kept on reset, a text, a secret, a widget's setting) and `other` (in the
/// same group a guarded switch and a number).
fn kernel() -> crate::Kernel {
    use paperpaw_sdk::{Manifest, Plugin, Registrar, Version};
    struct Settings(&'static str, fn() -> Vec<SettingSpec>);
    impl Plugin for Settings {
        fn manifest(&self) -> Manifest {
            Manifest::new(Id::new_static(self.0), "Probe", Version::new(1, 0, 0))
                .uses(paperpaw_sdk::Uses::NOTHING)
        }
        fn register(&self, r: &mut Registrar) -> Result<(), paperpaw_sdk::RegisterError> {
            if self.0 == "probe" {
                r.add_setting_group(&LOOK)?;
            }
            for setting in (self.1)() {
                r.add_setting(setting)?;
            }
            Ok(())
        }
    }
    let probe = || {
        let look = Id::new_static("probe.look");
        alloc::vec![
            SettingSpec::number("probe.size", "size", 1, 9, 3, look.clone()),
            SettingSpec::number("probe.kept", "kept", 1, 9, 3, look).kept_on_reset(),
            SettingSpec::text("probe.name", "name", "", groups::OTHER),
            SettingSpec::text("probe.pin", "pin", "", groups::OTHER).with_extra(&SECRET),
            SettingSpec::number("probe.clock.face", "face", 1, 9, 1, groups::OTHER),
        ]
    };
    let other = || {
        let look = Id::new_static("probe.look");
        alloc::vec![
            SettingSpec::switch("other.guarded", "guarded", false, look.clone()).guarded(),
            SettingSpec::number("other.size", "size", 1, 9, 5, look),
        ]
    };
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .plugin(Settings("probe", probe))
        .plugin(Settings("other", other))
        .storage(paperpaw_sdk::MemoryStorage::new())
        .build()
        .unwrap();
    for (id, value) in [
        ("probe.size", SettingValue::Int(7)),
        ("probe.kept", SettingValue::Int(8)),
        ("probe.name", SettingValue::Text("Ada".into())),
        ("probe.clock.face", SettingValue::Int(2)),
        ("probe.clock@2.face", SettingValue::Int(4)),
        ("probe.clock@3.face", SettingValue::Int(5)),
        ("other.guarded", SettingValue::Bool(true)),
        ("other.size", SettingValue::Int(6)),
    ] {
        kernel.apply(Command::SetSetting(Id::parse(id).unwrap(), value));
    }
    kernel.finish();
    let warnings = kernel.take_warnings();
    assert!(warnings.is_empty(), "{warnings:?}");
    kernel
}

fn reset(key: &str, flags: u32) -> WireCommand<'_> {
    WireCommand::ResetSetting { key, flags }
}

/// Which of the values the fixture set are still kept.
fn kept(kernel: &crate::Kernel) -> Vec<&'static str> {
    [
        "probe.size",
        "probe.kept",
        "probe.name",
        "probe.clock.face",
        "probe.clock@2.face",
        "probe.clock@3.face",
        "other.guarded",
        "other.size",
    ]
    .into_iter()
    .filter(|id| kernel.settings().is_set(id))
    .collect()
}

const ALL: [&str; 8] = [
    "probe.size",
    "probe.kept",
    "probe.name",
    "probe.clock.face",
    "probe.clock@2.face",
    "probe.clock@3.face",
    "other.guarded",
    "other.size",
];

#[test]
fn one_setting_goes_back_to_its_default() {
    let mut kernel = kernel();
    assert_eq!(kernel.settings().int("probe.size"), 7);
    assert_eq!(
        submit(&mut kernel, "probe", false, &[reset("probe.size", 0)]),
        OK
    );
    assert_eq!(kernel.settings().int("probe.size"), 3);
    assert!(!kernel.settings().is_set("probe.size"));
    // One named by its key goes though it is kept when many are reset, and
    // another plugin's goes as it may be set; in a work run as well.
    let batch = [reset("probe.kept", 0), reset("other.size", 0)];
    assert_eq!(submit(&mut kernel, "probe", true, &batch), OK);
    assert_eq!(
        kept(&kernel),
        [
            "probe.name",
            "probe.clock.face",
            "probe.clock@2.face",
            "probe.clock@3.face",
            "other.guarded"
        ]
    );
    // One that is not set is reset to no effect.
    assert_eq!(
        submit(&mut kernel, "probe", false, &[reset("probe.size", 0)]),
        OK
    );
    assert!(kernel.take_warnings().is_empty());
}

#[test]
fn the_resets_of_a_batch_are_applied_in_its_order() {
    let mut kernel = kernel();
    let set = |value| WireCommand::SetSetting {
        key: "probe.size",
        value: SettingValueRef::Int(value),
    };
    let batch = [reset("probe.size", 0), set(9)];
    assert_eq!(submit(&mut kernel, "probe", false, &batch), OK);
    assert_eq!(kernel.settings().int("probe.size"), 9);
    let batch = [set(8), reset("probe.size", 0), reset("probe.name", 0)];
    assert_eq!(submit(&mut kernel, "probe", false, &batch), OK);
    assert_eq!(kernel.settings().int("probe.size"), 3);
    assert_eq!(kernel.settings().text("probe.name"), "");
}

#[test]
fn what_a_placement_set_goes_with_it() {
    let mut kernel = kernel();
    assert_eq!(
        submit(&mut kernel, "other", false, &[reset("probe.clock@2", 8)]),
        OK
    );
    // The widget's own value and the other placement's stay.
    let mut left = ALL.to_vec();
    left.retain(|id| *id != "probe.clock@2.face");
    assert_eq!(kept(&kernel), left);
    // A placement that set nothing: accepted, nothing changes.
    assert_eq!(
        submit(&mut kernel, "probe", false, &[reset("probe.clock@9", 8)]),
        OK
    );
    assert_eq!(kept(&kernel), left);
    // No placement.
    assert_eq!(
        submit(&mut kernel, "probe", false, &[reset("probe.clock", 8)]),
        ERR_INVALID
    );
    assert_eq!(kept(&kernel), left);
}

#[test]
fn a_group_goes_back_but_what_is_kept_and_what_is_guarded() {
    let mut kernel = kernel();
    assert_eq!(
        submit(&mut kernel, "probe", false, &[reset("probe.look", 1)]),
        OK
    );
    // The group's numbers of both plugins; not the one kept on reset, not
    // the other plugin's guarded switch, nothing of another group.
    assert_eq!(
        kept(&kernel),
        [
            "probe.kept",
            "probe.name",
            "probe.clock.face",
            "probe.clock@2.face",
            "probe.clock@3.face",
            "other.guarded"
        ]
    );
    // Its own plugin resets the guarded one with the group.
    assert_eq!(
        submit(&mut kernel, "other", false, &[reset("probe.look", 1)]),
        OK
    );
    assert!(!kernel.settings().is_set("other.guarded"));
    assert_eq!(
        submit(&mut kernel, "probe", false, &[reset("probe.nowhere", 1)]),
        ERR_INVALID
    );
}

#[test]
fn a_plugin_resets_its_own_settings() {
    let mut kernel = kernel();
    assert_eq!(
        submit(&mut kernel, "probe", false, &[reset("probe", 2)]),
        OK
    );
    // What its placements set as well; not what is kept on reset, not
    // another plugin's.
    assert_eq!(kept(&kernel), ["probe.kept", "other.guarded", "other.size"]);
}

#[test]
fn what_is_no_reset_or_needs_the_readers_consent_is_refused() {
    let mut kernel = kernel();
    let cases: [(&str, u32, i32, &str); 11] = [
        ("probe.none", 0, ERR_INVALID, "no setting"),
        ("other.guarded", 0, ERR_INVALID, "another's guarded setting"),
        ("probe.pin", 0, ERR_UNSUPPORTED, "a secret"),
        ("core.grant.style.probe", 0, ERR_DENIED, "a grant"),
        ("other", 2, ERR_UNSUPPORTED, "another plugin's settings"),
        ("", 4, ERR_UNSUPPORTED, "every setting"),
        ("probe", 4, ERR_INVALID, "every setting, with a key"),
        ("probe.size", 2, ERR_INVALID, "no plugin"),
        ("probe.look", 3, ERR_INVALID, "two kinds at once"),
        ("probe.clock@2", 9, ERR_INVALID, "a group and a placement"),
        ("probe.size", 16, ERR_INVALID, "an unknown flag"),
    ];
    for (key, flags, status, what) in cases {
        // After a reset that would be accepted: the batch is refused whole.
        let batch = [reset("probe.size", 0), reset(key, flags)];
        assert_eq!(
            submit(&mut kernel, "probe", false, &batch),
            status,
            "{what}"
        );
        assert_eq!(kept(&kernel), ALL, "{what}");
    }
}
