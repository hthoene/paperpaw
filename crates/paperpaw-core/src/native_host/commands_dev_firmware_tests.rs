//! The firmware commands in the DEV stage: checked whole, accepted at submit,
//! given back when the batch never applies.
#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
use super::super::{DevBatchOutput, DevContext, DevHost, InputClock, NoStoredNetworks};
use super::*;
use crate::{
    Kernel,
    abi7::commands_wire::{commands_decode, commands_encode},
    native_host::{
        admitted_features::AdmittedFeatures,
        firmware_install::{BuiltIns, Event, FirmwareInstall},
    },
};
use paperpaw_native::host_glue::{ERR_FULL, ERR_STATE, OK};
use paperpaw_native::metadata::Feature;
use paperpaw_platform::{
    Instant, NoStorage,
    firmware::WRITE_BLOCK,
    firmware_fake::{FakeFirmware, paperpaw_facts},
};
use paperpaw_sdk::Id;

const HOST: &[Feature<'static>] = &[
    Feature {
        key: FEATURE_FIRMWARE,
        revision: 1,
    },
    Feature {
        key: FEATURE_INSTALL,
        revision: 1,
    },
    Feature {
        key: "uses.storage",
        revision: 1,
    },
];
const FIRMWARE: u32 = 1;
const INSTALL: u32 = 2;
const STORAGE: u32 = 4;
const OWNER: u64 = 9;

struct Names;
impl BuiltIns for Names {
    fn count(&self) -> usize {
        0
    }
    fn index_of(&self, _: &str) -> Option<usize> {
        None
    }
}

struct Clock;
impl InputClock for Clock {
    fn now_ms(&self) -> u64 {
        0
    }
    fn last_input_ms(&self) -> Option<u64> {
        None
    }
}

fn driver() -> Rc<RefCell<Driver>> {
    let fake = FakeFirmware::new(
        "0.5.0",
        "paperpaw-x4",
        5,
        paperpaw_facts("0.6.0", "paperpaw-x4", 2 * WRITE_BLOCK),
    );
    Rc::new(RefCell::new(Driver::new(
        alloc::boxed::Box::new(fake),
        Rc::new(RefCell::new(FirmwareInstall::new())),
    )))
}

fn kernel() -> Kernel {
    Kernel::builder(crate::tests::DEVICE).build().unwrap()
}

fn install(request: u32, path: &str) -> WireCommand<'_> {
    WireCommand::InstallFirmware {
        request,
        root: 0,
        path,
        key: &[],
        flags: 0,
    }
}

fn check(request: u32, kind: u32) -> WireCommand<'static> {
    WireCommand::CheckFile {
        request,
        root: 0,
        path: "update.ppu",
        kind,
    }
}

