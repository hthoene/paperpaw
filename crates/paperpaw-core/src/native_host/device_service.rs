//! Host side of the service `core.device` (`docs/abi/device.md` section 2): the
//! READ side of family DEV, ops 1-5, 7-12, 14 and 15.
//!
//! The backend is the generic [`Backend`](super::resources) function and holds
//! no state. Everything it needs comes from the frame's [`DeviceHost`]: the
//! device's facts as plain data, delivered through visitors so that nothing is
//! allocated for a reply and nothing is sized by plugin input. The backend owns
//! the contract: record order and field caps, the `end` record, paging like
//! `core.registry` (`cursor` / `generation`), row limits (8 values, 32
//! networks, 16 bonded devices, 32 plugins not installed, 32 keys), the checks
//! of every input and the rules of `device.md` section 2 that make a part
//! "not here" (`ERR_NOT_FOUND`, never a record of zeros).
//!
//! Not in this slice: op 6 `check` and op 13 `install` read the request tables
//! of the install commands, which do not exist yet; both answer
//! `ERR_UNSUPPORTED` explicitly (see `ops::backend`).
//!
//! Memory (X4): a call uses the 4,096-byte scratch the dispatcher lends and a
//! few hundred bytes of stack (the 32-entry rank table of op 9 is 256 bytes);
//! the cached memory probe is 32 bytes inside [`ProbeCache`]. Nothing is kept
//! per plugin.
//!
//! The X4 firmware does not offer this service in this slice; the desktop
//! emulator does ([`DEVICE_SERVICES`]).
use alloc::rc::Rc;
use core::ops::ControlFlow;
use paperpaw_native::host_glue::ERR_NOT_FOUND;

mod ops;
mod probe;
#[cfg(test)]
mod tests;

pub(crate) use ops::backend;
pub use probe::{PROBE_WINDOW_MS, ProbeCache};

/// The service key of this family.
pub const KEY: &str = "core.device";

/// What a host advertises to admission when it implements [`DeviceHost`]
/// (alongside, not inside, [`DATA_SERVICES`](super::data_services::DATA_SERVICES)
/// so that a shell that does not implement it, the X4, cannot offer it by
/// accident).
pub static DEVICE_SERVICES: &[paperpaw_native::metadata::Service<'static>] =
    &[paperpaw_native::metadata::Service {
        key: KEY,
        major: 1,
        semantic_revision: 0,
        min_table_bytes: 0,
    }];

/// Most values op 2 lists.
pub const MAX_VALUES: u32 = 8;
/// Most networks op 9 lists (the strongest).
pub const MAX_NETWORKS: u32 = 32;
/// Most bonded devices op 11 lists.
pub const MAX_BONDED: u32 = 16;
/// Most plugins op 14 lists.
pub const MAX_NOT_INSTALLED: u32 = 32;
/// Most keys op 15 lists.
pub const MAX_KEYS: u32 = 32;

/// `dev_device.caps` bits (`device.md` 2). Bits above `CAPS_MASK` are not
/// defined and never sent.
pub mod caps {
    /// A touch screen.
    pub const TOUCH: u64 = 1;
    /// The network can be reached.
    pub const NETWORK: u64 = 1 << 1;
    /// Wi-Fi managed by PaperPaw: ops 8 and 9 exist.
    pub const WIFI: u64 = 1 << 2;
    /// A Bluetooth radio the kernel uses: ops 10 and 11 exist.
    pub const BLUETOOTH: u64 = 1 << 3;
    /// Firmware updates: op 3 exists.
    pub const FIRMWARE: u64 = 1 << 4;
    /// Speech.
    pub const SPEECH: u64 = 1 << 5;
    /// A sleep screen.
    pub const SLEEP: u64 = 1 << 6;
    /// Screen rotation.
    pub const ROTATION: u64 = 1 << 7;
    /// Colours.
    pub const COLOURS: u64 = 1 << 8;
    /// A button row.
    pub const BUTTON_ROW: u64 = 1 << 9;
    /// A system clipboard.
    pub const SYSTEM_CLIPBOARD: u64 = 1 << 10;
    /// A front light.
    pub const FRONTLIGHT: u64 = 1 << 11;
    /// Black and white.
    pub const BLACK_AND_WHITE: u64 = 1 << 12;
    /// PaperPaw switches the device off.
    pub const POWER_OFF: u64 = 1 << 13;
    /// A pen.
    pub const PEN: u64 = 1 << 14;
    /// Every defined bit.
    pub const CAPS_MASK: u64 = (1 << 15) - 1;
}

/// `dev_device.controls` bits: bit 0 radio, 1 CPU clock, 2 buttons, 3
/// frontlight, 4 system saver.
pub const CONTROLS_MASK: u32 = 0x1f;

/// Op 1: what the device is. Strings are cut at their field's cap when sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceFacts<'a> {
    /// The stable device id (`xteink-x4`), at most 64 bytes.
    pub id: &'a str,
    /// Its name, at most 64 bytes.
    pub model: &'a str,
    /// The store's device name (`x4`), empty for none; at most 32 bytes.
    pub catalog: &'a str,
    /// The memory class: 0 small, 1 medium, 2 large (other values read as 0).
    pub class: u32,
    /// [`caps`] bits; undefined bits are dropped.
    pub caps: u64,
    /// The power controls, [`CONTROLS_MASK`] bits.
    pub controls: u32,
    /// How many TLS sessions may be open at once.
    pub tls_sessions: u32,
    /// How this start came about: 0 unknown, 1 power on, 2 woke, 3 restart, 4
    /// after a fault (other values read as 0).
    pub start: u32,
    /// How long the device slept before it woke last, where known.
    pub slept_ms: Option<u64>,
}

