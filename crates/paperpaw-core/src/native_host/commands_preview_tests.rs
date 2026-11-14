//! `command.preview_setting` and `command.preview_end` through the stage and
//! the kernel (`docs/abi/registry.md` 4.4): a set shows in place of the kept
//! values and is never saved, only the top screen's plugin shows one, only
//! its plugin ends it, and it ends with its screen and with the sleep.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
use super::*;
use crate::abi7::settings_wire::settings_encode;
use paperpaw_platform::Instant;
use paperpaw_sdk::{Env, Params, builtin::groups};

/// A kernel with two plugins: `probe` (a number, a text, a secret, two
/// screens) and `other` (a guarded switch, a number, a screen).
fn kernel() -> crate::Kernel {
    use paperpaw_sdk::{Manifest, Plugin, Registrar, Screen, ScreenSpec, Version};
    struct Empty;
    impl Screen for Empty {
        fn draw(&mut self, _: &mut paperpaw_graphics::Canvas<'_>, _: &Env<'_>) {}
        fn on_action(&mut self, _: &Id, _: &mut paperpaw_sdk::Cx<'_, '_>) -> paperpaw_sdk::Handled {
            paperpaw_sdk::Handled::No
        }
    }
    struct Screens(
        &'static str,
        &'static [&'static str],
        fn() -> Vec<SettingSpec>,
    );
    impl Plugin for Screens {
        fn manifest(&self) -> Manifest {
            Manifest::new(Id::new_static(self.0), "Probe", Version::new(1, 0, 0))
                .uses(paperpaw_sdk::Uses::NOTHING)
        }
        fn register(&self, r: &mut Registrar) -> Result<(), paperpaw_sdk::RegisterError> {
            for id in self.1 {
                r.add_screen(ScreenSpec::new(
                    Id::new_static(id),
                    "probe.title",
                    alloc::boxed::Box::new(|_, _| alloc::boxed::Box::new(Empty)),
                ))?;
            }
            for setting in (self.2)() {
                r.add_setting(setting)?;
            }
            Ok(())
        }
    }
    let probe = || {
        alloc::vec![
            SettingSpec::number("probe.size", "size", 1, 9, 3, groups::OTHER),
            SettingSpec::text("probe.name", "name", "", groups::OTHER),
            SettingSpec::text("probe.pin", "pin", "", groups::OTHER).with_extra(&SECRET),
        ]
    };
    let other = || {
        alloc::vec![
            SettingSpec::switch("other.guarded", "guarded", false, groups::OTHER).guarded(),
            SettingSpec::number("other.size", "size", 1, 9, 5, groups::OTHER),
        ]
    };
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .plugin(Screens("probe", &["probe.base", "probe.second"], probe))
        .plugin(Screens("other", &["other.base"], other))
        .build()
        .unwrap();
    kernel.apply(Command::OpenScreen(
        Id::new_static("probe.base"),
        Params::new(),
    ));
    kernel
}

static SECRET: paperpaw_sdk::SettingExtra = paperpaw_sdk::SettingExtra::new().secret();

/// A value as `command.preview_setting` carries it.
fn encoded(value: SettingValueRef<'_>) -> Vec<u8> {
    let mut out = [core::mem::MaybeUninit::new(0_u8); 512];
    let len = settings_encode(value, &mut out).unwrap();
    // SAFETY: every slot was initialized above; the first `len` hold the value.
    out[..len]
        .iter()
        .map(|byte| unsafe { byte.assume_init() })
        .collect()
}

fn show<'a>(key: &'a str, value: &'a [u8]) -> WireCommand<'a> {
    WireCommand::PreviewSetting {
        key,
        value,
        flags: 0,
    }
}

const END: WireCommand<'static> = WireCommand::PreviewEnd { flags: 0 };

