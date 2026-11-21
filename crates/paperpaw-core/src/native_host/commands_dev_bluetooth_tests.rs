#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
//! A native plugin's Bluetooth commands (1583-1591) are accepted where the
//! host offers `uses.bluetooth` and reach the kernel's radio: the search, the
//! link and the pairing the Bluetooth page asks for (#456).
use super::super::{
    AdmittedFeatures, DevContext, DevHost, FEATURE_BLUETOOTH, InputClock, NoStoredNetworks,
};
use super::*;
use crate::{
    BtCall, BtReport, Found, Io, Kernel,
    abi7::commands_wire::{commands_decode, commands_encode},
    native_host::commands::CommandStage,
};
use alloc::vec::Vec;
use paperpaw_native::host_glue::{ERR_DENIED, ERR_UNSUPPORTED, OK};
use paperpaw_native::metadata::Feature;
use paperpaw_platform::Instant;
use paperpaw_sdk::bluetooth::{ScanPower, Security};

const HOST: &[Feature<'static>] = &[Feature {
    key: FEATURE_BLUETOOTH,
    revision: 1,
}];
const OWNER: Id = Id::new_static("bluetooth");

struct Clock;
impl InputClock for Clock {
    fn now_ms(&self) -> u64 {
        0
    }
    fn last_input_ms(&self) -> Option<u64> {
        None
    }
}

fn kernel() -> Kernel {
    let mut kernel = Kernel::builder(crate::tests::DEVICE)
        .bluetooth(2)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel.take_io();
    kernel
}

/// Stages `commands` for the Bluetooth plugin admitted with the features
/// `bits` of `host`; drains them where they were accepted.
fn run(
    kernel: &mut Kernel,
    host: &'static [Feature<'static>],
    bits: u32,
    commands: &[WireCommand<'_>],
) -> i32 {
    let owner = OWNER;
    let (status, (staged, redraw, _, batch)) = {
        let env = kernel.shared.place(Instant(0));
        let ctx = DevContext::new(AdmittedFeatures::test(bits, host), &kernel.radio_requests);
        let mut stage = CommandStage::new(&owner, env.settings(), None, 16, 4096).with_dev(ctx);
        let mut bytes = [0; 256];
        let len = commands_encode(commands, &mut bytes).unwrap();
        let status = stage.stage(&commands_decode(&bytes[..len]).unwrap());
        (status, stage.into_dev_output())
    };
    let dev = DevHost {
        clock: &Clock,
        stored: &NoStoredNetworks,
    };
    kernel.drain_native_dev(&owner, staged, redraw, batch, &dev);
    status
}

fn calls(kernel: &mut Kernel) -> Vec<BtCall> {
    kernel
        .take_io()
        .into_iter()
        .filter_map(|io| match io {
            Io::Bluetooth(call) => Some(call),
            _ => None,
        })
        .collect()
}

#[test]
fn searching_linking_and_pairing_reach_the_radio() {
    let mut kernel = kernel();
    let scan = WireCommand::BtScan { lasting_ms: 30_000 };
    assert_eq!(run(&mut kernel, HOST, 1, &[scan]), OK);
    // The reader's search: balanced, asking devices for their names.
    let scanned = BtCall::Scan {
        on: true,
        power: ScanPower::Balanced,
        active: true,
    };
    assert!(calls(&mut kernel).contains(&scanned));
    kernel.bluetooth_event(
        Instant(10),
        BtReport::Found(Found {
            address: "11:22".into(),
            name: Some("Pager".into()),
            signal: Some(-60),
            ..Found::default()
        }),
    );
    let connect = [
        WireCommand::BtStopScan {},
        WireCommand::BtConnect { device: 1 },
    ];
    assert_eq!(run(&mut kernel, HOST, 1, &connect), OK);
    let link = calls(&mut kernel)
        .into_iter()
        .find_map(|call| match call {
            BtCall::Connect { link, .. } => Some(link),
            _ => None,
        })
        .expect("a link asked for");
    kernel.bluetooth_event(
        Instant(20),
        BtReport::Connected {
            link,
            mtu: 23,
            security: Security::None,
        },
    );
    assert_eq!(
        run(&mut kernel, HOST, 1, &[WireCommand::BtPair { device: 1 }]),
        OK
    );
    assert!(calls(&mut kernel).contains(&BtCall::Pair { link }));
}

#[test]
fn bluetooth_commands_need_the_feature_offered_and_admitted() {
    let mut kernel = kernel();
    let scan = [WireCommand::BtScan { lasting_ms: 1_000 }];
    // Not declared by the plugin.
    assert_eq!(run(&mut kernel, HOST, 0, &scan), ERR_DENIED);
    // Not offered by a host without a radio.
    assert_eq!(run(&mut kernel, &[], 0, &scan), ERR_UNSUPPORTED);
    assert!(calls(&mut kernel).is_empty());
}

#[test]
fn malformed_bluetooth_commands_refuse_the_batch() {
    let mut kernel = kernel();
    for bad in [
        WireCommand::BtScan { lasting_ms: 0 },
        WireCommand::BtKeys { device: 1, on: 2 },
        WireCommand::BtGive {
            device: 1,
            plugin: "Not An Id",
        },
    ] {
        let batch = [WireCommand::BtScan { lasting_ms: 1_000 }, bad];
        assert_eq!(run(&mut kernel, HOST, 1, &batch), ERR_INVALID, "{bad:?}");
    }
    assert!(calls(&mut kernel).is_empty());
}
