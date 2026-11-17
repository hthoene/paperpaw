//! `command.set_plugin_enabled` through the command stage (`registry.md` 4.2):
//! only a plugin admitted with `uses.registry` switches plugins; a switch is
//! staged for the next start; `core`, a plugin's own switch off and a plugin
//! the device cannot be without are denied and an unknown plugin is not
//! found, each with the whole batch.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
use super::*;
use paperpaw_native::host_glue::ERR_NOT_FOUND;
use paperpaw_sdk::{Manifest, PluginInfo, PluginKind, PluginState, RequiredFor, Version};

fn switch(plugin: &str, enabled: u32) -> WireCommand<'_> {
    WireCommand::SetPluginEnabled { plugin, enabled }
}

fn submit(stage: &mut CommandStage<'_>, commands: &[WireCommand<'_>]) -> i32 {
    let mut bytes = [0; 512];
    let len = commands_encode(commands, &mut bytes).unwrap();
    stage.stage(&commands_decode(&bytes[..len]).unwrap())
}

fn info(id: &'static str, required_for: RequiredFor) -> PluginInfo {
    PluginInfo::new(
        paperpaw_sdk::ManifestRef::try_new(
            Manifest::new(Id::new_static(id), id, Version::new(1, 0, 0)),
            None,
        )
        .unwrap(),
        PluginState::Running,
        true,
        required_for,
        PluginKind::BuiltIn,
        None,
        None,
    )
}

/// A kernel that lists `store`, `stats` and `home`, the last the only Home.
fn kernel() -> crate::Kernel {
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let mut home = RequiredFor::default();
    assert!(home.insert(&paperpaw_sdk::builtin::slots::HOME));
    for listed in [
        info("store", RequiredFor::default()),
        info("stats", RequiredFor::default()),
        info("home", home),
    ] {
        kernel.list_native(listed).unwrap();
    }
    kernel
}

/// The stage of `owner`, admitted with `uses.registry` or without.
fn stage<'a>(owner: &'a Id, env: &'a crate::Place<'a>, registry: bool) -> CommandStage<'a> {
    let mut stage = CommandStage::new(owner, env.settings(), None, 16, 4096).with_ui(
        env,
        Some(paperpaw_sdk::ScreenKey::new(7)),
        0,
        100,
    );
    stage.admit_registry(registry);
    stage
}

#[test]
fn a_switch_is_staged_and_core_and_ones_own_switch_off_are_denied() {
    let kernel = kernel();
    let env = kernel.shared.place(paperpaw_platform::Instant(0));
    let owner = Id::new_static("store");
    let mut stage = stage(&owner, &env, true);
    // Another plugin on and off: staged, in order, as kernel commands.
    assert_eq!(
        submit(&mut stage, &[switch("stats", 1), switch("stats", 0)]),
        OK
    );
    assert!(matches!(
        stage.staged(),
        [Command::SetPluginEnabled(a, true), Command::SetPluginEnabled(b, false)]
            if a.as_str() == "stats" && b.as_str() == "stats"
    ));
    // The host's own `core` and the submitter's own switch off: refused whole,
    // nothing of the batch staged.
    assert_eq!(
        submit(&mut stage, &[switch("stats", 1), switch("core", 1)]),
        ERR_DENIED
    );
    assert_eq!(submit(&mut stage, &[switch("store", 0)]), ERR_DENIED);
    assert_eq!(stage.staged().len(), 2);
    // Its own switch on is no harm.
    assert_eq!(submit(&mut stage, &[switch("store", 1)]), OK);
    assert_eq!(stage.staged().len(), 3);
    // A bad id is refused.
    assert_eq!(submit(&mut stage, &[switch("Not An Id", 1)]), ERR_INVALID);
}

#[test]
fn only_a_plugin_admitted_with_uses_registry_switches_plugins() {
    let kernel = kernel();
    let env = kernel.shared.place(paperpaw_platform::Instant(0));
    let owner = Id::new_static("store");
    // Without the feature: on or off, the whole batch is refused.
    let mut without = stage(&owner, &env, false);
    assert_eq!(submit(&mut without, &[switch("stats", 0)]), ERR_DENIED);
    assert_eq!(submit(&mut without, &[switch("stats", 1)]), ERR_DENIED);
    assert_eq!(submit(&mut without, &[switch("store", 1)]), ERR_DENIED);
    assert!(without.staged().is_empty());
    // With it the same switch is staged.
    let mut with = stage(&owner, &env, true);
    assert_eq!(submit(&mut with, &[switch("stats", 0)]), OK);
    assert_eq!(with.staged().len(), 1);
}

#[test]
fn a_required_plugin_stays_on_and_an_unknown_one_is_not_found() {
    let kernel = kernel();
    let env = kernel.shared.place(paperpaw_platform::Instant(0));
    let owner = Id::new_static("store");
    let mut stage = stage(&owner, &env, true);
    // The only Home: not switched off, and nothing else of its batch either.
    assert_eq!(
        submit(&mut stage, &[switch("stats", 0), switch("home", 0)]),
        ERR_DENIED
    );
    // Switching it on is no harm.
    assert_eq!(submit(&mut stage, &[switch("home", 1)]), OK);
    // A plugin the device does not list, on or off.
    assert_eq!(submit(&mut stage, &[switch("nowhere", 1)]), ERR_NOT_FOUND);
    assert_eq!(
        submit(&mut stage, &[switch("stats", 1), switch("nowhere", 0)]),
        ERR_NOT_FOUND
    );
    assert_eq!(stage.staged().len(), 1);
}
