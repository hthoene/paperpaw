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
/// `beq rs1, rs2, +offset` (offset even, -4096..4094).
fn beq_to(rs1: u32, rs2: u32, offset: i32) -> u32 {
    let o = offset as u32;
    ((o >> 12 & 1) << 31)
        | ((o >> 5 & 0x3f) << 25)
        | (rs2 << 20)
        | (rs1 << 15)
        | ((o >> 1 & 0xf) << 8)
        | ((o >> 11 & 1) << 7)
        | 0x63
}
/// `jalr x0, rs1, 0`: a jump through a register (`ret` when it is `ra`).
fn jr(rs1: u32) -> u32 {
    (rs1 << 15) | 0x67
}
const A0: u32 = 10;
const S0: u32 = 8;
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
    // `addi sp, s0, -N` with no frame pointer set in the function is not a
    // restore the scan can prove: refused.
    assert_eq!(
        found(&[W(addi(SP, 8, -2032))]),
        Some(Finding::Unknown { at: 0 })
    );
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
    // Set before a branch whose both ways keep it: a constant on every path.
    assert_eq!(
        found(&[W(addi(T0, 0, 64)), W(beq(0, 0)), W(sub(SP, SP, T0))]),
        None
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
        Some(Finding::Large { at: 4, bytes: 2544 })
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

#[test]
fn a_dynamic_alloca_is_refused_whatever_form_it_takes() {
    // clang: sub a0, sp, a0; mv sp, a0 (addi sp, a0, 0).
    assert_eq!(
        found(&[W(sub(A0, SP, A0)), W(addi(SP, A0, 0))]),
        Some(Finding::Unknown { at: 4 })
    );
    // The compressed move, c.mv sp, a0.
    assert_eq!(
        found(&[W(sub(A0, SP, A0)), N(c_mv(SP, A0))]),
        Some(Finding::Unknown { at: 4 })
    );
    // c.li sp, 1 and an add into sp from other registers.
    assert_eq!(found(&[N(c_li(SP, 1))]), Some(Finding::Unknown { at: 0 }));
    assert_eq!(
        found(&[W(add(SP, A0, T0))]),
        Some(Finding::Unknown { at: 0 })
    );
    // A load into sp, and a lui into it.
    let lw = (SP << 7) | (2 << 12) | (A0 << 15) | 0x03;
    assert_eq!(found(&[W(lw)]), Some(Finding::Unknown { at: 0 }));
    assert_eq!(found(&[W(lui(SP, 1))]), Some(Finding::Unknown { at: 0 }));
    // The frame pointer's restore, where the function set it from sp, is the
    // frame given back, not an alloca.
    assert_eq!(
        found(&[
            W(addi(SP, SP, -48)),
            W(addi(S0, SP, 48)),
            W(addi(SP, S0, -48)),
            W(RET)
        ]),
        None
    );
    assert_eq!(
        found(&[
            W(addi(SP, SP, -48)),
            N(c_mv(S0, SP)),
            N(c_mv(SP, S0)),
            W(RET)
        ]),
        None
    );
    // Without it set in the function (or written since), refused.
    assert_eq!(
        found(&[W(addi(SP, S0, -16))]),
        Some(Finding::Unknown { at: 0 })
    );
    assert_eq!(found(&[N(c_mv(SP, S0))]), Some(Finding::Unknown { at: 0 }));
    let lw_s0 = (S0 << 7) | (2 << 12) | (SP << 15) | 0x03;
    assert_eq!(
        found(&[W(addi(S0, SP, 0)), W(lw_s0), W(addi(SP, S0, 0))]),
        Some(Finding::Unknown { at: 8 })
    );
}

#[test]
fn a_constant_set_on_one_path_is_not_trusted_where_another_path_arrives() {
    // beq a1, zero, +8 jumps over `li a0, 16` to the `sub sp, sp, a0`: a0 is
    // not 16 there.
    assert_eq!(
        found(&[W(beq_to(11, 0, 8)), W(addi(A0, 0, 16)), W(sub(SP, SP, A0))]),
        Some(Finding::Unknown { at: 8 })
    );
    // A backward branch to the `sub` as well.
    assert_eq!(
        found(&[W(addi(A0, 0, 16)), W(sub(SP, SP, A0)), W(beq_to(11, 0, -4))]),
        Some(Finding::Unknown { at: 4 })
    );
    // The same without a branch to it stays known and small.
    assert_eq!(
        found(&[
            W(beq_to(11, 0, 12)),
            W(addi(A0, 0, 16)),
            W(sub(SP, SP, A0)),
            W(RET)
        ]),
        None
    );
    // An indirect jump (a switch): the code after it is entered with no
    // register known, so a size set before the jump is not trusted ...
    assert_eq!(
        found(&[W(addi(A0, 0, 16)), W(jr(T1)), W(sub(SP, SP, A0))]),
        Some(Finding::Unknown { at: 8 })
    );
    // ... and one built from immediates after it is a constant all the same.
    assert_eq!(
        found(&[W(jr(T1)), W(addi(A0, 0, 16)), W(sub(SP, SP, A0))]),
        None
    );
    // A return is not such a jump.
    assert_eq!(
        found(&[W(RET), W(addi(A0, 0, 16)), W(sub(SP, SP, A0))]),
        None
    );
}

#[test]
fn a_frame_taken_in_steps_counts_in_total() {
    // -1024 and -1024: 2048.
    assert_eq!(
        found(&[W(addi(SP, SP, -1024)), W(addi(SP, SP, -1024))]),
        Some(Finding::Large { at: 4, bytes: 2048 })
    );
    // -1024, back, -1024: 1024 at most.
    assert_eq!(
        found(&[
            W(addi(SP, SP, -1024)),
            W(addi(SP, SP, 1024)),
            W(addi(SP, SP, -1024))
        ]),
        None
    );
    // Each function by itself.
    let f = code(&[
        W(addi(SP, SP, -1024)),
        W(RET),
        W(addi(SP, SP, -1024)),
        W(RET),
    ]);
    assert_eq!(scan(&f, &[0, 8]), None);
}

#[test]
fn data_before_a_function_does_not_hide_its_start() {
    // Two bytes `03 00` (which look like the start of a 32-bit instruction)
    // and then a function at offset 2 with a large frame.
    let mut f = vec![0x03, 0x00];
    f.extend(code(&[W(addi(SP, SP, -1600)), W(RET)]));
    assert_eq!(scan(&f, &[2]), Some(Finding::Large { at: 2, bytes: 1600 }));
    assert_eq!(frame_sizes(&f, &[2]), vec![Some(1600)]);
}

#[test]
fn a_functions_frame_is_how_deep_sp_goes_below_its_start() {
    // Prologue -48, an early epilogue (+48, ret), the rest of the body, the
    // last epilogue: the frame is 48, not 0 and not 96.
    let f = code(&[
        W(addi(SP, SP, -48)),
        W(sw(1, SP)),
        W(addi(SP, SP, 48)),
        W(RET),
        W(sw(1, SP)),
        W(addi(SP, SP, 48)),
        W(RET),
    ]);
    assert_eq!(frame_sizes(&f, &[0]), vec![Some(48)]);
    // A frame pointer's restore leaves the frame as it was.
    let f = code(&[
        W(addi(SP, SP, -64)),
        W(addi(S0, SP, 64)),
        W(addi(SP, S0, -64)),
        W(RET),
    ]);
    assert_eq!(frame_sizes(&f, &[0]), vec![Some(64)]);
    // A dynamic one: unknown, for that function only.
    let f = code(&[W(sub(SP, SP, T1)), W(RET), W(addi(SP, SP, -32))]);
    assert_eq!(frame_sizes(&f, &[0, 8]), vec![None, Some(32)]);
}

#[test]
fn the_most_negative_sp_along_any_path_is_the_frame() {
    // A branch over a second allocation: one path takes -1900 more.
    let f = code(&[
        W(addi(SP, SP, -100)),
        W(beq_to(11, 0, 8)),
        W(addi(SP, SP, -1900)),
        W(addi(SP, SP, 1900)),
        W(addi(SP, SP, 100)),
        W(RET),
    ]);
    assert_eq!(frame_sizes(&f, &[0]), vec![Some(2000)]);
    // A loop that moves sp never settles: refused as unbounded.
    let f = code(&[W(addi(SP, SP, -16)), W(beq_to(0, 0, -4))]);
    assert_eq!(frame_sizes(&f, &[0]), vec![None]);
}

#[test]
fn a_register_built_frame_is_bounded_by_its_immediates_and_over_the_limit() {
    // What LLVM emits beyond addi's 2 KiB: lui t0,hi; addi t0,t0,lo; add sp,sp,t0.
    // -6448: lui t0, 0xffffe (-8192); addi t0, t0, 1744; add sp, sp, t0.
    let f = code(&[
        W(lui(T0, 0xffffe)),
        W(addi(T0, T0, 1744)),
        W(add(SP, SP, T0)),
        W(RET),
    ]);
    assert_eq!(frame_sizes(&f, &[0]), vec![Some(6448)]);
    assert_eq!(scan(&f, &[0]), Some(Finding::Large { at: 8, bytes: 6448 }));
    // From a register that is a constant on one path only: refused.
    let f = code(&[
        W(beq_to(11, 0, 8)),
        W(addi(T0, 0, 2000)),
        W(sub(SP, SP, T0)),
    ]);
    assert_eq!(scan(&f, &[0]), Some(Finding::Unknown { at: 8 }));
}

#[test]
fn code_that_no_function_covers_is_refused_unless_it_is_padding() {
    // A gap of zeros, c.nops and a nop between two functions: padding.
    let mut f = code(&[W(RET)]);
    f.extend([0, 0]);
    f.extend(code(&[N(C_NOP), W(0x0000_0013)]));
    let at = f.len();
    f.extend(code(&[W(RET)]));
    assert_eq!(uncovered(&f, &[(0, 4), (at, at + 4)]), None);
    // `addi sp, sp, -1600` in the gap: not covered, not padding.
    let mut f = code(&[W(RET)]);
    f.extend(code(&[W(addi(SP, SP, -1600)), W(RET)]));
    let at = f.len();
    f.extend(code(&[W(RET)]));
    assert_eq!(uncovered(&f, &[(0, 4), (at, at + 4)]), Some(4));
    // Code before the first function, and after the last.
    assert_eq!(uncovered(&code(&[W(addi(SP, SP, -1600))]), &[]), Some(0));
    let f = code(&[W(RET), W(addi(SP, SP, -1600))]);
    assert_eq!(uncovered(&f, &[(0, 4)]), Some(4));
    // A function's range ends where its symbol says: what lies beyond is read
    // as the neighbour's or not at all, never as part of it.
    let f = code(&[W(RET), W(addi(SP, SP, -1600))]);
    assert_eq!(scan_ranges(&f, &[(0, 4)], Env::none()), None);
}

#[test]
fn a_symbols_size_that_runs_into_its_neighbour_is_cut_at_the_neighbour() {
    // Two functions whose first symbol claims 100 bytes but the second starts
    // at 40: the first is read to 40, the second to its own end.
    assert_eq!(
        function_ranges(&[(0, 100), (40, 20)], 200),
        vec![(0, 40), (40, 60)]
    );
    // Aliases: one range, the larger size.
    assert_eq!(function_ranges(&[(8, 16), (8, 24)], 200), vec![(8, 32)]);
    // A size past the end of the code, a zero size, a start outside.
    assert_eq!(
        function_ranges(&[(0, 500), (300, 4), (10, 0)], 100),
        vec![(0, 100)]
    );
    // The bytes a cut leaves of the first symbol's claim belong to nobody
    // else: a function inside another leaves the rest of the outer one
    // uncovered, which the pack-time check then refuses unless padding.
    let r = function_ranges(&[(0, 100), (20, 10)], 200);
    assert_eq!(r, vec![(0, 20), (20, 30)]);
    let mut code = vec![0u8; 100];
    code[40..44].copy_from_slice(&addi(SP, SP, -1600).to_le_bytes());
    assert_eq!(uncovered(&code, &r), Some(40));
}

fn auipc(rd: u32, imm20: u32) -> u32 {
    (imm20 << 12) | (rd << 7) | 0x17
}
fn jalr(rd: u32, rs1: u32, imm: i32) -> u32 {
    ((imm as u32 & 0xfff) << 20) | (rs1 << 15) | (rd << 7) | 0x67
}

#[test]
fn code_after_a_call_that_never_returns_is_not_reached_from_it() {
    // A shrink-wrapped panic path, as rustc lays it out: the cold block
    // allocates its frame and calls a panic; the block after it is entered by
    // a branch only, and goes back to the cold block's start. Falling through
    // the call would make that a loop that moves sp.
    let f = code(&[
        W(beq_to(11, 0, 16)),            // 0: to 16
        W(addi(SP, SP, -16)),            // 4
        W(auipc(1, 0)),                  // 8: ra = pc
        W(jalr(1, 1, 24)),               // 12: call pc(8) + 24 = 32
        W(beq_to(12, 13, -12) | 0x1000), // 16: bne a2, a3, back to 4
        W(RET),                          // 20
    ]);
    let base = 0x4200_0000;
    let noreturn = [base + 32];
    let env = Env {
        base,
        noreturn: &noreturn,
    };
    assert_eq!(scan_ranges(&f, &[(0, 24)], env), None);
    assert_eq!(frame_sizes_in(&f, &[(0, 24)], env), vec![Some(16)]);
    // Without knowing that, the call falls through into a loop that moves sp.
    assert!(matches!(
        scan_ranges(&f, &[(0, 24)], Env::none()),
        Some(Finding::Unknown { .. })
    ));
}

#[test]
fn the_panics_of_the_languages_crates_are_the_functions_that_never_return() {
    for p in [
        "core::panicking::panic_fmt",
        "core::str::slice_error_fail",
        "core::slice::index::slice_index_order_fail",
        "core::result::unwrap_failed",
        "core::option::expect_failed",
        "alloc::alloc::handle_alloc_error",
        "alloc::raw_vec::capacity_overflow",
        "__rustc::rust_begin_unwind",
    ] {
        assert!(is_noreturn_path(p), "{p}");
    }
    for p in [
        "paperpaw_text::layout::pair",
        // A plugin's function named like a panic is not trusted not to return.
        "my_plugin::panicking::fail",
        "my_plugin::slice_error_fail",
        "core::fmt::write",
        "core::slice::sort::stable::merge",
    ] {
        assert!(!is_noreturn_path(p), "{p}");
    }
}
