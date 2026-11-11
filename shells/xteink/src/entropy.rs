//! Whether the chip's random numbers are random now. The generator mixes
//! physical noise into its state only while the radio (Wi-Fi or Bluetooth) is
//! on or an ADC feeds it (`esp_hal::rng`); the X4's ADC reads its buttons, so
//! only the radio can. Otherwise its numbers are pseudo-random, which a key or
//! a nonce for `core.secrets` must not be made of. The radio's start and stop
//! tell this module (`wifi`, `bluetooth`); nothing here polls.

use core::sync::atomic::{AtomicBool, Ordering};

static WIFI: AtomicBool = AtomicBool::new(false);
// Read by `strong` (ppn5); set where Bluetooth is built.
#[cfg_attr(not(feature = "ppn5"), allow(dead_code))]
static BLUETOOTH: AtomicBool = AtomicBool::new(false);

/// The Wi-Fi radio is on (`true`) or off.
pub fn wifi(on: bool) {
    WIFI.store(on, Ordering::Release);
}

/// The Bluetooth controller is on (`true`) or off.
#[cfg(feature = "bluetooth")]
pub fn bluetooth(on: bool) {
    BLUETOOTH.store(on, Ordering::Release);
}

/// A radio is on: the generator's numbers are random now.
#[cfg(feature = "ppn5")]
pub fn strong() -> bool {
    WIFI.load(Ordering::Acquire) || BLUETOOTH.load(Ordering::Acquire)
}
