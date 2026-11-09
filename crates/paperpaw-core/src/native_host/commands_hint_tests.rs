//! `command.set_hint` through the command stage: admission rules, the whole
//! batch refused or kept, nothing on a device without a row of hints.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
use super::*;
use crate::native_host::hint_labels::HintLabels;
use core::cell::RefCell;
use paperpaw_native::host_glue::ERR_NOT_FOUND;
use paperpaw_platform::Instant;
use paperpaw_sdk::{Hint, ScreenKey};

const KEY: u32 = 7;

fn hint<'a>(action: &'a str, kind: u32, label: &'a str) -> WireCommand<'a> {
    WireCommand::SetHint {
        action,
        kind,
        label,
    }
}

fn kernel() -> crate::Kernel {
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    kernel.shared.stack.push(paperpaw_sdk::StackEntry::new(
        Id::new_static("probe.screen"),
        None,
        false,
        ScreenKey::new(KEY),
    ));
    kernel
}

fn submit(stage: &mut CommandStage<'_>, commands: &[WireCommand<'_>]) -> i32 {
    let mut bytes = [0; 1024];
    let len = commands_encode(commands, &mut bytes).unwrap();
    stage.stage(&commands_decode(&bytes[..len]).unwrap())
}

#[test]
fn a_batch_of_hints_is_checked_whole_and_listed_for_the_top_instance() {
    let kernel = kernel();
    let env = kernel.shared.env(Instant(0));
    let owner = Id::new_static("probe");
    let labels = RefCell::new(HintLabels::default());
    let mut stage = CommandStage::new(&owner, env.settings(), None, 16, 4096)
        .with_ui(&env, Some(ScreenKey::new(KEY)), 0, 100)
        .with_hint_labels(&labels, true);
    assert_eq!(
        submit(
            &mut stage,
            &[hint("core.back", 1, "Close"), hint("probe.go", 3, "")]
        ),
        OK
    );
    // A bad command refuses the whole batch and lists nothing of it.
    assert_eq!(
        submit(
            &mut stage,
            &[hint("probe.a", 1, "A"), hint("core.menu", 3, "")]
        ),
        ERR_DENIED
    );
    assert_eq!(
        submit(
            &mut stage,
            &[hint("probe.a", 1, "A"), hint("probe.b", 2, "no-such-icon")]
        ),
        ERR_NOT_FOUND
    );
    assert_eq!(
        submit(
            &mut stage,
            &[hint("probe.a", 1, ""), hint("probe.b", 1, "B")]
        ),
        ERR_INVALID
    );
    let sets = stage.take_hint_sets();
    assert_eq!(sets.len(), 2, "only the first batch was kept");
    labels.borrow_mut().apply(sets);
    let held = labels.borrow();
    assert_eq!(
        held.hint(KEY, "core.back"),
        Some(Hint::Text("Close".into()))
    );
    assert_eq!(held.hint(KEY, "probe.go"), Some(Hint::Hidden));
    assert_eq!(held.hint(KEY, "probe.a"), None);
}

#[test]
fn a_ninth_action_in_a_callback_is_no_space_and_leaves_the_earlier_ones() {
    let kernel = kernel();
    let env = kernel.shared.env(Instant(0));
    let owner = Id::new_static("probe");
    let labels = RefCell::new(HintLabels::default());
    let mut stage = CommandStage::new(&owner, env.settings(), None, 64, 4096)
        .with_ui(&env, Some(ScreenKey::new(KEY)), 0, 100)
        .with_hint_labels(&labels, true);
    let names: Vec<String> = (0..9).map(|n| alloc::format!("probe.a{n}")).collect();
    for name in &names[..6] {
        assert_eq!(submit(&mut stage, &[hint(name, 1, "x")]), OK);
    }
    // Three more in one batch: the eighth fits, the ninth does not, none stays.
    assert_eq!(
        submit(
            &mut stage,
            &[
                hint(&names[6], 1, "x"),
                hint(&names[7], 1, "x"),
                hint(&names[8], 1, "x")
            ]
        ),
        ERR_NO_SPACE
    );
    assert_eq!(stage.take_hint_sets().len(), 6);
}

#[test]
fn only_the_own_top_screen_may_label_and_a_device_without_a_row_does_nothing() {
    let kernel = kernel();
    let env = kernel.shared.env(Instant(0));
    let labels = RefCell::new(HintLabels::default());
    // Another plugin's screen on top, or no instance of the callback: state.
    let foreign = Id::new_static("other");
    let mut stage = CommandStage::new(&foreign, env.settings(), None, 16, 4096)
        .with_ui(&env, Some(ScreenKey::new(KEY)), 0, 100)
        .with_hint_labels(&labels, true);
    assert_eq!(submit(&mut stage, &[hint("other.go", 1, "Go")]), ERR_STATE);
    let owner = Id::new_static("probe");
    let mut stage = CommandStage::new(&owner, env.settings(), None, 16, 4096)
        .with_ui(&env, Some(ScreenKey::new(KEY + 1)), 0, 100)
        .with_hint_labels(&labels, true);
    assert_eq!(submit(&mut stage, &[hint("probe.go", 1, "Go")]), ERR_STATE);
    // Without a row of hints: checked, accepted, nothing listed.
    let mut stage = CommandStage::new(&owner, env.settings(), None, 16, 4096)
        .with_ui(&env, Some(ScreenKey::new(KEY)), 0, 100)
        .with_hint_labels(&labels, false);
    assert_eq!(submit(&mut stage, &[hint("probe.go", 1, "Go")]), OK);
    assert_eq!(submit(&mut stage, &[hint("core.menu", 3, "")]), ERR_DENIED);
    assert!(stage.take_hint_sets().is_empty());
    assert!(labels.borrow().is_empty());
    // A stage the host gave no labels is unsupported, as before.
    let mut stage = CommandStage::new(&owner, env.settings(), None, 16, 4096).with_ui(
        &env,
        Some(ScreenKey::new(KEY)),
        0,
        100,
    );
    assert_eq!(
        submit(&mut stage, &[hint("probe.go", 1, "Go")]),
        ERR_UNSUPPORTED
    );
}

#[test]
fn a_labels_table_that_cannot_grow_refuses_the_batch_with_no_memory() {
    let kernel = kernel();
    let env = kernel.shared.env(Instant(0));
    let owner = Id::new_static("probe");
    let labels = RefCell::new(HintLabels::default());
    let mut stage = CommandStage::new(&owner, env.settings(), None, 16, 4096)
        .with_ui(&env, Some(ScreenKey::new(KEY)), 0, 100)
        .with_hint_labels(&labels, true);
    // A table in use elsewhere (a hint being drawn) is busy, never a panic.
    let held = labels.borrow();
    assert_eq!(submit(&mut stage, &[hint("probe.go", 1, "Go")]), ERR_BUSY);
    drop(held);
    assert_eq!(submit(&mut stage, &[hint("probe.go", 1, "Go")]), OK);
}
