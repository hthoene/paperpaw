//! Whether the chip's random numbers are random now. The generator mixes
//! physical noise into its state only while the radio (Wi-Fi or Bluetooth) is
//! on or an ADC feeds it (`esp_hal::rng`); the X4's ADC reads its buttons, so
//! only the radio can. Otherwise its numbers are pseudo-random, which a key or
//! a nonce for `core.secrets` must not be made of. The radio's start and stop
//! tell this module (`wifi`, `bluetooth`); nothing here polls.

use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

static WIFI: AtomicBool = AtomicBool::new(false);
/// How often a radio came on (each radio's off-to-on edge), so the start-up
/// re-save is tried at most once per radio-on (Senior M1, #254). Plain loads
/// and stores: the C3 has no atomic read-modify-write, and only the main loop
/// starts and stops the radios.
static RADIO_ONS: AtomicU32 = AtomicU32::new(0);

/// Count an off-to-on edge of a radio.
fn came_on(was: bool, on: bool) {
    if on && !was {
        let ons = RADIO_ONS.load(Ordering::Acquire);
        RADIO_ONS.store(ons.wrapping_add(1), Ordering::Release);
    }
}

/// How often a radio came on since the start.
#[cfg(feature = "ppn5")]
pub fn radio_ons() -> u32 {
    RADIO_ONS.load(Ordering::Acquire)
}
// Read by `strong` (ppn5); set where Bluetooth is built.
#[cfg_attr(not(feature = "ppn5"), allow(dead_code))]
static BLUETOOTH: AtomicBool = AtomicBool::new(false);

/// The Wi-Fi radio is on (`true`) or off.
pub fn wifi(on: bool) {
    came_on(WIFI.load(Ordering::Acquire), on);
    WIFI.store(on, Ordering::Release);
}

/// The Bluetooth controller is on (`true`) or off.
#[cfg(feature = "bluetooth")]
pub fn bluetooth(on: bool) {
    came_on(BLUETOOTH.load(Ordering::Acquire), on);
    BLUETOOTH.store(on, Ordering::Release);
}

/// A radio is on: the generator's numbers are random now.
#[cfg(feature = "ppn5")]
pub fn strong() -> bool {
    WIFI.load(Ordering::Acquire) || BLUETOOTH.load(Ordering::Acquire)
}
