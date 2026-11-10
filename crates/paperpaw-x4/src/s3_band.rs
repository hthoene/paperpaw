//! The band at the main stack's end on the ESP32-S3 (X4 Pro, X4 Classic): the
//! rules of `abi/heap-word.md` section 6 ("The ESP32-S3's band"), as pure
//! functions and one arming routine over a register block the tests fake.
//!
//! The S3 has neither the X4's store trigger over a 2 KiB region (its data
//! breakpoints reach 64 bytes) nor a usable stack-pointer monitor (the debug
//! assistant's is absent on the S3, and it would see the radio tasks' stacks).
//! The band is the **debug assistant's region monitor**: `ASSIST_DEBUG` DRAM0
//! area 0, write monitoring, which sees every CPU store into the range, the
//! window spills' `s32e` included, and never another task's stack.
//!
//! - **It is an interrupt, not an exception.** The store into the band
//!   completes and the interrupt follows a few instructions later; the band is
//!   spare stack, so that store harms nothing. The handler (the firmware's
//!   `stack.rs`) first stops the monitor ([`stop`]), then panics like the X4's
//!   band (crash slot, the frame owner blamed).
//! - **Window spills.** A windowed function stores nothing when it is entered
//!   (`entry` only moves `a1`). A chain of frames can walk the pointer through
//!   the band with no store, and the store that catches it is a spill into the
//!   base save area of the first frame whose pointer lies in the band. With 64
//!   address registers and `call8`, at most [`WINDOWS_BELOW`] frames run below
//!   the band before that spill, each under [`LIMIT`].
//! - **Rules.** [`LIMIT`] = [`BAND`] = 2,048 bytes, 16-aligned: every frame
//!   moves the pointer by less than `BAND - 16`, so some frame's pointer lands
//!   in `[band + 16, band + BAND]` (`a_frame_lands_in_the_band` is the test).
//!   [`RESERVE`] = [`WINDOWS_BELOW`] × [`LIMIT`] + [`DEPTH`] sits between the
//!   stack's end and the band: 14,336 bytes, 16 KiB with the band ([`SPARE`]).
//! - **Armed** means the monitor's bounds and enables read back as set and a
//!   self-test passed ([`arm`]): with its interrupt not yet enabled, one store
//!   into the band sets the monitor's raw bit, which is then cleared and the
//!   interrupt enabled. The loader refuses every format-5 file while it is
//!   not.
//! - **Interrupt level.** The host never runs plugin code at an interrupt
//!   level of 3 or more (the band's interrupt runs at 3): [`dispatch_allowed`]
//!   is the check on `PS.INTLEVEL`.
//!
//! What no test here can say (the device checklist S1 to S7 does): whether the
//! monitor's maximum is inclusive, the store-to-interrupt latency, the
//! handler's real depth. [`bounds`] puts the maximum on the band's last byte,
//! which is right whether the chip compares inclusively (the last word's
//! address is below it) or not (that word's first byte is still below it).

/// The largest frame a plugin's code or the firmware on a plugin's call chain
/// may have, plus one: the `entry` operand plus the 16-byte base save area is
/// under this.
pub const LIMIT: u32 = 2048;

/// The band's height, 16-aligned (no power of two is needed).
pub const BAND: u32 = 2048;

/// The most frames that run below the band before a window spill reaches it:
/// 64 address registers and `call8` keep at most 7 windows live; the caller and
/// the frame in the band take 2.
pub const WINDOWS_BELOW: u32 = 6;

/// The stack the exception handler needs below the band, the S3's exception
/// frame (256 bytes plus the spills) included, with a margin of 256 bytes:
/// assumed until the device measures it (checklist S3).
pub const HANDLER: u32 = 2048;

/// The depth an interrupt takes when it arrives near the band, assumed likewise.
pub const INTERRUPT: u32 = 2048;

/// The larger of [`HANDLER`] and [`INTERRUPT`].
pub const DEPTH: u32 = if HANDLER > INTERRUPT {
    HANDLER
} else {
    INTERRUPT
};

/// The room kept between the stack's end and the band.
pub const RESERVE: u32 = WINDOWS_BELOW * LIMIT + DEPTH;

/// What the band and its reserve take of the stack: 16 KiB, out of the S3's
/// internal heap (`HEAP_SIZE` is smaller by as much).
pub const SPARE: u32 = RESERVE + BAND;

/// The stack's size the S3 links with where the band is built in: the
/// deepest legitimate path measured (some 33 KiB, ADR 0017) plus [`SPARE`],
/// about 50 KiB.
pub const STACK_FLOOR: u32 = 50 * 1024;

