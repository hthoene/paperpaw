//! The X4's `core.device` host (`docs/abi/device.md` section 2): what the
//! firmware is, read by plugins through [`DeviceHost`].
//!
//! The firmware says what it knows once ([`Identity`], from its constants)
//! and keeps what changes in cells ([`X4Device::set_start`],
//! [`X4Device::set_value`]) where it already learns of the change: nothing is
//! polled, and a call allocates nothing (strings are borrowed from the
//! identity and the list the image carries, rows are visited one at a time).
//!
//! **Setters belong to the main loop.** Every `set_*` is called from the
//! firmware's main loop only: never from inside a visitor of this host (the
//! lists are borrowed while a visitor runs, and a setter that finds one in use
//! changes nothing and says so), never from an interrupt or another thread
//! (the cells are not `Sync`). A reader that runs between two setters sees one
//! state or the other, never a mix, because every setter replaces one whole
//! value.
//!
//! The device is made once at start with [`X4Device::shared`] and kept on the
//! heap behind an `Rc` (the kernel's `DeviceServices` holds that `Rc`).
//!
//! This part answers ops 1 (the device), 2 (named values), 3 (the running
//! firmware), 4 (the plugins built into the image), 5 (what a code plugin must
//! be built for), 7 (memory), 8 and 9 (the Wi-Fi radio and the networks
//! found), 12 (the power level and mode), 14 (the plugins a start could not
//! install) and 15 (the physical keys); every other part is
//! `ERR_NOT_FOUND` (the default of [`DeviceHost`]) until its slice lands, and
//! [`Identity::caps`] carries only the bits of the parts that are answered
//! (`check_caps_agree` is the test of it).

use alloc::rc::Rc;
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};
use core::ops::ControlFlow;

use paperpaw_core::native_host::device_service::{
    BuiltinRow, DeviceFacts, DeviceHost, FirmwareFacts, MAX_NETWORKS, MAX_NOT_INSTALLED,
    MAX_VALUES, MemoryFacts, NativeFacts, NetworkRow, NotInstalledRow, ProbeCache, Reading,
    WifiFacts,
};
use paperpaw_native::host_glue::ERR_NOT_FOUND;
use paperpaw_platform::firmware::{PROJECT_MAX, Running, Text, VERSION_MAX};
use paperpaw_sdk::firmware::Plugins;
use paperpaw_sdk::native::{DRAFT, INTERFACE};
use paperpaw_sdk::power::Power;

/// What the firmware is, from its constants.
#[derive(Clone, Copy, Debug)]
pub struct Identity {
    /// The stable device id (`xteink-x4`).
    pub id: &'static str,
    /// Its name.
    pub model: &'static str,
    /// The store's device name (`x4`), empty for none.
    pub catalog: &'static str,
    /// The memory class: 0 small, 1 medium, 2 large.
    pub class: u32,
    /// `dev_device.caps`: only the bits of the parts this host answers.
    pub caps: u64,
    /// The power controls, as the kernel is told them.
    pub controls: u32,
    /// How many TLS sessions may be open at once.
    pub tls_sessions: u32,
    /// The names of the physical keys, in the device's order.
    pub keys: &'static [&'static str],
    /// The names of the readings op 2 lists (at most [`MAX_VALUES`] are used).
    pub values: &'static [&'static str],
}

/// One named reading, empty until the firmware has one.
#[derive(Debug)]
struct Slot {
    name: &'static str,
    value: Cell<Option<i64>>,
}

/// The running firmware, kept in place (op 3's `running` part: the install
/// state, progress, reason and request are the core's).
#[derive(Clone, Copy, Debug)]
struct Own {
    version: Text<VERSION_MAX>,
    project: Text<PROJECT_MAX>,
    chip: u16,
    room: u64,
    undone: bool,
}

/// What a code plugin must be built for and how code plugins stand this
/// start (op 5), from the loader.
#[derive(Clone, Copy, Debug)]
pub struct NativeState {
    /// The chip id a code plugin is built for (5 ESP32-C3, 9 ESP32-S3).
    pub target: u32,
    /// The code file (`code.ppn`).
    pub code_file: &'static str,
    /// 0 none, 1 the last starts with code plugins failed, 2 the user held the
    /// key at power-on.
    pub safe_mode: u32,
    /// The plugin that was running when a start failed, empty if not known.
    pub crashed: Text<64>,
    /// The plugin loaded last, empty if not known.
    pub last_loaded: Text<64>,
    /// Code plugins that take effect at the next start.
    pub pending: u32,
    /// Free code flash.
    pub room_free: u64,
    /// The largest free run of code flash.
    pub room_largest: u64,
}

