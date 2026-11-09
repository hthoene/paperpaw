//! What the X4 firmware has, as `core.device` gives it ([`crate::device_host`]):
//! the pure mappings from the firmware's own types (the power controls the
//! kernel is built with, the memory reading, what code plugins stand on, how
//! the start came), tested here on a computer, so that the firmware's glue is
//! only the calls.

use alloc::string::String;

use paperpaw_core::native_host::device_service::{MemoryFacts, caps};
use paperpaw_platform::MemoryClass;
use paperpaw_platform::firmware::Text;
use paperpaw_sdk::bluetooth::{Bluetooth, DeviceKind, RadioState};
use paperpaw_sdk::memory::MemoryUse;
use paperpaw_sdk::native::{CodeRoom, Native, NotInstalledReason, SafeModeReason};
use paperpaw_sdk::power::{PowerControls, Start};
use paperpaw_sdk::wifi::{Wifi, WifiState as Radio};

use crate::device_host::{BluetoothState, Bonded, NativeState, WifiState};

/// What differs between X4 builds, for `dev_device.caps`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Features {
    /// A touch screen (the X4 Pro).
    pub touch: bool,
    /// The row of hints under the screen (the buttons of the X4 and Classic).
    pub button_row: bool,
    /// Ops 8 and 9 are answered: the firmware tells the device the Wi-Fi radio
    /// (`X4Device::set_wifi`).
    pub wifi: bool,
    /// Ops 10 and 11 are answered: a Bluetooth radio the kernel uses (the
    /// `bluetooth` build) that the firmware tells the device
    /// (`X4Device::set_bluetooth`).
    pub bluetooth: bool,
    /// A front light (the X4 Pro).
    pub frontlight: bool,
    /// PaperPaw's own updates run (the firmware has its OTA data).
    pub firmware: bool,
}

/// `dev_device.caps` of an X4: the parts [`crate::device_host::X4Device`]
/// answers and the capabilities the kernel is built with agree (the kernel's
/// builder is told `wifi`, `sleep`, `rotation` and `power_off` on every X4,
/// `bluetooth` and `firmware` only where they exist).
#[must_use]
pub fn caps_for(features: &Features) -> u64 {
    let bit = |on: bool, bit: u64| if on { bit } else { 0 };
    caps::NETWORK
        | bit(features.wifi, caps::WIFI)
        | caps::SLEEP
        | caps::ROTATION
        | caps::POWER_OFF
        | caps::BLACK_AND_WHITE
        | bit(features.touch, caps::TOUCH)
        | bit(features.button_row, caps::BUTTON_ROW)
        | bit(features.bluetooth, caps::BLUETOOTH)
        | bit(features.frontlight, caps::FRONTLIGHT)
        | bit(features.firmware, caps::FIRMWARE)
}

/// `dev_device.controls`: bit 0 radio, 1 CPU clock, 2 buttons, 3 frontlight,
/// 4 system saver. The same controls the kernel is built with.
#[must_use]
pub fn controls_bits(controls: &PowerControls) -> u32 {
    u32::from(controls.radio)
        | u32::from(controls.cpu_clock) << 1
        | u32::from(controls.buttons) << 2
        | u32::from(controls.frontlight) << 3
        | u32::from(controls.system_saver) << 4
}

/// `dev_device.class`: 0 small, 2 large (the X4 and X3 are small, the X4 Pro
/// and Classic large).
#[must_use]
pub fn class_of(memory: MemoryClass) -> u32 {
    match memory {
        MemoryClass::Small => 0,
        _ => 2,
    }
}

/// `dev_device.start`: 0 unknown, 1 power on, 2 woke, 3 restart.
#[must_use]
pub fn start_code(start: Start) -> u32 {
    match start {
        Start::PowerOn => 1,
        Start::Wake => 2,
        Start::Restart => 3,
        _ => 0,
    }
}

