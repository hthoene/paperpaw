//! The emulator's `core.device` host (`docs/abi/device.md` section 2): the
//! read side of family DEV over what an emulated device really has.
//!
//! What a profile has is what is answered: its identity, memory class,
//! capabilities, buttons, the pretend Wi-Fi radio where the profile's device
//! manages Wi-Fi itself, the pretend Bluetooth radio where it has one, the
//! heap budget as the memory probe, and the power level the kernel reports. A
//! part the profile does not have is `ERR_NOT_FOUND` (the default of
//! [`DeviceHost`]): firmware (PaperPaw is never the emulator's firmware),
//! native-plugin state, the built-in list and the plugins a start could not
//! install. `caps` and the answered parts agree (a test).
//!
//! The emulator offers the service to admission with [`offered_services`];
//! the X4 firmware does not.

use std::cell::{Cell, RefCell};
use std::ops::ControlFlow;
use std::rc::Rc;
use std::sync::OnceLock;
use std::time::Instant;

use paperpaw_core::native_host::device_service::{
    BluetoothFacts, BondedRow, DEVICE_SERVICES, DeviceFacts, DeviceHost, DeviceServices,
    MemoryFacts, NetworkRow, ProbeCache, WifiFacts, caps,
};
use paperpaw_core::native_host::shell::DATA_SERVICES;
use paperpaw_core::sdk::power::Power;
use paperpaw_native::metadata::Service;
use paperpaw_platform::{MemoryClass, PixelFormat};

use crate::profiles::Profile;

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
    // and no system clipboard, power off or firmware.
    caps::NETWORK
        | caps::ROTATION
        | bit(device.input.touch, caps::TOUCH)
        | bit(profile.wifi, caps::WIFI)
        | bit(profile.bluetooth > 0, caps::BLUETOOTH)
        | bit(profile.speech, caps::SPEECH)
        | bit(profile.sleep, caps::SLEEP)
        | bit(profile.colors, caps::COLOURS)
        | bit(!profile.button_row.is_empty(), caps::BUTTON_ROW)
        | bit(profile.light.is_some(), caps::FRONTLIGHT)
        | bit(
            device.display.format == PixelFormat::Mono1,
            caps::BLACK_AND_WHITE,
        )
        | bit(device.input.pen, caps::PEN)
}

/// An emulated device behind `core.device`.
#[derive(Debug)]
pub struct DesktopDevice {
    profile: &'static Profile,
    power: RefCell<Power>,
    /// `dev_wifi.state` (0 off, 1 on, ...) of the pretend radio.
    wifi_state: Cell<u32>,
    clock: Instant,
    memory: ProbeCache<MemoryFacts>,
}

impl DesktopDevice {
    /// The device of `profile`, power normal, radios off.
    #[must_use]
    pub fn new(profile: &'static Profile) -> Self {
        Self {
            profile,
            power: RefCell::new(Power::default()),
            wifi_state: Cell::new(0),
            clock: Instant::now(),
            memory: ProbeCache::new(),
        }
    }

    /// The level and mode the kernel reports now (`Kernel::power`).
    pub fn set_power(&self, power: Power) {
        *self.power.borrow_mut() = power;
    }

    /// The pretend Wi-Fi radio's state, `dev_wifi.state`.
    pub fn set_wifi_state(&self, state: u32) {
        self.wifi_state.set(state);
    }

    /// The device view for the shell to wire behind `core.device` (no shell
    /// does yet: `HostServices::wire_device` is the host's side).
    #[must_use]
    pub fn services(profile: &'static Profile) -> DeviceServices {
        DeviceServices(Rc::new(Self::new(profile)))
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
            catalog: self.profile.catalog_device.unwrap_or(""),
            class: match device.memory {
                MemoryClass::Small => 0,
                _ => 2,
            },
            caps: caps_of(self.profile),
            // The emulator declares no power controls to the kernel.
            controls: 0,
            tls_sessions: u32::from(paperpaw_serve::TLS_SESSIONS),
            // The emulator does not say how it started or how long it slept.
            start: 0,
            slept_ms: None,
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

    fn wifi(&self, visit: &mut dyn FnMut(&WifiFacts<'_>)) -> Result<(), i32> {
        if !self.profile.wifi {
            return Err(paperpaw_native::host_glue::ERR_NOT_FOUND);
        }
        visit(&WifiFacts {
            state: self.wifi_state.get(),
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

    fn networks(
        &self,
        visit: &mut dyn FnMut(&NetworkRow<'_>) -> ControlFlow<()>,
    ) -> Result<(), i32> {
        if !self.profile.wifi {
            return Err(paperpaw_native::host_glue::ERR_NOT_FOUND);
        }
        if crate::device::wifi_online() {
            let row = NetworkRow {
                ssid: crate::wifi::WORKSHOP_NETWORK,
                signal: -40,
                secured: true,
                known: true,
            };
            if visit(&row).is_break() {
                return Ok(());
            }
        }
        for &(ssid, signal, secured) in crate::wifi::PRETEND_SCAN {
            let row = NetworkRow {
                ssid,
                signal: i32::from(signal),
                secured,
                known: false,
            };
            if visit(&row).is_break() {
                break;
            }
        }
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

    fn power(&self, visit: &mut dyn FnMut(&Power)) -> Result<(), i32> {
        visit(&self.power.borrow());
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
        // The rows are the static list, strongest first (no allocation).
        let expected: Vec<(String, i32)> = crate::wifi::PRETEND_SCAN
            .iter()
            .map(|&(ssid, signal, _)| (ssid.to_string(), i32::from(signal)))
            .collect();
        assert_eq!(ssids, expected);
        assert!(ssids.windows(2).all(|pair| pair[0].1 >= pair[1].1));
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

    #[test]
    fn power_follows_what_the_kernel_reports() {
        let device = DesktopDevice::new(&PROFILES[0]);
        let mut level = None;
        device.power(&mut |p| level = Some(p.level)).unwrap();
        assert_eq!(level, Some(paperpaw_core::sdk::power::PowerLevel::Normal));
        device.set_power(Power::new(
            paperpaw_core::sdk::power::PowerLevel::Saving,
            paperpaw_core::sdk::Id::parse("core.saving").unwrap(),
            true,
            4,
        ));
        device.power(&mut |p| level = Some(p.level)).unwrap();
        assert_eq!(level, Some(paperpaw_core::sdk::power::PowerLevel::Saving));
    }
}
