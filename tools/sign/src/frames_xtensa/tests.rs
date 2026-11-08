//! The scan of `frames_xtensa.rs` on the encodings LLVM's assembler makes
//! (`forms.rs`) and on small functions made by hand.

use super::*;

#[path = "forms.rs"]
mod forms;

const ENTRY_32: [u8; 3] = [0x36, 0x41, 0x00];
const RETW_N: [u8; 2] = [0x1d, 0xf0];
const NOP: [u8; 3] = [0xf0, 0x20, 0x00];

fn find(code: &[u8]) -> Option<Finding> {
    scan_ranges(code, &[(0, code.len())])
}

#[test]
fn every_form_that_writes_the_stack_pointer_is_found() {
    for (text, bytes) in forms::WRITES_A1 {
        assert_eq!(
            find(bytes),
            Some(Finding::WritesSp { at: 0 }),
            "{text} {bytes:02x?} was let through"
        );
    }
}

#[test]
fn forms_that_only_read_it_or_write_another_register_pass() {
    for (text, bytes) in forms::READS_A1 {
        assert_eq!(find(bytes), None, "{text} {bytes:02x?} was refused");
    }
}

#[test]
fn the_frame_is_the_entry_operand_in_eight_byte_steps_and_the_base_save_area() {
    // entry a1, 32 (36 41 00) and a1, 48 (36 61 00), as LLVM writes them.
    let mut code = Vec::new();
    code.extend_from_slice(&ENTRY_32);
    code.extend_from_slice(&RETW_N);
    assert_eq!(find(&code), None);
    // The largest that passes: imm12 = 253 (2,024 bytes) + 16 = 2,040; the
    // first that does not: 254 (2,032) + 16 = 2,048.
    for (imm12, refused) in [(253u32, false), (254, true), (4095, true), (0, false)] {
        let entry = 0x36 | 1 << 8 | imm12 << 12;
        let mut code = entry.to_le_bytes()[..3].to_vec();
        code.extend_from_slice(&RETW_N);
        let found = find(&code);
        if refused {
            assert_eq!(
                found,
                Some(Finding::Frame {
                    at: 0,
                    size: (imm12 << 3) + BASE_SAVE
                })
            );
        } else {
            assert_eq!(found, None, "imm12 {imm12}");
        }
    }
}

#[test]
fn call0_and_callx0_are_refused_and_call8_is_not() {
    let mut code = ENTRY_32.to_vec();
    code.extend_from_slice(&[0x05, 0x00, 0x00]); // call0
    code.extend_from_slice(&RETW_N);
    assert_eq!(find(&code), Some(Finding::Call0 { at: 3 }));
    let mut code = ENTRY_32.to_vec();
    code.extend_from_slice(&[0xc0, 0x02, 0x00]); // callx0 a2
    assert_eq!(find(&code), Some(Finding::Call0 { at: 3 }));
    for ok in [
        [0x25, 0x00, 0x00],
        [0xe0, 0x02, 0x00],
        [0x15, 0x00, 0x00],
        [0xd0, 0x02, 0x00],
    ] {
        // call8, callx8 a2, call4, callx4 a2
        let mut code = ENTRY_32.to_vec();
        code.extend_from_slice(&ok);
        code.extend_from_slice(&RETW_N);
        assert_eq!(find(&code), None, "{ok:02x?}");
    }
}

#[test]
fn a_function_is_read_to_its_end_and_not_a_byte_further() {
    // Ends inside a 3-byte instruction.
    let mut code = ENTRY_32.to_vec();
    code.extend_from_slice(&NOP[..2]);
    assert_eq!(find(&code), Some(Finding::Cut { at: 3 }));
    // An op0 that no Xtensa instruction has (14, 15).
    assert_eq!(find(&[0x0e]), Some(Finding::Unknown { at: 0 }));
    assert_eq!(find(&[0xaf, 0x00, 0x00]), Some(Finding::Unknown { at: 0 }));
    // The bytes between functions are the literal pool: not decoded.
    let mut code = ENTRY_32.to_vec();
    code.extend_from_slice(&RETW_N);
    let end = code.len();
    code.extend_from_slice(&[0x12, 0xc1, 0x08, 0x00]); // data that looks like `addi a1, a1, 8`
    assert_eq!(scan_ranges(&code, &[(0, end)]), None);
    // A frame in the second function is found there.
    code.extend_from_slice(&(0x36u32 | 1 << 8 | 254 << 12).to_le_bytes()[..3]);
    code.extend_from_slice(&RETW_N);
    assert_eq!(
        scan_ranges(&code, &[(0, end), (end + 4, code.len())]),
        Some(Finding::Frame {
            at: end + 4,
            size: 2048
        })
    );
}

#[test]
fn the_length_of_an_instruction_is_in_its_first_byte() {
    for b0 in 0u8..=255 {
        let expect = match b0 & 0xF {
            0..=7 => Some(3),
            8..=13 => Some(2),
            _ => None,
        };
        assert_eq!(length(b0), expect, "{b0:#x}");
    }
}