/// The stack must reach this far above the band, or there is none (a stack
/// that small is no design this guards).
const ABOVE: u32 = 4 * 1024;

/// How many times the self-test reads the raw bit for the interrupt that
/// follows the store by a few instructions: far more than that, far less than
/// a millisecond (a polled register read takes tens of cycles).
pub const POLLS: u32 = 1000;

/// The band `[start, start + BAND)` for a stack from `end` (lowest address) to
/// `top` (where it starts): the first 16-aligned address at or above
/// `end + RESERVE`. `None` where the stack has no room for it.
#[must_use]
pub fn band(end: u32, top: u32) -> Option<u32> {
    let from = end.checked_add(RESERVE)?;
    let start = from.checked_add(15)? & !15;
    let last = start.checked_add(BAND)?.checked_add(ABOVE)?;
    (last <= top).then_some(start)
}

/// The monitor's `(minimum, maximum)` for the band at `start`: the maximum is
/// the band's last byte (see the module's note on inclusiveness).
#[must_use]
pub const fn bounds(start: u32) -> (u32, u32) {
    (start, start + BAND - 1)
}

/// Whether plugin code may be entered with `ps` (the `PS` register): never at
/// an interrupt level of 3 or more, where the band's interrupt cannot fire. The
/// level is `PS.INTLEVEL`, its low four bits; a call at level 0 is the only one
/// the host makes, so anything else is refused.
#[must_use]
pub const fn dispatch_allowed(ps: u32) -> bool {
    ps & 0xf == 0
}

/// The debug assistant's DRAM0 area 0 as the arming routine needs it. The
/// firmware implements it over the registers, the tests with a fake.
pub trait Monitor {
    /// Writes the area's bounds.
    fn set_bounds(&mut self, min: u32, max: u32);
    /// Reads the bounds back.
    fn bounds(&self) -> (u32, u32);
    /// Switches the area's write monitoring.
    fn set_write(&mut self, on: bool);
    /// Reads it back.
    fn write_enabled(&self) -> bool;
    /// Switches the area's write interrupt.
    fn set_interrupt(&mut self, on: bool);
    /// Reads it back.
    fn interrupt_enabled(&self) -> bool;
    /// The raw (pending) bit of the area's write interrupt.
    fn raw(&self) -> bool;
    /// Clears the raw bit.
    fn clear_raw(&mut self);
    /// One store into the band, at `at` (the firmware writes the stack's paint
    /// there: what the word holds already, the band being unused stack).
    fn probe_store(&mut self, at: u32);
}

/// Why the band did not arm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ArmError {
    /// The bounds or the enable did not read back as written.
    Readback,
    /// The raw bit was set before the self-test's store, and did not clear.
    RawStuck,
    /// The self-test's store did not set the raw bit.
    NoFire,
    /// The interrupt enable did not read back as set.
    InterruptNotSet,
    /// A store into the band set the raw bit but its interrupt was not
    /// delivered to the handler (the interrupt's mapping or level is wrong).
    NotDelivered,
}

/// Stops the monitor: no interrupt, no write monitoring, the raw bit cleared.
/// The handler does this first, so that the panic that follows is not cut off
/// by the next store.
pub fn stop<M: Monitor>(monitor: &mut M) {
    monitor.set_interrupt(false);
    monitor.set_write(false);
    monitor.clear_raw();
}

