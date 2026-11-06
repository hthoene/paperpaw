//! The band at the main stack's end that a store must never reach.
//!
//! The stack grows down towards its end, and past it lie the radio's
//! state and the heap's data: run over, a frame writes into them and
//! something fails later, somewhere else. Two checks watch the end, and
//! neither is sure by itself: esp-hal's trigger on one 4-byte guard word,
//! which a frame can step over without storing to it, and the painted
//! words the firmware's `stack::check` reads once per step of the main
//! loop, long after a call that overflowed inside itself. This band is a
//! third: the CPU's own data trigger, set to fire before *any store* into
//! it. It is a trigger rather than the debug assistant's stack-pointer
//! monitor because that one compares the stack pointer of whatever runs:
//! the Wi-Fi's and Bluetooth's tasks (esp-rtos) have stacks of their own
//! in the heap, below this stack, and would set it off at every switch.
//!
//! The trigger fires in the first store into the band, with the room the
//! band's height gives (at least [`BAND`] bytes) still free below it for
//! the exception handler, the panic's message and the crash slot.
//! What it cannot see: a single frame larger than the band that never
//! stores into it, nor a stack pointer moved by something other than a
//! store.

/// The band's height: a power of two, as the trigger matches a naturally
/// aligned region. About what the panic handler needs on the stack (its
/// 400-byte message, the backtrace's frames, the trap frame and the
/// formatting), with room to spare.
pub const BAND: u32 = 2048;

/// The stack must reach this far above the band, or there is none (a
/// stack that small is no design this guards).
const ABOVE: u32 = 4 * 1024;

/// The band `[start, start + BAND)` for a stack from `end` (lowest
/// address) to `top` (where it starts): the first aligned region at or
/// above `end`, so at least [`BAND`] bytes stay free below the first
/// store that reaches it. `None` where the stack has no room for it.
pub fn band(end: u32, top: u32) -> Option<u32> {
    let start = end.checked_add(BAND - 1)? & !(BAND - 1);
    let last = start.checked_add(BAND)?.checked_add(ABOVE)?;
    (last <= top).then_some(start)
}

/// The value for the trigger's address register (`tdata2`) that makes it
/// match the naturally aligned region `[start, start + BAND)`: the
/// encoding esp-hal uses for its own watchpoints on the ESP32-C3 (the
/// address with its low bits set but for the one below the size).
pub fn napot(start: u32) -> u32 {
    let mask = BAND - 1;
    (start & !mask) | (mask & !(BAND >> 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_band_is_the_first_aligned_region_above_the_end() {
        assert_eq!(band(0x3fcc_4378, 0x3fcc_e400), Some(0x3fcc_4800));
        assert_eq!(band(0x3fcc_4800, 0x3fcc_e400), Some(0x3fcc_4800));
        assert_eq!(band(0x3fcc_4801, 0x3fcc_e400), Some(0x3fcc_5000));
    }

    #[test]
    fn at_least_a_band_of_room_stays_below_its_top() {
        for end in [0x3fcc_4378u32, 0x3fcc_4800, 0x3fcc_4801, 0x3fcc_4fff] {
            let start = band(end, 0x3fcd_0000).unwrap();
            // The first store into the band lies at its top, `start + BAND - 1`.
            assert!(start + BAND - 1 - end >= BAND - 1);
            assert!(start >= end && start.is_multiple_of(BAND));
        }
    }

    #[test]
    fn a_stack_without_room_has_no_band() {
        assert_eq!(band(0x3fcc_4378, 0x3fcc_4378 + 2048), None);
        assert_eq!(band(0x3fcc_4378, 0x3fcc_4800 + BAND + ABOVE - 1), None);
        assert_eq!(
            band(0x3fcc_4378, 0x3fcc_4800 + BAND + ABOVE),
            Some(0x3fcc_4800)
        );
        assert_eq!(band(u32::MAX - 100, u32::MAX), None);
    }

    /// The band takes the CPU's data trigger 2. That is free in the
    /// versions below: esp-hal's stack guard (and esp-rtos', moved on each
    /// task switch) takes 0, its vector-table check 1 (off here). A new
    /// esp-hal or esp-rtos may take more: read what they do with
    /// `set_watchpoint`/`set_stack_watchpoint` (`debugger.rs`), then update
    /// the versions here. The firmware also finds a busy trigger at start
    /// (`stack::arm_band`) and then refuses interface-7 plugins.
    #[test]
    fn the_triggers_are_free_in_the_versions_checked() {
        let lock = include_str!("../../../shells/xteink/Cargo.lock");
        let version = |name: &str| -> &'static str {
            let mut lines = lock.lines();
            while let Some(line) = lines.next() {
                if line == alloc::format!("name = \"{name}\"") {
                    let v = lines.next().unwrap_or_default();
                    return v.trim_start_matches("version = ").trim_matches('"');
                }
            }
            panic!("{name} is not in the lock file");
        };
        assert_eq!(
            version("esp-hal"),
            "1.2.2",
            "esp-hal changed: check which CPU triggers it uses (stack_band.rs)"
        );
        assert_eq!(
            version("esp-rtos"),
            "0.4.0",
            "esp-rtos changed: check which CPU triggers it uses (stack_band.rs)"
        );
    }

    #[test]
    fn the_trigger_value_is_esp_hals_napot_encoding() {
        // esp-hal's `set_watchpoint(id, addr, len)` for len = 2048.
        let (addr, len) = (0x3fcc_4800usize, 2048usize);
        let z = len.trailing_zeros();
        let mask = (1usize << z) - 1;
        let reference = (addr & !mask) | (mask & !(1 << (z - 1)));
        assert_eq!(napot(addr as u32) as usize, reference);
        assert_eq!(napot(0x3fcc_4800), 0x3fcc_4bff);
    }
}
