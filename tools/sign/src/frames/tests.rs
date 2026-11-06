//! Hand-assembled sequences: what makes a frame, and what must not.

use super::*;

const SP: u32 = 2;
const T0: u32 = 5;
const T1: u32 = 6;

fn addi(rd: u32, rs1: u32, imm: i32) -> u32 {
    ((imm as u32 & 0xfff) << 20) | (rs1 << 15) | (rd << 7) | 0x13
}
fn lui(rd: u32, imm20: u32) -> u32 {
    (imm20 << 12) | (rd << 7) | 0x37
}
fn add(rd: u32, rs1: u32, rs2: u32) -> u32 {
    (rs2 << 20) | (rs1 << 15) | (rd << 7) | 0x33
}
fn sub(rd: u32, rs1: u32, rs2: u32) -> u32 {
    add(rd, rs1, rs2) | 0x4000_0000
}
fn beq(rs1: u32, rs2: u32) -> u32 {
    (rs2 << 20) | (rs1 << 15) | 0x63
}
fn sw(rs2: u32, rs1: u32) -> u32 {
    (rs2 << 20) | (rs1 << 15) | (2 << 12) | 0x23
}
const RET: u32 = 0x0000_8067;

/// c.addi16sp with `imm` (a multiple of 16, -512..=496).
fn c_addi16sp(imm: i32) -> u16 {
    let i = imm as u32;
    (0b011 << 13
        | (i >> 9 & 1) << 12
        | SP << 7
        | (i >> 4 & 1) << 6
        | (i >> 6 & 1) << 5
        | (i >> 7 & 3) << 3
        | (i >> 5 & 1) << 2
        | 0b01) as u16
}
fn c_li(rd: u32, imm: i32) -> u16 {
    let i = imm as u32;
    (0b010 << 13 | (i >> 5 & 1) << 12 | rd << 7 | (i & 31) << 2 | 0b01) as u16
}
fn c_lui(rd: u32, imm6: i32) -> u16 {
    let i = imm6 as u32;
    (0b011 << 13 | (i >> 5 & 1) << 12 | rd << 7 | (i & 31) << 2 | 0b01) as u16
}
fn c_addi(rd: u32, imm: i32) -> u16 {
    let i = imm as u32;
    ((i >> 5 & 1) << 12 | rd << 7 | (i & 31) << 2 | 0b01) as u16
}
fn c_mv(rd: u32, rs2: u32) -> u16 {
    (0b100 << 13 | rd << 7 | rs2 << 2 | 0b10) as u16
}
fn c_add(rd: u32, rs2: u32) -> u16 {
    (0b100 << 13 | 1 << 12 | rd << 7 | rs2 << 2 | 0b10) as u16
}
const C_JAL: u16 = 0x2001;
const C_NOP: u16 = 0x0001;

enum I {
    W(u32),
    N(u16),
}

fn code(items: &[I]) -> Vec<u8> {
    let mut out = Vec::new();
    for item in items {
        match item {
            I::W(w) => out.extend_from_slice(&w.to_le_bytes()),
            I::N(n) => out.extend_from_slice(&n.to_le_bytes()),
        }
    }
    out
}
use I::{N, W};

fn found(items: &[I]) -> Option<Finding> {
    scan(&code(items), &[0])
}

#[test]
fn an_addi_frame_at_the_limit_is_refused_and_one_below_it_is_not() {
    assert_eq!(
        found(&[W(addi(SP, SP, -2032)), W(RET)]),
        Some(Finding::Large { at: 0, bytes: 2032 })
    );
    assert_eq!(
        found(&[W(addi(SP, SP, -2048)), W(RET)]),
        Some(Finding::Large { at: 0, bytes: 2048 })
    );
    assert_eq!(
        found(&[W(addi(SP, SP, -1536))]),
        Some(Finding::Large { at: 0, bytes: 1536 })
    );
    assert_eq!(found(&[W(addi(SP, SP, -1535))]), None);
    assert_eq!(found(&[W(addi(SP, SP, -48)), W(RET)]), None);
}

#[test]
fn deallocation_and_sp_restores_are_not_frames() {
    // The epilogue's addi sp, sp, +N; `addi sp, s0, -N` (the frame pointer's).
    assert_eq!(found(&[W(addi(SP, SP, 2032))]), None);
    assert_eq!(found(&[W(addi(SP, 8, -2032))]), None);
    // Another register's adjustment is none of sp's.
    assert_eq!(found(&[W(addi(T0, T0, -2032))]), None);
}

