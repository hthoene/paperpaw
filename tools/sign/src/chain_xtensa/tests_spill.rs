//! A function address spilled to the function's own frame and loaded back (#445).

use crate::chain_xtensa::tests::{BASE, RETW_N, call, callx_reg, entry, f, l32r, outcome};

/// A load/store form of the LSAI group (`r`), register `t`, base `s`, its
/// 8-bit immediate.
fn lsai(r: u32, t: u32, s: u32, imm: u32) -> Vec<u8> {
    let w = 0x02 | t << 4 | s << 8 | r << 12 | imm << 16;
    vec![w as u8, (w >> 8) as u8, (w >> 16) as u8]
}

fn s32i(t: u32, s: u32, off: u32) -> Vec<u8> {
    lsai(6, t, s, off / 4)
}

fn l32i(t: u32, s: u32, off: u32) -> Vec<u8> {
    lsai(2, t, s, off / 4)
}

/// `s32i.n` (`op0` 9) or `l32i.n` (8) of `t`, base `s`.
fn narrow(op0: u32, t: u32, s: u32, off: u32) -> Vec<u8> {
    let w = op0 | t << 4 | s << 8 | (off / 4) << 12;
    vec![w as u8, (w >> 8) as u8]
}

/// `root`: `entry`, `l32r a8` (the helper), the `store` of a8 to the frame's
/// word 8, the `middle` (given its address), `call8 helper` (a8 holds what the
/// helper left after it), the `load` of a8 back from word 8, `callx8 a8`; the
/// helper after it. The resolved and the indirect calls.
fn spilled(store: &[u8], middle: &dyn Fn(u32) -> Vec<u8>, load: &[u8]) -> (u32, u32) {
    let at = BASE + 0x40;
    let mid_at = at + 6 + store.len() as u32;
    let mid = middle(mid_at);
    let root = |helper: u32| {
        let call_at = mid_at + mid.len() as u32;
        f(&[
            &entry(32),
            &l32r(8, at + 3, BASE),
            store,
            &mid,
            &call(8, call_at, helper),
            load,
            &callx_reg(8, 8),
            &RETW_N,
        ])
    };
    let helper_at = (at + root(0).len() as u32 + 15) & !15;
    let helper = f(&[&entry(64), &RETW_N]);
    outcome(
        &[helper_at],
        &[("root", root(helper_at)), ("helper", helper)],
        &[],
    )
}

#[test]
fn a_literal_spilled_to_the_frame_across_a_call_and_loaded_back_is_a_direct_call() {
    let none = |_| Vec::new();
    assert_eq!(spilled(&s32i(8, 1, 8), &none, &l32i(8, 1, 8)), (1, 0));
    assert_eq!(
        spilled(&narrow(9, 8, 1, 8), &none, &narrow(8, 8, 1, 8)),
        (1, 0),
        "the narrow forms"
    );
    // An address of the frame above the word does not reach it.
    let above = |_| lsai(12, 10, 1, 16); // addi a10, a1, 16
    assert_eq!(spilled(&s32i(8, 1, 8), &above, &l32i(8, 1, 8)), (1, 0));
    // Another word written, or a word loaded that holds nothing known.
    let other = |_| s32i(9, 1, 12);
    assert_eq!(spilled(&s32i(8, 1, 8), &other, &l32i(8, 1, 8)), (1, 0));
    assert_eq!(spilled(&s32i(8, 1, 8), &none, &l32i(8, 1, 12)), (0, 1));
}

#[test]
fn a_frame_word_that_may_have_changed_or_holds_a_loaded_pointer_stays_indirect() {
    type Middle = Box<dyn Fn(u32) -> Vec<u8>>;
    let cases: Vec<(&str, Middle)> = vec![
        // A callee may write through an address of the word or below it.
        ("addi a10, a1, 8", Box::new(|_| lsai(12, 10, 1, 8))),
        ("addi a10, a1, 0", Box::new(|_| lsai(12, 10, 1, 0))),
        ("addi.n a10, a1, 4", Box::new(|_| vec![0x4b, 0xa1])),
        ("mov.n a10, a1", Box::new(|_| vec![0xad, 0x01])),
        ("s32i a1 to memory", Box::new(|_| s32i(1, 2, 0))),
        // The word is written again with what a9 holds (nothing known).
        ("s32i a9, a1, 8", Box::new(|_| s32i(9, 1, 8))),
        ("s8i a9, a1, 10", Box::new(|_| lsai(4, 9, 1, 10))),
        // A path that writes the word again: the paths do not agree.
        (
            "beqz a3 over s32i a9, a1, 8",
            Box::new(|pc: u32| {
                let w = 0x16 | 3 << 8 | ((pc + 6) - pc - 4) << 12;
                let mut v = vec![w as u8, (w >> 8) as u8, (w >> 16) as u8];
                v.extend(s32i(9, 1, 8));
                v
            }),
        ),
        // The window moves: the frame is another's.
        ("wsr a2, windowbase", Box::new(|_| vec![0x20, 0x48, 0x13])),
    ];
    for (what, middle) in &cases {
        assert_eq!(
            spilled(&s32i(8, 1, 8), &**middle, &l32i(8, 1, 8)),
            (0, 1),
            "{what}"
        );
    }
    // A real indirect call: a pointer loaded from memory, spilled and loaded
    // back, is no literal; nor is it called straight.
    let at = BASE + 0x40;
    for spill in [true, false] {
        let mut parts = vec![entry(32).to_vec(), l32i(8, 2, 0)];
        if spill {
            parts.extend([s32i(8, 1, 8), l32i(8, 1, 8)]);
        }
        parts.extend([callx_reg(8, 8).to_vec(), RETW_N.to_vec()]);
        let refs: Vec<&[u8]> = parts.iter().map(Vec::as_slice).collect();
        let root = f(&refs);
        assert_eq!(outcome(&[at], &[("root", root)], &[]), (0, 1), "{spill}");
    }
}
