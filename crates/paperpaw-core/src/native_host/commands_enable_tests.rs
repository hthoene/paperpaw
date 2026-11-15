//! `command.set_plugin_enabled` through the command stage (`registry.md` 4.2):
//! a switch is staged for the next start, `core` and a plugin's own switch off
//! are refused with the whole batch.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
use super::*;

fn switch(plugin: &str, enabled: u32) -> WireCommand<'_> {
    WireCommand::SetPluginEnabled { plugin, enabled }
}

fn submit(stage: &mut CommandStage<'_>, commands: &[WireCommand<'_>]) -> i32 {
    let mut bytes = [0; 512];
    let len = commands_encode(commands, &mut bytes).unwrap();
    stage.stage(&commands_decode(&bytes[..len]).unwrap())
}

#[test]
fn a_switch_is_staged_and_core_and_ones_own_switch_off_are_denied() {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(paperpaw_platform::Instant(0));
    let owner = Id::new_static("store");
    let mut stage = CommandStage::new(&owner, env.settings(), None, 16, 4096).with_ui(
        &env,
        Some(paperpaw_sdk::ScreenKey::new(7)),
        0,
        100,
    );
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
