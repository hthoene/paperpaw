//! The time of day: set over the network (SNTP, by `network` once the
//! radio is online) and then counted on with the processor's timer, which
//! runs through light sleep. The X4 has no clock that runs while it is
//! off: its time is gone then, until the network sets it again, but the
//! RTC timer and its store registers go on through a restart and a deep
//! sleep, and the X4 keeps the time there (`paperpaw_x4::kept_time`, #280).
//! The X4 Pro's clock chip keeps it: read at start (`main`), and set
//! whenever the network sets the time (`input::keep_time`).
//!
//! The SNTP messages are `paperpaw_x4::sntp`.

use paperpaw_platform::{Instant, WallClock};
use paperpaw_x4::kept_time::Clock;

/// The time of day (one core, used from the main loop only).
static mut CLOCK: Clock = Clock::new();

fn clock() -> &'static mut Clock {
    // SAFETY: one core, used from the main loop only.
    unsafe { &mut *core::ptr::addr_of_mut!(CLOCK) }
}

/// The RTC, taken for a moment: it keeps no state of its own, the timer
/// and the store registers are the chip's.
#[cfg(feature = "x4")]
fn rtc() -> esp_hal::rtc_cntl::Rtc<'static> {
    // SAFETY: the RTC timer is only read here and by `power`, which holds
    // its own copy for the watchdogs; neither keeps state in the handle.
    esp_hal::rtc_cntl::Rtc::new(unsafe { esp_hal::peripherals::RTC_TIMER::steal() })
}

/// Sets the time: `unix_ms` at the timer's `now`. The X4 (no clock chip)
/// keeps it in the RTC as well, for a restart or a deep sleep to read
/// (`restore`): two registers, no card write.
pub fn set(now: Instant, unix_ms: i64) {
    clock().set(now.0, unix_ms);
    #[cfg(feature = "x4")]
    if let Some(us) = paperpaw_x4::kept_time::keep(unix_ms) {
        rtc().set_current_time_us(us);
    }
}

/// At start, on the X4: takes the time the RTC kept through the restart
/// (approximate, see `paperpaw_x4::kept_time`), if it holds one.
#[cfg(feature = "x4")]
pub fn restore(now: Instant) {
    let rtc_us = rtc().current_time_us();
    if clock().restore(now.0, rtc_us) {
        log::info!("clock: kept through the restart (approximate)");
    }
}

/// Whether the time was set since this was last asked (for the status
/// to be passed on at once).
pub fn take_changed() -> bool {
    clock().take_changed()
}

/// Milliseconds since 1970 (UTC) at the timer's `now`, once known
/// (approximate after a restart, until the network sets it).
pub fn unix_ms(now: Instant) -> Option<i64> {
    clock().unix_ms(now.0)
}

/// Whether the time was set in this run (by the network, the remote tool or
/// the clock chip), not just kept through a restart: what secure requests
/// wait for.
pub fn exact() -> bool {
    clock().exact()
}

/// Whether the time is one the RTC kept through a restart and nothing has
/// set since.
#[cfg(feature = "remote")]
pub fn approximate() -> bool {
    clock().approximate()
}

/// The wall clock for the kernel's status (its time zone is the user's
/// setting: the X4 has none of its own).
pub fn wall(now: Instant) -> Option<WallClock> {
    unix_ms(now).map(|unix_millis| WallClock::new(now, unix_millis, None))
}
