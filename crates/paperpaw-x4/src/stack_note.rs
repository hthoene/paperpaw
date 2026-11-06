//! What the panic handler says of its own stack use (checklist G7 of the
//! interface-7 gate: the handler's real depth, against the room
//! `stack_band::RESERVE` keeps for it).
//!
//! The stack is painted once at start (`stack::paint` in the firmware), so
//! the lowest word that is not the paint is the deepest the stack has been
//! since. The panic handler reads that at its very end, after the message and
//! the calls are written (the deepest it goes), and ends the message with it:
//!
//! ```text
//! ... @ 4200a1c4 4200a30e [stack handler=1480B left=7120B]
//! ```
//!
//! - `handler` is the depth the handler used: from the stack pointer where it
//!   was entered down to the deepest word touched since the paint. Zero if
//!   nothing below its entry was touched. That is the handler's own use
//!   (the exception handler's frames above its entry are not in it: the
//!   frame budget `shells/xteink/frame-budget.txt` has them).
//! - `left` is what lies below that deepest word, down to the stack's end:
//!   the room the handler did not need.
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
pub const NOTE_LEN: usize = 48;

/// The depth the handler used: from the stack pointer `entry` where it was
/// entered down to the deepest word touched, `end + left` (`end` the stack's
/// lowest address, `left` the bytes never touched above it). Zero if that is
/// not below `entry`.
pub fn handler_depth(entry: u32, end: u32, left: u32) -> u32 {
    entry.saturating_sub(end.saturating_add(left))
}

/// Writes the note: a space first, so it follows the calls.
pub fn write(out: &mut impl Write, handler: u32, left: u32) -> core::fmt::Result {
    write!(out, " [stack handler={handler}B left={left}B]")
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
    fn the_note_fits_its_room_at_any_size() {
        struct Count(usize);
        impl Write for Count {
            fn write_str(&mut self, s: &str) -> core::fmt::Result {
                self.0 += s.len();
                Ok(())
            }
        }
        for (handler, left) in [(0, 0), (1480, 7120), (u32::MAX, u32::MAX)] {
            let mut n = Count(0);
            write(&mut n, handler, left).unwrap();
            assert!(n.0 <= NOTE_LEN, "{handler} {left}: {}", n.0);
        }
    }

    #[test]
    fn the_note_has_no_word_the_calls_reader_takes_for_an_address() {
        // `tools/x4-remote.py` reads every word after ` @ ` as hexadecimal
        // and skips the ones that are not.
        let mut s = alloc::string::String::new();
        write(&mut s, 1480, 7120).unwrap();
        for word in s.as_str().split_whitespace() {
            assert!(
                u32::from_str_radix(word, 16).is_err(),
                "{word} would be read as a call"
            );
        }
        assert_eq!(s.as_str(), " [stack handler=1480B left=7120B]");
    }
}
