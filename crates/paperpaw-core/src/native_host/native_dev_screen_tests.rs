//! A native screen's power and Wi-Fi commands through the real kernel and an
//! installed plugin (#2384): the install hands the kernel's Wi-Fi table to the
//! host services, the screen's callback stage is admitted with the plugin's
//! features, and what it staged reaches the radio from the kernel's pump.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
use super::super::*;
use super::{BOUND, FEATURES, STAGED};
use crate::native_host::{
    admitted_features::AdmittedFeatures,
    commands::{
        WireCommand, commands_encode,
        dev::{FEATURE_POWER, FEATURE_WIFI},
    },
};
use paperpaw_native::metadata::Feature;
use paperpaw_platform::{ButtonState, InputEvent, Instant};
use paperpaw_sdk::{Id, Params, wifi::WifiCommand};

const HOST: &[Feature<'static>] = &[
    Feature {
        key: FEATURE_WIFI,
        revision: 1,
    },
    Feature {
        key: FEATURE_POWER,
        revision: 1,
    },
];

/// A Wi-Fi and sleep device with `factory.screen` (admitted `uses.wifi` and
/// `uses.power`, open policy `policy`) shown, whose screen submits `commands`
/// on its action. The Wi-Fi table keeps its outcomes for the test.
fn shown(commands: &[WireCommand<'_>], policy: u32) -> crate::Kernel {
    super::super::reset(NORMAL);
    super::super::ARMS.with(|arms| arms.set(1));
    super::super::MENUS.with(|menus| menus.set(BOUND));
    FEATURES.with(|features| features.set(AdmittedFeatures::test(0b11, HOST)));
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .wifi(true)
        .sleep(true)
        .build()
        .unwrap();
    kernel.radio_requests.borrow_mut().hold_back(u64::MAX);
    install(&mut kernel, policy << 6).unwrap();
    FEATURES.with(|features| features.set(AdmittedFeatures::NONE));
    super::super::MENUS.with(|menus| menus.set(0));
    super::super::ARMS.with(|arms| arms.set(0));
    let mut bytes = [0; 256];
    let len = commands_encode(commands, &mut bytes).unwrap();
    STAGED.with(|batch| *batch.borrow_mut() = Some(bytes[..len].to_vec()));
    kernel.open_screen(
        paperpaw_platform::Instant(1),
        Id::new_static("factory.screen"),
        Params::new(),
    );
    let top = kernel.stack.len() - 1;
    kernel.with_screen(top, |screen, cx| {
        screen.on_show(cx);
        Handled::Yes
    });
    kernel
}

#[test]
fn a_screens_scan_and_sleep_reach_the_radio_and_the_kernel_after_its_callback() {
    let mut kernel = shown(&[WireCommand::WifiScan {}, WireCommand::Sleep {}], 0);
    assert!(
        kernel
            .shared
            .native_instances
            .hosts()
            .and_then(|hosts| hosts.radio_requests())
            .is_some_and(|table| alloc::rc::Rc::ptr_eq(table, &kernel.radio_requests)),
        "the install wired the kernel's own table"
    );
    kernel.io.wifi_calls.clear();
    // The shell is told to sleep by the effects of the input that ran it.
    let sleep = [ButtonState::Pressed, ButtonState::Released]
        .into_iter()
        .zip([100, 140])
        .map(|(state, at)| {
            let name = "confirm";
            kernel.handle(Instant(at), InputEvent::Button { name, state })
        })
        .fold(false, |sleep, effects| sleep | effects.sleep());
    assert_eq!(kernel.io.wifi_calls, [WifiCommand::Scan]);
    assert!(sleep, "sleep is applied after the callback, with the scan");
    STAGED.with(|batch| *batch.borrow_mut() = None);
}

/// The screen ids on the stack, bottom to top.
fn stack(kernel: &crate::Kernel) -> alloc::vec::Vec<&str> {
    kernel.stack.iter().map(|s| s.id.as_str()).collect()
}

fn confirm(kernel: &mut crate::Kernel) {
    for (state, at) in [(ButtonState::Pressed, 100), (ButtonState::Released, 140)] {
        let name = "confirm";
        let _ = kernel.handle(Instant(at), InputEvent::Button { name, state });
    }
}

#[test]
fn a_scan_between_two_opens_applies_and_both_opens_apply_around_it() {
    let open = WireCommand::Open {
        target: "factory.screen",
        mode: 0,
        slot: "",
        handover: 0,
    };
    // Policy 2: every open makes a new instance.
    let mut kernel = shown(&[open, WireCommand::WifiScan {}, open], 2);
    kernel.io.wifi_calls.clear();
    confirm(&mut kernel);
    assert_eq!(kernel.io.wifi_calls, [WifiCommand::Scan]);
    assert_eq!(stack(&kernel), ["factory.screen"; 3]);
    STAGED.with(|batch| *batch.borrow_mut() = None);
}

#[test]
fn a_stopped_plugins_wifi_requests_are_released_undelivered() {
    let join = WireCommand::WifiJoin {
        request: 7,
        ssid: "home",
        password: crate::abi7::commands_wire::SecretText::new("hunter2-PASSWORD"),
        flags: 0,
    };
    let mut kernel = shown(&[join], 0);
    confirm(&mut kernel);
    assert!(kernel.radio_requests.borrow().holds("factory", 7));
    let owner = Id::new_static("factory");
    let key = kernel.shared.native_instances.find("factory").unwrap();
    kernel.begin_native_unload(key, &owner);
    assert!(!kernel.radio_requests.borrow().holds("factory", 7));
    assert!(
        kernel
            .radio_requests
            .borrow()
            .next_outcome("factory")
            .is_none()
    );
    STAGED.with(|batch| *batch.borrow_mut() = None);
}
