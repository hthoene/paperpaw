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
fn entry_is_accepted_only_with_a1() {
    // entry a1, 32 is ENTRY_32 (36 41 00); entry a2, 32 and a15, 32.
    for reg in [0u32, 2, 3, 8, 15] {
        let entry = 0x36 | reg << 8 | 4 << 12; // entry a<reg>, 32
        let mut code = entry.to_le_bytes()[..3].to_vec();
        code.extend_from_slice(&RETW_N);
        assert_eq!(find(&code), Some(Finding::Form { at: 0 }), "entry a{reg}");
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
        [0x35, 0x00, 0x00],
        [0xf0, 0x02, 0x00],
    ] {
        // call8, callx8 a2, call12, callx12 a2
        let mut code = ENTRY_32.to_vec();
        code.extend_from_slice(&ok);
        code.extend_from_slice(&RETW_N);
        assert_eq!(find(&code), None, "{ok:02x?}");
    }
    for refused in [[0x15, 0x00, 0x00], [0xd0, 0x02, 0x00]] {
        // call4, callx4 a2: only call8 and call12 are allowed
        let mut code = ENTRY_32.to_vec();
        code.extend_from_slice(&refused);
        code.extend_from_slice(&RETW_N);
        assert_eq!(find(&code), Some(Finding::Form { at: 3 }), "{refused:02x?}");
    }
}

/// `l32r a0, <the word at target>` as the instruction at `at` makes it.
fn l32r_at(at: usize, target: usize) -> [u8; 3] {
    let words = (target as i64 - (((at + 3) & !3) as i64)) / 4;
    let imm16 = (words & 0xFFFF) as u32;
    let w = 0x01 | imm16 << 8;
    [w as u8, (w >> 8) as u8, (w >> 16) as u8]
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
    // A frame in the second function is found there.
    let mut code = ENTRY_32.to_vec();
    code.extend_from_slice(&RETW_N);
    let end = code.len();
    code.push(0); // padding to the next function
    code.extend_from_slice(&(0x36u32 | 1 << 8 | 254 << 12).to_le_bytes()[..3]);
    code.extend_from_slice(&RETW_N);
    assert_eq!(
        scan_ranges(&code, &[(0, end), (end + 1, code.len())]),
        Some(Finding::Frame {
            at: end + 1,
            size: 2048
        })
    );
}

#[test]
fn the_bytes_between_functions_are_padding_or_a_literal_an_l32r_reads() {
    // f1 (entry; retw.n), a literal at 8 that looks like `addi a1, a1, 8`,
    // then f2 (entry; l32r a0, <the word at 8>; retw.n).
    let mut code = ENTRY_32.to_vec();
    code.extend_from_slice(&RETW_N);
    let end1 = code.len();
    code.resize(8, 0);
    code.extend_from_slice(&[0x12, 0xc1, 0x08, 0x00]);
    let start2 = code.len();
    code.extend_from_slice(&ENTRY_32);
    code.extend_from_slice(&l32r_at(start2 + 3, 8));
    code.extend_from_slice(&RETW_N);
    let ranges = [(0, end1), (start2, code.len())];
    assert_eq!(scan_ranges(&code, &ranges), None);
    // The same bytes, read by no `l32r`, are code no function covers.
    let mut hidden = code.clone();
    hidden[start2 + 3] = 0xf0; // the l32r becomes a nop (f0 20 00)
    hidden[start2 + 4] = 0x20;
    hidden[start2 + 5] = 0x00;
    assert_eq!(
        scan_ranges(&hidden, &ranges),
        Some(Finding::Uncovered { at: 8 })
    );
    // A whole function hidden behind the last symbol is not scanned and is
    // refused, not let through.
    let mut tail = ENTRY_32.to_vec();
    tail.extend_from_slice(&RETW_N);
    let end = tail.len();
    tail.extend_from_slice(&ENTRY_32);
    tail.extend_from_slice(&RETW_N);
    assert_eq!(
        scan_ranges(&tail, &[(0, end)]),
        Some(Finding::Uncovered { at: end })
    );
    // Padding: zeros, `nop`, `nop.n`.
    let mut padded = ENTRY_32.to_vec();
    padded.extend_from_slice(&RETW_N);
    let end = padded.len();
    padded.extend_from_slice(&[0, 0xf0, 0x20, 0x00, 0x3d, 0xf0]);
    let start = padded.len();
    padded.extend_from_slice(&ENTRY_32);
    padded.extend_from_slice(&RETW_N);
    assert_eq!(
        scan_ranges(&padded, &[(0, end), (start, padded.len())]),
        None
    );
    // A literal pool before the first function (the linker script puts the
    // `.literal` sections first) that its own `l32r` reads.
    let mut front = vec![0x78, 0x56, 0x34, 0x12];
    let at = front.len();
    front.extend_from_slice(&ENTRY_32);
    front.extend_from_slice(&l32r_at(at + 3, 0));
    front.extend_from_slice(&RETW_N);
    assert_eq!(scan_ranges(&front, &[(at, front.len())]), None);
}

