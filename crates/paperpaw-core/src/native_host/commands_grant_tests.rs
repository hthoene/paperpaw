//! No plugin writes a user grant: a command that sets, resets or previews a
//! `core.grant.*` setting is `ERR_DENIED` and refuses the whole batch, for
//! every plugin (the one the grant names and the settings editor included),
//! whether or not such a setting is registered.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
use super::*;
use crate::abi7::commands_wire::commands_encode;
use paperpaw_native::host_glue::OK;
use paperpaw_sdk::builtin::groups;

fn settings() -> Settings {
    let mut settings = Settings::new();
    settings
        .define(SettingSpec::switch(
            "probe.flag",
            "flag",
            false,
            groups::OTHER,
        ))
        .unwrap();
    // A grant registered as an ordinary switch of the core: still not
    // writable by a plugin.
    settings
        .define(SettingSpec::switch(
            "core.grant.style.probe",
            "grant",
            false,
            groups::OTHER,
        ))
        .unwrap();
    settings
}

fn submit(stage: &mut CommandStage<'_>, batch: &[WireCommand<'_>]) -> i32 {
    let scope = core::pin::pin!(CommandScope::new());
    let mut bytes = [0; 4096];
    let len = commands_encode(batch, &mut bytes).unwrap();
    scope
        .as_ref()
        .scope(stage, || {
            // SAFETY: the initialized encoded input stays readable and disjoint
            // from the stage until the synchronous call ends.
            unsafe { scope.submit(bytes.as_ptr(), len) }
        })
        .unwrap()
}

fn set(key: &str) -> WireCommand<'_> {
    WireCommand::SetSetting {
        key,
        value: SettingValueRef::Bool(true),
    }
}

#[test]
fn a_grant_is_never_written_whoever_asks_and_the_whole_batch_is_refused() {
    let settings = settings();
    for (owner, authority) in [
        ("probe", None),
        ("core", None),
        // The settings editor may set what keeps the device safe, not this.
        (
            "probe",
            Some(CommandAuthority {
                guarded_exempt: true,
            }),
        ),
    ] {
        let owner = Id::parse(owner).unwrap();
        let mut stage = CommandStage::new(&owner, &settings, authority, 64, 4096);
        let ordinary = set("probe.flag");
        // Registered, unregistered, the owner's own grant, another's.
        for key in [
            "core.grant.style.probe",
            "core.grant.install.probe",
            "core.grant.style.other",
            "core.grant.",
        ] {
            let writes: [WireCommand<'_>; 3] = [
                set(key),
                WireCommand::ResetSetting { key, flags: 0 },
                WireCommand::PreviewSetting {
                    key,
                    value: &[],
                    flags: 0,
                },
            ];
            for write in writes {
                // After an ordinary command of the same batch: refused whole.
                assert_eq!(submit(&mut stage, &[ordinary, write]), ERR_DENIED, "{key}");
                assert_eq!(submit(&mut stage, &[write]), ERR_DENIED, "{key}");
                assert!(stage.staged().is_empty(), "{key}: nothing staged");
            }
        }
        // The ordinary command stays admitted.
        assert_eq!(submit(&mut stage, &[ordinary]), OK);
        assert_eq!(stage.staged().len(), 1);
    }
}

#[test]
fn a_key_that_only_looks_like_a_grant_is_no_grant() {
    let settings = settings();
    let owner = Id::new_static("probe");
    let mut stage = CommandStage::new(&owner, &settings, None, 64, 4096);
    // Not under `core.grant.`: the ordinary rules, here an unknown key.
    assert_ne!(submit(&mut stage, &[set("probe.core.grant.x")]), ERR_DENIED);
    assert_ne!(submit(&mut stage, &[set("core.grants.x")]), ERR_DENIED);
}
