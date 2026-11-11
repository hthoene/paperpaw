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
/// `uses.power`) shown, whose screen submits `commands` on its action.
fn shown(commands: &[WireCommand<'_>]) -> crate::Kernel {
    super::super::reset(NORMAL);
    super::super::ARMS.with(|arms| arms.set(1));
    super::super::MENUS.with(|menus| menus.set(BOUND));
    FEATURES.with(|features| features.set(AdmittedFeatures::test(0b11, HOST)));
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .wifi(true)
        .sleep(true)
        .build()
        .unwrap();
    install(&mut kernel, 0).unwrap();
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
    let mut kernel = shown(&[WireCommand::WifiScan {}, WireCommand::Sleep {}]);
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
