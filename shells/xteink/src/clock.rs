//! The time of day: set over the network (SNTP, by `network` once the
//! radio is online) and then counted on with the processor's timer, which
//! runs through light sleep. The X4 has no clock that runs while it is
//! off: its time is gone then, until the network sets it again. The X4
//! Pro's clock chip keeps it: read at start (`main`), and set whenever the
//! network sets the time (`input::keep_time`).
//!
//! The SNTP messages are `paperpaw_x4::sntp`.

use paperpaw_platform::{Instant, WallClock};

/// Milliseconds since 1970 (UTC) minus the timer's milliseconds, once
/// set; and whether it was set since last asked.
static mut BASE: (Option<i64>, bool) = (None, false);

fn base() -> Option<i64> {
    // SAFETY: one core, used from the main loop only.
    unsafe { core::ptr::addr_of!(BASE).read().0 }
}

/// Sets the time: `unix_ms` at the timer's `now`.
pub fn set(now: Instant, unix_ms: i64) {
    // SAFETY: as in `base`.
    unsafe { core::ptr::addr_of_mut!(BASE).write((Some(unix_ms - now.0 as i64), true)) };
}

/// Whether the time was set since this was last asked (for the status
/// to be passed on at once).
pub fn take_changed() -> bool {
    // SAFETY: as in `base`.
    unsafe {
        let base = &mut *core::ptr::addr_of_mut!(BASE);
        core::mem::take(&mut base.1)
    }
}

/// Milliseconds since 1970 (UTC) at the timer's `now`, once known.
pub fn unix_ms(now: Instant) -> Option<i64> {
    base().map(|base| base + now.0 as i64)
}

/// The wall clock for the kernel's status (its time zone is the user's
/// setting: the X4 has none of its own).
pub fn wall(now: Instant) -> Option<WallClock> {
    unix_ms(now).map(|unix_millis| WallClock::new(now, unix_millis, None))
}
