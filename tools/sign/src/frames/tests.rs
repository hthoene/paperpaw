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
    // The limit is the band's height (2 KiB): a frame of less is let through,
    // one of that or more is not.
    assert_eq!(LIMIT, 2048);
    assert_eq!(found(&[W(addi(SP, SP, -2032)), W(RET)]), None);
    assert_eq!(
        found(&[W(addi(SP, SP, -2048)), W(RET)]),
        Some(Finding::Large { at: 0, bytes: 2048 })
    );
    // 1,952 bytes: `core`'s float formatting.
    assert_eq!(found(&[W(addi(SP, SP, -1952)), W(RET)]), None);
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
    // li t0, 2000; sub sp, sp, t0: under the limit
    assert_eq!(found(&[W(addi(T0, 0, 2000)), W(sub(SP, SP, T0))]), None);
    // li t0, 1; slli: no. lui t0, 1; sub sp, sp, t0 (4096)
    assert_eq!(
        found(&[W(lui(T0, 1)), W(sub(SP, SP, T0))]),
        Some(Finding::Large { at: 4, bytes: 4096 })
    );
    // li t0, 512; sub sp, sp, t0: small.
    assert_eq!(found(&[W(addi(T0, 0, 512)), W(sub(SP, SP, T0))]), None);
}

#[test]
fn the_epilogue_that_gives_a_large_frame_back_is_not_a_frame() {
    // lui t0, 2; add sp, sp, t0 (+8192)
    assert_eq!(found(&[W(lui(T0, 2)), W(add(SP, SP, T0)), W(RET)]), None);
    // li t0, -96; add sp, sp, t0: a small negative one.
    assert_eq!(found(&[W(addi(T0, 0, -96)), W(add(SP, SP, T0))]), None);
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
        found(&[N(C_NOP), W(sw(1, SP)), W(addi(SP, SP, -2048))]),
        Some(Finding::Large { at: 6, bytes: 2048 })
    );
}

