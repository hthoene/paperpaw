//! Power, Wi-Fi and firmware commands (`docs/abi/device.md` section 3) from a
//! real ABI callback through the kernel's own stage (#2384): the instance's
//! admitted features and the kernel's Wi-Fi table reach the stage, a scan and
//! a join reach the radio, a foreign guard refuses the batch whole, sleep
//! applies with the commands around it, and a firmware command without the
//! firmware table stays `ERR_UNSUPPORTED`.
use super::*;
use crate::abi7::commands_wire::SecretText;
use crate::native_host::{
    admitted_features::AdmittedFeatures,
    commands::dev::{FEATURE_POWER, FEATURE_WIFI, NETWORK_MANAGER},
};
use alloc::vec::Vec;
use core::cell::RefCell;
use paperpaw_native::metadata::Feature;
use paperpaw_sdk::{SettingValue, wifi::WifiCommand};

const HOST: &[Feature<'static>] = &[
    Feature {
        key: FEATURE_WIFI,
        revision: 1,
    },
    Feature {
        key: FEATURE_POWER,
        revision: 1,
    },
    Feature {
        key: "uses.firmware",
        revision: 1,
    },
];

std::thread_local! {
    /// The encoded batch the callback submits.
    static DEV_BATCH: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    /// What `commands.submit` answered it.
    static DEV_STATUS: Cell<i32> = const { Cell::new(OK) };
}

unsafe extern "C" fn make_dev(
    host: *const PpHost,
    ctx: *mut c_void,
    out: *mut *const PpPlugin,
) -> i32 {
    // SAFETY: forwarded factory arguments satisfy the original fixture's contract.
    let status = unsafe { make(host, ctx, out) };
    if status == OK {
        // SAFETY: make just initialized this uniquely owned fixture table; it
        // is not published to the host until this factory returns.
        unsafe { (*(*out).cast_mut()).input = Some(input_dev) };
    }
    status
}

unsafe extern "C" fn input_dev(state: *mut c_void, _: u32, _: PpBytes) -> i32 {
    let bytes = DEV_BATCH.with(|batch| batch.borrow().clone());
    // SAFETY: owner admission keeps the fixture and its table alive; the bytes
    // are readable until the synchronous submit returns.
    let status = unsafe {
        let fake = &*state.cast::<Fake>();
        ((*fake.host).commands_submit)(
            fake.ctx,
            PpBytes {
                ptr: bytes.as_ptr(),
                len: bytes.len(),
            },
        )
    };
    DEV_STATUS.with(|value| value.set(status));
    OK
}

/// A Wi-Fi, sleep and power-off device whose screen `fake.screen` is shown.
fn dev_kernel() -> crate::Kernel {
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .wifi(true)
        .sleep(true)
        .power_off(true)
        .build()
        .unwrap();
    // The table's outcomes stay where the test reads them.
    kernel.radio_requests.borrow_mut().hold_back(u64::MAX);
    kernel
        .shared
        .settings
        .define(paperpaw_sdk::SettingSpec::switch(
            "fake.toggle",
            "toggle",
            false,
            paperpaw_sdk::builtin::groups::OTHER,
        ))
        .unwrap();
    kernel.shared.stack.push(StackEntry::new(
        Id::new_static("fake.screen"),
        None,
        false,
        ScreenKey::new(7),
    ));
    kernel
}

/// Runs the input callback of `fake` (every DEV feature admitted), which
/// submits `commands`; the table wired as the kernel wires it at install
/// unless `unwired`. What `commands.submit` answered.
fn submit_dev(kernel: &mut crate::Kernel, commands: &[WireCommand<'_>], unwired: bool) -> i32 {
    reset(NORMAL);
    let mut bytes = [0; 256];
    let len = commands_encode(commands, &mut bytes).unwrap();
    DEV_BATCH.with(|batch| *batch.borrow_mut() = bytes[..len].to_vec());
    DEV_STATUS.with(|value| value.set(ERR_INVALID));
    // SAFETY: this fixture's factory is callable for the retained module lifetime.
    let hosted = unsafe {
        Hosted::new(
            Heap,
            Module::new(),
            make_dev,
            "fake",
            (ptr::null(), 0),
            report,
            crate::native_host::HostResources::try_new().unwrap(),
            DeclaredServices::none().with_features(AdmittedFeatures::test(0b111, HOST)),
        )
    }
    .unwrap();
    if !unwired {
        let context = hosted.owner().context();
        context
            .resources
            .hosts()
            .wire_radio_requests(&kernel.radio_requests);
    }
    let batch = hosted
        .make_registered(hosted.begin().unwrap())
        .unwrap()
        .commit()
        .unwrap();
    let result = kernel.with_native_commands(
        &batch,
        &DispatchDepth::new(),
        crate::native_host::shell::command_dispatch::CommandTarget {
            screen: Some(ScreenKey::new(7)),
            hop: 0,
            result: crate::native_host::shell::command_dispatch::CommandResult::Input,
        },
        paperpaw_platform::Instant(0),
        |context, frame| {
            crate::native_host::shell::tokens::invoke_token(
                context,
                TOKEN,
                frame,
                TokenInvocation::Input(&[]),
            )
        },
    );
    assert_eq!(result, Ok(OK));
    drop(batch);
    finish(&hosted);
    DEV_STATUS.with(Cell::get)
}

fn wifi_calls(kernel: &mut crate::Kernel) -> Vec<WifiCommand> {
    core::mem::take(&mut kernel.io.wifi_calls)
}

fn toggle(on: bool) -> WireCommand<'static> {
    WireCommand::SetSetting {
        key: "fake.toggle",
        value: crate::abi7::settings_wire::SettingValueRef::Bool(on),
    }
}