#[test]
fn a_form_not_known_to_leave_the_stack_pointer_alone_is_refused() {
    for (text, bytes) in forms::UNKNOWN {
        let mut code = ENTRY_32.to_vec();
        code.extend_from_slice(bytes);
        code.extend_from_slice(&RETW_N);
        assert!(
            matches!(find(&code), Some(Finding::Form { at: 3 })),
            "{text} {bytes:02x?} was let through: {:?}",
            find(&code)
        );
    }
}

#[test]
fn every_wide_word_is_classed_and_no_safe_class_names_a1_as_a_destination() {
    // The 24-bit words whose `r`/`s`/`t` all vary over the registers that
    // can be a destination: a class `Safe` must be one that cannot write
    // `a1`, so for each `op0` family the words with destination `a1` are
    // `Writes` or `Unknown`, never `Safe`, where the destination field is
    // known. Here: the destination is `ar`, `at` or `as` of the dozens of
    // forms in `forms::WRITES_A1`, which is checked above; this test checks
    // that the other word with the same `op0`, `op1`, `op2` and a different
    // destination register is not `Writes` unless it names `a1`.
    for (text, bytes) in forms::WRITES_A1.iter().filter(|(_, b)| b.len() == 3) {
        let w = u32::from(bytes[0]) | u32::from(bytes[1]) << 8 | u32::from(bytes[2]) << 16;
        assert_ne!(wide_class(w), Class::Safe, "{text}");
    }
    for (text, bytes) in forms::READS_A1.iter().filter(|(_, b)| b.len() == 3) {
        let w = u32::from(bytes[0]) | u32::from(bytes[1]) << 8 | u32::from(bytes[2]) << 16;
        assert_eq!(wide_class(w), Class::Safe, "{text}");
    }
}

#[test]
fn a_function_symbol_of_size_zero_is_refused() {
    assert_eq!(unsized_function(&[(0, 8), (8, 0), (16, 4)]), Some(8));
    assert_eq!(unsized_function(&[(0, 8), (8, 4)]), None);
}

/// A linked-looking ELF with `code` as `.text` at 0x4200_0000 and the
/// symbols `(name, offset, size)`.
fn linked(code: &[u8], symbols: &[(&str, u32, u32)]) -> Vec<u8> {
    use crate::elf::build::Builder;
    let mut b = Builder::new(crate::elf::EM_XTENSA, crate::elf::ET_EXEC);
    let text = b.section(".text", 0x6, 0x4200_0000, code, 16);
    for &(name, at, size) in symbols {
        b.function(name, 0x4200_0000 + at, size, text as u16, false);
    }
    b.build()
}