#[test]
fn compressed_forms_build_a_size_too() {
    // c.lui t0, -2 (-8192); c.add sp, t0
    assert_eq!(
        found(&[N(c_lui(T0, -2)), N(c_add(SP, T0))]),
        Some(Finding::Large { at: 2, bytes: 8192 })
    );
    // c.li t0, 31; c.mv t1, t0; c.addi t1, 1; sub sp, sp, t1: 32, small.
    assert_eq!(
        found(&[
            N(c_li(T0, 31)),
            N(c_mv(T1, T0)),
            N(c_addi(T1, 1)),
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
    // Half of a 32-bit instruction: refused, not clipped away.
    assert_eq!(scan(&[0x13, 0x01], &[0]), Some(Finding::Unknown { at: 0 }));
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
    // An indirect jump (a tail call, with no frame): the code after it may
    // be entered from it with no register known, so a size set before the
    // jump is not trusted ...
    assert_eq!(
        found(&[W(addi(A0, 0, 16)), W(jr(T1)), W(sub(SP, SP, A0))]),
        Some(Finding::Unknown { at: 8 })
    );
    // ... and one built from immediates after it is not either: the jump may
    // go past the `li`.
    assert_eq!(
        found(&[W(jr(T1)), W(addi(A0, 0, 16)), W(sub(SP, SP, A0))]),
        Some(Finding::Unknown { at: 8 })
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
    f.extend(code(&[W(addi(SP, SP, -2048)), W(RET)]));
    assert_eq!(scan(&f, &[2]), Some(Finding::Large { at: 2, bytes: 2048 }));
    assert_eq!(frame_sizes(&f, &[2]), vec![Some(2048)]);
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
        W(addi(SP, SP, -96)),
        W(beq_to(11, 0, 8)),
        W(addi(SP, SP, -1904)),
        W(addi(SP, SP, 1904)),
        W(addi(SP, SP, 96)),
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
        tables: Tables::none(),
        lenient: false,
    };
    assert_eq!(scan_ranges(&f, &[(0, 24)], env), None);
    assert_eq!(frame_sizes_in(&f, &[(0, 24)], env), vec![Some(16)]);
    // Without knowing that, the call falls through into a loop that moves sp.
    assert!(matches!(
        scan_ranges(&f, &[(0, 24)], Env::none()),
        Some(Finding::Unknown { .. })
    ));
}

/// `jal x0, +0`: a jump to itself.
const SPIN: u32 = 0x0000_006f;
const EBREAK: u32 = 0x0010_0073;

#[test]
fn a_function_that_never_returns_is_proved_from_its_code_not_its_name() {
    // 0: spin; 4: calls the spinner and nothing else; 12: ebreak; 16: returns
    // (a function named like a panic that gives a bool back); 20: calls that
    // one and then returns itself.
    let f = code(&[
        W(SPIN),           // 0
        W(auipc(1, 0)),    // 4: ra = pc
        W(jalr(1, 1, -4)), // 8: call pc(4) - 4 = 0
        W(EBREAK),         // 12
        W(RET),            // 16
        W(auipc(1, 0)),    // 20
        W(jalr(1, 1, -4)), // 24: call pc(20) - 4 = 16
        W(RET),            // 28
    ]);
    let ranges = [(0, 4), (4, 12), (12, 16), (16, 20), (20, 32)];
    let base = 0x4200_0000;
    let proved = noreturn_functions(&f, &ranges, base, Tables::none(), false);
    // The spinner, its caller (the call never comes back) and the trap
    // never return; the one with `ret` does, and so does its caller.
    assert_eq!(proved, vec![base, base + 4, base + 12]);
}

#[test]
fn a_panic_like_name_does_not_end_the_path_of_a_call_that_returns() {
    // Two 1024-byte allocations with a call between them to a function that
    // returns (`std::panicking::panicking` gives a bool back): 2048 bytes.
    let f = code(&[
        W(addi(SP, SP, -1024)),
        W(auipc(1, 0)),
        W(jalr(1, 1, 24)), // call pc(4) + 24 = 28
        W(addi(SP, SP, -1024)),
        W(addi(SP, SP, 1024)),
        W(addi(SP, SP, 1024)),
        W(RET),
        W(RET), // 28: the callee returns
    ]);
    let ranges = [(0, 28), (28, 32)];
    let base = 0x4200_0000;
    let proved = noreturn_functions(&f, &ranges, base, Tables::none(), false);
    assert_eq!(proved, Vec::<u32>::new());
    let env = Env {
        base,
        noreturn: &proved,
        tables: Tables::none(),
        lenient: false,
    };
    assert!(matches!(
        scan_ranges(&f, &ranges, env),
        Some(Finding::Large { bytes: 2048, .. })
    ));
}

#[test]
fn a_jump_through_a_register_inside_a_frame_is_refused() {
    // The repro: the frame grows each time a0 sends the jump to the start.
    assert_eq!(
        found(&[W(addi(SP, SP, -1024)), W(jr(A0))]),
        Some(Finding::Table { at: 4 })
    );
    // With no frame (a tail call through a vtable, `jr a5` after the frame
    // is given back) it is a way out of the function.
    assert_eq!(
        found(&[W(addi(SP, SP, -16)), W(addi(SP, SP, 16)), W(jr(A0))]),
        None
    );
    // A jump that is a frame's growth: the frame given back, then `jr`, but
    // the jump goes back to a frame allocation.
    assert_eq!(
        found(&[W(addi(SP, SP, -1024)), W(addi(SP, SP, 1024)), W(jr(A0))]),
        None
    );
    // In a lenient walk (the firmware's own listing) the jump's cases are
    // read from the state at the jump, but the function is not sized: the
    // frame is a lower bound, which the listing says and a budget refuses.
    let f = code(&[W(addi(SP, SP, -1024)), W(jr(A0))]);
    let env = Env {
        lenient: true,
        ..Env::none()
    };
    assert_eq!(frame_sizes_in(&f, &[(0, 8)], env), vec![None]);
    assert_eq!(
        sizes_in(&f, &[(0, 8)], env),
        vec![Sized {
            bytes: None,
            at_least: 1024
        }]
    );
}

#[test]
fn an_instruction_cut_by_a_symbols_end_is_refused() {
    // `addi sp, sp, -1600` is four bytes; ranges of two bytes each hold half.
    let f = code(&[W(addi(SP, SP, -1600))]);
    assert_eq!(
        scan_ranges(&f, &[(0, 2), (2, 4)], Env::none()),
        Some(Finding::Unknown { at: 0 })
    );
    // A function that does not start on a halfword.
    assert!(matches!(
        scan_ranges(&f, &[(1, 4)], Env::none()),
        Some(Finding::Unknown { .. })
    ));
}

#[test]
fn many_paths_into_one_instruction_do_not_overflow_the_visit_count() {
    // 300 branches, each at a lower sp, into the one `ret`: it is visited
    // again each time. The walk gives up on it, and does not panic.
    let mut items = Vec::new();
    let end = 300 * 8;
    for k in 0..300i32 {
        items.push(W(addi(SP, SP, -4)));
        items.push(W(beq_to(11, 0, end - (k * 8 + 4))));
    }
    items.push(W(RET));
    assert!(found(&items).is_some());
}

#[test]
fn a_mangled_name_with_a_length_that_overflows_is_no_name() {
    assert_eq!(path("_ZN18446744073709551615xE"), "");
    assert_eq!(
        path("_ZN4core9panicking5panic17h0123456789abcdefE"),
        "core::panicking::panic"
    );
}

/// A small deterministic generator for the fuzz-like tests below.
fn xorshift(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

#[test]
fn random_code_and_ranges_never_panic_the_scan() {
    // Debug builds check every overflow: this is where a counter or a length
    // that wraps would show. Random bytes (mostly branches and sp writes are
    // not likely, so some runs are seeded with them), ranges that cut
    // instructions, overlap, start off a halfword or run past the code.
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    for round in 0..4000 {
        let len = (xorshift(&mut seed) % 400) as usize;
        let mut bytes: Vec<u8> = (0..len).map(|_| xorshift(&mut seed) as u8).collect();
        if round % 2 == 0 {
            // Many branches and stack changes among the noise.
            for chunk in bytes.as_chunks_mut::<4>().0 {
                match xorshift(&mut seed) % 4 {
                    0 => *chunk = addi(SP, SP, -16).to_le_bytes(),
                    1 => *chunk = beq_to(11, 0, -8).to_le_bytes(),
                    2 => *chunk = jr(A0).to_le_bytes(),
                    _ => {}
                }
            }
        }
        let n = (xorshift(&mut seed) % 6) as usize;
        let ranges: Vec<(usize, usize)> = (0..n)
            .map(|_| {
                let s = (xorshift(&mut seed) % (len as u64 + 8)) as usize;
                let e = s + (xorshift(&mut seed) % (len as u64 + 8)) as usize;
                (s, e)
            })
            .collect();
        let base = xorshift(&mut seed) as u32;
        // A random .rodata whose words point into the code, and random
        // relocated offsets: the tables the code may jump by.
        let table_bytes: Vec<u8> = (0..64)
            .map(|k| {
                if k % 4 == 0 {
                    base.wrapping_add((xorshift(&mut seed) % (len as u64 + 8)) as u32) as u8
                } else {
                    (xorshift(&mut seed) >> 8) as u8
                }
            })
            .collect();
        let relocated: std::collections::BTreeSet<u32> = (0..16)
            .map(|_| (xorshift(&mut seed) % 20) as u32 * 4)
            .collect();
        let tables = Tables {
            addr: base,
            bytes: &table_bytes,
            relocated: Some(&relocated),
        };
        let proved = noreturn_functions(&bytes, &ranges, base, tables, false);
        for lenient in [false, true] {
            let env = Env {
                base,
                noreturn: &proved,
                tables,
                lenient,
            };
            let _ = scan_ranges(&bytes, &ranges, env);
            let _ = frame_sizes_in(&bytes, &ranges, env);
        }
        let _ = uncovered(&bytes, &ranges);
    }
}

#[test]
fn random_symbol_names_never_panic_the_path_reader() {
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let alphabet = b"_ZNRC0123456789abcdefhsxE";
    for _ in 0..20000 {
        let len = (xorshift(&mut seed) % 60) as usize;
        let mut name = String::from(["_ZN", "_R", ""][(xorshift(&mut seed) % 3) as usize]);
        for _ in 0..len {
            name.push(alphabet[(xorshift(&mut seed) % alphabet.len() as u64) as usize] as char);
        }
        let _ = path(&name);
    }
    // Lengths at the limits of usize and u64.
    for n in ["18446744073709551615", "4294967295", "99999999999999999999"] {
        let _ = path(&format!("_ZN{n}x4core5panicE"));
        let _ = path(&format!("_RNvC{n}x"));
    }
}

/// `slli rd, rs1, shamt`.
fn slli(rd: u32, rs1: u32, shamt: u32) -> u32 {
    (shamt << 20) | (rs1 << 15) | (1 << 12) | (rd << 7) | 0x13
}
fn lw(rd: u32, rs1: u32, imm: i32) -> u32 {
    ((imm as u32 & 0xfff) << 20) | (rs1 << 15) | (2 << 12) | (rd << 7) | 0x03
}
const A1: u32 = 11;

/// A function that jumps by a table: its frame of 16, a `lui` of the table's
/// address, the index scaled, added and loaded, the jump. Cases follow at 24
/// (give the frame back and return) and 32 (the same).
fn table_function(case1: &[I]) -> Vec<u8> {
    let mut items = vec![
        W(addi(SP, SP, -16)),
        W(lui(A1, 0x3c000)),
        W(slli(A0, A0, 2)),
        W(add(A0, A0, A1)),
        W(lw(A0, A0, 0)),
        W(jr(A0)),
        // 24: case 0
        W(addi(SP, SP, 16)),
        W(RET),
    ];
    // 32: case 1
    items.extend(case1.iter().map(|i| match i {
        W(w) => W(*w),
        N(n) => N(*n),
    }));
    code(&items)
}

const ROD: u32 = 0x3c00_0000;
const TEXT: u32 = 0x4200_0000;

fn rodata(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

fn with_table(f: &[u8], words: &[u32], relocated: &[u32]) -> Option<Finding> {
    let bytes = rodata(words);
    let relocated: std::collections::BTreeSet<u32> = relocated.iter().copied().collect();
    let env = Env {
        tables: Tables {
            addr: ROD,
            bytes: &bytes,
            relocated: Some(&relocated),
        },
        ..Env::none()
    };
    scan_ranges(f, &[(0, f.len())], env)
}

#[test]
fn a_jump_table_is_followed_to_each_of_its_entries() {
    let case1 = [W(addi(SP, SP, 16)), W(RET)];
    let f = table_function(&case1);
    // Both entries are labels in the function, both relocated words.
    assert_eq!(with_table(&f, &[TEXT + 24, TEXT + 32], &[0, 4]), None);
    // Every entry's state is checked: a case that takes a big frame.
    let big = [W(addi(SP, SP, -2032)), W(addi(SP, SP, 2032)), W(RET)];
    let f = table_function(&big);
    assert!(matches!(
        with_table(&f, &[TEXT + 24, TEXT + 32], &[0, 4]),
        Some(Finding::Large { bytes: 2048, .. })
    ));
    // The same without a table to read: a jump through a register in a frame.
    assert!(matches!(
        scan_ranges(&f, &[(0, f.len())], Env::none()),
        Some(Finding::Table { .. })
    ));
}

#[test]
fn a_table_whose_entry_goes_back_to_the_start_grows_the_frame_and_is_refused() {
    let case1 = [W(addi(SP, SP, 16)), W(RET)];
    let f = table_function(&case1);
    // Entry 1 is the first instruction: each round takes 16 more.
    assert!(matches!(
        with_table(&f, &[TEXT + 24, TEXT], &[0, 4]),
        Some(Finding::Unknown { .. })
    ));
}

#[test]
fn a_table_that_cannot_be_read_is_a_jump_through_a_register() {
    let case1 = [W(addi(SP, SP, 16)), W(RET)];
    let f = table_function(&case1);
    let table = |words: &[u32], relocated: &[u32]| with_table(&f, words, relocated);
    // The first word points out of the function: no entry.
    assert!(matches!(
        table(&[TEXT + 4096, TEXT + 32], &[0, 4]),
        Some(Finding::Table { .. })
    ));
    // A word that is not relocated (it would not move with the plugin).
    assert!(matches!(
        table(&[TEXT + 24, TEXT + 32], &[4]),
        Some(Finding::Table { .. })
    ));
    // An entry into the middle of an instruction.
    assert!(matches!(
        table(&[TEXT + 26, TEXT + 32], &[0, 4]),
        Some(Finding::Table { .. })
    ));
    // A table outside .rodata.
    assert!(matches!(table(&[], &[]), Some(Finding::Table { .. })));
}

#[test]
fn a_table_ends_at_the_first_word_that_points_elsewhere() {
    // Entries 0 and 1 are the function's; the next word is the start of
    // another function's table: not read.
    let case1 = [W(addi(SP, SP, 16)), W(RET)];
    let f = table_function(&case1);
    assert_eq!(
        with_table(&f, &[TEXT + 24, TEXT + 32, TEXT + 4096], &[0, 4, 8]),
        None
    );
}

#[test]
fn the_compressed_forms_of_the_table_jump_are_followed_too() {
    // c.slli a0, 2; c.add a0, a1; c.lw a0, 0(a0); c.jr a0
    let c_slli = |rd: u32, shamt: u32| ((rd << 7) | (shamt << 2) | 0b10) as u16;
    let c_lw = 0x4108u16; // c.lw a0, 0(a0)
    let c_jr = 0x8502u16; // c.jr a0
    let f = code(&[
        W(addi(SP, SP, -16)),
        W(lui(A1, 0x3c000)),
        N(c_slli(A0, 2)),
        N(c_add(A0, A1)),
        N(c_lw),
        N(c_jr),
        // 16: the case
        W(addi(SP, SP, 16)),
        W(RET),
    ]);
    assert_eq!(with_table(&f, &[TEXT + 16], &[0]), None);
    // And with the case's frame too big.
    let g = code(&[
        W(addi(SP, SP, -16)),
        W(lui(A1, 0x3c000)),
        N(c_slli(A0, 2)),
        N(c_add(A0, A1)),
        N(c_lw),
        N(c_jr),
        W(addi(SP, SP, -2032)),
        W(RET),
    ]);
    assert!(matches!(
        with_table(&g, &[TEXT + 16], &[0]),
        Some(Finding::Large { bytes: 2048, .. })
    ));
}

#[test]
fn a_table_longer_than_the_function_is_refused_not_cut() {
    // The function has 11 instructions. A table of 12 words that all point
    // into it, the last the only way into a case with a big frame: reading
    // only the first 11 would miss it, so the table is not read at all.
    let big = [W(addi(SP, SP, -2032)), W(addi(SP, SP, 2032)), W(RET)];
    let f = table_function(&big);
    let mut words = vec![TEXT + 24; 11];
    words.push(TEXT + 32);
    let relocated: Vec<u32> = (0..12).map(|k| k * 4).collect();
    assert!(matches!(
        with_table(&f, &words, &relocated),
        Some(Finding::Table { .. })
    ));
    // The same table of exactly 11 words, the last the big case, is read to
    // its end: the case is found.
    let mut words = vec![TEXT + 24; 10];
    words.push(TEXT + 32);
    assert!(matches!(
        with_table(&f, &words, &relocated[..11]),
        Some(Finding::Large { bytes: 2048, .. })
    ));
    // Many entries for the same target are one target.
    let case1 = [W(addi(SP, SP, 16)), W(RET)];
    let g = table_function(&case1);
    let words = vec![TEXT + 24; 8];
    assert_eq!(with_table(&g, &words, &relocated[..8]), None);
}

/// The firmware's walk: the code is linked in place, so no word is relocated
/// and `relocated` is `None`.
fn linked_sizes(f: &[u8], words: &[u32]) -> Sized {
    let bytes = rodata(words);
    let env = Env {
        tables: Tables {
            addr: ROD,
            bytes: &bytes,
            relocated: None,
        },
        lenient: true,
        ..Env::none()
    };
    sizes_in(f, &[(0, f.len())], env)[0]
}

#[test]
fn a_linked_firmwares_table_is_resolved_and_its_function_is_sized() {
    let big = [W(addi(SP, SP, -1600)), W(addi(SP, SP, 1616)), W(RET)];
    let f = table_function(&big);
    // Both entries are addresses in the function: every case is walked from
    // the jump, the frame is the largest one, and it is not a lower bound.
    let s = linked_sizes(&f, &[TEXT + 24, TEXT + 32]);
    assert_eq!(s.bytes, Some(1616));
    // A table that cannot be read leaves the function not sized, with what
    // the walk saw as its lower bound.
    let s = linked_sizes(&f, &[]);
    assert_eq!(s.bytes, None);
    let s = linked_sizes(&f, &[0x1234_5678]);
    assert_eq!(s.bytes, None);
}

#[test]
fn a_linked_table_ends_at_the_first_word_that_is_not_in_the_function() {
    let big = [W(addi(SP, SP, -1600)), W(addi(SP, SP, 1616)), W(RET)];
    let f = table_function(&big);
    // Data after the table (a word out of the function) is not an entry.
    let s = linked_sizes(&f, &[TEXT + 24, TEXT + 32, 0x3c00_1234, TEXT + 32]);
    assert_eq!(s.bytes, Some(1616));
    // Into the middle of an instruction: the table is refused (not sized).
    let s = linked_sizes(&f, &[TEXT + 24, TEXT + 34]);
    assert_eq!(s.bytes, None);
}

#[test]
fn an_unreadable_jump_at_sp_zero_may_enter_code_already_walked_in_the_listing_too() {
    // Code Reviewer's probe (#254): one path sets t0 = 16 and lowers sp by
    // it; another jumps through a register that cannot be read, at sp 0, and
    // may land on the `sub` with any t0. The listing joined that state into
    // nothing already walked and sized the function 16; it must not.
    let f = code(&[
        W(beq_to(A0, 0, 12)),
        W(jr(A1)),
        W(RET),
        W(addi(T0, 0, 16)),
        W(sub(SP, SP, T0)),
        W(addi(SP, SP, 16)),
        W(RET),
    ]);
    for lenient in [false, true] {
        let env = Env {
            lenient,
            ..Env::none()
        };
        assert_eq!(
            sizes_in(&f, &[(0, f.len())], env)[0].bytes,
            None,
            "lenient {lenient}"
        );
    }
}

#[test]
fn a_tail_call_by_auipc_and_jr_is_a_known_jump_out_not_an_unreadable_one() {
    const T1: u32 = 6;
    // auipc t1, 0x10; jr 0x20(t1): the compilers' `tail`, to a function
    // elsewhere. Known, so nothing is joined into the rest of the function.
    let f = code(&[W(auipc(T1, 0x10)), W(jalr(0, T1, 0x20))]);
    assert_eq!(scan(&f, &[0]), None);
    assert_eq!(frame_sizes(&f, &[0]), vec![Some(0)]);
    // Into the function itself (a jump back to its start with sp lowered once
    // would be a loop that moves sp: refused as unbounded).
    let f = code(&[W(addi(SP, SP, -16)), W(auipc(T1, 0)), W(jalr(0, T1, -4))]);
    assert_eq!(frame_sizes(&f, &[0]), vec![None]);
}

#[test]
fn a_jump_table_beside_a_tail_call_is_still_read() {
    const T1: u32 = 6;
    // Case 1 gives its frame back and tail-calls out. An unreadable jump at
    // sp 0 would be joined into every instruction, the table's code included,
    // and the table could no longer be read; this one is known.
    let case1 = [W(addi(SP, SP, 16)), W(auipc(T1, 1)), W(jalr(0, T1, 0))];
    let f = table_function(&case1);
    assert_eq!(with_table(&f, &[TEXT + 24, TEXT + 32], &[0, 4]), None);
    assert_eq!(linked_sizes(&f, &[TEXT + 24, TEXT + 32]).bytes, Some(16));
}

#[test]
fn a_tail_call_target_wraps_in_32_bits_and_clears_the_low_bit_as_jalr_does() {
    // Code Reviewer's probe (#254): `li t1, -16; jalr x0, 16(t1)` goes to
    // address 0 (the sum wraps), the function's start when it is linked at 0:
    // a loop that moves sp, not a jump far out of the function.
    const T1: u32 = 6;
    let at_zero = |items: &[I]| {
        let f = code(items);
        let env = Env {
            base: 0,
            ..Env::none()
        };
        sizes_in(&f, &[(0, f.len())], env)[0].bytes
    };
    assert_eq!(
        at_zero(&[
            W(addi(SP, SP, -16)),
            W(addi(T1, 0, -16)),
            W(jalr(0, T1, 16)),
        ]),
        None
    );
    // An odd sum has its low bit cleared: 0 + 1 is address 0, the start.
    assert_eq!(
        at_zero(&[W(addi(SP, SP, -16)), W(addi(T1, 0, 0)), W(jalr(0, T1, 1)),]),
        None
    );
    // A target out of the function stays a tail call.
    assert_eq!(
        at_zero(&[W(addi(T1, 0, 0x100)), W(jalr(0, T1, 0x20)), W(RET)]),
        Some(0)
    );
}

#[test]
fn a_change_of_sp_that_is_not_a_multiple_of_16_is_refused() {
    // The psABI keeps sp 16-aligned and the stack band's reasoning assumes it.
    let f = code(&[W(addi(SP, SP, -12)), W(addi(SP, SP, 12)), W(RET)]);
    assert_eq!(scan(&f, &[0]), Some(Finding::Misaligned { at: 0 }));
    // Aligned: fine, however it is reached (two steps of 16, then back).
    let f = code(&[
        W(addi(SP, SP, -16)),
        W(addi(SP, SP, -32)),
        W(addi(SP, SP, 48)),
        W(RET),
    ]);
    assert_eq!(scan(&f, &[0]), None);
    // Two aligned-looking halves that add up to a misaligned sp.
    let f = code(&[
        W(addi(SP, SP, -16)),
        W(addi(SP, SP, 4)),
        W(addi(SP, SP, 12)),
        W(RET),
    ]);
    assert_eq!(scan(&f, &[0]), Some(Finding::Misaligned { at: 4 }));
    // The listing of the firmware does not size such a function.
    let f = code(&[W(addi(SP, SP, -12)), W(addi(SP, SP, 12)), W(RET)]);
    assert_eq!(frame_sizes(&f, &[0]), vec![None]);
}

/// A table jump whose base lives in `s8` across a call that links in `link`
/// (`ra`, a call; `t0`, one of the compiler's outlined sequences). Case 0 at
/// 32 gives the frame back; case 1 at 40 takes a large one.
fn table_by_saved_register(link: u32) -> Vec<u8> {
    const S8: u32 = 24;
    code(&[
        W(addi(SP, SP, -16)),
        W(lui(S8, 0x3c000)),
        W(auipc(6, 0)),
        W(jalr(link, 6, 0)),
        W(slli(A0, A0, 2)),
        W(add(A0, A0, S8)),
        W(lw(A0, A0, 0)),
        W(jr(A0)),
        // 32
        W(addi(SP, SP, 16)),
        W(RET),
        // 40
        W(addi(SP, SP, -2032)),
        W(addi(SP, SP, 1024)),
        W(addi(SP, SP, 1024)),
        W(RET),
    ])
}

#[test]
fn a_tables_base_in_a_callee_saved_register_survives_a_call() {
    // The compilers keep the base of a switch's table in `s8` across the
    // calls of the arms before it; the calling convention keeps it.
    let f = table_by_saved_register(1);
    assert!(matches!(
        with_table(&f, &[TEXT + 32, TEXT + 40], &[0, 4]),
        Some(Finding::Large { bytes: 2048, .. })
    ));
    assert_eq!(linked_sizes(&f, &[TEXT + 32, TEXT + 40]).bytes, Some(2048));
}

/// A function that sets a table's base in `s8`, calls an outlined sequence
/// (`auipc t0, 0; jalr t0, 48(t0)`, linked in `t0`; the sequence follows the
/// function at 56 and ends in `jr t0`) and jumps by the table. Case 0 (at 32)
/// gives the frame back, case 1 (at 40) takes a large one.
fn table_after_outlined_call(sequence: &[I]) -> Vec<u8> {
    const T0: u32 = 5;
    const S8: u32 = 24;
    let mut items = vec![
        W(addi(SP, SP, -16)),
        W(lui(S8, 0x3c000)),
        W(auipc(T0, 0)),
        W(jalr(T0, T0, 48)),
        W(slli(A0, A0, 2)),
        W(add(A0, A0, S8)),
        W(lw(A0, A0, 0)),
        W(jr(A0)),
        W(addi(SP, SP, 16)),
        W(RET),
        W(addi(SP, SP, -2032)),
        W(addi(SP, SP, 2032)),
        W(addi(SP, SP, 16)),
        W(RET),
    ];
    items.extend(sequence.iter().map(|i| match i {
        W(w) => W(*w),
        N(n) => N(*n),
    }));
    code(&items)
}

/// The listing's frame of [`table_after_outlined_call`]'s function (firmware
/// mode: linked in place, the tables' words are addresses).
fn outlined_frame(sequence: &[I]) -> Option<u32> {
    let f = table_after_outlined_call(sequence);
    let bytes = rodata(&[TEXT + 32, TEXT + 40]);
    let env = Env {
        tables: Tables {
            addr: ROD,
            bytes: &bytes,
            relocated: None,
        },
        lenient: true,
        ..Env::none()
    };
    sizes_in(&f, &[(0, 56)], env)[0].bytes
}

#[test]
fn an_outlined_call_forgets_only_the_registers_its_sequence_writes() {
    const T0: u32 = 5;
    const S9: u32 = 25;
    // The sequence writes `s9` and returns by `jr t0`: `s8` still holds the
    // table's base, the table is read and case 1 is found.
    assert_eq!(outlined_frame(&[W(addi(S9, 0, 1)), W(jr(T0))]), Some(2048));
}

#[test]
fn an_outlined_sequence_that_writes_the_base_register_or_is_not_straight_forgets_it() {
    const T0: u32 = 5;
    const S8: u32 = 24;
    // The sequence writes `s8`: the base is lost, the jump cannot be read.
    assert_eq!(outlined_frame(&[W(addi(S8, 0, 1)), W(jr(T0))]), None);
    // A sequence with a branch is not read: all registers are forgotten.
    assert_eq!(outlined_frame(&[W(beq_to(0, 0, 8)), W(jr(T0))]), None);
    // One that never ends in `jr t0` within 64 instructions, likewise.
    let long: Vec<I> = (0..70).map(|_| W(addi(25, 0, 1))).collect();
    assert_eq!(outlined_frame(&long), None);
}

/// `jal rd, +offset` (offset even, -1 MiB..1 MiB).
fn jal(rd: u32, offset: i32) -> u32 {
    let o = offset as u32;
    ((o >> 20 & 1) << 31)
        | ((o >> 1 & 0x3ff) << 21)
        | ((o >> 11 & 1) << 20)
        | ((o >> 12 & 0xff) << 12)
        | (rd << 7)
        | 0x6f
}
/// `jalr x0, 0(t0)`: the end of an outlined sequence.
const JR_T0: u32 = 0x0002_8067;

#[test]
fn an_outlined_sequence_that_lowers_sp_is_part_of_the_callers_frame() {
    // `-msave-restore`: `jal t0, __riscv_save_0`, whose code lowers sp by its
    // save area and goes back through t0. The caller's own frame comes on top.
    let f = code(&[
        W(addi(SP, SP, -2000)), // 0
        W(jal(T0, 16)),         // 4: to the helper at 20
        W(addi(SP, SP, 2000)),  // 8
        W(RET),                 // 12
        W(0),                   // 16: padding
        W(addi(SP, SP, -64)),   // 20: the helper
        W(sw(S0, SP)),          // 24
        W(JR_T0),               // 28
    ]);
    let env = Env::none();
    assert_eq!(
        frame_sizes_in(&f, &[(0, 16)], env),
        vec![Some(2064)],
        "2000 B and the helper's 64 B"
    );
    // The two together are over the 2 KiB limit although each part is not.
    assert!(matches!(
        scan_ranges(&f, &[(0, 16)], env),
        Some(Finding::Large { .. })
    ));
}

#[test]
fn an_outlined_sequence_whose_effect_on_sp_cannot_be_followed_is_refused() {
    // The helper ends in a `ret`, not a `jr t0`: its code is not read to its
    // end, so how it leaves sp is not known.
    let f = code(&[
        W(addi(SP, SP, -16)), // 0
        W(jal(T0, 16)),       // 4: to the helper at 20
        W(addi(SP, SP, 16)),  // 8
        W(RET),               // 12
        W(0),                 // 16
        W(addi(SP, SP, -64)), // 20: the helper
        W(RET),               // 24
    ]);
    assert!(matches!(
        scan_ranges(&f, &[(0, 16)], Env::none()),
        Some(Finding::Unknown { .. })
    ));
}

#[test]
fn a_register_an_outlined_sequence_writes_is_forgotten_whatever_value_it_gets() {
    // The helper writes a0 = a1 - 1, which is the very value a0 would hold
    // if it were untouched in a run that gives each register its number as a
    // marker (a0 = marker + 10, a1 = marker + 11). The caller's a0 must not
    // survive: `sub sp, sp, a0` after it is a size nobody knows.
    let f = code(&[
        W(addi(A0, 0, 100)),  // 0: a0 = 100
        W(jal(T0, 20)),       // 4: to the helper at 24
        W(sub(SP, SP, A0)),   // 8: sp -= a0
        W(addi(SP, SP, 100)), // 12
        W(RET),               // 16
        W(0),                 // 20
        W(addi(A0, 11, -1)),  // 24: the helper: a0 = a1 - 1
        W(JR_T0),             // 28
    ]);
    assert!(matches!(
        scan_ranges(&f, &[(0, 20)], Env::none()),
        Some(Finding::Unknown { .. })
    ));
}
