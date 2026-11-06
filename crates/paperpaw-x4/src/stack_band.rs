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
//! The trigger fires in the first store into the band, and the panic
//! handler then runs on what is left *below the stack pointer*, which is
//! not the store's address: a frame of N bytes that reaches into the
//! band from above has its pointer N bytes below where it stood, and the
//! store may be N bytes above it. So the room counts from the pointer, and
//! the band lies [`RESERVE`] bytes above the stack's end, aligned: whatever
//! frame of less than [`BAND`] bytes reaches the band, the pointer is still
//! at least [`RESERVE`] bytes above the end (see [`room_at_trap`]), for the
//! exception handler, the panic's message and the crash slot (the
//! handler's real depth is measured on the device).
//! What it cannot see: a single frame of [`BAND`] bytes or more that never
//! stores into it (`paperpaw-sign pack` refuses frames of 1.5 KiB or more
//! in a plugin's code), nor a stack pointer moved by something other than
//! a store.

/// The band's height: a power of two, as the trigger matches a naturally
/// aligned region.
pub const BAND: u32 = 2048;

/// The room kept below the band for the panic handler: the trap frame,
/// its 400-byte message, the backtrace's frames and the formatting, with
/// room to spare.
pub const RESERVE: u32 = 2048;

/// The stack must reach this far above the band, or there is none (a
/// stack that small is no design this guards).
const ABOVE: u32 = 4 * 1024;

/// The band `[start, start + BAND)` for a stack from `end` (lowest
/// address) to `top` (where it starts): the first aligned region at or
/// above `end + RESERVE`. `None` where the stack has no room for it.
pub fn band(end: u32, top: u32) -> Option<u32> {
    let from = end.checked_add(RESERVE)?;
    let start = from.checked_add(BAND - 1)? & !(BAND - 1);
    let last = start.checked_add(BAND)?.checked_add(ABOVE)?;
    (last <= top).then_some(start)
}

/// The bytes between the stack's end and the stack pointer when the
/// trigger fires for a frame of `frame` bytes that reaches into the band
/// from above, at the worst: the pointer stood at the band's top
/// (`start + BAND`), nothing stored in the band yet, and the frame
/// brought it `frame` bytes down. Nothing for a frame that takes it below
/// the end.
pub fn room_at_trap(end: u32, start: u32, frame: u32) -> u32 {
    (start + BAND).saturating_sub(frame).saturating_sub(end)
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
    fn the_band_is_the_first_aligned_region_a_reserve_above_the_end() {
        assert_eq!(band(0x3fcc_4378, 0x3fcc_e400), Some(0x3fcc_5000));
        assert_eq!(band(0x3fcc_4800, 0x3fcc_e400), Some(0x3fcc_5000));
        assert_eq!(band(0x3fcc_4801, 0x3fcc_e400), Some(0x3fcc_5800));
    }

    #[test]
    fn the_reserve_stays_below_the_pointer_for_any_frame_under_the_band() {
        // Whatever the stack's end and the frame (up to the band's height
        // less one), at least the reserve is left below the pointer.
        for end in (0x3fcc_4000u32..0x3fcc_5000).step_by(4) {
            let start = band(end, 0x3fcd_0000).unwrap();
            assert!(start.is_multiple_of(BAND));
            assert!(start >= end + RESERVE, "end {end:#x}");
            for frame in [0, 16, 512, 1024, 1536, BAND - 1] {
                let room = room_at_trap(end, start, frame);
                assert!(room >= RESERVE, "end {end:#x} frame {frame}: {room}");
            }
        }
    }

    #[test]
    fn without_a_reserve_a_large_frame_would_leave_little() {
        // The band at the end (no reserve): a 1 KiB frame leaves 1 KiB.
        let (end, start) = (0x3fcc_4800u32, 0x3fcc_4800u32);
        assert_eq!(room_at_trap(end, start, 1024), 1024);
        // With the reserve, the same stack's band lies higher.
        let start = band(end, 0x3fcd_0000).unwrap();
        assert_eq!(start, 0x3fcc_5000);
        assert!(room_at_trap(end, start, 1024) >= RESERVE + 1024);
    }

    #[test]
    fn a_stack_without_room_has_no_band() {
        assert_eq!(band(0x3fcc_4378, 0x3fcc_4378 + 2048), None);
        assert_eq!(band(0x3fcc_4378, 0x3fcc_5000 + BAND + ABOVE - 1), None);
        assert_eq!(
            band(0x3fcc_4378, 0x3fcc_5000 + BAND + ABOVE),
            Some(0x3fcc_5000)
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
