//! `allow jx`: a `jx` tied to a jump table of its own function (#283).

use crate::chain_xtensa::tests::{BASE, RETW_N, callx_reg, entry, l32r, outcome, root_of, run_lit};

/// `root`: `entry; jx a3; l32r a8 (a callee); callx8 a8; l32r a9 (the table
/// literal); retw.n`, at `BASE + 0x40`; the third literal is the table's
/// address (`BASE + 0x1_0000`).
fn root_with_table_literal(literal: u32) -> (Vec<u32>, Vec<(&'static str, Vec<u8>)>) {
    let at = BASE + 0x40;
    let code = [
        &entry(32)[..],
        &[0xa0, 0x03, 0x00],
        &l32r(8, at + 6, BASE),
        &callx_reg(8, 8),
        &l32r(9, at + 12, BASE + 8),
        &RETW_N,
    ];
    let (mut lits, funcs) = root_of(&code);
    lits.push(literal);
    (lits, funcs)
}

#[test]
fn a_jx_whose_table_no_literal_of_the_function_gives_fails_unless_it_is_listed() {
    let at = BASE + 0x40;
    // The table words (at the data section) hold the `l32r`: found by the image-wide
    // search (four words) in both cases; only the literal ties it to the function.
    let table = [at + 6; 4];
    // The function loads the table's address: tied, no problem.
    let (lits, funcs) = root_with_table_literal(BASE + 0x1_0000);
    let tied = run_lit(&lits, &funcs, &table, "");
    assert!(
        !tied.problems.iter().any(|p| p.contains("`jx`")),
        "{:?}",
        tied.problems
    );
    // A literal that is not the table's address: the `jx` is not tied, the scan fails.
    let (lits, funcs) = root_with_table_literal(0x1234_5678);
    let missed = run_lit(&lits, &funcs, &table, "");
    assert!(
        missed.problems.iter().any(|p| p.contains("has a `jx`")),
        "{:?}",
        missed.problems
    );
    // Listed with a reason: it passes (the `jx` and its callx stay counted as before),
    // and a line that matches nothing only warns.
    let listed = run_lit(
        &lits,
        &funcs,
        &table,
        "allow jx root read by hand: a switch\n",
    );
    assert!(
        !listed.problems.iter().any(|p| p.contains("`jx`")),
        "{:?}",
        listed.problems
    );
    let stale = run_lit(
        &[lits[0], lits[1], BASE + 0x1_0000],
        &funcs,
        &table,
        "allow jx root read by hand\n",
    );
    assert!(
        stale.warnings.iter().any(|w| w.contains("Jx")),
        "{:?}",
        stale.warnings
    );
    assert!(!stale.problems.iter().any(|p| p.contains("`jx`")));
}

#[test]
fn a_table_the_function_loads_is_followed_at_any_length() {
    let at = BASE + 0x40;
    // One word: the image-wide search takes no table of fewer than four, but the
    // function loads this one itself, so the `l32r` after the `jx` is reached.
    let (lits, funcs) = root_with_table_literal(BASE + 0x1_0000);
    assert_eq!(outcome(&lits, &funcs, &[at + 6]), (1, 1));
    // A table of the function's own start only is a function pointer: no tie.
    let report = run_lit(&lits, &funcs, &[at], "");
    assert!(report.problems.iter().any(|p| p.contains("has a `jx`")));
}