/// The memory probe's answer from the shell's reading: the heap, the largest
/// block and the stack where they are known, and the external memory (PSRAM)
/// where the heap has it.
#[must_use]
pub fn memory_facts(used: &MemoryUse) -> MemoryFacts {
    let external = used
        .regions
        .iter()
        .flatten()
        .find(|region| region.external)
        .map(|region| (region.free, region.size));
    MemoryFacts {
        heap_free: used.heap_free,
        heap_size: used.heap_size,
        largest: used.largest_free,
        stack: used.stack_free.zip(used.stack_size),
        external,
        cached: false,
    }
}

/// `dev_native.safe_mode`: 0 none, 1 the last starts failed (and the 8 s
/// escape, which is the same to a reader), 2 the key held at power-on.
#[must_use]
pub fn safe_mode_code(native: &Native) -> u32 {
    match native.safe_mode.as_ref().map(|mode| mode.reason) {
        None => 0,
        Some(SafeModeReason::KeyHeld) => 2,
        Some(_) => 1,
    }
}

/// What code plugins stand on, from the kernel's own record of it: `target`
/// the chip id (5 ESP32-C3, 9 ESP32-S3), `pending` the code plugins that take
/// effect at the next start, and the code flash `room` as (free, largest).
#[must_use]
pub fn native_state(
    native: &Native,
    target: u32,
    pending: u32,
    room: (u64, u64),
    code_file: &'static str,
) -> NativeState {
    let name = |text: Option<&String>| Text::new(text.map_or("", String::as_str));
    let mode = native.safe_mode.as_ref();
    NativeState {
        target,
        code_file,
        safe_mode: safe_mode_code(native),
        crashed: name(mode.and_then(|mode| mode.crashed.as_ref())),
        last_loaded: name(mode.and_then(|mode| mode.last_loaded.as_ref())),
        pending,
        room_free: room.0,
        room_largest: room.1,
    }
}

/// The code flash room as (free, largest free run): the loader places images
/// one after the other from the start of its region, so what is free is one
/// run at the end.
#[must_use]
pub fn code_room(room: &CodeRoom) -> (u64, u64) {
    let free = room.total.saturating_sub(room.used);
    (free, free)
}

/// `dev_not_installed.reason`: 1 other author, 2 damaged.
#[must_use]
pub fn not_installed_reason(reason: NotInstalledReason) -> u32 {
    match reason {
        NotInstalledReason::OtherAuthor => 1,
        _ => 2,
    }
}

/// Op 8 from Wi-Fi as the kernel keeps it (what plugins see too): the state
/// codes of `device.md` (0 off, 1 on, 2 joining, 3 online, 4 hosting), the
/// network and address where there is one, and the three flags.
#[must_use]
pub fn wifi_facts(wifi: &Wifi) -> WifiState {
    let (state, ssid, address, host_name, guests) = match &wifi.state {
        Radio::On => (1, "", "", "", 0),
        Radio::Joining { ssid } => (2, ssid.as_str(), "", "", 0),
        Radio::Online {
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
        Radio::Hosting {
            ssid,
            address,
            guests,
        } => (4, ssid.as_str(), address.as_str(), "", u32::from(*guests)),
        // Off, and any state a later interface adds: not on.
        _ => (0, "", "", "", 0),
    };
    WifiState {
        state,
        ssid: Text::new(ssid),
        address: Text::new(address),
        host_name: Text::new(host_name),
        guests,
        scanning: wifi.scanning,
        wanted: wifi.wanted,
        busy: wifi.busy,
    }
}

/// Op 10 from Bluetooth as the kernel keeps it.
#[must_use]
pub fn bluetooth_facts(bluetooth: &Bluetooth) -> BluetoothState {
    BluetoothState {
        radio: u32::from(bluetooth.state == RadioState::On),
        max_links: u32::from(bluetooth.max_links),
    }
}

/// `dev_bonded.kind`: 0 other, 1 keyboard, 2 remote, 3 sensor, 4 phone.
#[must_use]
pub fn device_kind_code(kind: DeviceKind) -> u32 {
    match kind {
        DeviceKind::Keyboard => 1,
        DeviceKind::Remote => 2,
        DeviceKind::Sensor => 3,
        DeviceKind::Phone => 4,
        _ => 0,
    }
}

/// One row of op 11 from a device the kernel keeps bonded. `given` is the
/// first plugin it is given to (op 11 has room for one).
#[must_use]
pub fn bonded_row(bonded: &paperpaw_sdk::bluetooth::Bonded) -> Bonded<'_> {
    Bonded {
        device: bonded.id.number(),
        name: &bonded.name,
        kind: device_kind_code(bonded.kind),
        connected: bonded.connected,
        remote_keys: bonded.keys,
        given: bonded.given_plugins().first().map_or("", |id| id.as_str()),
    }
}

