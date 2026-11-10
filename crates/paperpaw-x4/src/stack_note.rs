//! What the panic handler says of its own stack use (checklist G7 of the
//! interface-7 gate: the handler's real depth, against the room
//! `stack_band::RESERVE` keeps for it).
//!
//! The stack is painted once at start (`stack::paint` in the firmware), so
//! the lowest word that is not the paint is the deepest the stack has been
//! since. The panic handler reads that at its very end, after the message and
//! the calls are written (the deepest it goes), and ends the message with it.
//! What the figure means depends on the panic:
//!
//! ```text
//! ... @ 4200a1c4 4200a30e [stack trap handler=1480B left=7120B]
//! ... @ 4200a1c4 4200a30e [stack left=7120B]
//! ```
//!
//! - After a **band trap** (a breakpoint exception, the stack pointer inside
//!   the main stack at or below the band's top: the store trigger of
//!   `stack_band` fired), `handler` is the depth the handler used: from the
//!   stack pointer where it was entered down to the deepest word touched
//!   since the paint. The pointer was at the band, so the handler's frames
//!   are the deepest the stack went: this is G7's figure. It does not count
//!   the exception handler's frames above the entry (the frame budget
//!   `shells/xteink/frame-budget.txt` has them).
//! - After **any other panic** the deepest word touched since boot may lie
//!   far below where the handler ran (a deep call that returned, long
//!   before): a `handler` figure would be wrong, so none is printed, and
//!   `left` says only how much of the stack was never touched since boot.
//! - A panic entered with the pointer **outside the main stack** (a radio
//!   task's, on the heap) has no note: the painted words say nothing of that
//!   stack.
//!
//! `left` is what lies below the deepest word, down to the stack's end.
//!
//! The text is kept through the reset in the crash slot with the rest of the
//! message, so it survives what a read of the painted RAM after the reset
//! could not (the bootloader runs in that RAM first), and `tools/x4-remote.py
//! --crashes` and `crashes.txt` on the card show it with the panic. Its words
//! are not hexadecimal numbers, so the reader of the calls skips them.

use core::fmt::Write;

/// The most bytes the note takes in the message: the handler keeps this
/// many at the end of the text's room, so the note is never cut off (the
/// calls before it are cut instead).
pub const NOTE_LEN: usize = 56;

/// The depth the handler used: from the stack pointer `entry` where it was
/// entered down to the deepest word touched, `end + left` (`end` the stack's
/// lowest address, `left` the bytes never touched above it). Zero if that is
/// not below `entry`.
pub fn handler_depth(entry: u32, end: u32, left: u32) -> u32 {
    entry.saturating_sub(end.saturating_add(left))
}

/// Whether `entry` (the stack pointer where the handler was entered) lies in
/// the main stack, `end` (its lowest address) to `start` (where it starts).
pub fn in_main_stack(entry: u32, end: u32, start: u32) -> bool {
    entry >= end && entry <= start
}

/// Whether a panic is a band trap: a breakpoint exception with the stack
/// pointer at or below the band's top (`band` its first address, `end` and
/// `start` the main stack's). A breakpoint elsewhere (a plugin's `ebreak`, a
/// debugger's) is not.
pub fn is_band_trap(breakpoint: bool, entry: u32, end: u32, start: u32) -> bool {
    breakpoint
        && in_main_stack(entry, end, start)
        && crate::stack_band::band(end, start)
            .is_some_and(|band| entry < band.saturating_add(crate::stack_band::BAND))
}

/// Whether a panic is a band trap on the ESP32-S3: its band's interrupt fired
/// (`hit`) with the stack pointer at or below the band's top. The monitor
/// interrupts instead of trapping, and its handler runs on the same stack
/// (`s3_band`).
pub fn is_band_trap_s3(hit: bool, entry: u32, end: u32, start: u32) -> bool {
    hit && in_main_stack(entry, end, start)
        && crate::s3_band::band(end, start)
            .is_some_and(|band| entry < band.saturating_add(crate::s3_band::BAND))
}