fn check_of(bytes: &[u8]) -> Result<(), String> {
    let elf = Elf::parse(bytes).expect("elf");
    let (index, text) = elf.section(".text").expect(".text");
    let (_, symbols) = elf.symbols().expect("symbols").expect("a symbol table");
    check(&elf, text, index, &symbols)
}

#[test]
fn check_refuses_a_size_zero_function_and_code_no_symbol_covers() {
    let mut code = ENTRY_32.to_vec();
    code.extend_from_slice(&RETW_N);
    let end = code.len() as u32;
    // Sized: passes.
    assert_eq!(check_of(&linked(&code, &[("f", 0, end)])), Ok(()));
    // A symbol of size 0 beside it: refused by name.
    let err = check_of(&linked(&code, &[("f", 0, end), ("g", 0, 0)])).unwrap_err();
    assert!(err.contains("`g` has size 0"), "{err}");
    // Code after the last sized symbol.
    code.extend_from_slice(&ENTRY_32);
    code.extend_from_slice(&RETW_N);
    let err = check_of(&linked(&code, &[("f", 0, end)])).unwrap_err();
    assert!(err.contains("no sized function symbol covers"), "{err}");
    // An unknown form is refused with its function named.
    let mut vector = ENTRY_32.to_vec();
    vector.extend_from_slice(&[0x04, 0x00, 0x60]); // a CUST0 form
    vector.extend_from_slice(&RETW_N);
    let err = check_of(&linked(&vector, &[("v", 0, vector.len() as u32)])).unwrap_err();
    assert!(
        err.contains("does not know to leave the stack pointer"),
        "{err}"
    );
}

/// The dead word the toolchain leaves: `0x0000ff00`, its first byte read as
/// padding.
const DEAD: [u8; 4] = [0x00, 0xff, 0x00, 0x00];

/// f1 (5 bytes), a gap of `gap` bytes, f2 (`entry`, `body`, `retw.n`).
fn pool(gap: &[u8], body: &[u8]) -> (Vec<u8>, Vec<(usize, usize)>) {
    let mut code = ENTRY_32.to_vec();
    code.extend_from_slice(&RETW_N);
    let end1 = code.len();
    code.extend_from_slice(gap);
    let start2 = code.len();
    code.extend_from_slice(&ENTRY_32);
    code.extend_from_slice(body);
    code.extend_from_slice(&RETW_N);
    let end = code.len();
    (code, vec![(0, end1), (start2, end)])
}

fn nothing() -> Reach {
    Reach::default()
}

#[test]
fn a_dead_word_nothing_reaches_is_let_through_and_noted() {
    // f1 at 0..5, padding 5..8, the dead word at 8, f2 at 12.
    let mut gap = vec![0, 0, 0];
    gap.extend_from_slice(&DEAD);
    let (code, ranges) = pool(&gap, &[]);
    // Without the image's view of what points where: still refused.
    assert_eq!(
        scan_ranges(&code, &ranges),
        Some(Finding::Uncovered { at: 9 })
    );
    assert_eq!(
        scan_ranges_in(&code, &ranges, Some(&nothing())),
        (None, vec![8])
    );
    // Before the first function: the same.
    let mut front = DEAD.to_vec();
    front.extend_from_slice(&ENTRY_32);
    front.extend_from_slice(&RETW_N);
    assert_eq!(
        scan_ranges_in(&front, &[(4, front.len())], Some(&nothing())),
        (None, vec![0])
    );
    // Something that points at a byte of it (anywhere in 8..12) refuses it,
    // one at the next function does not.
    for at in 8..12 {
        let reach = Reach { pointed: vec![at] };
        assert_eq!(
            scan_ranges_in(&code, &ranges, Some(&reach)),
            (Some(Finding::Uncovered { at: 9 }), vec![]),
            "{at}"
        );
    }
    let reach = Reach {
        pointed: vec![7, 12],
    };
    assert_eq!(scan_ranges_in(&code, &ranges, Some(&reach)).0, None);
}