#[cfg(test)]
mod tests {
    use paperpaw_core::native_host::device_service::{DeviceHost, check_caps_agree};
    use paperpaw_sdk::memory::MemoryRegion;
    use paperpaw_sdk::native::SafeMode;

    use super::*;
    use crate::device_host::{Identity, X4Device};

    #[test]
    fn caps_follow_the_build() {
        let x4 = caps_for(&Features {
            wifi: true,
            button_row: true,
            firmware: true,
            ..Features::default()
        });
        for bit in [
            caps::NETWORK,
            caps::WIFI,
            caps::SLEEP,
            caps::ROTATION,
            caps::POWER_OFF,
            caps::BLACK_AND_WHITE,
            caps::BUTTON_ROW,
            caps::FIRMWARE,
        ] {
            assert_ne!(x4 & bit, 0, "{bit:#x}");
        }
        for bit in [caps::TOUCH, caps::BLUETOOTH, caps::FRONTLIGHT] {
            assert_eq!(x4 & bit, 0, "{bit:#x}");
        }
        let pro = caps_for(&Features {
            touch: true,
            frontlight: true,
            bluetooth: true,
            firmware: true,
            ..Features::default()
        });
        for bit in [caps::TOUCH, caps::FRONTLIGHT, caps::BLUETOOTH] {
            assert_ne!(pro & bit, 0, "{bit:#x}");
        }
        assert_eq!(pro & caps::BUTTON_ROW, 0);
        assert_eq!(x4 & !caps::CAPS_MASK, 0);
        // A device that cannot run PaperPaw's updates has no firmware bit.
        assert_eq!(caps_for(&Features::default()) & caps::FIRMWARE, 0);
    }

    /// The controls are the ones the kernel is built with on each device.
    #[test]
    fn the_controls_are_the_kernels() {
        assert_eq!(
            controls_bits(&crate::controls::power_controls(false)),
            0b0100
        );
        assert_eq!(
            controls_bits(&crate::controls::power_controls(true)),
            0b1100
        );
        assert_eq!(controls_bits(&PowerControls::default()), 0);
    }

    #[test]
    fn the_memory_class_and_the_start_have_their_codes() {
        assert_eq!(class_of(MemoryClass::Small), 0);
        assert_eq!(class_of(MemoryClass::Large), 2);
        assert_eq!(start_code(Start::Unknown), 0);
        assert_eq!(start_code(Start::PowerOn), 1);
        assert_eq!(start_code(Start::Wake), 2);
        assert_eq!(start_code(Start::Restart), 3);
    }

    #[test]
    fn the_memory_reading_gives_what_it_knows() {
        let mut used = MemoryUse::new(100, 400)
            .with_stack(10, 20)
            .with_largest_free(60);
        let facts = memory_facts(&used);
        assert_eq!(
            (
                facts.heap_free,
                facts.heap_size,
                facts.largest,
                facts.stack,
                facts.external
            ),
            (100, 400, Some(60), Some((10, 20)), None)
        );
        assert!(!facts.cached);
        let bare = memory_facts(&MemoryUse::new(1, 2));
        assert_eq!((bare.largest, bare.stack), (None, None));
        used = used
            .with_region(MemoryRegion::new(50, 100))
            .with_region(MemoryRegion::new(7_000_000, 8_000_000).external());
        assert_eq!(memory_facts(&used).external, Some((7_000_000, 8_000_000)));
    }