#[test]
fn a_scan_and_a_join_reach_the_radio_and_the_join_waits_in_the_kernels_table() {
    let mut kernel = dev_kernel();
    // The network manager may join without a fresh input.
    kernel
        .shared
        .settings
        .set(NETWORK_MANAGER, SettingValue::Text("fake".into()))
        .unwrap();
    let join = WireCommand::WifiJoin {
        request: 7,
        ssid: "home",
        password: SecretText::new("hunter2-PASSWORD"),
        flags: 0,
    };
    let status = submit_dev(&mut kernel, &[WireCommand::WifiScan {}, join], false);
    assert_eq!(status, OK);
    let calls = wifi_calls(&mut kernel);
    assert_eq!(calls.first(), Some(&WifiCommand::Scan));
    assert!(
        matches!(calls.get(1), Some(WifiCommand::Join { ssid, .. }) if ssid == "home"),
        "{calls:?}"
    );
    assert!(kernel.radio_requests.borrow().holds("fake", 7));
}

#[test]
fn a_foreign_guard_refuses_radio_off_and_power_off_whole_and_its_own_plugin_may() {
    let batch = [WireCommand::WifiOff {}, WireCommand::PowerOff {}];
    let mut kernel = dev_kernel();
    let mut guard = StackEntry::new(
        Id::new_static("guard.screen"),
        None,
        false,
        ScreenKey::new(8),
    );
    guard.guards = true;
    kernel.shared.stack.push(guard);
    assert_eq!(
        submit_dev(&mut kernel, &batch, false),
        host_glue::ERR_DENIED
    );
    assert!(
        wifi_calls(&mut kernel).is_empty(),
        "nothing of the batch applied"
    );
    assert!(!kernel.power_off);
    // The guard is the submitter's own: the batch applies.
    let mut kernel = dev_kernel();
    kernel.shared.stack[0].guards = true;
    assert_eq!(submit_dev(&mut kernel, &batch, false), OK);
    assert_eq!(wifi_calls(&mut kernel), [WifiCommand::Off]);
    assert!(kernel.power_off);
}

#[test]
fn sleep_applies_with_the_commands_around_it() {
    let mut kernel = dev_kernel();
    let batch = [
        WireCommand::WifiOn {},
        toggle(true),
        WireCommand::Sleep {},
        WireCommand::Redraw { mode: 0 },
    ];
    assert_eq!(submit_dev(&mut kernel, &batch, false), OK);
    assert_eq!(wifi_calls(&mut kernel), [WifiCommand::On]);
    assert!(kernel.shared.settings.bool("fake.toggle"));
    assert!(kernel.sleep, "the shell sleeps once the frame is shown");
    assert!(kernel.output.redraw);
}

#[test]
fn firmware_without_its_table_and_any_dev_command_without_the_radio_table_are_unsupported() {
    let mut kernel = dev_kernel();
    // Admitted, but this device wired no firmware table.
    let restart = [WireCommand::FirmwareRestart {}];
    assert_eq!(
        submit_dev(&mut kernel, &restart, false),
        host_glue::ERR_UNSUPPORTED
    );
    // No Wi-Fi table wired (no native install yet): as before the wiring.
    let sleep = [toggle(true), WireCommand::Sleep {}];
    assert_eq!(
        submit_dev(&mut kernel, &sleep, true),
        host_glue::ERR_UNSUPPORTED
    );
    assert!(!kernel.sleep);
    assert!(!kernel.shared.settings.bool("fake.toggle"));
}
