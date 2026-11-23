//! The emulator's `core.device` host (`docs/abi/device.md` section 2): the
//! read side of family DEV over what an emulated device really has.
//!
//! What a profile has is what is answered: its identity, memory class,
//! capabilities, buttons, the networks the pretend Wi-Fi radio's last scan
//! found where the profile's device manages Wi-Fi itself, and the heap budget
//! as the memory probe. What the radios are doing, the bonded devices and the
//! power level are the kernel's own models (`kernel_models`, ops 8, 10, 11
//! and 12): this host's answers for them are the idle ones, as the X4's
//! (`paperpaw_x4::device_host`), for a call without the kernel. A part the
//! profile does not have is `ERR_NOT_FOUND` (the default of [`DeviceHost`]):
//! firmware (PaperPaw is never the emulator's firmware), native-plugin state,
//! the built-in list and the plugins a start could not install. `caps` and
//! the answered parts agree (a test).
//!
//! The emulator offers the service to admission with [`offered_services`];
//! the X4 firmware does not.

use std::cell::RefCell;
use std::ops::ControlFlow;
use std::rc::Rc;
use std::sync::OnceLock;
use std::time::Instant;

use paperpaw_core::native_host::device_service::{
    BluetoothFacts, BondedRow, DEVICE_SERVICES, DeviceFacts, DeviceHost, MemoryFacts, NativeFacts,
    NetworkRow, ProbeCache, WifiFacts, caps,
};
use paperpaw_core::native_host::shell::DATA_SERVICES;
use paperpaw_core::sdk::power::Power;
use paperpaw_native::metadata::Service;
use paperpaw_platform::{MemoryClass, PixelFormat};

use crate::profiles::Profile;

/// What plugin libraries call the Linux emulator of an X4-family device that
/// installs code plugins built for this computer (`--trust-code`): its own
/// name, as its builds are not the X4's `.ppn` files (#278).
pub const HOST_CATALOG: &str = "emulator-linux-x86_64";

/// The services this shell advertises to admission: the shared data services
/// and `core.device`, which this module implements.
pub fn offered_services() -> &'static [Service<'static>] {
    static OFFERED: OnceLock<Vec<Service<'static>>> = OnceLock::new();
    OFFERED.get_or_init(|| {
        DATA_SERVICES
            .iter()
            .chain(DEVICE_SERVICES)
            .copied()
            .collect()
    })
}

/// The `caps` bits of `profile`'s device, as the kernel's builder sets the
/// capabilities of the emulated device (`device::build`, `Kernel::builder`).
#[must_use]
pub fn caps_of(profile: &Profile) -> u64 {
    let device = &profile.device;
    let bit = |on: bool, bit: u64| if on { bit } else { 0 };
    // The emulator builds the kernel with the network on, a rotatable screen
    // and no system clipboard or firmware (power off where the profile has it).
    caps::NETWORK
        | caps::ROTATION
        | bit(device.input.touch, caps::TOUCH)
        | bit(profile.wifi, caps::WIFI)
        | bit(profile.bluetooth > 0, caps::BLUETOOTH)
        | bit(profile.speech, caps::SPEECH)
        | bit(profile.sleep, caps::SLEEP)
        | bit(profile.power_off, caps::POWER_OFF)
        | bit(profile.refresh_asleep, caps::REFRESH_ASLEEP)
        | bit(profile.colors, caps::COLOURS)
        | bit(!profile.button_row.is_empty(), caps::BUTTON_ROW)
        | bit(profile.light.is_some(), caps::FRONTLIGHT)
        | bit(
            device.display.format == PixelFormat::Mono1,
            caps::BLACK_AND_WHITE,
        )
        | bit(device.input.pen, caps::PEN)
}

/// Whether the emulated device of `profile` installs code plugins built for
/// this computer: Linux on x86-64, code plugins on, a device of the X4 family.
fn card_code(profile: &Profile) -> bool {
    cfg!(all(target_os = "linux", target_arch = "x86_64"))
        && crate::device::code_plugins_on()
        && matches!(profile.catalog_device, Some("x4" | "x4-pro" | "x4-classic"))
}