/// The Wi-Fi radio as the firmware last saw it (op 8): 0 off, 1 on, 2
/// joining, 3 online, 4 hosting.
#[derive(Clone, Copy, Debug)]
pub struct WifiState {
    pub state: u32,
    /// The network joined or hosted, empty for none.
    pub ssid: Text<32>,
    /// The device's address, empty for none.
    pub address: Text<45>,
    /// The host name, empty for none.
    pub host_name: Text<63>,
    /// Guests of the hosted network.
    pub guests: u32,
    pub scanning: bool,
    pub wanted: bool,
    pub busy: bool,
}

/// One network a scan found.
#[derive(Clone, Copy, Debug)]
struct Found {
    ssid: Text<32>,
    signal: i32,
    secured: bool,
}

/// The longest plugin id (and what is kept of a longer one).
const ID_MAX: usize = 64;

/// What reads the heap and the stack (nothing to say: `None`).
pub type MemoryProbe = fn() -> Option<MemoryFacts>;

/// The X4's device, behind `core.device`.
#[derive(Debug)]
pub struct X4Device {
    identity: Identity,
    /// How this start came about (`dev_device.start`).
    start: Cell<u32>,
    /// How long the device slept before it woke, where known.
    slept_ms: Cell<Option<u64>>,
    /// The plugin crates the image carries; `None` for an image without a list.
    built_in: Option<Plugins>,
    slots: [Option<Slot>; MAX_VALUES as usize],
    own: Cell<Option<Own>>,
    native: Cell<Option<NativeState>>,
    /// What reads the heap and the stack, and the clock for its cache.
    probe: Cell<Option<MemoryProbe>>,
    clock_ms: Cell<Option<fn() -> u64>>,
    memory: ProbeCache<MemoryFacts>,
    not_installed: RefCell<Vec<(Text<ID_MAX>, u32)>>,
    wifi: Cell<Option<WifiState>>,
    /// The networks the last scan found, strongest first.
    networks: RefCell<Vec<Found>>,
    power: RefCell<Power>,
    /// Changes when a list changes (never 0).
    generation: Cell<u32>,
}

impl X4Device {
    /// The device for `identity`; `list` is the image's own record of the
    /// plugin crates built in (`paperpaw_update::Plugins`, the firmware's
    /// `PLUGINS`).
    #[must_use]
    pub fn new(identity: Identity, list: &'static [u8]) -> Self {
        let mut slots = [const { None }; MAX_VALUES as usize];
        for (slot, name) in slots.iter_mut().zip(identity.values) {
            *slot = Some(Slot {
                name,
                value: Cell::new(None),
            });
        }
        Self {
            identity,
            start: Cell::new(0),
            slept_ms: Cell::new(None),
            built_in: Plugins::read_static(list),
            slots,
            own: Cell::new(None),
            native: Cell::new(None),
            probe: Cell::new(None),
            clock_ms: Cell::new(None),
            memory: ProbeCache::new(),
            not_installed: RefCell::new(Vec::new()),
            wifi: Cell::new(None),
            networks: RefCell::new(Vec::new()),
            power: RefCell::new(Power::default()),
            generation: Cell::new(1),
        }
    }

    /// [`X4Device::new`] on the heap behind an `Rc`, where the firmware keeps
    /// it (the one frame that holds the whole struct is this one's, once, at
    /// start).
    #[inline(never)]
    #[must_use]
    pub fn shared(identity: Identity, list: &'static [u8]) -> Rc<Self> {
        Rc::new(Self::new(identity, list))
    }

    /// The running firmware (op 3), as the updates know it; `Identity::caps`
    /// has the firmware bit exactly where the firmware calls this.
    pub fn set_firmware(&self, running: &Running<'_>) {
        self.own.set(Some(Own {
            version: Text::new(running.version),
            project: Text::new(running.project),
            chip: running.chip,
            room: running.room,
            undone: running.undone,
        }));
    }

    /// What code plugins stand on this start (op 5).
    pub fn set_native(&self, state: NativeState) {
        self.native.set(Some(state));
    }

    /// What reads the memory (op 7) and the clock its answers are cached by
    /// (one real probe per `PROBE_WINDOW_MS`).
    pub fn set_memory_probe(&self, probe: MemoryProbe, clock_ms: fn() -> u64) {
        self.probe.set(Some(probe));
        self.clock_ms.set(Some(clock_ms));
    }