/// One reading of op 2. `value` is `None` for a reading that cannot be taken
/// now; a reading outside `i32` is left out too, never clamped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reading<'a> {
    /// `[a-z0-9-]{1,32}`; another name is left out.
    pub name: &'a str,
    /// The value, if it can be read now.
    pub value: Option<i64>,
}

/// Op 3: the firmware. States: 0 idle, 1 waiting for the user, 2 installing,
/// 3 installed (restart to use), 4 failed (other values read as 4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FirmwareFacts<'a> {
    /// The running version, at most 64 bytes.
    pub version: &'a str,
    /// The project name, at most 32 bytes.
    pub project: &'a str,
    /// The chip id.
    pub chip: u32,
    /// Room for an image.
    pub room: u64,
    /// The last start undid a new image.
    pub undone: bool,
    /// The install state.
    pub state: u32,
    /// Bytes written.
    pub done: u64,
    /// Bytes to write.
    pub total: u64,
    /// The outcome reason (`device.md` 5.5) of the last request, 0 while none ended.
    pub reason: u32,
    /// The request it belongs to.
    pub request: u32,
}

/// One row of op 4: a plugin built into the image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuiltinRow<'a> {
    /// Plugin id, at most 64 bytes.
    pub id: &'a str,
    /// Version, at most 32 bytes.
    pub version: &'a str,
}

/// Op 5: what a code plugin must be built for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeFacts<'a> {
    /// The interface major (7).
    pub abi_major: u32,
    /// The interface draft.
    pub abi_draft: u32,
    /// The chip id (5 ESP32-C3).
    pub target: u32,
    /// The code file (`code.ppn`), at most 32 bytes.
    pub code_file: &'a str,
    /// 0 none, 1 the last starts with code plugins failed, 2 the user held the
    /// key at power-on (other values read as 1).
    pub safe_mode: u32,
    /// The plugin that was running when a start failed, empty if not known.
    pub crashed: &'a str,
    /// The plugin loaded last, empty if not known.
    pub last_loaded: &'a str,
    /// Code plugins that take effect at the next start.
    pub pending: u32,
    /// How many rows op 14 has.
    pub not_installed: u32,
    /// Free code flash.
    pub room_free: u64,
    /// The largest free run of code flash.
    pub room_largest: u64,
}