    #[test]
    fn what_code_plugins_stand_on_comes_from_the_kernels_record() {
        let plain = Native::new("code.ppn");
        let state = native_state(&plain, 5, 2, (100, 60), "code.ppn");
        assert_eq!((state.safe_mode, state.pending, state.target), (0, 2, 5));
        assert_eq!((state.room_free, state.room_largest), (100, 60));
        assert_eq!(
            (state.crashed.as_str(), state.last_loaded.as_str()),
            ("", "")
        );
        let mut crashed = Native::new("code.ppn");
        let mut mode = SafeMode::new(SafeModeReason::PowerEscape);
        mode.crashed = Some("stats".into());
        mode.last_loaded = Some("widgets".into());
        crashed.safe_mode = Some(mode);
        let state = native_state(&crashed, 9, 0, (0, 0), "code.ppn");
        assert_eq!(state.safe_mode, 1);
        assert_eq!(
            (state.crashed.as_str(), state.last_loaded.as_str()),
            ("stats", "widgets")
        );
        let mut held = Native::new("code.ppn");
        held.safe_mode = Some(SafeMode::new(SafeModeReason::KeyHeld));
        assert_eq!(safe_mode_code(&held), 2);
        held.safe_mode = Some(SafeMode::new(SafeModeReason::FailedStarts));
        assert_eq!(safe_mode_code(&held), 1);
    }

    #[test]
    fn the_code_room_is_what_the_loader_left_free() {
        assert_eq!(code_room(&CodeRoom::new(1_000, 300, 4_096)), (700, 700));
        assert_eq!(code_room(&CodeRoom::new(100, 300, 4_096)), (0, 0));
    }

    #[test]
    fn not_installed_reasons_have_their_codes() {
        assert_eq!(not_installed_reason(NotInstalledReason::OtherAuthor), 1);
        assert_eq!(not_installed_reason(NotInstalledReason::Damaged), 2);
    }

    /// A device made the way the firmware makes it (with the Wi-Fi, firmware
    /// and Bluetooth parts set for the build) agrees with its own caps.
    #[test]
    fn the_firmwares_caps_agree_with_the_parts_it_sets() {
        use paperpaw_platform::firmware::Running;

        use crate::device_host::{BluetoothState, WifiState};
        let features = Features {
            wifi: true,
            button_row: true,
            bluetooth: true,
            firmware: true,
            ..Features::default()
        };
        let device = X4Device::new(
            Identity {
                id: "xteink-x4",
                model: "Xteink X4",
                catalog: "x4",
                class: class_of(MemoryClass::Small),
                caps: caps_for(&features),
                controls: controls_bits(&crate::controls::power_controls(false)),
                tls_sessions: 1,
                keys: &["back"],
                values: &[],
            },
            &[],
        );
        assert!(check_caps_agree(&device).is_err());
        device.set_wifi(WifiState {
            state: 0,
            ssid: Text::new(""),
            address: Text::new(""),
            host_name: Text::new(""),
            guests: 0,
            scanning: false,
            wanted: false,
            busy: false,
        });
        device.set_firmware(&Running::new("0.6.0", "paperpaw-xteink", 5, 0, false));
        device.set_bluetooth(BluetoothState {
            radio: 0,
            max_links: 2,
        });
        assert_eq!(check_caps_agree(&device), Ok(()));
        let mut seen = None;
        device
            .device(&mut |facts| seen = Some((facts.class, facts.controls)))
            .unwrap();
        assert_eq!(seen, Some((0, 0b0100)));
    }