/// One batch of a callback of `owner` for the screen on top (a work run's
/// when `quiet`): staged, and applied by the kernel when it was accepted.
fn submit(kernel: &mut crate::Kernel, owner: &str, quiet: bool, batch: &[WireCommand<'_>]) -> i32 {
    let owner = Id::parse(owner).unwrap();
    let held = kernel.preview_held(owner.as_str());
    let (status, commands, redraw) = {
        let env = kernel.shared.env(Instant(0));
        let top = env.stack().last().map(|top| top.key);
        let mut stage =
            CommandStage::new(&owner, env.settings(), None, 8, 4096).with_ui(&env, top, 0, 0);
        stage.set_preview_held(held);
        stage.set_quiet(quiet);
        let mut bytes = [0; COMMAND_MAX_BYTES];
        let len = commands_encode(batch, &mut bytes).unwrap();
        let status = stage.stage(&commands_decode(&bytes[..len]).unwrap());
        let (commands, redraw, _) = stage.into_ui_output();
        (status, commands, redraw)
    };
    if status == OK {
        kernel.drain_native_ui(&owner, commands, redraw);
        kernel.finish();
    } else {
        assert!(commands.is_empty(), "a refused batch stages nothing");
    }
    status
}

#[test]
fn a_number_previewed_shows_until_the_preview_ends_and_is_never_kept() {
    let mut kernel = kernel();
    let seven = encoded(SettingValueRef::Int(7));
    assert_eq!(kernel.settings().int("probe.size"), 3);
    // An end with no preview is accepted and ends nothing.
    assert_eq!(submit(&mut kernel, "probe", false, &[END]), OK);
    assert_eq!(
        submit(&mut kernel, "probe", false, &[show("probe.size", &seven)]),
        OK
    );
    // Shown (slot 5 and `reg_setting` read the same registry), and the
    // preview is the plugin's.
    assert_eq!(kernel.settings().int("probe.size"), 7);
    assert!(kernel.settings().is_previewed("probe.size"));
    assert_eq!(kernel.preview_held("probe"), PreviewHeld::Own);
    assert_eq!(kernel.preview_held("other"), PreviewHeld::Other);
    assert_eq!(submit(&mut kernel, "probe", false, &[END]), OK);
    assert_eq!(kernel.settings().int("probe.size"), 3, "never kept");
    assert!(!kernel.settings().previewing());
    assert_eq!(kernel.preview_held("probe"), PreviewHeld::Nobody);
    assert!(kernel.take_warnings().is_empty());
}

#[test]
fn the_rows_of_a_batch_are_one_set_that_replaces_the_one_before() {
    let mut kernel = kernel();
    let (eight, name) = (
        encoded(SettingValueRef::Int(8)),
        encoded(SettingValueRef::Text("Ada")),
    );
    kernel.apply(Command::SetSetting(
        Id::new_static("probe.size"),
        SettingValue::Int(4),
    ));
    kernel.finish();
    // A name, and the size as if it were not set (its default).
    let unset = WireCommand::PreviewSetting {
        key: "probe.size",
        value: &[],
        flags: 1,
    };
    let batch = [show("probe.name", &name), unset];
    assert_eq!(submit(&mut kernel, "probe", false, &batch), OK);
    assert_eq!(kernel.settings().text("probe.name"), "Ada");
    assert_eq!(kernel.settings().int("probe.size"), 3);
    // A second batch is a new set: what it leaves out shows as kept again.
    assert_eq!(
        submit(&mut kernel, "probe", false, &[show("probe.size", &eight)]),
        OK
    );
    assert_eq!(kernel.settings().int("probe.size"), 8);
    assert_eq!(kernel.settings().text("probe.name"), "");
    assert!(!kernel.settings().is_previewed("probe.name"));
    assert_eq!(submit(&mut kernel, "probe", false, &[END]), OK);
    assert_eq!(kernel.settings().int("probe.size"), 4);
}

#[test]
fn what_is_not_a_preview_is_refused_and_nothing_shows() {
    let mut kernel = kernel();
    let (seven, ten, on, name) = (
        encoded(SettingValueRef::Int(7)),
        encoded(SettingValueRef::Int(10)),
        encoded(SettingValueRef::Bool(true)),
        encoded(SettingValueRef::Text("Ada")),
    );
    let flagged = |flags, value| WireCommand::PreviewSetting {
        key: "probe.size",
        value,
        flags,
    };
    let long = encoded(SettingValueRef::Text(&"n".repeat(300)));
    let cases: [(&[WireCommand<'_>], i32, &str); 12] = [
        (&[flagged(2, &seven)], ERR_INVALID, "an unknown flag"),
        (&[flagged(1, &seven)], ERR_INVALID, "not set, with a value"),
        (&[flagged(0, &[])], ERR_INVALID, "no value"),
        (&[flagged(0, &seven[..11])], ERR_INVALID, "a cut value"),
        (&[flagged(0, &ten)], ERR_INVALID, "out of range"),
        (&[flagged(0, &on)], ERR_INVALID, "another kind"),
        (&[show("probe.none", &seven)], ERR_INVALID, "unknown"),
        (&[show("probe.pin", &name)], ERR_UNSUPPORTED, "a secret"),
        (&[show("other.guarded", &on)], ERR_INVALID, "guarded"),
        (&[show("probe.size", &seven), END], ERR_INVALID, "both"),
        (
            &[WireCommand::PreviewEnd { flags: 1 }],
            ERR_INVALID,
            "an end with a flag",
        ),
        // Eight rows of 322 bytes each: over the 2,048 bytes of one set.
        (&[show("probe.name", &long); 8], ERR_NO_SPACE, "too large"),
    ];
    for (batch, status, what) in cases {
        assert_eq!(submit(&mut kernel, "probe", false, batch), status, "{what}");
        assert!(!kernel.settings().previewing(), "{what}");
    }
    // Good rows beside a bad one: the set is refused whole.
    let batch = [show("probe.size", &seven), show("probe.size", &ten)];
    assert_eq!(submit(&mut kernel, "probe", false, &batch), ERR_INVALID);
    assert!(!kernel.settings().previewing());
    // Six such rows fit.
    assert_eq!(
        submit(&mut kernel, "probe", false, &[show("probe.name", &long); 6]),
        OK
    );
    assert!(kernel.settings().is_previewed("probe.name"));
}

#[test]
fn only_a_screen_on_top_previews_and_never_a_work_run() {
    let mut kernel = kernel();
    let seven = encoded(SettingValueRef::Int(7));
    let set = [show("probe.size", &seven)];
    // A work run shows nothing, and ends nothing.
    assert_eq!(submit(&mut kernel, "probe", true, &set), ERR_STATE);
    assert_eq!(submit(&mut kernel, "probe", true, &[END]), ERR_STATE);
    // Another plugin's screen is on top: not the submitter's to preview on.
    kernel.apply(Command::OpenScreen(
        Id::new_static("other.base"),
        Params::new(),
    ));
    assert_eq!(submit(&mut kernel, "probe", false, &set), ERR_STATE);
    assert!(!kernel.settings().previewing());
    // The plugin on top may, also a setting of another plugin (not guarded).
    assert_eq!(submit(&mut kernel, "other", false, &set), OK);
    assert_eq!(kernel.settings().int("probe.size"), 7);
}

#[test]
fn another_plugins_preview_is_replaced_but_not_ended() {
    let mut kernel = kernel();
    let (seven, two) = (
        encoded(SettingValueRef::Int(7)),
        encoded(SettingValueRef::Int(2)),
    );
    assert_eq!(
        submit(&mut kernel, "probe", false, &[show("probe.size", &seven)]),
        OK
    );
    // Not the other plugin's to end, at submit and when a staged end applies.
    assert_eq!(submit(&mut kernel, "other", false, &[END]), ERR_STATE);
    let other = Id::new_static("other");
    kernel.drain_native_ui(
        &other,
        alloc::vec![Command::PreviewSettings(Vec::new())],
        None,
    );
    kernel.finish();
    assert_eq!(kernel.settings().int("probe.size"), 7, "still shown");
    let warnings = kernel.take_warnings();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].message.contains("native preview dropped"));
    // A set staged for a screen that is no longer on top does not apply.
    kernel.apply(Command::OpenScreen(
        Id::new_static("other.base"),
        Params::new(),
    ));
    let probe = Id::new_static("probe");
    let late = alloc::vec![(Id::new_static("probe.size"), Some(SettingValue::Int(9)))];
    kernel.drain_native_ui(&probe, alloc::vec![Command::PreviewSettings(late)], None);
    kernel.finish();
    assert_eq!(kernel.settings().int("probe.size"), 7, "the set before");
    assert_eq!(kernel.take_warnings().len(), 1);
    // The other plugin's own set, from its screen on top, replaces it whole.
    assert_eq!(
        submit(&mut kernel, "other", false, &[show("other.size", &two)]),
        OK
    );
    assert_eq!(kernel.settings().int("other.size"), 2);
    assert_eq!(kernel.settings().int("probe.size"), 3);
    assert_eq!(kernel.preview_held("probe"), PreviewHeld::Other);
    assert_eq!(kernel.preview_held("other"), PreviewHeld::Own);
    assert_eq!(submit(&mut kernel, "probe", false, &[END]), ERR_STATE);
    assert_eq!(submit(&mut kernel, "other", false, &[END]), OK);
    assert_eq!(kernel.settings().int("other.size"), 5);
}

#[test]
fn a_preview_ends_with_its_screen_and_when_the_device_sleeps() {
    let mut kernel = kernel();
    let seven = encoded(SettingValueRef::Int(7));
    let set = [show("probe.size", &seven)];
    // Asleep: the kept value shows after the sleep.
    assert_eq!(submit(&mut kernel, "probe", false, &set), OK);
    assert_eq!(kernel.settings().int("probe.size"), 7);
    kernel.suspend(Instant(10));
    assert_eq!(kernel.settings().int("probe.size"), 3);
    assert_eq!(kernel.preview_held("probe"), PreviewHeld::Nobody);
    kernel.resume(Instant(20));
    // Its screen closes: over as well.
    kernel.apply(Command::OpenScreen(
        Id::new_static("probe.second"),
        Params::new(),
    ));
    assert_eq!(submit(&mut kernel, "probe", false, &set), OK);
    assert_eq!(kernel.settings().int("probe.size"), 7);
    let top = kernel.shared.stack.last().unwrap().key;
    kernel.apply(Command::Close(top));
    kernel.finish();
    assert_eq!(kernel.settings().int("probe.size"), 3);
    assert!(!kernel.settings().previewing());
}