#[test]
fn a_frame_built_in_a_register_is_found_through_lui_and_addi() {
    // lui t0, 0xffffe (-8192); add sp, sp, t0
    assert_eq!(
        found(&[W(lui(T0, 0xffffe)), W(add(SP, SP, T0))]),
        Some(Finding::Large { at: 4, bytes: 8192 })
    );
    // lui t0, 1; addi t0, t0, -96 (4000); sub sp, sp, t0
    assert_eq!(
        found(&[W(lui(T0, 1)), W(addi(T0, T0, -96)), W(sub(SP, SP, T0))]),
        Some(Finding::Large { at: 8, bytes: 4000 })
    );
    // li t0, 2000; sub sp, sp, t0
    assert_eq!(
        found(&[W(addi(T0, 0, 2000)), W(sub(SP, SP, T0))]),
        Some(Finding::Large { at: 4, bytes: 2000 })
    );
    // li t0, 512; sub sp, sp, t0: small.
    assert_eq!(found(&[W(addi(T0, 0, 512)), W(sub(SP, SP, T0))]), None);
}

#[test]
fn the_epilogue_that_gives_a_large_frame_back_is_not_a_frame() {
    // lui t0, 2; add sp, sp, t0 (+8192)
    assert_eq!(found(&[W(lui(T0, 2)), W(add(SP, SP, T0)), W(RET)]), None);
    // li t0, -100; add sp, sp, t0: a small negative one.
    assert_eq!(found(&[W(addi(T0, 0, -100)), W(add(SP, SP, T0))]), None);
}

#[test]
fn a_size_that_is_not_known_is_refused() {
    // sub sp, sp, t1 with nothing set before.
    assert_eq!(
        found(&[W(sub(SP, SP, T1))]),
        Some(Finding::Unknown { at: 0 })
    );
    assert_eq!(
        found(&[W(add(SP, SP, T1))]),
        Some(Finding::Unknown { at: 0 })
    );
    // Set before a branch: not trusted after it.
    assert_eq!(
        found(&[W(addi(T0, 0, 64)), W(beq(0, 0)), W(sub(SP, SP, T0))]),
        Some(Finding::Unknown { at: 8 })
    );
    // Overwritten by something the scan does not follow (a load).
    let lw = (T0 << 7) | (2 << 12) | (SP << 15) | 0x03;
    assert_eq!(
        found(&[W(addi(T0, 0, 64)), W(lw), W(sub(SP, SP, T0))]),
        Some(Finding::Unknown { at: 8 })
    );
}

#[test]
fn what_a_register_holds_is_not_carried_into_the_next_function() {
    // A store between (no jump, so nothing else forgets the value).
    let bytes = code(&[W(addi(T0, 0, 64)), W(sw(1, SP)), W(sub(SP, SP, T0))]);
    assert_eq!(scan(&bytes, &[0, 8]), Some(Finding::Unknown { at: 8 }));
    // The same bytes in one function: 64 is known and small.
    assert_eq!(scan(&bytes, &[0]), None);
}

#[test]
fn compressed_instructions_do_not_throw_the_walk_off() {
    // c.addi16sp -512 and -496 are small; c.nop is two bytes; the large
    // frame after them is found at its own offset.
    assert_eq!(
        found(&[N(c_addi16sp(-512)), N(c_addi16sp(-496)), N(C_NOP), W(RET)]),
        None
    );
    assert_eq!(
        found(&[N(c_addi16sp(-512)), N(C_NOP), W(addi(SP, SP, -2032))]),
        Some(Finding::Large { at: 4, bytes: 2032 })
    );
    // An odd number of 16-bit instructions before a 32-bit one.
    assert_eq!(
        found(&[N(C_NOP), W(sw(1, SP)), W(addi(SP, SP, -1600))]),
        Some(Finding::Large { at: 6, bytes: 1600 })
    );
}

#[test]
fn compressed_forms_build_a_size_too() {
    // c.lui t0, -2 (-8192); c.add sp, t0
    assert_eq!(
        found(&[N(c_lui(T0, -2)), N(c_add(SP, T0))]),
        Some(Finding::Large { at: 2, bytes: 8192 })
    );
    // c.li t0, 31; c.mv t1, t0; c.addi t1, -1; sub sp, sp, t1: 30, small.
    assert_eq!(
        found(&[
            N(c_li(T0, 31)),
            N(c_mv(T1, T0)),
            N(c_addi(T1, -1)),
            W(sub(SP, SP, T1))
        ]),
        None
    );
    // c.li then c.jal: forgotten.
    assert_eq!(
        found(&[N(c_li(T0, 31)), N(C_JAL), W(sub(SP, SP, T0))]),
        Some(Finding::Unknown { at: 4 })
    );
}

#[test]
fn code_that_ends_in_the_middle_of_an_instruction_is_not_a_frame() {
    assert_eq!(scan(&[0x13, 0x01], &[0]), None);
    assert_eq!(scan(&[], &[]), None);
}
