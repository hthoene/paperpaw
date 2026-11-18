//! The spare of a crate's lines covers a site that inlining moved (#315).

use crate::chain_xtensa::tests::{BASE, RETW_N, call, callx, entry, f, run, sym};

/// `root` calls the function at `a + 16` and the one after it (`a + 32`).
fn root() -> Vec<u8> {
    let a = BASE;
    f(&[
        &entry(32),
        &call(8, a + 3, a + 16),
        &call(8, a + 6, a + 32),
        &RETW_N,
    ])
}

fn sites(n: usize) -> Vec<u8> {
    f(&[&entry(32), &callx(8).repeat(n), &RETW_N])
}

#[test]
fn a_site_moved_between_functions_of_a_crate_is_covered_by_the_spare_of_its_lines() {
    let (open, load) = (
        sym("paperpaw_image", "open", 1),
        sym("paperpaw_image", "load", 2),
    );
    // `open` has two sites no line names; the line for `load` (which has none now) has a spare of 2.
    let funcs = [
        ("root", root()),
        (open.as_str(), sites(2)),
        (load.as_str(), sites(0)),
    ];
    let moved = run(
        &funcs,
        &["root"],
        "allow indirect paperpaw_image::load 2 the glyph cache\n",
    );
    assert!(moved.problems.is_empty(), "{:?}", moved.problems);
    assert!(
        moved
            .warnings
            .iter()
            .any(|w| w.contains("spare of the lines of `paperpaw_image`"))
    );
    // Three sites more than the spare holds fail, so does a line of another crate.
    let funcs = [
        ("root", root()),
        (open.as_str(), sites(3)),
        (load.as_str(), sites(0)),
    ];
    let more = run(
        &funcs,
        &["root"],
        "allow indirect paperpaw_image::load 2 the glyph cache\n",
    );
    assert!(
        more.problems[0].contains("3 call(s) through a register"),
        "{:?}",
        more.problems
    );
    let other = run(
        &funcs,
        &["root"],
        "allow indirect paperpaw_text::load 9 elsewhere\n",
    );
    assert!(!other.problems.is_empty());
    // A function over its own line draws the rest from the spare of the others.
    let funcs = [
        ("root", root()),
        (open.as_str(), sites(5)),
        (load.as_str(), sites(0)),
    ];
    let over = run(
        &funcs,
        &["root"],
        "allow indirect paperpaw_image::open 1 one\nallow indirect paperpaw_image::load 2 two\n",
    );
    assert!(over.problems.is_empty(), "{:?}", over.problems);
    // The spare is used once: a second function finds none left.
    let funcs = [
        ("root", root()),
        (open.as_str(), sites(2)),
        (load.as_str(), sites(2)),
    ];
    let twice = run(
        &funcs,
        &["root"],
        "allow indirect paperpaw_image::gone 2 gone\n",
    );
    assert_eq!(twice.problems.len(), 1, "{:?}", twice.problems);
}

#[test]
fn a_call_out_of_the_image_is_covered_by_an_unused_line_of_the_same_crate_and_callee() {
    let a = BASE;
    let rom = BASE + 0x1000;
    let one = f(&[&entry(32), &call(8, a + 19, rom), &RETW_N]);
    let caller = sym("paperpaw_x", "a", 1);
    let root = f(&[&entry(32), &call(8, a + 3, a + 16), &RETW_N]);
    let funcs = [("root", root), (caller.as_str(), one)];
    let same = run(
        &funcs,
        &["root"],
        "allow call paperpaw_x::b -> 0x42001000 ROM memcpy\n",
    );
    assert!(same.problems.is_empty(), "{:?}", same.problems);
    assert!(
        same.warnings
            .iter()
            .any(|w| w.contains("covered by an unused"))
    );
    for line in [
        "allow call paperpaw_y::b -> 0x42001000 another crate\n",
        "allow call paperpaw_x::b -> 0x42001004 another callee\n",
    ] {
        let other = run(&funcs, &["root"], line);
        assert_eq!(other.problems.len(), 1, "{line}: {:?}", other.problems);
    }
}
