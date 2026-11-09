//! `core.device` on the X4 (interface 7 builds): the device the kernel's host
//! services answer from, made once at start with what is fixed for this
//! boot. What changes while it runs is told to it by the main loop.
//!
//! The mappings from the firmware's types to what plugins read are
//! `paperpaw_x4::device_facts`, tested on a computer; this is the calls.

use alloc::rc::Rc;
use paperpaw_core::native_host::device_service::{DEVICE_SERVICES, MemoryFacts};
use paperpaw_core::native_host::shell::DATA_SERVICES;
use paperpaw_native::metadata::Service;
use paperpaw_platform::firmware::Running;
use paperpaw_sdk::native::Native;
use paperpaw_sdk::power::Start;
use paperpaw_x4::device_facts::{
    Features, caps_for, class_of, controls_bits, memory_facts, native_state, start_code,
};
use paperpaw_x4::device_host::{Identity, X4Device};

/// The readings op 2 lists: the USB lines' (diagnostics), and the Pro's light.
#[cfg(feature = "x4-pro")]
const VALUES: &[&str] = &["usb-pin-high", "usb-frames", "frontlight-percent"];
#[cfg(not(feature = "x4-pro"))]
const VALUES: &[&str] = &["usb-pin-high", "usb-frames"];

/// The store's name for this device.
#[cfg(feature = "x4-pro")]
const CATALOG: &str = "x4-pro";
#[cfg(feature = "x4-classic")]
const CATALOG: &str = "x4-classic";
#[cfg(not(any(feature = "x4-pro", feature = "x4-classic")))]
const CATALOG: &str = "x4";

/// The chip id a code plugin is built for (5 ESP32-C3, 9 ESP32-S3).
const TARGET: u32 = if cfg!(feature = "esp32s3") { 9 } else { 5 };

/// The heap and the stack as op 7 gives them.
fn memory_probe() -> Option<MemoryFacts> {
    crate::memory().map(|used| memory_facts(&used))
}

/// The clock the memory probe's cache goes by.
fn clock_ms() -> u64 {
    crate::micros() / 1000
}

/// The device for this start. `running` is the firmware's own facts where the
/// updates run (no firmware bit in caps where they do not), `native` what code
/// plugins stand on at this start, `start` how the start came about.
#[inline(never)]
pub fn make(running: Option<&Running<'_>>, native: &Native, start: Start) -> Rc<X4Device> {
    let features = Features {
        touch: crate::DEVICE.input.touch,
        button_row: cfg!(any(feature = "x4", feature = "x4-classic", feature = "x3")),
        bluetooth: cfg!(feature = "bluetooth"),
        frontlight: cfg!(feature = "x4-pro"),
        firmware: running.is_some(),
    };
    let identity = Identity {
        id: crate::DEVICE.id,
        model: crate::DEVICE.name,
        catalog: CATALOG,
        class: class_of(crate::DEVICE.memory),
        caps: caps_for(&features),
        controls: controls_bits(&crate::power_controls()),
        tls_sessions: u32::from(crate::tls_slots::SESSIONS),
        keys: crate::DEVICE.input.keys,
        values: VALUES,
    };
    let device = X4Device::shared(identity, &crate::ota::PLUGINS);
    device.set_start(start_code(start));
    if let Some(running) = running {
        device.set_firmware(running);
    }
    // The code plugins this start stood on; the room is told once the loader is done.
    // Plugins that take effect only at the next start are the core's to count
    // (it holds the registry): none are the shell's.
    tell_native(&device, native);
    let rows: alloc::vec::Vec<(&str, u32)> = native
        .not_installed
        .iter()
        .map(|row| {
            (
                row.plugin.as_str(),
                paperpaw_x4::device_facts::not_installed_reason(row.reason),
            )
        })
        .collect();
    if !device.set_not_installed(&rows) {
        log::warn!("device: the list of plugins not installed could not be kept");
    }
    device.set_memory_probe(memory_probe, clock_ms);
    device
}

/// The services offered to admission: the data services and `core.device`
/// (this host answers it), built once at compile time.
pub static OFFERED: [Service<'static>; DATA_SERVICES.len() + DEVICE_SERVICES.len()] = {
    let mut all = [DATA_SERVICES[0]; DATA_SERVICES.len() + DEVICE_SERVICES.len()];
    let mut at = 0;
    while at < DATA_SERVICES.len() {
        all[at] = DATA_SERVICES[at];
        at += 1;
    }
    let mut more = 0;
    while more < DEVICE_SERVICES.len() {
        all[at + more] = DEVICE_SERVICES[more];
        more += 1;
    }
    all
};

/// Tells the device what code plugins stand on after the start chose safe
/// mode (the loader is not made then, so the room stays 0).
pub fn tell_native(device: &X4Device, native: &Native) {
    device.set_native(native_state(native, TARGET, 0, (0, 0), "code.ppn"));
}

/// Tells the device what the kernel keeps of Wi-Fi, Bluetooth and the power,
/// as plugins see it: what the kernel changes, the device follows. Called
/// where the kernel was just stepped (never on a timer), so a plugin that
/// hears of a change reads it. Copies and compares small values; allocates
/// only where a list changed.
#[inline(never)]
pub fn sync(device: &X4Device, kernel: &paperpaw_core::Kernel) {
    if let Some(wifi) = kernel.wifi_model() {
        device.set_wifi(paperpaw_x4::device_facts::wifi_facts(wifi));
    }
    #[cfg(feature = "bluetooth")]
    if let Some(bluetooth) = kernel.bluetooth_model() {
        device.set_bluetooth(paperpaw_x4::device_facts::bluetooth_facts(bluetooth));
        device.set_bonded_from(
            bluetooth
                .bonded_devices()
                .iter()
                .map(paperpaw_x4::device_facts::bonded_row),
        );
    }
    device.sync_power(kernel.power());
}

/// Tells the device the networks the last scan found (the kernel's list: the
/// strongest first, each name once). The rows are borrowed from the kernel;
/// the list that is made for them is small and fallible.
#[inline(never)]
pub fn sync_networks(device: &X4Device, kernel: &paperpaw_core::Kernel) {
    let Some(wifi) = kernel.wifi_model() else {
        return;
    };
    let mut rows: alloc::vec::Vec<(&str, i32, bool)> = alloc::vec::Vec::new();
    if rows.try_reserve_exact(wifi.networks.len()).is_err() {
        log::warn!("device: the networks found could not be kept");
        return;
    }
    rows.extend(
        wifi.networks
            .iter()
            .map(|network| (network.ssid.as_str(), i32::from(network.signal), network.secured)),
    );
    device.set_networks(&rows);
}

/// The USB lines' readings, as they go to the kernel's status.
pub fn tell_usb(device: &X4Device, pin_high: Option<bool>, frames: Option<bool>) {
    device.set_value("usb-pin-high", pin_high.map(i64::from));
    device.set_value("usb-frames", frames.map(i64::from));
}