#[test]
fn a_dead_word_that_a_call_a_jump_or_a_branch_goes_to_is_refused() {
    let mut gap = vec![0, 0, 0];
    gap.extend_from_slice(&DEAD);
    // f2 is at 12 (its `entry` 12..15); the instruction under test is at 15.
    let at = 15i64;
    let sized = |imm: i64, shift: u32, op: u32| -> [u8; 3] {
        let w = op | ((imm as u32) & 0x3FFFF) << shift;
        [w as u8, (w >> 8) as u8, (w >> 16) as u8]
    };
    // j: at + 4 + imm18 = 8.
    let j = sized(8 - (at + 4), 6, 0x06);
    // call8: (at & ~3) + 4 + imm18 * 4 = 8.
    let call = sized((8 - ((at & !3) + 4)) / 4, 6, 0x05 | 2 << 4);
    // beqz a2, ...: n = 1, `s` 2, imm12 at bit 12.
    let beqz = {
        let imm = (8 - (at + 4)) as u32 & 0xFFF;
        let w = 0x06 | 1 << 4 | 2 << 8 | imm << 12;
        [w as u8, (w >> 8) as u8, (w >> 16) as u8]
    };
    // beq a2, a3, ...: op0 7, imm8 at bit 16.
    let beq = {
        let w = 0x07 | 3 << 4 | 2 << 8 | (((8 - (at + 4)) as u32) & 0xFF) << 16;
        [w as u8, (w >> 8) as u8, (w >> 16) as u8]
    };
    for (name, instruction) in [("j", j), ("call8", call), ("beqz", beqz), ("beq", beq)] {
        let (code, ranges) = pool(&gap, &instruction);
        assert_eq!(
            scan_ranges_in(&code, &ranges, Some(&nothing())),
            (Some(Finding::Uncovered { at: 9 }), vec![]),
            "{name}"
        );
        // The same instruction going to the second function's start is fine.
    }
    let away = sized(12 - (at + 4), 6, 0x06);
    let (code, ranges) = pool(&gap, &away);
    assert_eq!(
        scan_ranges_in(&code, &ranges, Some(&nothing())),
        (None, vec![8])
    );
}

#[test]
fn a_hardware_loops_end_is_unsigned_and_a_word_after_a_running_on_function_is_refused() {
    // f1: entry, `loop a2, +imm8`, 98 `nop.n`, `retw.n` (204 bytes), the dead
    // word at 204, f2 at 208. A loop's end is at + 4 + imm8, imm8 unsigned.
    let f1 = |imm8: u32, last: &[u8]| {
        let mut code = ENTRY_32.to_vec();
        let w = 0x06 | 7 << 4 | 2 << 8 | 8 << 12 | imm8 << 16;
        code.extend_from_slice(&[w as u8, (w >> 8) as u8, (w >> 16) as u8]);
        for _ in 0..98 {
            code.extend_from_slice(&[0x3d, 0xf0]);
        }
        code.extend_from_slice(last);
        assert_eq!(code.len(), 204);
        code.extend_from_slice(&DEAD);
        code.extend_from_slice(&ENTRY_32);
        code.extend_from_slice(&RETW_N);
        let ranges = vec![(0, 204), (208, code.len())];
        (code, ranges)
    };
    // 200 (a signed read makes it -56): 3 + 4 + 200 = 207, in the word.
    let (code, ranges) = f1(200, &RETW_N);
    assert_eq!(
        scan_ranges_in(&code, &ranges, Some(&nothing())),
        (Some(Finding::Uncovered { at: 205 }), vec![])
    );
    // 190: the end is at 197, inside the function.
    let (code, ranges) = f1(190, &RETW_N);
    assert_eq!(
        scan_ranges_in(&code, &ranges, Some(&nothing())),
        (None, vec![204])
    );
    // The same, but the function runs on into the word (it ends in a `nop.n`).
    let (code, ranges) = f1(190, &[0x3d, 0xf0]);
    assert_eq!(
        scan_ranges_in(&code, &ranges, Some(&nothing())),
        (Some(Finding::Uncovered { at: 205 }), vec![])
    );
}

