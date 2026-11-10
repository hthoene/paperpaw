//! The navigation commands through the command stage on a stack of rows:
//! each command's rules, the guard's, the no-effect cases and the whole batch
//! refused.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
use super::super::*;
use super::PolicyChange;
use crate::native_host::stack_rows::{GUARDS, OVERLAY, StackRow};
use alloc::vec;
use paperpaw_native::host_glue::{ERR_DENIED, ERR_NOT_FOUND, ERR_UNSUPPORTED};
use paperpaw_platform::Instant;
use paperpaw_sdk::{Command, Params, ScreenKey, StackEntry};

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
    // Every form of the Power key, the chords and the escape.
    for power in [
        "button.power",
        "long.power",
        "double.power",
        "chord.power_back",
        "chord.back_power",
        "escape",
    ] {
        assert_eq!(raise(power).0, ERR_DENIED, "{power}");
    }
    assert_eq!(raise("long.menu").0, OK);
    assert_eq!(raise("double.menu").0, OK);
    // Not an input any key map reads.
    assert_eq!(raise("bogus").0, ERR_INVALID);
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
fn a_stage_without_the_stack_refuses_them() {
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
}

#[test]
fn more_than_sixteen_screens_fail_closed() {
    let names: [&'static str; 17] = [
        "a.0", "a.1", "a.2", "a.3", "a.4", "a.5", "a.6", "a.7", "a.8", "a.9", "a.10", "a.11",
        "a.12", "a.13", "a.14", "a.15", "a.16",
    ];
    let rows: Vec<(&'static str, u32, u64, u8)> = names
        .iter()
        .enumerate()
        .map(|(i, name)| (*name, i as u32 + 1, if i == 16 { OWN } else { OTHER }, 0))
        .collect();
    let kernel = kernel(&rows);
    // The mirror holds 16 rows: a stack deeper than that judges nothing, so
    // the commands that read it are refused (`ui_guard` fails closed: the
    // rows past the mirror read as a host guard).
    assert_eq!(submit(&kernel, 17, 0, &[close(0)]).0, ERR_STATE);
    assert_eq!(
        submit(
            &kernel,
            17,
            0,
            &[WireCommand::RaiseInput {
                input: "button.menu"
            }]
        )
        .0,
        ERR_DENIED
    );
}

/// A kernel with the screens of plugin `lock` (`lock.screen` guards the
/// device) and of plugin `probe`.
fn guarded_kernel() -> crate::Kernel {
    use paperpaw_sdk::{Manifest, Plugin, Registrar, Screen, ScreenSpec, Version};
    struct Plain;
    impl Screen for Plain {
        fn draw(&mut self, _: &mut paperpaw_graphics::Canvas<'_>, _: &paperpaw_sdk::Env<'_>) {}
        fn on_action(&mut self, _: &Id, _: &mut paperpaw_sdk::Cx<'_, '_>) -> paperpaw_sdk::Handled {
            paperpaw_sdk::Handled::No
        }
    }
    struct Guard;
    impl Screen for Guard {
        fn draw(&mut self, _: &mut paperpaw_graphics::Canvas<'_>, _: &paperpaw_sdk::Env<'_>) {}
        fn on_action(&mut self, _: &Id, _: &mut paperpaw_sdk::Cx<'_, '_>) -> paperpaw_sdk::Handled {
            paperpaw_sdk::Handled::No
        }
        fn guards(&self) -> bool {
            true
        }
    }
    struct Screens(&'static str, &'static [&'static str]);
    impl Plugin for Screens {
        fn manifest(&self) -> Manifest {
            Manifest::new(Id::new_static(self.0), self.0, Version::new(1, 0, 0))
                .uses(paperpaw_sdk::Uses::NOTHING)
        }
        fn register(&self, r: &mut Registrar) -> Result<(), paperpaw_sdk::RegisterError> {
            for id in self.1 {
                let guard = id.ends_with(".screen");
                r.add_screen(ScreenSpec::new(
                    Id::new_static(id),
                    "t.title",
                    alloc::boxed::Box::new(move |_, _| {
                        if guard {
                            alloc::boxed::Box::new(Guard)
                        } else {
                            alloc::boxed::Box::new(Plain)
                        }
                    }),
                ))?;
            }
            Ok(())
        }
    }
    crate::Kernel::builder(crate::tests::DEVICE)
        .plugin(Screens("probe", &["probe.base", "probe.old", "probe.more"]))
        .plugin(Screens("lock", &["lock.screen", "lock.next"]))
        .build()
        .unwrap()
}

fn open(kernel: &mut crate::Kernel, screen: &'static str) {
    kernel.apply(Command::OpenScreen(Id::new_static(screen), Params::new()));
}

fn stack_of(kernel: &crate::Kernel) -> Vec<&str> {
    kernel
        .shared
        .stack
        .iter()
        .map(|entry| entry.screen.as_str())
        .collect()
}

/// The drain applies the guard's plugin's commands as the guard's own and
/// holds every other plugin's below it (ui.md 4.3), through the kernel's
/// real stack.
#[test]
fn the_drain_applies_the_guards_own_commands_and_holds_the_others() {
    let probe = Id::new_static("probe");
    let lock = Id::new_static("lock");
    for guard_plugin in [true, false] {
        let mut kernel = guarded_kernel();
        for screen in ["probe.base", "probe.old", "lock.screen"] {
            open(&mut kernel, screen);
        }
        assert_eq!(
            stack_of(&kernel),
            ["probe.base", "probe.old", "lock.screen"]
        );
        let submitter = if guard_plugin { &lock } else { &probe };
        kernel.take_warnings();
        // `back_to`: the guard's plugin closes what is above (the guard too);
        // another plugin's closes nothing.
        kernel.drain_native_command(submitter, Command::BackTo(Id::new_static("probe.base")));
        if guard_plugin {
            assert_eq!(stack_of(&kernel), ["probe.base"]);
        } else {
            assert_eq!(
                stack_of(&kernel),
                ["probe.base", "probe.old", "lock.screen"]
            );
        }
    }
    // The guard closes itself (the lock screen's way out); another plugin
    // cannot close it.
    let mut kernel = guarded_kernel();
    for screen in ["probe.base", "lock.screen"] {
        open(&mut kernel, screen);
    }
    let guard_key = kernel.shared.stack.last().unwrap().key;
    kernel.drain_native_command(&probe, Command::Close(guard_key));
    assert_eq!(stack_of(&kernel), ["probe.base", "lock.screen"]);
    kernel.drain_native_command(&lock, Command::Close(guard_key));
    assert_eq!(stack_of(&kernel), ["probe.base"]);
    // A raised input: held for another plugin, raised for the guard's (an
    // input no key map reads is told in the log only where it was raised).
    for (guard_plugin, raised) in [(false, false), (true, true)] {
        let mut kernel = guarded_kernel();
        for screen in ["probe.base", "lock.screen"] {
            open(&mut kernel, screen);
        }
        kernel.take_warnings();
        let submitter = if guard_plugin { &lock } else { &probe };
        kernel.drain_native_command(submitter, Command::Input("bogus.key".into()));
        let told = kernel
            .take_warnings()
            .iter()
            .any(|warning| warning.message.contains("unknown input"));
        assert_eq!(told, raised);
    }
    // An open: the guard's plugin's goes above the guard, another's below.
    for (guard_plugin, expected) in [
        (true, ["probe.base", "lock.screen", "lock.next"]),
        (false, ["probe.base", "probe.more", "lock.screen"]),
    ] {
        let mut kernel = guarded_kernel();
        for screen in ["probe.base", "lock.screen"] {
            open(&mut kernel, screen);
        }
        let (submitter, screen) = if guard_plugin {
            (&lock, "lock.next")
        } else {
            (&probe, "probe.more")
        };
        kernel.drain_native_command(
            submitter,
            Command::OpenScreen(Id::new_static(screen), Params::new()),
        );
        assert_eq!(stack_of(&kernel), expected);
    }
}

/// What `submit` leaves of a batch: the commands and the policy changes.
fn submit_policy(
    kernel: &crate::Kernel,
    top: u32,
    commands: &[WireCommand<'_>],
) -> (i32, Vec<Command>, Vec<PolicyChange>) {
    let env = kernel.shared.env(Instant(0));
    let owner = Id::new_static("probe");
    let mut stage = CommandStage::new(&owner, env.settings(), None, 16, 4096)
        .with_ui(&env, Some(ScreenKey::new(top)), 0, 100)
        .with_stack(&kernel.shared.stack_rows, OWN);
    let mut bytes = [0; 1024];
    let len = commands_encode(commands, &mut bytes).unwrap();
    let status = stage.stage(&commands_decode(&bytes[..len]).unwrap());
    let policies = stage.take_policies();
    (status, stage.into_commands(), policies)
}

fn policy(instance: u32, flags: u32) -> WireCommand<'static> {
    WireCommand::ScreenPolicy { instance, flags }
}

#[test]
fn a_screen_policy_is_the_submitters_own_and_staged_apart() {
    let kernel = kernel(&[BASE, MINE]);
    // The own top one (0 or by number): staged apart from the commands.
    for instance in [0, 3] {
        let (status, staged, policies) = submit_policy(&kernel, 3, &[policy(instance, 0x12)]);
        assert_eq!(status, OK);
        assert!(staged.is_empty());
        assert_eq!(
            policies,
            [PolicyChange {
                instance: 3,
                flags: 0x12,
                ready: false,
            }]
        );
    }
    // Bits other than 1, 4 and 5.
    for flags in [1, 4, 8, 0x40, 0x100] {
        assert_eq!(
            submit_policy(&kernel, 3, &[policy(0, flags)]).0,
            ERR_INVALID
        );
    }
    // Another plugin's instance, one that is not open.
    assert_eq!(submit_policy(&kernel, 3, &[policy(1, 0x10)]).0, ERR_DENIED);
    assert_eq!(
        submit_policy(&kernel, 3, &[policy(9, 0x10)]).0,
        ERR_NOT_FOUND
    );
    // Outside its own top screen the own top one is `ERR_STATE`.
    assert_eq!(submit_policy(&kernel, 1, &[policy(0, 0x10)]).0, ERR_STATE);
    // Whole batch refused: nothing of it is kept.
    let (status, staged, policies) =
        submit_policy(&kernel, 3, &[WireCommand::ShowHints {}, policy(1, 0x10)]);
    assert_eq!(status, ERR_DENIED);
    assert!(staged.is_empty() && policies.is_empty());
}

#[test]
fn guards_turn_on_only_for_the_top_and_off_anywhere() {
    // Own instance below another plugin's screen: guards on is `ERR_STATE`,
    // `secret` and `holds_edges` and guards off still work.
    let below = kernel(&[("probe.screen", 3, OWN, 0), ("base.home", 1, OTHER, 0)]);
    assert_eq!(submit_policy(&below, 1, &[policy(3, 0x02)]).0, ERR_STATE);
    assert_eq!(submit_policy(&below, 1, &[policy(3, 0x30)]).0, OK);
    assert_eq!(submit_policy(&below, 1, &[policy(3, 0)]).0, OK);
    // The own top one: on. Another's host screen above does not count.
    let top = kernel(&[
        BASE,
        MINE,
        ("core.sleep", 4, 0, crate::native_host::stack_rows::HOST),
    ]);
    assert_eq!(submit_policy(&top, 3, &[policy(3, 0x02)]).0, OK);
    // Already guarding under another's screen: it may stay on.
    let guarding = kernel(&[("probe.screen", 3, OWN, GUARDS), ("base.home", 1, OTHER, 0)]);
    assert_eq!(submit_policy(&guarding, 1, &[policy(3, 0x02)]).0, OK);
}

/// The kernel applies the change on the real stack: the instance guards or is
/// secret at once, stays so when the stack is synced again, and goes back.
#[test]
fn the_kernel_applies_a_policy_to_the_real_screen() {
    let probe = Id::new_static("probe");
    let mut kernel = guarded_kernel();
    for screen in ["probe.base", "probe.old"] {
        open(&mut kernel, screen);
    }
    let key = kernel.shared.stack.last().unwrap().key;
    assert!(!kernel.is_secret());
    // Secret on, then off (any own instance).
    let change = |flags| PolicyChange {
        instance: key.serial(),
        flags,
        ready: false,
    };
    kernel.native_screen_policies(&probe, OWN, vec![change(0x10)]);
    assert!(kernel.is_secret() && kernel.guard().is_none());
    kernel.sync_stack();
    assert!(kernel.is_secret());
    kernel.native_screen_policies(&probe, OWN, vec![change(0)]);
    assert!(!kernel.is_secret());
    // Guards on needs the kernel's rows to name the submitter's top screen.
    kernel.take_warnings();
    kernel.native_screen_policies(&probe, OWN, vec![change(0x02)]);
    assert!(
        kernel.guard().is_none(),
        "no row names the submitter: refused"
    );
    assert_eq!(kernel.take_warnings().len(), 1);
    let rows: Vec<StackRow> = kernel
        .stack
        .iter()
        .map(|active| StackRow {
            instance: active.serial,
            owner: if active.serial == key.serial() {
                OWN
            } else {
                OTHER
            },
            flags: 0,
        })
        .collect();
    kernel.shared.stack_rows.update(rows);
    kernel.native_screen_policies(&probe, OWN, vec![change(0x02)]);
    assert_eq!(kernel.guard(), Some(kernel.stack.len() - 1));
    assert!(kernel.is_secret());
    // Another plugin's change and a gone screen are dropped.
    kernel.take_warnings();
    kernel.native_screen_policies(&Id::new_static("lock"), 12, vec![change(0)]);
    kernel.native_screen_policies(
        &probe,
        OWN,
        vec![PolicyChange {
            instance: 999,
            flags: 0,
            ready: false,
        }],
    );
    assert!(kernel.guard().is_some());
    assert_eq!(kernel.take_warnings().len(), 2);
    // Off: nothing guards.
    kernel.native_screen_policies(&probe, OWN, vec![change(0)]);
    assert!(kernel.guard().is_none());
}

/// The lock's keyboard (another plugin's screen above the lock) cannot take
/// the guard over: guards on is `ERR_STATE` while another plugin's screen
/// guards, at submit and at apply, and the lock stays.
#[test]
fn a_screen_above_anothers_guard_cannot_turn_guards_on() {
    let above = kernel(&[
        BASE,
        ("lock.screen", 2, OTHER, GUARDS),
        ("kbd.screen", 3, OWN, 0),
    ]);
    for instance in [0, 3] {
        assert_eq!(
            submit_policy(&above, 3, &[policy(instance, 0x02)]).0,
            ERR_STATE
        );
    }
    // The whole batch is refused, whatever else it holds.
    let (status, staged, policies) =
        submit_policy(&above, 3, &[WireCommand::ShowHints {}, policy(3, 0x02)]);
    assert_eq!(status, ERR_STATE);
    assert!(staged.is_empty() && policies.is_empty());
    // Not the guard's own business: secret and edges still work.
    assert_eq!(submit_policy(&above, 3, &[policy(3, 0x30)]).0, OK);
    // The guard's plugin itself may (it is the guard already).
    let own = kernel(&[
        BASE,
        ("lock.screen", 2, OWN, GUARDS),
        ("kbd.screen", 3, OWN, 0),
    ]);
    assert_eq!(submit_policy(&own, 3, &[policy(3, 0x02)]).0, OK);
}

#[test]
fn the_kernel_refuses_guards_on_above_anothers_guard_at_apply() {
    let probe = Id::new_static("probe");
    let lock = Id::new_static("lock");
    let mut kernel = guarded_kernel();
    open(&mut kernel, "probe.base");
    open(&mut kernel, "lock.screen");
    // The lock opens the keyboard (another plugin's screen) above itself.
    kernel.drain_native_command(
        &lock,
        Command::OpenScreen(Id::new_static("probe.more"), Params::new()),
    );
    assert_eq!(
        stack_of(&kernel),
        ["probe.base", "lock.screen", "probe.more"]
    );
    let keyboard = kernel.stack.last().unwrap().serial;
    // The kernel's rows name the owners: lock 12, the keyboard (probe) OWN.
    let rows: Vec<StackRow> = kernel
        .stack
        .iter()
        .map(|active| StackRow {
            instance: active.serial,
            owner: if active.serial == keyboard {
                OWN
            } else {
                OTHER
            },
            flags: if active.guards() { GUARDS } else { 0 },
        })
        .collect();
    kernel.shared.stack_rows.update(rows);
    kernel.take_warnings();
    kernel.native_screen_policies(
        &probe,
        OWN,
        vec![PolicyChange {
            instance: keyboard,
            flags: 0x02,
            ready: false,
        }],
    );
    // The lock is still the guard, nothing else guards, one warning.
    assert_eq!(kernel.guard(), Some(1));
    assert!(!kernel.stack[2].guards());
    assert_eq!(kernel.take_warnings().len(), 1);
    // The lock closes itself still (it was never displaced).
    let lock_key = kernel.shared.stack[1].key;
    kernel.drain_native_command(&lock, Command::Close(lock_key));
    assert_eq!(stack_of(&kernel), ["probe.base", "probe.more"]);
}

/// `command.screen_ready` is the own top instance's, else `ERR_STATE`, and is
/// staged apart from the commands like a policy.
#[test]
fn screen_ready_is_the_own_top_instance_and_staged_apart() {
    let kernel = kernel(&[BASE, MINE]);
    let (status, staged, changes) = submit_policy(&kernel, 3, &[WireCommand::ScreenReady {}]);
    assert_eq!(status, OK);
    assert!(staged.is_empty());
    assert_eq!(
        changes,
        [PolicyChange {
            instance: 3,
            flags: 0,
            ready: true,
        }]
    );
    // From a callback of a screen that is not the top one.
    let (status, staged, changes) = submit_policy(&kernel, 1, &[WireCommand::ScreenReady {}]);
    assert_eq!(status, ERR_STATE);
    assert!(staged.is_empty() && changes.is_empty());
}
