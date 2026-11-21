//! The Bluetooth commands of the DEV stage (`docs/abi/device.md` 3, commands
//! 1583-1591, feature `uses.bluetooth`): checked at submit, applied after the
//! callback as the kernel's own Bluetooth commands (`bluetooth.rs`), which
//! answer with `event.bluetooth` (98) to the plugin bound to `core.bluetooth`
//! (`bluetooth_events.rs`).
//!
//! * **Checked at submit:** `bt_scan` lasts at least 1 ms (the kernel cuts it
//!   to its longest), `bt_give` names a valid plugin id (checked as it is
//!   copied) and `bt_keys.on` is 0 or 1, else `ERR_INVALID` for the whole
//!   batch. Whether a device is the
//!   plugin's, in reach or bonded is the kernel's to say, as a `failed` event.
//! * **A scan** is the search the reader starts on the Bluetooth page: at
//!   balanced power, asking devices for their names, as interface 6's page
//!   asked (the wire carries only its length).
//! * **`bt_take_back`** takes the device back from the plugin `core.device`
//!   op 11 names as the one it is given to.
use super::{CommandStage, DevOp, WireCommand, ui};
use crate::native_host::bluetooth_events::EVENTS;
use core::time::Duration;
use paperpaw_native::host_glue::ERR_INVALID;
use paperpaw_sdk::{
    Id,
    bluetooth::{
        self, BluetoothView, BtCommand, BtError, BtEvent, BtFailure, BtOp, BtRequest, DeviceId,
        ScanPower,
    },
    event::EventData,
    secrets::Anyone,
};

/// The feature key of commands 1583-1591.
pub const FEATURE_BLUETOOTH: &str = "uses.bluetooth";

/// One accepted Bluetooth command, by the host's device numbers.
#[derive(Debug)]
pub(crate) enum BtWire {
    Scan { lasting_ms: u32 },
    StopScan,
    Pair(u32),
    Connect(u32),
    Disconnect(u32),
    Forget(u32),
    Give { device: u32, plugin: Id },
    TakeBack(u32),
    Keys { device: u32, on: bool },
}

/// `Some(FEATURE_BLUETOOTH)` for commands 1583-1591.
pub(super) fn area(command: &WireCommand<'_>) -> Option<&'static str> {
    matches!(
        command,
        WireCommand::BtScan { .. }
            | WireCommand::BtStopScan {}
            | WireCommand::BtPair { .. }
            | WireCommand::BtConnect { .. }
            | WireCommand::BtDisconnect { .. }
            | WireCommand::BtForget { .. }
            | WireCommand::BtGive { .. }
            | WireCommand::BtTakeBack { .. }
            | WireCommand::BtKeys { .. }
    )
    .then_some(FEATURE_BLUETOOTH)
}

impl CommandStage<'_> {
    /// Checks one Bluetooth command; the bytes it will copy.
    pub(super) fn validate_bluetooth(command: &WireCommand<'_>) -> Result<usize, i32> {
        match *command {
            WireCommand::BtScan { lasting_ms: 0 } => Err(ERR_INVALID),
            WireCommand::BtKeys { on, .. } if on > 1 => Err(ERR_INVALID),
            // The id is checked as it is copied ([`Self::bluetooth_op`]).
            WireCommand::BtGive { plugin, .. } => Ok(plugin.len()),
            _ => Ok(0),
        }
    }

    /// The DEV item of a validated Bluetooth command.
    pub(super) fn bluetooth_op(command: &WireCommand<'_>) -> Result<DevOp, i32> {
        let op = match *command {
            WireCommand::BtScan { lasting_ms } => BtWire::Scan { lasting_ms },
            WireCommand::BtStopScan {} => BtWire::StopScan,
            WireCommand::BtPair { device } => BtWire::Pair(device),
            WireCommand::BtConnect { device } => BtWire::Connect(device),
            WireCommand::BtDisconnect { device } => BtWire::Disconnect(device),
            WireCommand::BtForget { device } => BtWire::Forget(device),
            WireCommand::BtGive { device, plugin } => BtWire::Give {
                device,
                plugin: ui::copy_id(plugin)?,
            },
            WireCommand::BtTakeBack { device } => BtWire::TakeBack(device),
            WireCommand::BtKeys { device, on } => BtWire::Keys {
                device,
                on: on != 0,
            },
            _ => return Err(ERR_INVALID),
        };
        Ok(DevOp::Bluetooth(op))
    }
}

impl crate::Kernel {
    /// `owner`'s accepted Bluetooth command as the kernel's: its answers come
    /// as [`EVENTS`].
    pub(super) fn apply_native_bluetooth(&mut self, owner: &Id, op: BtWire) {
        let events = || EVENTS.id().clone();
        let command = match op {
            BtWire::Scan { lasting_ms } => {
                let mut request = bluetooth::scan(&EVENTS)
                    .power(ScanPower::Balanced)
                    .active()
                    .lasting(Duration::from_millis(u64::from(lasting_ms)))
                    .command();
                request.set_from(owner.clone());
                return self.apply_admitted_bluetooth(request);
            }
            BtWire::Connect(device) => {
                let mut request = bluetooth::connect(DeviceId::new(device), &EVENTS).command();
                request.set_from(owner.clone());
                return self.apply_admitted_bluetooth(request);
            }
            BtWire::StopScan => BtCommand::StopScan { events: events() },
            BtWire::Pair(device) => BtCommand::Pair {
                device: DeviceId::new(device),
            },
            BtWire::Disconnect(device) => BtCommand::Disconnect {
                device: DeviceId::new(device),
            },
            BtWire::Forget(device) => BtCommand::Forget {
                device: DeviceId::new(device),
                events: events(),
            },
            BtWire::Give { device, plugin } => BtCommand::Give {
                device: DeviceId::new(device),
                plugin,
                events: events(),
            },
            BtWire::TakeBack(device) => {
                let device = DeviceId::new(device);
                let Some(plugin) = self.bluetooth_given(device) else {
                    let failure = BtFailure::new(BtOp::Give, BtError::NotFound).device(device);
                    let failed = BtEvent::Failed(failure).into_params();
                    return self
                        .output
                        .commands
                        .push(crate::Ask::Emit(events(), failed));
                };
                BtCommand::TakeBack {
                    device,
                    plugin,
                    events: events(),
                }
            }
            BtWire::Keys { device, on } => {
                let device = DeviceId::new(device);
                if on {
                    BtCommand::KeysOn {
                        device,
                        events: events(),
                    }
                } else {
                    BtCommand::KeysOff {
                        device,
                        events: events(),
                    }
                }
            }
        };
        let mut request = BtRequest::from(command);
        request.set_from(owner.clone());
        self.apply_admitted_bluetooth(request);
    }

    /// The plugin bonded `device` is given to, as `core.device` op 11 names it.
    fn bluetooth_given(&self, device: DeviceId) -> Option<Id> {
        let all = self.shared.bluetooth.as_ref()?;
        BluetoothView::new(all, &Anyone)
            .given(device)
            .first()
            .cloned()
    }
}

#[cfg(all(test, feature = "bluetooth"))]
#[path = "commands_dev_bluetooth_tests.rs"]
mod tests;
