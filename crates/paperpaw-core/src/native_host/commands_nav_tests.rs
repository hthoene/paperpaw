//! The navigation commands through the command stage on a stack of rows:
//! each command's rules, the guard's, the no-effect cases and the whole batch
//! refused.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
use super::super::*;
use crate::native_host::stack_rows::{GUARDS, OVERLAY, StackRow};
use paperpaw_native::host_glue::{ERR_DENIED, ERR_NOT_FOUND, ERR_UNSUPPORTED};
use paperpaw_platform::Instant;
use paperpaw_sdk::{Command, ScreenKey, StackEntry};

const OWN: u64 = 13;
const OTHER: u64 = 12;

/// A stack of (screen id, instance, owner, flags), bottom to top.
fn kernel(rows: &[(&'static str, u32, u64, u8)]) -> crate::Kernel {
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    for (screen, instance, _, flags) in rows {
        kernel.shared.stack.push(StackEntry::new(
            Id::new_static(screen),
            None,
            flags & OVERLAY != 0,
            ScreenKey::new(*instance),
        ));
    }
    kernel
        .shared
        .stack_rows
        .update(rows.iter().map(|(_, instance, owner, flags)| StackRow {
            instance: *instance,
            owner: *owner,
            flags: *flags,
        }));
    kernel
}

/// The kernel's stage for plugin `probe` (host owner `OWN`) whose top screen is
/// instance `top`.
fn submit(
    kernel: &crate::Kernel,
    top: u32,
    hop: u8,
    commands: &[WireCommand<'_>],
) -> (i32, Vec<Command>) {
    let env = kernel.shared.env(Instant(0));
    let owner = Id::new_static("probe");
    let mut stage = CommandStage::new(&owner, env.settings(), None, 16, 4096)
        .with_ui(&env, Some(ScreenKey::new(top)), hop, 100)
        .with_stack(&kernel.shared.stack_rows, OWN);
    let mut bytes = [0; 1024];
    let len = commands_encode(commands, &mut bytes).unwrap();
    let status = stage.stage(&commands_decode(&bytes[..len]).unwrap());
    (status, stage.into_commands())
}

fn close(instance: u32) -> WireCommand<'static> {
    WireCommand::Close { instance }
}

const BASE: (&str, u32, u64, u8) = ("base.home", 1, OTHER, 0);
const HELP: (&str, u32, u64, u8) = ("help.overlay", 2, OTHER, OVERLAY);
const MINE: (&str, u32, u64, u8) = ("probe.screen", 3, OWN, 0);

#[test]
fn close_follows_the_submitters_rights_and_the_guard() {
    let kernel = kernel(&[BASE, HELP, MINE]);
    // 0 and its own instance close the top; another plugin's overlay closes
    // where it is; another plugin's plain screen is not the submitter's.
    for instance in [0, 3] {
        let (status, staged) = submit(&kernel, 3, 0, &[close(instance)]);
        assert_eq!(status, OK);
        assert!(matches!(staged[..], [Command::Close(key)] if key == ScreenKey::new(3)));
    }
    let (status, staged) = submit(&kernel, 3, 0, &[close(2)]);
    assert_eq!(status, OK);
    assert!(matches!(staged[..], [Command::Close(key)] if key == ScreenKey::new(2)));
    assert_eq!(submit(&kernel, 3, 0, &[close(1)]).0, ERR_DENIED);
    assert_eq!(submit(&kernel, 3, 0, &[close(99)]).0, ERR_NOT_FOUND);
    // Outside its own top screen, 0 is a state error.
    assert_eq!(
        submit(&kernel, 3, 0, &[close(0), close(99)]).0,
        ERR_NOT_FOUND,
        "whole batch"
    );
    // A guard below: only its plugin closes the screens at or above it.
    let guarded = self::kernel(&[BASE, ("lock.screen", 2, OTHER, GUARDS), MINE]);
    assert_eq!(submit(&guarded, 3, 0, &[close(3)]).0, ERR_DENIED);
    assert_eq!(submit(&guarded, 3, 0, &[close(2)]).0, ERR_DENIED);
}

#[test]
fn the_root_screen_is_never_closed_and_stages_nothing() {
    let kernel = kernel(&[("probe.root", 1, OWN, 0)]);
    let (status, staged) = submit(&kernel, 1, 0, &[close(0)]);
    assert_eq!((status, staged.len()), (OK, 0));
    let (status, staged) = submit(&kernel, 1, 0, &[close(1)]);
    assert_eq!((status, staged.len()), (OK, 0));
}

#[test]
fn back_to_names_the_topmost_open_screen_and_does_nothing_where_none_is() {
    let kernel = kernel(&[BASE, HELP, MINE]);
    let (status, staged) = submit(
        &kernel,
        3,
        0,
        &[WireCommand::BackTo {
            target: "base.home",
        }],
    );
    assert_eq!(status, OK);
    assert!(matches!(&staged[..], [Command::BackTo(id)] if id.as_str() == "base.home"));
    // Nothing open: OK, nothing staged. The top itself: nothing above.
    for target in ["nowhere.screen", "probe.screen"] {
        let (status, staged) = submit(&kernel, 3, 0, &[WireCommand::BackTo { target }]);
        assert_eq!((status, staged.len()), (OK, 0), "{target}");
    }
    assert_eq!(
        submit(&kernel, 3, 0, &[WireCommand::BackTo { target: "Bad Id" }]).0,
        ERR_INVALID
    );
    // Not across a guard of another plugin.
    let guarded = self::kernel(&[BASE, ("lock.screen", 2, OTHER, GUARDS), MINE]);
    assert_eq!(
        submit(
            &guarded,
            3,
            0,
            &[WireCommand::BackTo {
                target: "base.home"
            }]
        )
        .0,
        ERR_DENIED
    );
}

#[test]
fn a_key_layer_is_the_own_top_screens_and_an_empty_one_removes_it() {
    let kernel = kernel(&[BASE, MINE]);
    let (status, staged) = submit(
        &kernel,
        3,
        0,
        &[WireCommand::SetKeyLayer {
            layer: "probe.book",
        }],
    );
    assert_eq!(status, OK);
    assert!(matches!(&staged[..], [Command::SetKeyLayer(key, Some(id))]
        if *key == ScreenKey::new(3) && id.as_str() == "probe.book"));
    let (_, staged) = submit(&kernel, 3, 0, &[WireCommand::SetKeyLayer { layer: "" }]);
    assert!(matches!(staged[..], [Command::SetKeyLayer(_, None)]));
    // Not from a callback of a screen that is not its own top one.
    assert_eq!(
        submit(
            &kernel,
            1,
            0,
            &[WireCommand::SetKeyLayer {
                layer: "probe.book"
            }]
        )
        .0,
        ERR_STATE
    );
}

#[test]
fn show_hints_is_always_open() {
    let kernel = kernel(&[BASE, MINE]);
    let (status, staged) = submit(&kernel, 3, 0, &[WireCommand::ShowHints {}]);
    assert_eq!(status, OK);
    assert!(matches!(staged[..], [Command::ShowHints]));
}

#[test]
fn a_raised_input_is_the_submitters_to_raise_and_never_the_power_keys() {
    let kernel = kernel(&[BASE, MINE]);
    let raise = |input| submit(&kernel, 3, 0, &[WireCommand::RaiseInput { input }]);
    let (status, staged) = raise("button.menu");
    assert_eq!(status, OK);
    assert!(matches!(&staged[..], [Command::Input(name)] if name == "button.menu"));
    assert_eq!(raise("input.probe.next").0, OK);
    assert_eq!(raise("gesture.probe.swirl").0, OK);
    assert_eq!(raise("input.other.next").0, ERR_DENIED);
    assert_eq!(raise("button.power").0, ERR_DENIED);
    assert_eq!(raise("escape").0, ERR_DENIED);
    assert_eq!(raise("").0, ERR_INVALID);
    assert_eq!(raise("Button Menu").0, ERR_INVALID);
    // A chain step: the ninth hop is refused.
    assert_eq!(
        submit(
            &kernel,
            3,
            8,
            &[WireCommand::RaiseInput {
                input: "button.menu"
            }]
        )
        .0,
        ERR_BUSY
    );
    // While another plugin's screen guards, only its plugin raises.
    let guarded = self::kernel(&[BASE, ("lock.screen", 2, OTHER, GUARDS), MINE]);
    assert_eq!(
        submit(
            &guarded,
            3,
            0,
            &[WireCommand::RaiseInput {
                input: "button.menu"
            }]
        )
        .0,
        ERR_DENIED
    );
}

#[test]
fn a_stage_without_the_stack_refuses_them_and_screen_policy_is_unsupported() {
    let kernel = kernel(&[BASE, MINE]);
    let env = kernel.shared.env(Instant(0));
    let owner = Id::new_static("probe");
    let mut stage = CommandStage::new(&owner, env.settings(), None, 16, 4096).with_ui(
        &env,
        Some(ScreenKey::new(3)),
        0,
        100,
    );
    let mut bytes = [0; 64];
    let len = commands_encode(&[WireCommand::ShowHints {}], &mut bytes).unwrap();
    assert_eq!(
        stage.stage(&commands_decode(&bytes[..len]).unwrap()),
        ERR_UNSUPPORTED
    );
    assert_eq!(
        submit(
            &kernel,
            3,
            0,
            &[WireCommand::ScreenPolicy {
                instance: 0,
                flags: 2
            }]
        )
        .0,
        ERR_UNSUPPORTED
    );
}