#[test]
fn a_dead_word_that_reads_as_control_flow_is_refused() {
    for word in [
        [0x36, 0x41, 0x00, 0x00], // entry a1, 32
        [0x00, 0x36, 0x41, 0x00], // the same, one byte on
        [0x80, 0x00, 0x00, 0x00], // ret
        [0x90, 0x00, 0x00, 0x00], // retw
        [0xa0, 0x00, 0x00, 0x00], // jx a0
        [0xc0, 0x00, 0x00, 0x00], // callx8 a0
        [0x1d, 0xf0, 0x00, 0x00], // retw.n
        [0x00, 0x00, 0x0d, 0xf0], // ret.n
    ] {
        let (code, ranges) = pool(&[0, 0, 0, word[0], word[1], word[2], word[3]], &[]);
        assert_eq!(code[8..12], word);
        assert_eq!(
            scan_ranges_in(&code, &ranges, Some(&nothing())).1,
            Vec::<usize>::new(),
            "{word:02x?}"
        );
    }
    assert!(!dead::looks_like_control(DEAD));
}

#[test]
fn a_gap_that_is_partial_unaligned_or_after_the_last_function_is_refused() {
    // The gap is 5..8: the word at 4 is not wholly in it.
    let (code, ranges) = pool(&[0x00, 0xff, 0x00], &[]);
    assert_eq!(ranges[1].0, 8);
    assert_eq!(
        scan_ranges_in(&code, &ranges, Some(&nothing())),
        (Some(Finding::Uncovered { at: 6 }), vec![])
    );
    // A word that straddles two aligned words (the gap is 5..11).
    let (code, ranges) = pool(&[0, 0, 0, 0, 0xff, 0], &[]);
    assert_eq!(
        scan_ranges_in(&code, &ranges, Some(&nothing())),
        (Some(Finding::Uncovered { at: 9 }), vec![])
    );
    // After the last function.
    let mut tail = ENTRY_32.to_vec();
    tail.extend_from_slice(&RETW_N);
    let end = tail.len();
    tail.extend_from_slice(&[0, 0, 0]);
    tail.extend_from_slice(&DEAD);
    assert_eq!(
        scan_ranges_in(&tail, &[(0, end)], Some(&nothing())),
        (Some(Finding::Uncovered { at: end + 4 }), vec![])
    );
}

#[test]
fn at_most_sixty_four_bytes_of_dead_words_are_let_through() {
    let mut front = Vec::new();
    for _ in 0..16 {
        front.extend_from_slice(&DEAD);
    }
    let end = front.len();
    front.extend_from_slice(&ENTRY_32);
    front.extend_from_slice(&RETW_N);
    let (found, words) = scan_ranges_in(&front, &[(end, front.len())], Some(&nothing()));
    assert_eq!((found, words.len()), (None, 16));
    // One more.
    let mut more = DEAD.to_vec();
    more.extend_from_slice(&front);
    let (found, words) = scan_ranges_in(&more, &[(end + 4, more.len())], Some(&nothing()));
    assert_eq!(
        (found, words.len()),
        (Some(Finding::Uncovered { at: 65 }), 16)
    );
}