/// The networks the last scan found: what op 9 lists, as on the X4
/// (`paperpaw_x4::device_host::X4Device::set_networks`). The device writes it
/// as the scan comes, before the kernel tells the plugins of it (they read it
/// then); `core.device` reads it.
#[derive(Debug, Default)]
pub struct Networks(RefCell<Vec<(String, i32, bool)>>);

impl Networks {
    /// The networks of a scan, `(ssid, signal in dBm, secured)`, kept as the
    /// kernel keeps them: the strongest first, each name once, no hidden one
    /// (an empty name, which no plugin can join).
    pub fn set<'a>(&self, rows: impl Iterator<Item = (&'a str, i32, bool)>) {
        let mut kept: Vec<(String, i32, bool)> = Vec::new();
        for (ssid, signal, secured) in rows.filter(|row| !row.0.is_empty()) {
            if let Some(at) = kept.iter().position(|row| row.0 == ssid) {
                if kept[at].1 >= signal {
                    continue;
                }
                kept.remove(at);
            }
            // After the last network that is as strong: stable for equals.
            let at = kept.partition_point(|row| row.1 >= signal);
            kept.insert(at, (ssid.to_owned(), signal, secured));
        }
        *self.0.borrow_mut() = kept;
    }

    fn visit(&self, visit: &mut dyn FnMut(&str, i32, bool) -> ControlFlow<()>) {
        for (ssid, signal, secured) in self.0.borrow().iter() {
            if visit(ssid, *signal, *secured).is_break() {
                break;
            }
        }
    }
}

/// An emulated device behind `core.device`.
#[derive(Debug)]
pub struct DesktopDevice {
    profile: &'static Profile,
    clock: Instant,
    memory: ProbeCache<MemoryFacts>,
    /// The networks the last scan found; none: no scan is ever seen.
    networks: Option<Rc<Networks>>,
    /// The emulator installs and runs code plugins built for this computer.
    card_code: bool,
}

impl DesktopDevice {
    /// The device of `profile`.
    #[must_use]
    pub fn new(profile: &'static Profile) -> Self {
        Self {
            profile,
            clock: Instant::now(),
            memory: ProbeCache::new(),
            networks: None,
            card_code: card_code(profile),
        }
    }

    /// Answers op 9 from `networks`, which the device keeps.
    #[must_use]
    pub fn with_networks(mut self, networks: Rc<Networks>) -> Self {
        self.networks = Some(networks);
        self
    }