/// Op 7: heap and stack, as the host probed them. A part the probe could not
/// measure is `None`, and its flag stays clear: not known is never "none free".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemoryFacts {
    /// Free heap bytes.
    pub heap_free: u32,
    /// Heap size.
    pub heap_size: u32,
    /// The largest block the heap could give.
    pub largest: Option<u32>,
    /// Stack never used, and the stack's size.
    pub stack: Option<(u32, u32)>,
    /// External memory (PSRAM) free and its size; `None` where there is none.
    pub external: Option<(u32, u32)>,
    /// This is the answer of an earlier probe inside the 250 ms window.
    pub cached: bool,
}

/// `dev_wifi.state`: 0 off, 1 on, 2 joining, 3 online, 4 hosting (other values
/// read as 0).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WifiFacts<'a> {
    /// The radio state.
    pub state: u32,
    /// The network joined or hosted, at most 32 bytes, empty for none.
    pub ssid: &'a str,
    /// The device's address, at most 45 bytes, empty for none.
    pub address: &'a str,
    /// The host name, at most 63 bytes, empty for none.
    pub host_name: &'a str,
    /// Guests of the hosted network.
    pub guests: u32,
    /// A scan runs.
    pub scanning: bool,
    /// The radio is wanted on.
    pub wanted: bool,
    /// The radio is busy.
    pub busy: bool,
}

/// One row of op 9. A network with an empty `ssid` is left out (no plugin can
/// join it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetworkRow<'a> {
    /// The network's name, at most 32 bytes.
    pub ssid: &'a str,
    /// Signal strength (dBm); stronger is greater.
    pub signal: i32,
    /// It needs a password.
    pub secured: bool,
    /// A password for it is stored.
    pub known: bool,
}

/// Op 10: the Bluetooth radio. `dev_bluetooth.flags` has no defined bit in
/// `device.md` yet, so it is sent as 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BluetoothFacts {
    /// 0 off, 1 on (other values read as 0).
    pub radio: u32,
    /// The most links kept at once.
    pub max_links: u32,
}

/// One row of op 11.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BondedRow<'a> {
    /// The device's number.
    pub device: u32,
    /// Its name, at most 64 bytes.
    pub name: &'a str,
    /// Its kind.
    pub kind: u32,
    /// A link is up.
    pub connected: bool,
    /// Its keys go to the key maps.
    pub remote_keys: bool,
    /// The plugin a sensor is handed to; sent only to the plugin bound to the
    /// host slot `core.bluetooth` ([`DeviceHost::bluetooth_manager`]).
    pub given: &'a str,
}

/// One row of op 14: a plugin this start could not install.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotInstalledRow<'a> {
    /// Plugin id, at most 64 bytes.
    pub plugin: &'a str,
    /// 1 other author, 2 damaged, 3 revoked (other values read as 2).
    pub reason: u32,
}