/// `.text` at 0x4200_0000 (a dead word at 8, a function at 12), `.rodata`
/// with `rodata` as its bytes, and `rela` relocations `(offset, target)`
/// against symbols at `.text + target`.
fn dead_image(rodata: &[u8], relocs: &[u32], debug: &[u32]) -> Vec<u8> {
    use crate::elf::build::Builder;
    let mut gap = vec![0, 0, 0];
    gap.extend_from_slice(&DEAD);
    let (code, ranges) = pool(&gap, &[]);
    let mut b = Builder::new(crate::elf::EM_XTENSA, crate::elf::ET_EXEC);
    let text = b.section(".text", 0x6, 0x4200_0000, &code, 16);
    for (n, &(start, end)) in ranges.iter().enumerate() {
        b.function(
            &format!("f{n}"),
            0x4200_0000 + start as u32,
            (end - start) as u32,
            text as u16,
            false,
        );
    }
    if !rodata.is_empty() {
        b.section(".rodata", 0x2, 0x3c00_0000, rodata, 4);
    }
    for (n, &target) in relocs.iter().enumerate() {
        let symbol = b.symbol(&format!("r{n}"), 0x4200_0000 + target, 0, text as u16, true);
        b.rela(text, 0, symbol, 1);
    }
    if !debug.is_empty() {
        let section = b.section(".debug_line", 0, 0, &[0; 16], 1);
        for &target in debug {
            let symbol = b.symbol("d", 0x4200_0000 + target, 0, text as u16, true);
            b.rela(section, 0, symbol, 1);
        }
    }
    b.build()
}

#[test]
fn check_lets_a_dead_word_through_unless_a_relocation_or_a_stored_address_reaches_it() {
    assert_eq!(check_of(&dead_image(&[], &[], &[])), Ok(()));
    // Data that holds other things, and the address of the function.
    assert_eq!(
        check_of(&dead_image(&0x4200_000cu32.to_le_bytes(), &[12], &[])),
        Ok(())
    );
    // A debug section's address (`.text + 8`: a line) reaches nothing.
    assert_eq!(check_of(&dead_image(&[], &[], &[8, 9])), Ok(()));
    // A relocation to any byte of the word.
    for target in [8, 9, 10, 11] {
        let err = check_of(&dead_image(&[], &[target], &[])).unwrap_err();
        assert!(err.contains("no `l32r` reads"), "{target}: {err}");
    }
    // A word of the image that holds an address in it.
    for address in [0x4200_0008u32, 0x4200_000b] {
        let err = check_of(&dead_image(&address.to_le_bytes(), &[], &[])).unwrap_err();
        assert!(err.contains("no `l32r` reads"), "{address:#x}: {err}");
    }
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

/// The scan over the code LLVM wrote for the S3: every `.text.*` section of
/// the objects (compiled with the `esp` toolchain, one function per section)
/// in the folder `PAPERPAW_XTENSA_CORPUS`. Run by hand:
/// `PAPERPAW_XTENSA_CORPUS=<folder of .o files> cargo test -p paperpaw-sign
/// -- --ignored corpus`. Compiler output has `entry` as its only change of
/// `sp`, so any other finding is a form the scan mislabels or misreads.
#[test]
#[ignore = "needs the objects of PAPERPAW_XTENSA_CORPUS"]
fn corpus_of_compiler_output_has_no_finding_but_frames() {
    let folder = std::env::var("PAPERPAW_XTENSA_CORPUS").expect("PAPERPAW_XTENSA_CORPUS");
    let (mut functions, mut bad) = (0usize, Vec::new());
    for entry in std::fs::read_dir(folder).expect("folder") {
        let path = entry.expect("entry").path();
        if path.extension().is_none_or(|e| e != "o") {
            continue;
        }
        let bytes = std::fs::read(&path).expect("object");
        let elf = Elf::parse(&bytes).expect("an ELF object");
        for section in elf.sections.iter().filter(|s| s.name.starts_with(".text.")) {
            let code = elf.data(section).expect("section data");
            functions += 1;
            match find(code) {
                None | Some(Finding::Frame { .. }) => {}
                Some(other) => bad.push(format!("{} {}: {other:?}", path.display(), section.name)),
            }
        }
    }
    assert!(
        functions > 100,
        "only {functions} sections: is the corpus there?"
    );
    assert!(
        bad.is_empty(),
        "{} of {functions}: {:#?}",
        bad.len(),
        &bad[..bad.len().min(10)]
    );
    eprintln!("corpus: {functions} sections scanned, no finding");
}