    /// The Wi-Fi radio (ops 8 and 9): `Identity::caps` has the Wi-Fi bit
    /// exactly where the firmware calls this (its radio is PaperPaw's).
    pub fn set_wifi(&self, state: WifiState) {
        self.wifi.set(Some(state));
    }

    /// The networks the last scan found as `(ssid, signal in dBm, secured)`:
    /// the strongest [`MAX_NETWORKS`] are kept. False where they could not be
    /// kept (no room): the list is then as it was.
    pub fn set_networks(&self, rows: &[(&str, i32, bool)]) -> bool {
        let mut kept = Vec::new();
        if kept.try_reserve_exact(rows.len()).is_err() {
            return false;
        }
        kept.extend(rows.iter().map(|&(ssid, signal, secured)| Found {
            ssid: Text::new(ssid),
            signal,
            secured,
        }));
        kept.sort_unstable_by_key(|row| core::cmp::Reverse(row.signal));
        kept.truncate(MAX_NETWORKS as usize);
        // A list in use by a visitor is left as it is: never a panic.
        let Ok(mut list) = self.networks.try_borrow_mut() else {
            return false;
        };
        *list = kept;
        self.bump();
        true
    }

    /// The power level and mode the kernel reports now (op 12). False where a
    /// visitor is reading it (it is then as it was): never a panic.
    pub fn set_power(&self, power: Power) -> bool {
        let Ok(mut held) = self.power.try_borrow_mut() else {
            return false;
        };
        *held = power;
        true
    }

    fn bump(&self) {
        self.generation
            .set(self.generation.get().checked_add(1).unwrap_or(1));
    }

    /// The plugins this start could not install (op 14): `(id, reason)`, with
    /// reason 1 other author, 2 damaged, 3 revoked. At most
    /// [`MAX_NOT_INSTALLED`] are kept. False where the rows could not be kept
    /// (no room): the list is then as it was.
    pub fn set_not_installed(&self, rows: &[(&str, u32)]) -> bool {
        let rows = &rows[..rows.len().min(MAX_NOT_INSTALLED as usize)];
        let mut kept = Vec::new();
        if kept.try_reserve_exact(rows.len()).is_err() {
            return false;
        }
        // Each id is kept in place, cut at 64 bytes (a plugin id's cap).
        kept.extend(rows.iter().map(|&(id, reason)| (Text::new(id), reason)));
        // A list in use by a visitor is left as it is: never a panic.
        let Ok(mut list) = self.not_installed.try_borrow_mut() else {
            return false;
        };
        *list = kept;
        self.bump();
        true
    }

    /// How this start came about: 0 unknown, 1 power on, 2 woke, 3 restart,
    /// 4 after a fault.
    pub fn set_start(&self, start: u32) {
        self.start.set(start);
    }

    /// How long the device slept before it woke.
    pub fn set_slept_ms(&self, slept_ms: Option<u64>) {
        self.slept_ms.set(slept_ms);
    }

    /// The reading `name` (one of [`Identity::values`]), `None` where it
    /// cannot be read now. False where the identity names no such reading.
    pub fn set_value(&self, name: &str, value: Option<i64>) -> bool {
        let found = self.slots.iter().flatten().find(|slot| slot.name == name);
        found.is_some_and(|slot| {
            slot.value.set(value);
            true
        })
    }
}