/// Arms the band at `start`: bounds, write monitoring, read back; the
/// self-test (one store into the band sets the raw bit, with the interrupt
/// still off); the raw bit cleared; the interrupt enabled and read back. The
/// monitor is left stopped on any failure. Never a panic.
///
/// # Errors
/// Why it did not arm, see [`ArmError`].
pub fn arm<M: Monitor>(monitor: &mut M, start: u32) -> Result<(), ArmError> {
    let (min, max) = bounds(start);
    stop(monitor);
    if monitor.raw() {
        return Err(ArmError::RawStuck);
    }
    monitor.set_bounds(min, max);
    monitor.set_write(true);
    if monitor.bounds() != (min, max) || !monitor.write_enabled() || monitor.interrupt_enabled() {
        stop(monitor);
        return Err(ArmError::Readback);
    }
    monitor.probe_store(start);
    if !(0..POLLS).any(|_| monitor.raw()) {
        stop(monitor);
        return Err(ArmError::NoFire);
    }
    monitor.clear_raw();
    if monitor.raw() {
        stop(monitor);
        return Err(ArmError::RawStuck);
    }
    monitor.set_interrupt(true);
    if !monitor.interrupt_enabled() {
        stop(monitor);
        return Err(ArmError::InterruptNotSet);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use core::cell::Cell;

    use super::*;

    /// A register block that behaves as the chip is documented to, with the
    /// failures a test asks for.
    #[derive(Default)]
    struct Fake {
        min: u32,
        max: u32,
        write: bool,
        interrupt: bool,
        raw: Cell<bool>,
        /// Reads of `raw` since the store, while its interrupt is pending.
        since: Cell<Option<u32>>,
        /// The reads of `raw` after a store before it shows (the latency).
        latency: u32,
        // Faults.
        bounds_stuck: bool,
        write_stuck: bool,
        never_fires: bool,
        raw_sticks: bool,
        interrupt_stuck: bool,
        /// The raw bit was set before the routine ran.
        raw_at_start: Cell<bool>,
        stores: u32,
    }

    impl Monitor for Fake {
        fn set_bounds(&mut self, min: u32, max: u32) {
            if !self.bounds_stuck {
                (self.min, self.max) = (min, max);
            }
        }
        fn bounds(&self) -> (u32, u32) {
            (self.min, self.max)
        }
        fn set_write(&mut self, on: bool) {
            if !self.write_stuck {
                self.write = on;
            }
        }
        fn write_enabled(&self) -> bool {
            self.write
        }
        fn set_interrupt(&mut self, on: bool) {
            if !self.interrupt_stuck || !on {
                self.interrupt = on;
            }
        }
        fn interrupt_enabled(&self) -> bool {
            self.interrupt
        }
        fn raw(&self) -> bool {
            if let Some(reads) = self.since.get() {
                if reads >= self.latency {
                    self.raw.set(true);
                    self.since.set(None);
                } else {
                    self.since.set(Some(reads + 1));
                }
            }
            self.raw.get() || self.raw_at_start.get()
        }
        fn clear_raw(&mut self) {
            if !self.raw_sticks {
                self.raw.set(false);
                self.raw_at_start.set(false);
                self.since.set(None);
            }
        }
        fn probe_store(&mut self, at: u32) {
            self.stores += 1;
            if self.write && !self.never_fires && at >= self.min && at <= self.max {
                self.since.set(Some(0));
            }
        }
    }

    fn run(fake: &mut Fake, start: u32) -> Result<(), ArmError> {
        arm(fake, start)
    }

    const START: u32 = 0x3fc9_c000 + RESERVE;

    #[test]
    fn the_rules_are_the_rulings_numbers() {
        assert_eq!(LIMIT, 2048);
        assert_eq!(BAND, LIMIT);
        assert_eq!(WINDOWS_BELOW, 6);
        assert_eq!(DEPTH, HANDLER.max(INTERRUPT));
        assert_eq!(RESERVE, 6 * 2048 + 2048);
        assert_eq!(RESERVE, 14_336);
        assert_eq!(SPARE, 16 * 1024);
        assert_eq!(STACK_FLOOR, 50 * 1024);
        // The floor leaves the deepest legitimate path (some 33 KiB) above the band.
        const { assert!(STACK_FLOOR - SPARE >= 33 * 1024) };
    }

    /// The shell's link-time floor and the heap's give-back are numbers the
    /// shell's own files hold (it is a separate workspace and cannot use this
    /// module in its build script): they are read here, so a change of one is
    /// a change of both.
    #[test]
    fn the_shells_floor_is_this_floor() {
        let build = include_str!("../../../shells/xteink/build.rs");
        let line = alloc::format!(
            "const STACK_FLOOR_BAND: u32 = {} * 1024;",
            STACK_FLOOR / 1024
        );
        assert!(build.contains(&line), "build.rs: {line}");
    }

    #[test]
    fn the_band_is_the_first_16_aligned_address_a_reserve_above_the_end() {
        let top = 0x3fca_8000;
        assert_eq!(band(0x3fc9_c000, top), Some(0x3fc9_c000 + RESERVE));
        assert_eq!(band(0x3fc9_c001, top), Some(0x3fc9_c000 + RESERVE + 16));
        for end in (0x3fc9_c000u32..0x3fc9_c100).step_by(4) {
            let start = band(end, top).unwrap();
            assert_eq!(start % 16, 0);
            assert!(start >= end + RESERVE && start < end + RESERVE + 16);
        }
    }

    #[test]
    fn a_stack_without_room_has_no_band() {
        let end = 0x3fc9_c000;
        let start = end + RESERVE;
        assert_eq!(band(end, start + BAND + ABOVE - 1), None);
        assert_eq!(band(end, start + BAND + ABOVE), Some(start));
        assert_eq!(band(u32::MAX - 100, u32::MAX), None);
    }

    #[test]
    fn the_monitor_covers_the_band_and_not_a_byte_beyond() {
        let (min, max) = bounds(START);
        assert_eq!((min, max), (START, START + BAND - 1));
        // Every word store into the band is inside; the words next to it are not.
        assert!(
            (START..START + BAND)
                .step_by(4)
                .all(|at| at >= min && at <= max)
        );
        assert!(START - 4 < min);
        assert!(START + BAND > max);
    }

    /// Whatever the alignment and the frame sizes (16-aligned, each under the
    /// limit's 16 below the band), some frame's pointer lands in the band's
    /// upper part, so its base save area lies in the band.
    #[test]
    fn a_frame_lands_in_the_band() {
        let top = START + BAND;
        for above in (16..=(LIMIT - 16)).step_by(16) {
            for size in (16..=(BAND - 16)).step_by(16) {
                // The pointer comes down in frames of `size`, from `above` over the band.
                let mut sp = top + above;
                while sp > top {
                    sp -= size;
                }
                assert!(sp >= START + 16, "above {above} size {size}: {sp:#x}");
                assert!(sp <= top);
            }
        }
    }

    #[test]
    fn the_level_check_refuses_every_level_but_zero() {
        assert!(dispatch_allowed(0));
        // Other bits of PS (the window and the exception state) do not count.
        assert!(dispatch_allowed(0x0004_0020 & !0xf));
        for level in 1..=15 {
            assert!(!dispatch_allowed(level), "level {level}");
            assert!(!dispatch_allowed(0x0004_0020 | level));
        }
    }

    #[test]
    fn a_working_monitor_arms_and_ends_with_the_interrupt_on() {
        let mut fake = Fake::default();
        assert_eq!(run(&mut fake, START), Ok(()));
        assert_eq!(fake.bounds(), bounds(START));
        assert!(fake.write && fake.interrupt);
        // The self-test's raw bit is cleared before the interrupt is enabled.
        assert!(!fake.raw.get());
        assert_eq!(fake.stores, 1);
    }

    #[test]
    fn the_self_test_waits_for_the_interrupts_latency() {
        let mut fake = Fake {
            latency: POLLS - 1,
            ..Fake::default()
        };
        assert_eq!(run(&mut fake, START), Ok(()));
        let mut slow = Fake {
            latency: POLLS,
            ..Fake::default()
        };
        assert_eq!(run(&mut slow, START), Err(ArmError::NoFire));
        assert!(!slow.write && !slow.interrupt);
    }

    #[test]
    fn bounds_that_do_not_read_back_do_not_arm() {
        let mut fake = Fake {
            bounds_stuck: true,
            ..Fake::default()
        };
        assert_eq!(run(&mut fake, START), Err(ArmError::Readback));
        assert!(!fake.write && !fake.interrupt);
        assert_eq!(fake.stores, 0, "no store into the band before the readback");
    }

    #[test]
    fn an_enable_that_does_not_read_back_does_not_arm() {
        let mut fake = Fake {
            write_stuck: true,
            ..Fake::default()
        };
        assert_eq!(run(&mut fake, START), Err(ArmError::Readback));
    }

    #[test]
    fn a_monitor_that_does_not_fire_does_not_arm() {
        let mut fake = Fake {
            never_fires: true,
            ..Fake::default()
        };
        assert_eq!(run(&mut fake, START), Err(ArmError::NoFire));
        assert!(!fake.write && !fake.interrupt);
    }

    #[test]
    fn a_raw_bit_that_will_not_clear_does_not_arm() {
        let mut stuck = Fake {
            raw_sticks: true,
            raw_at_start: Cell::new(true),
            ..Fake::default()
        };
        assert_eq!(run(&mut stuck, START), Err(ArmError::RawStuck));
        let mut after = Fake {
            raw_sticks: true,
            ..Fake::default()
        };
        assert_eq!(run(&mut after, START), Err(ArmError::RawStuck));
        assert!(!after.write && !after.interrupt);
    }

    #[test]
    fn an_interrupt_enable_that_does_not_take_does_not_arm() {
        let mut fake = Fake {
            interrupt_stuck: true,
            ..Fake::default()
        };
        assert_eq!(run(&mut fake, START), Err(ArmError::InterruptNotSet));
        assert!(!fake.write && !fake.interrupt);
    }

    #[test]
    fn stop_switches_everything_off_and_clears_the_raw_bit() {
        let mut fake = Fake::default();
        run(&mut fake, START).unwrap();
        fake.raw.set(true);
        stop(&mut fake);
        assert!(!fake.write && !fake.interrupt && !fake.raw.get());
    }

    #[test]
    fn arming_again_after_a_sleep_is_the_same_as_arming() {
        let mut fake = Fake::default();
        run(&mut fake, START).unwrap();
        assert_eq!(run(&mut fake, START), Ok(()));
        assert!(fake.write && fake.interrupt);
    }
}