/// Stages `commands` for the plugin `OWNER` with `bits` of the features and
/// `firmware` as the device's table (none: no firmware host); the status and
/// the output, undrained.
fn stage(
    kernel: &Kernel,
    bits: u32,
    firmware: Option<&Rc<RefCell<Driver>>>,
    commands: &[WireCommand<'_>],
) -> (i32, DevBatchOutput) {
    let id = Id::new_static("update");
    let env = kernel.shared.env(Instant(0));
    let mut ctx = DevContext::new(AdmittedFeatures::test(bits, HOST), &kernel.radio_requests);
    if let Some(driver) = firmware {
        ctx = ctx.with_firmware(FirmwareContext {
            driver: Rc::clone(driver),
            owner: OWNER,
        });
    }
    let mut stage =
        crate::native_host::commands::CommandStage::new(&id, env.settings(), None, 16, 4096)
            .with_dev(ctx);
    let mut bytes = [0; 2048];
    let len = commands_encode(commands, &mut bytes).unwrap();
    let status = stage.stage(&commands_decode(&bytes[..len]).unwrap());
    (status, stage.into_dev_output())
}

fn drain(kernel: &mut Kernel, output: DevBatchOutput) {
    let (staged, redraw, _, batch) = output;
    let host = DevHost {
        clock: &Clock,
        stored: &NoStoredNetworks,
    };
    kernel.drain_native_dev(&Id::new_static("update"), staged, redraw, batch, &host);
}

#[test]
fn without_a_firmware_host_or_a_feature_the_commands_are_refused() {
    let kernel = kernel();
    let d = driver();
    let all = FIRMWARE | INSTALL | STORAGE;
    // No host: unsupported. No feature admitted: denied.
    assert_eq!(
        stage(&kernel, all, None, &[install(1, "a.ppu")]).0,
        ERR_UNSUPPORTED
    );
    assert_eq!(
        stage(&kernel, 0, Some(&d), &[install(1, "a.ppu")]).0,
        ERR_DENIED
    );
    assert_eq!(
        stage(&kernel, 0, Some(&d), &[WireCommand::FirmwareRestart {}]).0,
        ERR_DENIED
    );
    assert_eq!(d.borrow().waiting(), 0);
}

#[test]
fn the_fields_are_checked_before_anything_is_accepted() {
    let kernel = kernel();
    let d = driver();
    let all = FIRMWARE | INSTALL | STORAGE;
    let key5 = [1_u8; 5];
    let bad_key = WireCommand::InstallFirmware {
        request: 1,
        root: 0,
        path: "a.ppu",
        key: &key5,
        flags: 0,
    };
    let bad_flags = WireCommand::InstallFirmware {
        request: 1,
        root: 0,
        path: "a.ppu",
        key: &[],
        flags: 4,
    };
    let bad_root = WireCommand::InstallFirmware {
        request: 1,
        root: 2,
        path: "a.ppu",
        key: &[],
        flags: 0,
    };
    for command in [bad_key, bad_flags, bad_root, install(0, "a.ppu")] {
        assert_eq!(stage(&kernel, all, Some(&d), &[command]).0, ERR_INVALID);
    }
    assert_eq!(
        stage(&kernel, all, Some(&d), &[install(1, "")]).0,
        ERR_INVALID
    );
    // The user's files (root 1) need `uses.storage`.
    let card = WireCommand::InstallFirmware {
        request: 1,
        root: 1,
        path: "a.ppu",
        key: &[],
        flags: 0,
    };
    assert_eq!(stage(&kernel, FIRMWARE, Some(&d), &[card]).0, ERR_DENIED);
    // A plugin check is not here yet; a kind beyond 1 is denied.
    assert_eq!(
        stage(&kernel, all, Some(&d), &[check(1, 0)]).0,
        ERR_UNSUPPORTED
    );
    assert_eq!(stage(&kernel, all, Some(&d), &[check(1, 2)]).0, ERR_DENIED);
    assert_eq!(d.borrow().waiting(), 0, "a refusal accepts nothing");
}

#[test]
fn one_firmware_command_a_batch() {
    let kernel = kernel();
    let d = driver();
    let (status, _) = stage(
        &kernel,
        FIRMWARE,
        Some(&d),
        &[install(1, "a.ppu"), check(2, 1)],
    );
    assert_eq!(status, ERR_INVALID);
    assert_eq!(d.borrow().waiting(), 0);
}

#[test]
fn an_accepted_request_waits_and_a_batch_that_never_applies_gives_it_back() {
    let mut kernel = kernel();
    let d = driver();
    let (status, output) = stage(&kernel, FIRMWARE, Some(&d), &[install(1, "a.ppu")]);
    assert_eq!(status, OK);
    assert!(d.borrow().holds(OWNER, 1), "accepted at submit");
    drop(output);
    assert_eq!(d.borrow().waiting(), 0, "an undrained output takes it back");
    // A drained one keeps it.
    let (status, output) = stage(&kernel, FIRMWARE, Some(&d), &[check(2, 1)]);
    assert_eq!(status, OK);
    drain(&mut kernel, output);
    assert!(d.borrow().holds(OWNER, 2));
    // The same number again is refused while it waits.
    assert_eq!(
        stage(&kernel, FIRMWARE, Some(&d), &[check(2, 1)]).0,
        ERR_INVALID
    );
}

#[test]
fn the_fifth_request_is_full_and_nothing_of_it_is_kept() {
    let kernel = kernel();
    let d = driver();
    let mut kept = Vec::new();
    for request in 1..=4 {
        let (status, output) = stage(&kernel, FIRMWARE, Some(&d), &[install(request, "a.ppu")]);
        assert_eq!(status, OK);
        kept.push(output);
    }
    let (status, output) = stage(&kernel, FIRMWARE, Some(&d), &[install(5, "a.ppu")]);
    assert_eq!(status, ERR_FULL);
    drop(output);
    assert_eq!(d.borrow().waiting(), 4);
    assert!(!d.borrow().holds(OWNER, 5));
}

#[test]
fn a_restart_is_for_the_plugin_whose_install_waits_and_the_host_then_asks() {
    let mut kernel = kernel();
    let d = driver();
    // None waiting: `ERR_STATE`.
    assert_eq!(
        stage(
            &kernel,
            FIRMWARE,
            Some(&d),
            &[WireCommand::FirmwareRestart {}]
        )
        .0,
        ERR_STATE
    );
    // The owner's install runs to state 3.
    d.borrow_mut()
        .submit_install(OWNER, 1, "a.ppu", None, 0)
        .unwrap();
    let ask = loop {
        if let Event::Ask(ask) = d.borrow_mut().pump(&NoStorage, &Names, 100) {
            break ask;
        }
    };
    d.borrow_mut().answer(
        ask.question(),
        crate::native_host::firmware_install::Answer::Yes,
    );
    loop {
        if matches!(
            d.borrow_mut().pump(&NoStorage, &Names, 100),
            Event::Ended(_)
        ) {
            break;
        }
    }
    let (status, output) = stage(
        &kernel,
        FIRMWARE,
        Some(&d),
        &[WireCommand::FirmwareRestart {}],
    );
    assert_eq!(status, OK);
    drain(&mut kernel, output);
    let ask = d.borrow_mut().take_ask().unwrap();
    assert!(ask.restart, "the host asks again");
}
