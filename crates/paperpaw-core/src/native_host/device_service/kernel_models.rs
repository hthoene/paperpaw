//! The parts of `core.device` the kernel answers from its own models, for
//! every shell: Wi-Fi (op 8), Bluetooth (op 10), the bonded devices (op 11)
//! and the power level and mode (op 12).
//!
//! The kernel is the one that knows them (`Env::wifi`, `Env::bluetooth`,
//! `Env::power`, built afresh for each call), so a plugin that reads after
//! another one changed them in the same delivery pass sees the new state, and
//! a shell does not keep a second copy in step. [`KernelHost`] wraps the
//! shell's [`DeviceHost`] and answers those ops from the frame's `Env`; every
//! other op, and the network list (op 9: the password flag is the shell's
//! secrets), stays the shell's.
//!
//! Where the frame has no kernel `Env` (a fixture) the shell's host answers
//! as it is. Where it has one and the kernel has no model (Wi-Fi the system
//! manages, no Bluetooth radio) the op is `ERR_NOT_FOUND`: "this device has no
//! such part", also for ops 9 and 11, which go with their radio.
//!
//! Op 11 lists what the plugin that asks may use: the bonded devices given to
//! it, and all of them to the Bluetooth manager, which alone sees `given`
//! (`BluetoothView`, the interface-6 rule).
use super::{
    BluetoothFacts, BondedRow, DeviceFacts, DeviceHost, FirmwareFacts, NativeFacts, NetworkRow,
    NotInstalledRow, WifiFacts,
};
use super::{BuiltinRow, MemoryFacts, Reading};
use core::ops::ControlFlow;
use paperpaw_native::host_glue::ERR_NOT_FOUND;
use paperpaw_sdk::Env;
use paperpaw_sdk::bluetooth::{DeviceKind, RadioState};
use paperpaw_sdk::wifi::WifiState;

/// The shell's host with the kernel's own answers over it.
pub(super) struct KernelHost<'a> {
    pub(super) inner: &'a dyn DeviceHost,
    pub(super) env: &'a Env<'a>,
}

/// `dev_bonded.kind`: 0 other, 1 keyboard, 2 remote, 3 sensor, 4 phone.
fn kind_code(kind: DeviceKind) -> u32 {
    match kind {
        DeviceKind::Keyboard => 1,
        DeviceKind::Remote => 2,
        DeviceKind::Sensor => 3,
        DeviceKind::Phone => 4,
        _ => 0,
    }
}

impl DeviceHost for KernelHost<'_> {
    fn device(&self, visit: &mut dyn FnMut(&DeviceFacts<'_>)) -> Result<(), i32> {
        self.inner.device(visit)
    }
    fn values(&self, visit: &mut dyn FnMut(&Reading<'_>) -> ControlFlow<()>) -> Result<(), i32> {
        self.inner.values(visit)
    }
    fn firmware(&self, visit: &mut dyn FnMut(&FirmwareFacts<'_>)) -> Result<(), i32> {
        self.inner.firmware(visit)
    }
    fn builtin(
        &self,
        visit: &mut dyn FnMut(&BuiltinRow<'_>) -> ControlFlow<()>,
    ) -> Result<(), i32> {
        self.inner.builtin(visit)
    }
    fn native(&self, visit: &mut dyn FnMut(&NativeFacts<'_>)) -> Result<(), i32> {
        self.inner.native(visit)
    }
    fn memory(&self) -> Result<MemoryFacts, i32> {
        self.inner.memory()
    }

    /// Op 8 from the kernel's Wi-Fi.
    fn wifi(&self, visit: &mut dyn FnMut(&WifiFacts<'_>)) -> Result<(), i32> {
        let wifi = self.env.wifi().ok_or(ERR_NOT_FOUND)?;
        let (state, ssid, address, host_name, guests) = match &wifi.state {
            WifiState::On => (1, "", "", "", 0),
            WifiState::Joining { ssid } => (2, ssid.as_str(), "", "", 0),
            WifiState::Online {
                ssid,
                address,
                name,
            } => (
                3,
                ssid.as_str(),
                address.as_str(),
                name.as_deref().unwrap_or(""),
                0,
            ),
            WifiState::Hosting {
                ssid,
                address,
                guests,
            } => (4, ssid.as_str(), address.as_str(), "", u32::from(*guests)),
            // Off, and a state a later interface adds: not on.
            _ => (0, "", "", "", 0),
        };
        visit(&WifiFacts {
            state,
            ssid,
            address,
            host_name,
            guests,
            scanning: wifi.scanning,
            wanted: wifi.wanted,
            busy: wifi.busy,
        });
        Ok(())
    }
    /// Op 9 is the shell's (which networks have a stored password is its
    /// secrets'), where the kernel manages Wi-Fi at all.
    fn networks(
        &self,
        visit: &mut dyn FnMut(&NetworkRow<'_>) -> ControlFlow<()>,
    ) -> Result<(), i32> {
        self.env.wifi().ok_or(ERR_NOT_FOUND)?;
        self.inner.networks(visit)
    }

    /// Op 10 from the kernel's Bluetooth.
    fn bluetooth(&self) -> Result<BluetoothFacts, i32> {
        let view = self.env.bluetooth().ok_or(ERR_NOT_FOUND)?;
        Ok(BluetoothFacts {
            radio: u32::from(view.state() == RadioState::On),
            max_links: u32::from(view.max_links()),
        })
    }
    /// Op 11 from the kernel's bonded devices, as the plugin that asks may use
    /// them.
    fn bonded(&self, visit: &mut dyn FnMut(&BondedRow<'_>) -> ControlFlow<()>) -> Result<(), i32> {
        let view = self.env.bluetooth().ok_or(ERR_NOT_FOUND)?;
        for bonded in view.bonded() {
            let given = view
                .given(bonded.id)
                .first()
                .map_or("", paperpaw_sdk::Id::as_str);
            let row = BondedRow {
                device: bonded.id.number(),
                name: &bonded.name,
                kind: kind_code(bonded.kind),
                connected: bonded.connected,
                remote_keys: bonded.keys,
                given,
            };
            if visit(&row).is_break() {
                break;
            }
        }
        Ok(())
    }
    /// `given` is empty for every plugin but the manager already.
    fn bluetooth_manager(&self, _caller: u64) -> bool {
        true
    }

    /// Op 12 from the kernel's power level and mode.
    fn power(&self, visit: &mut dyn FnMut(&paperpaw_sdk::power::Power)) -> Result<(), i32> {
        visit(self.env.power());
        Ok(())
    }

    fn not_installed(
        &self,
        visit: &mut dyn FnMut(&NotInstalledRow<'_>) -> ControlFlow<()>,
    ) -> Result<(), i32> {
        self.inner.not_installed(visit)
    }
    fn keys(&self, visit: &mut dyn FnMut(&str) -> ControlFlow<()>) -> Result<(), i32> {
        self.inner.keys(visit)
    }
    /// The shell's generation, changed by the bonded devices the plugin that
    /// asks may use (the kernel changes them, the shell is not told): a page
    /// read across a change is `ERR_STATE`, as with the shell's own lists.
    fn generation(&self) -> u32 {
        let mut hash: u32 = 0x811c_9dc5;
        if let Some(view) = self.env.bluetooth() {
            for bonded in view.bonded() {
                for byte in bonded.id.number().to_le_bytes() {
                    hash = (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193);
                }
            }
        }
        self.inner.generation().wrapping_add(hash)
    }
}
