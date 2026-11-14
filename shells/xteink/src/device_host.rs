//! `core.device` on the X4 (interface 7 builds): the device the kernel's host
//! services answer from, made once at start with what is fixed for this
//! boot. What changes while it runs is told to it by the main loop.
//!
//! The mappings from the firmware's types to what plugins read are
//! `paperpaw_x4::device_facts`, tested on a computer; this is the calls.

use alloc::rc::Rc;
use paperpaw_core::native_host::device_service::{MAX_NOT_INSTALLED, MemoryFacts};
use paperpaw_platform::firmware::Running;
use paperpaw_sdk::native::{NotInstalled, SafeMode};
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
pub fn make(
    running: Option<&Running<'_>>,
    not_installed: &[NotInstalled],
    start: Start,
) -> Rc<X4Device> {
    let features = Features {
        touch: crate::DEVICE.input.touch,
        button_row: cfg!(any(feature = "x4", feature = "x4-classic", feature = "x3")),
        // A bit goes with its part ([`present`], called as soon as the kernel
        // exists), or ops 8 to 11 answer "not found" under a caps bit that
        // says they are there.
        wifi: true,
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
    tell_native(&device, None);
    // From the card: at most what op 14 lists is copied, and a copy that
    // finds no room leaves the list empty (the start goes on).
    let listed = &not_installed[..not_installed.len().min(MAX_NOT_INSTALLED as usize)];
    let mut rows: alloc::vec::Vec<(&str, u32)> = alloc::vec::Vec::new();
    if rows.try_reserve_exact(listed.len()).is_ok() {
        rows.extend(listed.iter().map(|row| {
            (
                row.plugin.as_str(),
                paperpaw_x4::device_facts::not_installed_reason(row.reason),
            )
        }));
        if !device.set_not_installed(&rows) {
            log::warn!("device: the list of plugins not installed could not be kept");
        }
    } else {
        log::warn!("device: no room for the list of plugins not installed");
    }
    device.set_memory_probe(memory_probe, clock_ms);
    device
}

/// Which of the two families the loader wired this start. Reset and written
/// by the loader's `prepare` hook (before the first plugin is admitted) and
/// read at each plugin's admission, in the same task: plain loads and stores (no read-modify-write, which the X4's
/// core does not have).
static WIRED: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);
const WIRED_WORK: u8 = 1;
const WIRED_CARD: u8 = 2;
const WIRED_SECRETS: u8 = 4;
const WIRED_LOOKUP: u8 = 8;

/// Forgets what was wired, before a start wires again.
pub fn reset_wired() {
    WIRED.store(0, core::sync::atomic::Ordering::Relaxed);
}

/// Records that the wiring of `core.work` (with the network) succeeded.
pub fn wired_work() {
    use core::sync::atomic::Ordering::Relaxed;
    WIRED.store(WIRED.load(Relaxed) | WIRED_WORK, Relaxed);
}

/// Records that the wiring of `core.document`, `core.library` and
/// `core.files` succeeded.
pub fn wired_card() {
    use core::sync::atomic::Ordering::Relaxed;
    WIRED.store(WIRED.load(Relaxed) | WIRED_CARD, Relaxed);
}

/// Records that the wiring of `core.secrets` (the device's sealer, or the one
/// that does not seal yet) succeeded.
pub fn wired_secrets() {
    use core::sync::atomic::Ordering::Relaxed;
    WIRED.store(WIRED.load(Relaxed) | WIRED_SECRETS, Relaxed);
}

#[cfg(feature = "native-on")]
/// Records that the wiring of `core.lookup` (the dictionaries' query service,
/// kept in the host's table; it holds nothing of its own) succeeded.
pub fn wired_lookup() {
    use core::sync::atomic::Ordering::Relaxed;
    WIRED.store(WIRED.load(Relaxed) | WIRED_LOOKUP, Relaxed);
}

/// What this start wired, as the lists a linked plugin and a card plugin are
/// offered are made (`paperpaw_x4::native_offer`): `core.work` with `core.net` and
/// `core.sockets` (one wiring), the card's three, and `core.secrets`: the groups
/// the lists are kept in, static. `wifi`:
/// the kernel has a Wi-Fi model.
pub fn wired_families(wifi: bool) -> paperpaw_x4::native_offer::Wired {
    use core::sync::atomic::Ordering::Relaxed;
    let wired = WIRED.load(Relaxed);
    let (work, card) = (wired & WIRED_WORK != 0, wired & WIRED_CARD != 0);
    paperpaw_x4::native_offer::Wired {
        work,
        card,
        secrets: wired & WIRED_SECRETS != 0,
        lookup: wired & WIRED_LOOKUP != 0,
        wifi,
    }
}

/// Tells the device what code plugins stand on: the safe mode this start is
/// in, if it is (no loader is made then, so the room stays 0). The plugins
/// not installed are not part of it: op 14 has them ([`make`]).
pub fn tell_native(device: &X4Device, safe_mode: Option<&SafeMode>) {
    let mut native = crate::native::this_firmware();
    native.safe_mode = safe_mode.cloned();
    device.set_native(native_state(&native, TARGET, 0, (0, 0), "code.ppn"));
}

/// Tells the device which radios this kernel has, once at start (its parts
/// are then there, as its caps say). What the radios do is not kept here: the
/// kernel answers `core.device` ops 8, 10, 11 and 12 from its own models
/// (`Env::wifi`, `Env::bluetooth`, `Env::power`), also in the middle of a
/// delivery pass, so no second copy is kept in step. Op 9 (the networks) is
/// this shell's: [`sync_networks`].
#[inline(never)]
pub fn present(device: &X4Device, kernel: &paperpaw_core::Kernel) {
    let bluetooth = cfg!(feature = "bluetooth") && kernel.bluetooth_model().is_some();
    device.set_radios(kernel.wifi_model().is_some(), bluetooth);
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
    rows.extend(wifi.networks.iter().map(|network| {
        (
            network.ssid.as_str(),
            i32::from(network.signal),
            network.secured,
        )
    }));
    if !device.set_networks(&rows) {
        // No room, or a plugin is reading the list: op 9 keeps the old one
        // until the next scan.
        log::warn!("device: the networks a scan found could not be kept");
    }
}

/// The USB lines' readings, as they go to the kernel's status.
pub fn tell_usb(device: &X4Device, pin_high: Option<bool>, frames: Option<bool>) {
    device.set_value("usb-pin-high", pin_high.map(i64::from));
    device.set_value("usb-frames", frames.map(i64::from));
}

/// The caps say what is answered, checked the way `core.device` answers (the
/// kernel's models over this device): said in the log where they do not
/// (called once, after [`present`]).
pub fn check(device: &X4Device, kernel: &paperpaw_core::Kernel) {
    if let Err(why) = kernel.check_device_host(device) {
        log::error!("device: its caps and its parts disagree: {why}");
    }
}