/// The device behind `core.device`. Object-safe; the shell implements it over
/// its real state, tests over fakes.
///
/// Every method defaults to `Err(ERR_NOT_FOUND)`: "this device has no such
/// part" is `ERR_NOT_FOUND`, never a record of zeros. A host that has a part
/// whose list is merely empty answers `Ok(())` without calling the visitor.
/// The host's `caps` (op 1) and the parts it answers must agree
/// ([`check_caps_agree`]).
///
/// Row visitors are called in the host's order and may be stopped
/// (`ControlFlow::Break`). A visitor is host-only: it never calls plugin code,
/// and the host must not call back into the service from inside one.
pub trait DeviceHost {
    /// Op 1. `ERR_NOT_FOUND` if the host cannot name the device.
    ///
    /// # Errors
    /// The host's status.
    fn device(&self, visit: &mut dyn FnMut(&DeviceFacts<'_>)) -> Result<(), i32> {
        let _ = visit;
        Err(ERR_NOT_FOUND)
    }
    /// Op 2: the named hardware values; at most [`MAX_VALUES`] are sent.
    /// `ERR_NOT_FOUND` is an empty list.
    ///
    /// # Errors
    /// The host's status.
    fn values(&self, visit: &mut dyn FnMut(&Reading<'_>) -> ControlFlow<()>) -> Result<(), i32> {
        let _ = visit;
        Err(ERR_NOT_FOUND)
    }
    /// Op 3. `ERR_NOT_FOUND` where PaperPaw is not the device's firmware.
    ///
    /// # Errors
    /// The host's status.
    fn firmware(&self, visit: &mut dyn FnMut(&FirmwareFacts<'_>)) -> Result<(), i32> {
        let _ = visit;
        Err(ERR_NOT_FOUND)
    }
    /// Op 4: the plugins built into the image.
    ///
    /// # Errors
    /// The host's status.
    fn builtin(
        &self,
        visit: &mut dyn FnMut(&BuiltinRow<'_>) -> ControlFlow<()>,
    ) -> Result<(), i32> {
        let _ = visit;
        Err(ERR_NOT_FOUND)
    }
    /// Op 5. `ERR_NOT_FOUND` where PaperPaw loads no code plugins.
    ///
    /// # Errors
    /// The host's status.
    fn native(&self, visit: &mut dyn FnMut(&NativeFacts<'_>)) -> Result<(), i32> {
        let _ = visit;
        Err(ERR_NOT_FOUND)
    }
    /// Op 7: a memory probe. The host keeps its own cache (one real probe per
    /// [`PROBE_WINDOW_MS`], see [`ProbeCache`]) and says in `cached` which it
    /// gave. `ERR_NOT_FOUND` where there is no probe or it cannot read now.
    ///
    /// # Errors
    /// The host's status.
    fn memory(&self) -> Result<MemoryFacts, i32> {
        Err(ERR_NOT_FOUND)
    }
    /// Op 8. `ERR_NOT_FOUND` where PaperPaw does not manage Wi-Fi.
    ///
    /// # Errors
    /// The host's status.
    fn wifi(&self, visit: &mut dyn FnMut(&WifiFacts<'_>)) -> Result<(), i32> {
        let _ = visit;
        Err(ERR_NOT_FOUND)
    }
    /// Op 9: the networks found, in any order (the service sorts them
    /// strongest first and sends at most [`MAX_NETWORKS`]).
    ///
    /// # Errors
    /// `ERR_NOT_FOUND` where PaperPaw does not manage Wi-Fi.
    fn networks(
        &self,
        visit: &mut dyn FnMut(&NetworkRow<'_>) -> ControlFlow<()>,
    ) -> Result<(), i32> {
        let _ = visit;
        Err(ERR_NOT_FOUND)
    }
    /// Op 10. `ERR_NOT_FOUND` where there is no Bluetooth radio.
    ///
    /// # Errors
    /// The host's status.
    fn bluetooth(&self) -> Result<BluetoothFacts, i32> {
        Err(ERR_NOT_FOUND)
    }
    /// Op 11: the bonded devices in the host's order (at most [`MAX_BONDED`] are
    /// sent).
    ///
    /// # Errors
    /// `ERR_NOT_FOUND` where there is no Bluetooth radio.
    fn bonded(&self, visit: &mut dyn FnMut(&BondedRow<'_>) -> ControlFlow<()>) -> Result<(), i32> {
        let _ = visit;
        Err(ERR_NOT_FOUND)
    }
    /// Whether `caller` (a host owner id) is the plugin bound to the host slot
    /// `core.bluetooth`; only it sees `given` in op 11. Defaults to nobody.
    fn bluetooth_manager(&self, caller: u64) -> bool {
        let _ = caller;
        false
    }
    /// Op 12: the power level and mode in use. The service computes `every_ms`
    /// with [`Power::every`](paperpaw_sdk::power::Power::every), the rule of interface 6.
    ///
    /// # Errors
    /// The host's status.
    fn power(&self, visit: &mut dyn FnMut(&paperpaw_sdk::power::Power)) -> Result<(), i32> {
        let _ = visit;
        Err(ERR_NOT_FOUND)
    }
    /// Op 14: the plugins this start could not install (at most
    /// [`MAX_NOT_INSTALLED`] are sent).
    ///
    /// # Errors
    /// The host's status.
    fn not_installed(
        &self,
        visit: &mut dyn FnMut(&NotInstalledRow<'_>) -> ControlFlow<()>,
    ) -> Result<(), i32> {
        let _ = visit;
        Err(ERR_NOT_FOUND)
    }
    /// Op 15: the names of the physical keys in the device's order (at most
    /// [`MAX_KEYS`] are sent). `ERR_NOT_FOUND` is an empty list: not known.
    ///
    /// # Errors
    /// The host's status.
    fn keys(&self, visit: &mut dyn FnMut(&str) -> ControlFlow<()>) -> Result<(), i32> {
        let _ = visit;
        Err(ERR_NOT_FOUND)
    }
    /// The generation of the host's lists (ops 4, 9, 11, 14): it changes when a
    /// list's set or order changes, as the registry's. 0 is never sent (the
    /// service reads 0 as 1); the host skips 0 when it wraps.
    fn generation(&self) -> u32 {
        1
    }
}

/// The shell's view of the device behind `core.device`, kept by
/// `HostServices::wire_device` for the backend (which reaches it through the
/// caller's `hosts`).
///
/// It holds an `Rc` and not a borrow, as `DocServices` does: the shell keeps
/// its host alive and shares it.
pub struct DeviceServices(pub Rc<dyn DeviceHost>);

impl core::fmt::Debug for DeviceServices {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DeviceServices").finish_non_exhaustive()
    }
}

/// Checks that a host's `caps` and the parts it answers agree (`device.md`
/// 2, "a host test"): Wi-Fi off means ops 8 and 9 are `ERR_NOT_FOUND` and on
/// means they are not; the same for Bluetooth (ops 10 and 11) and firmware
/// (op 3). For host tests of any shell.
///
/// # Errors
/// The first disagreement, in words.
pub fn check_caps_agree(host: &dyn DeviceHost) -> Result<(), &'static str> {
    let mut caps_bits = None;
    host.device(&mut |facts| caps_bits = Some(facts.caps & caps::CAPS_MASK))
        .map_err(|_| "op 1 is not answered")?;
    let bits = caps_bits.ok_or("op 1 gives no device")?;
    let has = |bit: u64| bits & bit != 0;
    let missing = |status: Result<(), i32>| status == Err(ERR_NOT_FOUND);
    let none = &mut |_: &NetworkRow<'_>| ControlFlow::Break(());
    let none_bonded = &mut |_: &BondedRow<'_>| ControlFlow::Break(());
    let wifi_here = !missing(host.wifi(&mut |_| {})) && !missing(host.networks(none));
    let wifi_gone = missing(host.wifi(&mut |_| {})) && missing(host.networks(none));
    if has(caps::WIFI) && !wifi_here || !has(caps::WIFI) && !wifi_gone {
        return Err("caps bit 2 (Wi-Fi) and ops 8, 9 disagree");
    }
    let bt_here = host.bluetooth().is_ok() && !missing(host.bonded(none_bonded));
    let bt_gone = host.bluetooth() == Err(ERR_NOT_FOUND) && missing(host.bonded(none_bonded));
    if has(caps::BLUETOOTH) && !bt_here || !has(caps::BLUETOOTH) && !bt_gone {
        return Err("caps bit 3 (Bluetooth) and ops 10, 11 disagree");
    }
    let firmware = host.firmware(&mut |_| {});
    if has(caps::FIRMWARE) == missing(firmware) {
        return Err("caps bit 4 (firmware) and op 3 disagree");
    }
    Ok(())
}