/// Writes the note: a space first, so it follows the calls. `handler` is
/// `Some` after a band trap only (see the module's text).
pub fn write(out: &mut impl Write, handler: Option<u32>, left: u32) -> core::fmt::Result {
    match handler {
        Some(handler) => write!(out, " [stack trap handler={handler}B left={left}B]"),
        None => write!(out, " [stack left={left}B]"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_depth_is_from_the_entry_to_the_deepest_word() {
        // Entered at 0x3fcc_8000, the deepest word touched at 0x3fcc_7a38
        // (the stack's end at 0x3fcc_4388, 0x36b0 bytes never touched).
        assert_eq!(
            handler_depth(0x3fcc_8000, 0x3fcc_4388, 0x36b0),
            0x3fcc_8000 - 0x3fcc_7a38
        );
        // Nothing below the entry was touched (the handler's frames were
        // above the deepest the program had been): zero, not a wrap.
        assert_eq!(handler_depth(0x3fcc_8000, 0x3fcc_4388, 0x5000), 0);
        assert_eq!(handler_depth(0, u32::MAX, u32::MAX), 0);
        assert_eq!(handler_depth(10, 0, 3), 7);
    }

    #[test]
    fn the_s3_band_trap_needs_the_hit_and_a_pointer_at_or_below_the_band() {
        let (end, start) = (0x3fc9_c000u32, 0x3fca_8000u32);
        let band = crate::s3_band::band(end, start).unwrap();
        let top = band + crate::s3_band::BAND;
        assert!(is_band_trap_s3(true, top - 16, end, start));
        assert!(is_band_trap_s3(true, band, end, start));
        // Above the band, or no hit (a panic elsewhere), or off the stack: no.
        assert!(!is_band_trap_s3(true, top, end, start));
        assert!(!is_band_trap_s3(false, top - 16, end, start));
        assert!(!is_band_trap_s3(true, end - 4, end, start));
        // A stack with no room for a band has no trap.
        assert!(!is_band_trap_s3(true, end + 100, end, end + 1000));
    }

    #[test]
    fn the_note_fits_its_room_at_any_size() {
        struct Count(usize);
        impl Write for Count {
            fn write_str(&mut self, s: &str) -> core::fmt::Result {
                self.0 += s.len();
                Ok(())
            }
        }
        for (handler, left) in [
            (Some(0), 0),
            (Some(1480), 7120),
            (Some(u32::MAX), u32::MAX),
            (None, u32::MAX),
        ] {
            let mut n = Count(0);
            write(&mut n, handler, left).unwrap();
            assert!(n.0 <= NOTE_LEN, "{handler:?} {left}: {}", n.0);
        }
    }

    #[test]
    fn the_note_has_no_word_the_calls_reader_takes_for_an_address() {
        // `tools/x4-remote.py` reads every word after ` @ ` as hexadecimal
        // and skips the ones that are not.
        let mut s = alloc::string::String::new();
        write(&mut s, Some(1480), 7120).unwrap();
        for word in s.as_str().split_whitespace() {
            assert!(
                u32::from_str_radix(word, 16).is_err(),
                "{word} would be read as a call"
            );
        }
        assert_eq!(s.as_str(), " [stack trap handler=1480B left=7120B]");
        let mut s = alloc::string::String::new();
        write(&mut s, None, 7120).unwrap();
        assert_eq!(s.as_str(), " [stack left=7120B]");
        for word in s.as_str().split_whitespace() {
            assert!(u32::from_str_radix(word, 16).is_err(), "{word}");
        }
    }

    #[test]
    fn only_a_breakpoint_at_the_band_with_the_pointer_in_the_main_stack_is_a_band_trap() {
        let (end, start) = (0x3fcc_4388u32, 0x3fcc_e400u32);
        let band = crate::stack_band::band(end, start).unwrap();
        let top = band + crate::stack_band::BAND;
        // Entered below the band's top, in the stack, on a breakpoint.
        assert!(is_band_trap(true, band + 100, end, start));
        assert!(is_band_trap(true, end + 10, end, start));
        assert!(is_band_trap(true, top - 1, end, start));
        // Not a breakpoint, or above the band, or outside the main stack
        // (a radio task's, on the heap), or just past the stack's start.
        assert!(!is_band_trap(false, band + 100, end, start));
        assert!(!is_band_trap(true, top, end, start));
        assert!(!is_band_trap(true, start, end, start));
        assert!(!is_band_trap(true, end - 4, end, start));
        assert!(!is_band_trap(true, 0x3fc9_0000, end, start));
        assert!(!is_band_trap(true, start + 4, end, start));
        // A stack with no room for a band has none to trap in.
        assert!(!is_band_trap(true, end + 4, end, end + 2048));
    }

    #[test]
    fn the_main_stack_is_end_to_start() {
        assert!(in_main_stack(0x100, 0x100, 0x200));
        assert!(in_main_stack(0x200, 0x100, 0x200));
        assert!(!in_main_stack(0xfc, 0x100, 0x200));
        assert!(!in_main_stack(0x204, 0x100, 0x200));
    }
}