    #[test]
    fn wifi_states_have_the_codes_of_device_md() {
        let mut wifi = Wifi::default();
        assert_eq!(wifi_facts(&wifi).state, 0);
        wifi.state = Radio::On;
        wifi.scanning = true;
        wifi.busy = true;
        let facts = wifi_facts(&wifi);
        assert_eq!(
            (facts.state, facts.scanning, facts.wanted, facts.busy),
            (1, true, false, true)
        );
        wifi.state = Radio::Joining {
            ssid: "home".into(),
        };
        assert_eq!(wifi_facts(&wifi).state, 2);
        wifi.state = Radio::Online {
            ssid: "home".into(),
            address: "192.168.1.20".into(),
            name: Some("paperpaw.local".into()),
        };
        let facts = wifi_facts(&wifi);
        assert_eq!(facts.state, 3);
        assert_eq!(facts.ssid.as_str(), "home");
        assert_eq!(facts.address.as_str(), "192.168.1.20");
        assert_eq!(facts.host_name.as_str(), "paperpaw.local");
        wifi.state = Radio::Hosting {
            ssid: "paperpaw".into(),
            address: "192.168.4.1".into(),
            guests: 2,
        };
        let facts = wifi_facts(&wifi);
        assert_eq!(
            (facts.state, facts.guests, facts.host_name.as_str()),
            (4, 2, "")
        );
    }

    #[test]
    fn bluetooth_facts_follow_the_radio_and_bonds_keep_their_flags() {
        use paperpaw_sdk::bluetooth::{Bonded as Sdk, DeviceId};
        let mut bluetooth = Bluetooth::new(2);
        assert_eq!(bluetooth_facts(&bluetooth).radio, 0);
        assert_eq!(bluetooth_facts(&bluetooth).max_links, 2);
        bluetooth.state = RadioState::On;
        assert_eq!(bluetooth_facts(&bluetooth).radio, 1);
        let mut remote = Sdk::new(DeviceId::new(7), "Page turner", DeviceKind::Remote);
        remote.connected = true;
        remote.keys = true;
        let row = bonded_row(&remote);
        assert_eq!(
            (
                row.device,
                row.name,
                row.kind,
                row.connected,
                row.remote_keys,
                row.given
            ),
            (7, "Page turner", 2, true, true, "")
        );
        assert_eq!(device_kind_code(DeviceKind::Keyboard), 1);
        assert_eq!(device_kind_code(DeviceKind::Sensor), 3);
        assert_eq!(device_kind_code(DeviceKind::Phone), 4);
        assert_eq!(device_kind_code(DeviceKind::Other), 0);
    }

    /// A device whose Wi-Fi and Bluetooth parts are not told to it yet says so
    /// in its caps, on every X4 build; a bit without its part is what the
    /// check catches.
    #[test]
    fn the_caps_of_the_shells_devices_agree_with_what_it_tells_them() {
        use paperpaw_platform::firmware::Running;
        let builds = [
            Features {
                button_row: true,
                firmware: true,
                ..Features::default()
            },
            Features {
                touch: true,
                frontlight: true,
                firmware: true,
                ..Features::default()
            },
            Features {
                button_row: true,
                ..Features::default()
            },
        ];
        for features in builds {
            let device = X4Device::new(
                Identity {
                    id: "xteink-x4",
                    model: "Xteink X4",
                    catalog: "x4",
                    class: class_of(MemoryClass::Small),
                    caps: caps_for(&features),
                    controls: 0,
                    tls_sessions: 1,
                    keys: &[],
                    values: &[],
                },
                &[],
            );
            if features.firmware {
                device.set_firmware(&Running::new("0.6.0", "paperpaw-xteink", 5, 0, false));
            }
            assert_eq!(check_caps_agree(&device), Ok(()), "{features:?}");
            // The bit before its part is a disagreement.
            let early = X4Device::new(
                Identity {
                    id: "xteink-x4",
                    model: "Xteink X4",
                    catalog: "x4",
                    class: 0,
                    caps: caps_for(&Features {
                        wifi: true,
                        ..features
                    }),
                    controls: 0,
                    tls_sessions: 1,
                    keys: &[],
                    values: &[],
                },
                &[],
            );
            assert!(check_caps_agree(&early).is_err());
        }
    }
}
