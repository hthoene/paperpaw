//! A jump table that was missed fails closed (Senior #3225 S1).

use super::tests::{BASE, RETW_N, callx_reg, entry, l32r, outcome, root_of};

#[test]
fn a_function_with_more_jx_than_tables_found_gives_up() {
    let at = BASE + 0x40;
    // entry; jx a3; l32r a8; callx8 a8; jx a3; retw.n: two jumps through a register.
    let code = [
        &entry(32)[..],
        &[0xa0, 0x03, 0x00],
        &l32r(8, at + 6, BASE),
        &callx_reg(8, 8),
        &[0xa0, 0x03, 0x00],
        &RETW_N,
    ];
    let (lits, funcs) = root_of(&code);
    // One table of four words (LLVM's least) and a second of three, apart: the
    // second `jx` has no table found, so nothing is resolved (both `jx`
    // and the `callx8` stay indirect).
    let missed = [&[at + 6; 4][..], &[0], &[at + 6; 3]].concat();
    assert_eq!(outcome(&lits, &funcs, &missed), (0, 3));
    // A table for each: resolved as before.
    let both = [&[at + 6; 4][..], &[0], &[at + 6; 4]].concat();
    assert_eq!(outcome(&lits, &funcs, &both), (1, 2));
}