impl DeviceHost for X4Device {
    fn device(&self, visit: &mut dyn FnMut(&DeviceFacts<'_>)) -> Result<(), i32> {
        let this = &self.identity;
        visit(&DeviceFacts {
            id: this.id,
            model: this.model,
            catalog: this.catalog,
            class: this.class,
            caps: this.caps,
            controls: this.controls,
            tls_sessions: this.tls_sessions,
            start: self.start.get(),
            slept_ms: self.slept_ms.get(),
        });
        Ok(())
    }

    fn values(&self, visit: &mut dyn FnMut(&Reading<'_>) -> ControlFlow<()>) -> Result<(), i32> {
        for slot in self.slots.iter().flatten() {
            let reading = Reading {
                name: slot.name,
                value: slot.value.get(),
            };
            if visit(&reading).is_break() {
                break;
            }
        }
        Ok(())
    }

    /// The crates the image lists whose name is a plugin id
    /// (`paperpaw-plugin-<id>`); any other crate is not a plugin and is left
    /// out, as an update's check leaves it out ([`PluginCrate::plugin_id`]).
    ///
    /// [`PluginCrate::plugin_id`]: paperpaw_sdk::firmware::PluginCrate::plugin_id
    fn builtin(
        &self,
        visit: &mut dyn FnMut(&BuiltinRow<'_>) -> ControlFlow<()>,
    ) -> Result<(), i32> {
        let Some(list) = &self.built_in else {
            return Ok(());
        };
        for krate in list.iter() {
            let Some(id) = krate.plugin_id() else {
                continue;
            };
            let row = BuiltinRow {
                id,
                version: krate.version,
            };
            if visit(&row).is_break() {
                break;
            }
        }
        Ok(())
    }

    fn firmware(&self, visit: &mut dyn FnMut(&FirmwareFacts<'_>)) -> Result<(), i32> {
        let own = self.own.get().ok_or(ERR_NOT_FOUND)?;
        // The state, progress, reason and request are the core's: idle here.
        visit(&FirmwareFacts {
            version: own.version.as_str(),
            project: own.project.as_str(),
            chip: u32::from(own.chip),
            room: own.room,
            undone: own.undone,
            state: 0,
            done: 0,
            total: 0,
            reason: 0,
            request: 0,
        });
        Ok(())
    }

    fn native(&self, visit: &mut dyn FnMut(&NativeFacts<'_>)) -> Result<(), i32> {
        let state = self.native.get().ok_or(ERR_NOT_FOUND)?;
        let not_installed = u32::try_from(self.not_installed.borrow().len()).unwrap_or(u32::MAX);
        visit(&NativeFacts {
            abi_major: u32::from(INTERFACE),
            abi_draft: u32::from(DRAFT),
            target: state.target,
            code_file: state.code_file,
            safe_mode: state.safe_mode,
            crashed: state.crashed.as_str(),
            last_loaded: state.last_loaded.as_str(),
            pending: state.pending,
            not_installed,
            room_free: state.room_free,
            room_largest: state.room_largest,
        });
        Ok(())
    }

    fn memory(&self) -> Result<MemoryFacts, i32> {
        let (Some(probe), Some(clock)) = (self.probe.get(), self.clock_ms.get()) else {
            return Err(ERR_NOT_FOUND);
        };
        let (facts, cached) = self.memory.get(clock(), probe);
        facts
            .map(|facts| MemoryFacts { cached, ..facts })
            .ok_or(ERR_NOT_FOUND)
    }

    fn not_installed(
        &self,
        visit: &mut dyn FnMut(&NotInstalledRow<'_>) -> ControlFlow<()>,
    ) -> Result<(), i32> {
        for (plugin, reason) in self.not_installed.borrow().iter() {
            let row = NotInstalledRow {
                plugin: plugin.as_str(),
                reason: *reason,
            };
            if visit(&row).is_break() {
                break;
            }
        }
        Ok(())
    }

    fn wifi(&self, visit: &mut dyn FnMut(&WifiFacts<'_>)) -> Result<(), i32> {
        let radio = self.wifi.get().ok_or(ERR_NOT_FOUND)?;
        visit(&WifiFacts {
            state: radio.state,
            ssid: radio.ssid.as_str(),
            address: radio.address.as_str(),
            host_name: radio.host_name.as_str(),
            guests: radio.guests,
            scanning: radio.scanning,
            wanted: radio.wanted,
            busy: radio.busy,
        });
        Ok(())
    }

    /// The networks of the last scan, strongest first. No password is looked
    /// for: `known` is false until the secrets service is wired on the X4,
    /// then a presence check per call (never a decrypt, never a copy).
    fn networks(
        &self,
        visit: &mut dyn FnMut(&NetworkRow<'_>) -> ControlFlow<()>,
    ) -> Result<(), i32> {
        self.wifi.get().ok_or(ERR_NOT_FOUND)?;
        for found in self.networks.borrow().iter() {
            let row = NetworkRow {
                ssid: found.ssid.as_str(),
                signal: found.signal,
                secured: found.secured,
                known: false,
            };
            if visit(&row).is_break() {
                break;
            }
        }
        Ok(())
    }

    fn power(&self, visit: &mut dyn FnMut(&Power)) -> Result<(), i32> {
        visit(&self.power.borrow());
        Ok(())
    }

    fn generation(&self) -> u32 {
        self.generation.get()
    }

    fn keys(&self, visit: &mut dyn FnMut(&str) -> ControlFlow<()>) -> Result<(), i32> {
        for name in self.identity.keys {
            if visit(name).is_break() {
                break;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use std::string::{String, ToString};
    use std::vec::Vec;

    use paperpaw_core::native_host::device_service::{caps, check_caps_agree};
    use paperpaw_platform::firmware::Running;
    use paperpaw_sdk::firmware::PluginCrate;

    use super::*;

    const KEYS: &[&str] = &["back", "confirm", "left", "right"];
    const VALUES: &[&str] = &["usb-pin-high", "usb-frames"];

    fn list(crates: &[(&'static str, &'static str)]) -> &'static [u8] {
        let record = Plugins::record(
            crates
                .iter()
                .map(|&(name, version)| PluginCrate { name, version }),
        )
        .unwrap();
        std::boxed::Box::leak(record.into_boxed_slice())
    }

    fn identity() -> Identity {
        Identity {
            id: "xteink-x4",
            model: "Xteink X4",
            catalog: "x4",
            class: 0,
            caps: caps::NETWORK | caps::SLEEP | caps::ROTATION | caps::BUTTON_ROW,
            controls: 0b111,
            tls_sessions: 1,
            keys: KEYS,
            values: VALUES,
        }
    }

    fn device(crates: &[(&'static str, &'static str)]) -> X4Device {
        X4Device::new(identity(), list(crates))
    }

    fn built_in(device: &X4Device) -> Vec<(String, String)> {
        let mut rows = Vec::new();
        device
            .builtin(&mut |row| {
                rows.push((row.id.to_string(), row.version.to_string()));
                ControlFlow::Continue(())
            })
            .unwrap();
        rows
    }

    #[test]
    fn the_device_says_what_the_firmware_is() {
        let device = device(&[]);
        device.set_start(2);
        device.set_slept_ms(Some(9_000));
        let mut seen = None;
        device
            .device(&mut |facts| {
                seen = Some((
                    facts.id.to_string(),
                    facts.catalog.to_string(),
                    facts.class,
                    facts.caps,
                    facts.controls,
                    facts.tls_sessions,
                    facts.start,
                    facts.slept_ms,
                ));
            })
            .unwrap();
        let (id, catalog, class, caps_seen, controls, tls, start, slept) = seen.unwrap();
        assert_eq!(
            (id.as_str(), catalog.as_str(), class),
            ("xteink-x4", "x4", 0)
        );
        assert_eq!(caps_seen, identity().caps);
        assert_eq!((controls, tls, start, slept), (0b111, 1, 2, Some(9_000)));
    }

    /// The caps name exactly the parts that are answered: Wi-Fi, Bluetooth
    /// and firmware are not here yet.
    #[test]
    fn caps_and_the_answered_parts_agree() {
        assert_eq!(check_caps_agree(&device(&[])), Ok(()));
        let device = device(&[]);
        assert_eq!(device.wifi(&mut |_| {}), Err(ERR_NOT_FOUND));
        assert_eq!(device.bluetooth(), Err(ERR_NOT_FOUND));
        assert_eq!(device.firmware(&mut |_| {}), Err(ERR_NOT_FOUND));
    }

    /// The ids are the crates' plugin ids, in the image's order, so the list
    /// compares equal with an update's check; crates that are no plugin are
    /// not rows.
    #[test]
    fn the_built_in_list_is_the_images_plugin_ids() {
        let device = device(&[
            ("paperpaw-plugin-stats", "1.0.8"),
            ("paperpaw-update", "0.6.0"),
            ("paperpaw-plugin-hyphenation-de", "0.2.0"),
            ("paperpaw-plugin-Bad", "9"),
            ("paperpaw-plugin-", "9"),
        ]);
        assert_eq!(
            built_in(&device),
            [
                ("stats".to_string(), "1.0.8".to_string()),
                ("hyphenation-de".to_string(), "0.2.0".to_string()),
            ]
        );
    }

    #[test]
    fn a_visitor_that_stops_is_not_called_again() {
        let device = device(&[
            ("paperpaw-plugin-a", "1"),
            ("paperpaw-plugin-b", "1"),
            ("paperpaw-plugin-c", "1"),
        ]);
        let mut calls = 0;
        device
            .builtin(&mut |_| {
                calls += 1;
                ControlFlow::Break(())
            })
            .unwrap();
        assert_eq!(calls, 1);
        let mut keys = 0;
        device
            .keys(&mut |_| {
                keys += 1;
                ControlFlow::Break(())
            })
            .unwrap();
        assert_eq!(keys, 1);
    }

    /// An image without a list (or one that cannot be read) has no rows, and
    /// still is a host with the part: an empty list, not "not here".
    #[test]
    fn an_image_without_a_list_has_an_empty_one() {
        let device = X4Device::new(identity(), b"not a list");
        assert_eq!(built_in(&device), []);
    }

    #[test]
    fn the_keys_are_in_the_devices_order() {
        let device = device(&[]);
        let mut names = Vec::new();
        device
            .keys(&mut |name| {
                names.push(name.to_string());
                ControlFlow::Continue(())
            })
            .unwrap();
        assert_eq!(names, KEYS);
    }

    /// A reading the firmware has not got is `None`, never a zero; a name
    /// the identity does not list cannot be set.
    #[test]
    fn values_are_what_the_firmware_last_read() {
        let device = device(&[]);
        let read = |device: &X4Device| {
            let mut rows = Vec::new();
            device
                .values(&mut |reading| {
                    rows.push((reading.name.to_string(), reading.value));
                    ControlFlow::Continue(())
                })
                .unwrap();
            rows
        };
        assert_eq!(
            read(&device),
            [
                ("usb-pin-high".to_string(), None),
                ("usb-frames".to_string(), None)
            ]
        );
        assert!(device.set_value("usb-frames", Some(7)));
        assert!(!device.set_value("temperature-c", Some(20)));
        assert_eq!(read(&device)[1], ("usb-frames".to_string(), Some(7)));
        assert!(device.set_value("usb-frames", None));
        assert_eq!(read(&device)[1].1, None);
    }

    /// More names than op 2 lists are not kept.
    #[test]
    fn no_more_values_than_op_2_lists() {
        const MANY: &[&str] = &["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"];
        let device = X4Device::new(
            Identity {
                values: MANY,
                ..identity()
            },
            list(&[]),
        );
        let mut count = 0;
        device
            .values(&mut |_| {
                count += 1;
                ControlFlow::Continue(())
            })
            .unwrap();
        assert_eq!(count, MAX_VALUES);
    }

    fn native_state() -> NativeState {
        NativeState {
            target: 5,
            code_file: "code.ppn",
            safe_mode: 1,
            crashed: Text::new("stats"),
            last_loaded: Text::new("widgets"),
            pending: 2,
            room_free: 100_000,
            room_largest: 60_000,
        }
    }

    /// Op 3 gives only the running part: the state is idle for the core to
    /// fill; the part is not here until the firmware says what it is.
    #[test]
    fn the_firmware_is_the_running_part_and_idle() {
        let device = device(&[]);
        assert_eq!(device.firmware(&mut |_| {}), Err(ERR_NOT_FOUND));
        device.set_firmware(&Running::new(
            "0.6.0",
            "paperpaw-xteink",
            5,
            6_000_000,
            true,
        ));
        let mut seen = None;
        device
            .firmware(&mut |facts| {
                seen = Some((
                    facts.version.to_string(),
                    facts.project.to_string(),
                    facts.chip,
                    facts.room,
                    facts.undone,
                    (
                        facts.state,
                        facts.done,
                        facts.total,
                        facts.reason,
                        facts.request,
                    ),
                ));
            })
            .unwrap();
        let (version, project, chip, room, undone, rest) = seen.unwrap();
        assert_eq!(
            (version.as_str(), project.as_str()),
            ("0.6.0", "paperpaw-xteink")
        );
        assert_eq!(
            (chip, room, undone, rest),
            (5, 6_000_000, true, (0, 0, 0, 0, 0))
        );
    }

    /// With the firmware set and its bit in caps, the answers agree.
    #[test]
    fn the_firmware_bit_and_op_3_agree() {
        let device = X4Device::new(
            Identity {
                caps: identity().caps | caps::FIRMWARE,
                ..identity()
            },
            list(&[]),
        );
        assert!(check_caps_agree(&device).is_err(), "op 3 not set yet");
        device.set_firmware(&Running::new("0.6.0", "paperpaw-xteink", 5, 0, false));
        assert_eq!(check_caps_agree(&device), Ok(()));
    }

    #[test]
    fn what_code_plugins_stand_on_is_answered_and_counts_the_rows_of_op_14() {
        let device = device(&[]);
        assert_eq!(device.native(&mut |_| {}), Err(ERR_NOT_FOUND));
        device.set_native(native_state());
        assert!(device.set_not_installed(&[("a", 1), ("b", 3)]));
        let mut seen = None;
        device
            .native(&mut |f| {
                seen = Some((
                    f.abi_major,
                    f.abi_draft,
                    f.target,
                    f.code_file.to_string(),
                    f.safe_mode,
                    f.crashed.to_string(),
                    f.last_loaded.to_string(),
                    f.pending,
                    f.not_installed,
                    f.room_free,
                    f.room_largest,
                ));
            })
            .unwrap();
        assert_eq!(
            seen.unwrap(),
            (
                u32::from(INTERFACE),
                u32::from(DRAFT),
                5,
                "code.ppn".to_string(),
                1,
                "stats".to_string(),
                "widgets".to_string(),
                2,
                2,
                100_000,
                60_000
            )
        );
    }

    #[test]
    fn plugins_not_installed_are_listed_and_the_generation_follows() {
        let device = device(&[]);
        let before = device.generation();
        let mut rows = Vec::new();
        device
            .not_installed(&mut |row| {
                rows.push(row.plugin.to_string());
                ControlFlow::Continue(())
            })
            .unwrap();
        assert!(rows.is_empty(), "an empty list, not 'not here'");
        assert!(device.set_not_installed(&[("alpha", 2), ("beta", 1)]));
        assert_ne!(device.generation(), before);
        let mut seen = Vec::new();
        device
            .not_installed(&mut |row| {
                seen.push((row.plugin.to_string(), row.reason));
                ControlFlow::Continue(())
            })
            .unwrap();
        assert_eq!(seen, [("alpha".to_string(), 2), ("beta".to_string(), 1)]);
        // No more than op 14 lists.
        let many: Vec<(String, u32)> = (0..40).map(|n| (std::format!("p{n}"), 2)).collect();
        let borrowed: Vec<(&str, u32)> = many.iter().map(|(a, b)| (a.as_str(), *b)).collect();
        assert!(device.set_not_installed(&borrowed));
        let mut count = 0;
        device
            .not_installed(&mut |_| {
                count += 1;
                ControlFlow::Continue(())
            })
            .unwrap();
        assert_eq!(count, MAX_NOT_INSTALLED);
    }

    #[test]
    fn memory_is_probed_once_per_window_and_says_when_it_is_cached() {
        use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
        static PROBES: AtomicU32 = AtomicU32::new(0);
        static NOW: AtomicU64 = AtomicU64::new(1_000);
        fn probe() -> Option<MemoryFacts> {
            PROBES.fetch_add(1, Ordering::Relaxed);
            Some(MemoryFacts {
                heap_free: 100,
                heap_size: 200,
                largest: Some(50),
                stack: None,
                external: None,
                cached: false,
            })
        }
        fn clock() -> u64 {
            NOW.load(Ordering::Relaxed)
        }
        let device = device(&[]);
        assert_eq!(device.memory(), Err(ERR_NOT_FOUND), "nothing to probe with");
        device.set_memory_probe(probe, clock);
        assert!(!device.memory().unwrap().cached);
        assert!(device.memory().unwrap().cached);
        assert_eq!(PROBES.load(Ordering::Relaxed), 1);
        NOW.store(1_000 + 10_000, Ordering::Relaxed);
        assert!(!device.memory().unwrap().cached);
        assert_eq!(PROBES.load(Ordering::Relaxed), 2);
    }

    /// A probe that cannot read now is not found, never zeros.
    #[test]
    fn a_probe_that_cannot_read_is_not_found() {
        let device = device(&[]);
        device.set_memory_probe(|| None, || 0);
        assert_eq!(device.memory(), Err(ERR_NOT_FOUND));
    }

    fn radio() -> WifiState {
        WifiState {
            state: 3,
            ssid: Text::new("home"),
            address: Text::new("192.168.1.20"),
            host_name: Text::new("paperpaw"),
            guests: 0,
            scanning: true,
            wanted: true,
            busy: false,
        }
    }

    /// Wi-Fi is here exactly where the caps say so: the radio and the
    /// networks answer together.
    #[test]
    fn the_wifi_bit_and_ops_8_and_9_agree() {
        let device = X4Device::new(
            Identity {
                caps: identity().caps | caps::WIFI,
                ..identity()
            },
            list(&[]),
        );
        assert!(check_caps_agree(&device).is_err(), "no radio yet");
        device.set_wifi(radio());
        assert_eq!(check_caps_agree(&device), Ok(()));
        let mut seen = None;
        device
            .wifi(&mut |f| {
                seen = Some((
                    f.state,
                    f.ssid.to_string(),
                    f.address.to_string(),
                    f.host_name.to_string(),
                    (f.guests, f.scanning, f.wanted, f.busy),
                ));
            })
            .unwrap();
        assert_eq!(
            seen.unwrap(),
            (
                3,
                "home".to_string(),
                "192.168.1.20".to_string(),
                "paperpaw".to_string(),
                (0, true, true, false)
            )
        );
    }

    /// Strongest first, at most 32, none known until the secrets are wired.
    #[test]
    fn the_networks_are_the_strongest_found() {
        let device = device(&[]);
        assert_eq!(
            device.networks(&mut |_| ControlFlow::Break(())),
            Err(ERR_NOT_FOUND)
        );
        device.set_wifi(radio());
        let before = device.generation();
        let found: Vec<(String, i32, bool)> = (0..40)
            .map(|n| (std::format!("net{n}"), -90 + n, n % 2 == 0))
            .collect();
        let rows: Vec<(&str, i32, bool)> =
            found.iter().map(|(s, a, b)| (s.as_str(), *a, *b)).collect();
        assert!(device.set_networks(&rows));
        assert_ne!(device.generation(), before);
        let mut seen = Vec::new();
        device
            .networks(&mut |row| {
                seen.push((row.ssid.to_string(), row.signal, row.secured, row.known));
                ControlFlow::Continue(())
            })
            .unwrap();
        assert_eq!(seen.len(), MAX_NETWORKS as usize);
        assert_eq!(seen[0], ("net39".to_string(), -51, false, false));
        assert!(seen.windows(2).all(|pair| pair[0].1 >= pair[1].1));
        assert!(seen.iter().all(|row| !row.3));
    }

    /// Power is the kernel's: normal until it says otherwise.
    #[test]
    fn power_follows_what_the_kernel_reports() {
        use paperpaw_sdk::power::PowerLevel;
        let device = device(&[]);
        let mut level = None;
        device.power(&mut |p| level = Some(p.level)).unwrap();
        assert_eq!(level, Some(PowerLevel::Normal));
        let mut saving = Power::default();
        saving.level = PowerLevel::Saving;
        saving.stretch = 4;
        assert!(device.set_power(saving));
        let mut seen = None;
        device
            .power(&mut |p| seen = Some((p.level, p.stretch)))
            .unwrap();
        assert_eq!(seen, Some((PowerLevel::Saving, 4)));
    }

    /// A setter called from inside a visitor changes nothing and says so; it
    /// never panics on the borrow the visitor holds.
    #[test]
    fn a_setter_inside_a_visitor_is_refused_not_a_panic() {
        let device = device(&[]);
        assert!(device.set_not_installed(&[("a", 2)]));
        let mut inside = None;
        device
            .not_installed(&mut |_| {
                inside = Some(device.set_not_installed(&[("b", 1)]));
                ControlFlow::Continue(())
            })
            .unwrap();
        assert_eq!(inside, Some(false));
        let mut ids = Vec::new();
        device
            .not_installed(&mut |row| {
                ids.push(row.plugin.to_string());
                ControlFlow::Continue(())
            })
            .unwrap();
        assert_eq!(ids, ["a"]);
    }

    /// A stored id is cut at 64 bytes, a plugin id's cap.
    #[test]
    fn a_long_id_is_cut_at_64_bytes() {
        let device = device(&[]);
        let long = "x".repeat(100);
        assert!(device.set_not_installed(&[(long.as_str(), 2)]));
        let mut len = None;
        device
            .not_installed(&mut |row| {
                len = Some(row.plugin.len());
                ControlFlow::Continue(())
            })
            .unwrap();
        assert_eq!(len, Some(64));
    }

    /// The interface and its draft are the SDK's, not a copy.
    #[test]
    fn the_interface_is_the_sdks() {
        let device = device(&[]);
        device.set_native(native_state());
        let mut seen = None;
        device
            .native(&mut |f| seen = Some((f.abi_major, f.abi_draft)))
            .unwrap();
        assert_eq!(seen, Some((u32::from(INTERFACE), u32::from(DRAFT))));
    }

    /// The device is made once on the heap; the struct stays small enough for
    /// that to be the only frame that holds it.
    #[test]
    fn the_device_is_shared_and_small() {
        let shared = X4Device::shared(identity(), list(&[]));
        assert_eq!(Rc::strong_count(&shared), 1);
        assert!(
            core::mem::size_of::<X4Device>() <= 1_024,
            "{}",
            core::mem::size_of::<X4Device>()
        );
    }

    /// The Wi-Fi and power setters are refused, not panicked, inside a visitor.
    #[test]
    fn the_networks_and_power_setters_are_refused_inside_a_visitor() {
        let device = device(&[]);
        device.set_wifi(radio());
        assert!(device.set_networks(&[("home", -40, true)]));
        let mut inside = None;
        device
            .networks(&mut |_| {
                inside = Some(device.set_networks(&[("other", -30, false)]));
                ControlFlow::Continue(())
            })
            .unwrap();
        assert_eq!(inside, Some(false));
        let mut power_inside = None;
        device
            .power(&mut |_| power_inside = Some(device.set_power(Power::default())))
            .unwrap();
        assert_eq!(power_inside, Some(false));
    }
}