    fn now_ms(&self) -> u64 {
        u64::try_from(self.clock.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
}

impl DeviceHost for DesktopDevice {
    fn device(&self, visit: &mut dyn FnMut(&DeviceFacts<'_>)) -> Result<(), i32> {
        let device = &self.profile.device;
        visit(&DeviceFacts {
            id: device.id,
            model: device.name,
            catalog: if self.card_code {
                HOST_CATALOG
            } else {
                self.profile.catalog_device.unwrap_or("")
            },
            class: match device.memory {
                MemoryClass::Small => 0,
                _ => 2,
            },
            caps: caps_of(self.profile),
            // The emulator declares no power controls to the kernel.
            controls: 0,
            tls_sessions: u32::from(self.profile.tls_sessions()),
            // The emulator does not say how it started or how long it slept.
            start: 0,
            slept_ms: None,
        });
        Ok(())
    }

    /// Op 5 where code plugins built for this computer are installed: the
    /// file they are in and the interface the library is built for.
    fn native(&self, visit: &mut dyn FnMut(&NativeFacts<'_>)) -> Result<(), i32> {
        if !self.card_code {
            return Err(paperpaw_native::host_glue::ERR_NOT_FOUND);
        }
        let file = crate::native::file_name();
        visit(&NativeFacts {
            abi_major: u32::from(paperpaw_core::sdk::native::INTERFACE),
            abi_draft: u32::from(paperpaw_core::sdk::native::DRAFT),
            target: 0,
            code_file: &file,
            safe_mode: 0,
            crashed: "",
            last_loaded: "",
            pending: 0,
            not_installed: 0,
            room_free: u64::MAX / 2,
            room_largest: u64::MAX / 2,
        });
        Ok(())
    }

    fn memory(&self) -> Result<MemoryFacts, i32> {
        let (probe, cached) = self.memory.get(self.now_ms(), || {
            crate::budget::memory_use().map(|use_| MemoryFacts {
                heap_free: use_.heap_free,
                heap_size: use_.heap_size,
                largest: use_.largest_free,
                stack: use_.stack_free.zip(use_.stack_size),
                external: None,
                cached: false,
            })
        });
        // No budget: no heap size, so no probe.
        probe
            .map(|facts| MemoryFacts { cached, ..facts })
            .ok_or(paperpaw_native::host_glue::ERR_NOT_FOUND)
    }

    /// Idle facts where the radio is here: the kernel answers op 8.
    fn wifi(&self, visit: &mut dyn FnMut(&WifiFacts<'_>)) -> Result<(), i32> {
        if !self.profile.wifi {
            return Err(paperpaw_native::host_glue::ERR_NOT_FOUND);
        }
        visit(&WifiFacts {
            state: 0,
            ssid: "",
            address: "",
            host_name: "",
            guests: 0,
            scanning: false,
            wanted: false,
            busy: false,
        });
        Ok(())
    }

    /// The networks of the last scan, strongest first, as the X4 answers
    /// them: none before a scan, and `known` false (no password is looked
    /// for).
    fn networks(
        &self,
        visit: &mut dyn FnMut(&NetworkRow<'_>) -> ControlFlow<()>,
    ) -> Result<(), i32> {
        if !self.profile.wifi {
            return Err(paperpaw_native::host_glue::ERR_NOT_FOUND);
        }
        let Some(networks) = &self.networks else {
            return Ok(());
        };
        networks.visit(&mut |ssid, signal, secured| {
            visit(&NetworkRow {
                ssid,
                signal,
                secured,
                known: false,
            })
        });
        Ok(())
    }

    fn bluetooth(&self) -> Result<BluetoothFacts, i32> {
        if self.profile.bluetooth == 0 {
            return Err(paperpaw_native::host_glue::ERR_NOT_FOUND);
        }
        Ok(BluetoothFacts {
            radio: 0,
            max_links: u32::from(self.profile.bluetooth),
        })
    }

    fn bonded(&self, _visit: &mut dyn FnMut(&BondedRow<'_>) -> ControlFlow<()>) -> Result<(), i32> {
        // A radio with nothing bonded yet: an empty list, not "not here".
        if self.profile.bluetooth == 0 {
            return Err(paperpaw_native::host_glue::ERR_NOT_FOUND);
        }
        Ok(())
    }

    /// The normal level: the kernel answers op 12.
    fn power(&self, visit: &mut dyn FnMut(&Power)) -> Result<(), i32> {
        visit(&Power::default());
        Ok(())
    }

    fn keys(&self, visit: &mut dyn FnMut(&str) -> ControlFlow<()>) -> Result<(), i32> {
        for name in self.profile.buttons {
            if visit(name).is_break() {
                break;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles::PROFILES;
    use paperpaw_core::native_host::device_service::check_caps_agree;
    use paperpaw_native::host_glue::ERR_NOT_FOUND;

    #[test]
    fn caps_and_the_answered_parts_agree_for_every_profile() {
        for profile in PROFILES {
            let device = DesktopDevice::new(profile);
            assert_eq!(check_caps_agree(&device), Ok(()), "{}", profile.device.id);
            let mut caps_seen = 0;
            device.device(&mut |facts| caps_seen = facts.caps).unwrap();
            assert_eq!(caps_seen, caps_of(profile));
            // PaperPaw is never the emulator's firmware.
            assert_eq!(device.firmware(&mut |_| {}), Err(ERR_NOT_FOUND));
            assert_eq!(caps_seen & caps::FIRMWARE, 0);
        }
    }

    /// Op 9 is the last scan's networks, none before one, as on the X4.
    #[test]
    fn the_networks_are_the_last_scans() {
        let networks = Rc::new(Networks::default());
        let x4 = DesktopDevice::new(crate::profiles::find("xteink-x4").unwrap())
            .with_networks(networks.clone());
        let rows = |device: &DesktopDevice| {
            let mut rows = Vec::new();
            device
                .networks(&mut |row| {
                    rows.push((row.ssid.to_string(), row.signal, row.known));
                    ControlFlow::Continue(())
                })
                .unwrap();
            rows
        };
        assert_eq!(rows(&x4), [], "no scan yet");
        networks.set(
            crate::wifi::pretend_networks(true)
                .iter()
                .map(|n| (n.ssid.as_str(), i32::from(n.signal), n.secured)),
        );
        let found = rows(&x4);
        assert_eq!(found.len(), crate::wifi::PRETEND_SCAN.len() + 1);
        assert_eq!(found[0].0, crate::wifi::WORKSHOP_NETWORK);
        assert!(found.iter().all(|(_, _, known)| !known));
        // As the kernel keeps them: strongest first, each name once at its
        // strongest, no hidden one.
        networks.set(
            [
                ("b", -70, true),
                ("a", -60, false),
                ("b", -50, true),
                ("", -40, true),
            ]
            .into_iter(),
        );
        let found: Vec<_> = rows(&x4)
            .into_iter()
            .map(|(ssid, signal, _)| (ssid, signal))
            .collect();
        assert_eq!(found, [(String::from("b"), -50), (String::from("a"), -60)]);
    }

    #[test]
    fn the_x4_profile_has_wifi_and_no_bluetooth_and_android_neither() {
        let x4 = DesktopDevice::new(crate::profiles::find("xteink-x4").unwrap());
        assert_eq!(x4.wifi(&mut |_| {}), Ok(()));
        let mut ssids = Vec::new();
        x4.networks(&mut |row| {
            ssids.push((row.ssid.to_string(), row.signal));
            ControlFlow::Continue(())
        })
        .unwrap();
        assert!(ssids.is_empty(), "no scan yet");
        assert_eq!(x4.bluetooth(), Err(ERR_NOT_FOUND));
        assert_eq!(
            x4.bonded(&mut |_| ControlFlow::Break(())),
            Err(ERR_NOT_FOUND)
        );
        let mut keys = Vec::new();
        x4.keys(&mut |name| {
            keys.push(name.to_string());
            ControlFlow::Continue(())
        })
        .unwrap();
        assert_eq!(keys, crate::profiles::find("xteink-x4").unwrap().buttons);
        let android = DesktopDevice::new(
            PROFILES
                .iter()
                .find(|p| p.catalog_device == Some("android"))
                .unwrap(),
        );
        assert_eq!(android.wifi(&mut |_| {}), Err(ERR_NOT_FOUND));
        assert_eq!(
            android.networks(&mut |_| ControlFlow::Break(())),
            Err(ERR_NOT_FOUND)
        );
    }

    #[test]
    fn the_emulator_offers_core_device_and_the_x4_catalog_does_not() {
        assert!(offered_services().iter().any(|s| s.key == "core.device"));
        assert!(!DATA_SERVICES.iter().any(|s| s.key == "core.device"));
        assert_eq!(offered_services().len(), DATA_SERVICES.len() + 1);
    }

    /// Where the emulator installs code plugins built for this computer it
    /// has a catalogue name of its own and says which file they are in; where
    /// it does not (the default), it is the X4 it emulates and says no code.
    #[test]
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    fn code_plugins_on_give_the_emulator_its_own_catalogue_name_and_file() {
        let _one = crate::one_device_at_a_time();
        let x4 = crate::profiles::find("xteink-x4").unwrap();
        let catalog = |device: &DesktopDevice| {
            let mut name = String::new();
            device
                .device(&mut |f| name = f.catalog.to_string())
                .unwrap();
            name
        };
        let file = |device: &DesktopDevice| {
            let mut file = None;
            device
                .native(&mut |f| file = Some((f.code_file.to_string(), f.abi_major)))
                .map(|()| file)
        };
        let off = DesktopDevice::new(x4);
        assert_eq!(catalog(&off), "x4");
        assert_eq!(file(&off), Err(ERR_NOT_FOUND));
        crate::device::set_code_plugins(true);
        let on = DesktopDevice::new(x4);
        let boox = DesktopDevice::new(crate::profiles::find("boox-go-7").unwrap());
        crate::device::set_code_plugins(false);
        assert_eq!(catalog(&on), HOST_CATALOG);
        assert_eq!(
            file(&on),
            Ok(Some((crate::native::file_name(), 7))),
            "libraries of this computer"
        );
        assert_ne!(catalog(&boox), HOST_CATALOG, "no X4 family, no host builds");
        assert_eq!(file(&boox), Err(ERR_NOT_FOUND));
    }
}
