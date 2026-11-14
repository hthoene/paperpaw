//! The reach of the S3 firmware check, on small firmwares made here.

use super::*;
use crate::elf::ET_EXEC;
use crate::elf::build::Builder;

const BASE: u32 = 0x4200_0000;
const RETW_N: [u8; 2] = [0x1d, 0xf0];

/// `entry a1, <frame>` (frame in bytes, a multiple of 8).
fn entry(frame: u32) -> [u8; 3] {
    let w = 0x36 | 1 << 8 | (frame / 8) << 12;
    [w as u8, (w >> 8) as u8, (w >> 16) as u8]
}

/// `call<n>` from `at` to `target` (n = 0, 4, 8 or 12).
fn call(n: u32, at: u32, target: u32) -> [u8; 3] {
    let offset = ((i64::from(target >> 2) - i64::from(at >> 2) - 1) as u32) & 0x3_FFFF;
    let w = 0x05 | (n / 4) << 4 | offset << 6;
    [w as u8, (w >> 8) as u8, (w >> 16) as u8]
}

/// `j` from `at` to `target`.
fn jump(at: u32, target: u32) -> [u8; 3] {
    let offset = ((i64::from(target) - i64::from(at) - 4) as u32) & 0x3_FFFF;
    let w = 0x06 | offset << 6;
    [w as u8, (w >> 8) as u8, (w >> 16) as u8]
}

/// `callx<n> a2`.
fn callx(n: u32) -> [u8; 3] {
    let t = 0xC + n / 4;
    let w = t << 4 | 2 << 8;
    [w as u8, (w >> 8) as u8, (w >> 16) as u8]
}

/// A firmware of functions `(name, code)` one after the other, 4-aligned
/// (the name is the mangled `_ZN` form for a path with its segments).
fn firmware(funcs: &[(&str, Vec<u8>)]) -> (Vec<u8>, Vec<u32>) {
    let mut code = Vec::new();
    let mut starts = Vec::new();
    let mut b = Builder::new(EM_XTENSA, ET_EXEC);
    let mut layout = Vec::new();
    for (name, bytes) in funcs {
        while code.len() % 16 != 0 {
            code.push(0);
        }
        starts.push(BASE + code.len() as u32);
        layout.push((*name, code.len() as u32, bytes.len() as u32));
        code.extend_from_slice(bytes);
    }
    let text = b.section(".text", 0x6, BASE, &code, 16);
    for (name, at, size) in layout {
        b.function(name, BASE + at, size, text as u16, false);
    }
    (b.build(), starts)
}

fn run(funcs: &[(&str, Vec<u8>)], roots: &[&str], extra: &str) -> Report {
    let (bytes, _) = firmware(funcs);
    let elf = Elf::parse(&bytes).unwrap();
    let text = format!(
        "{}{extra}",
        roots
            .iter()
            .map(|r| format!("root {r}\n"))
            .collect::<String>()
    );
    check(&elf, &Budget::parse(&text).unwrap()).unwrap()
}

fn f(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

#[test]
fn a_call_chain_is_followed_from_the_roots_and_its_frames_are_read() {
    let a = BASE;
    // root: entry 32; call8 helper; retw.n. helper: entry 64; retw.n.
    let root = f(&[&entry(32), &call(8, a + 3, a + 16), &RETW_N]);
    let helper = f(&[&entry(64), &RETW_N]);
    let report = run(&[("root", root), ("helper", helper)], &["root"], "");
    assert_eq!(report.problems, Vec::<String>::new());
    assert_eq!(report.checked, 2);
    assert_eq!(report.largest, Some((64 + 16, "helper".into())));
}

#[test]
fn root_impl_names_the_trait_impl_methods_of_a_type_and_only_those() {
    // `<a::Type as T>::m` and `<a::Gen<X> as T>::m`: the symbols a path cannot name.
    let plain = "_ZN36_$LT$a..Type$u20$as$u20$a..T$GT$1m17h0123456789abcdefE";
    let generic = "_ZN45_$LT$a..Gen$LT$X$GT$$u20$as$u20$a..T$GT$1m17h0123456789abcdefE";
    let other = "_ZN40_$LT$a..TypeTwo$u20$as$u20$a..T$GT$1m17h0123456789abcdefE";
    assert!(impl_of(plain, "a::Type") && !impl_of(plain, "a::Gen"));
    assert!(impl_of(generic, "a::Gen") && !impl_of(generic, "a::Type"));
    assert!(
        !impl_of(other, "a::Type"),
        "a longer type name is not the type"
    );
    assert!(!impl_of("a_plain_name", "a::Type"));
    // A frame of 2,048 in the impl method is found, through the root alone.
    let big = f(&[&entry(2032), &RETW_N]);
    let fine = f(&[&entry(32), &RETW_N]);
    let report = run(&[(plain, big), (other, fine)], &[], "root impl a::Type\n");
    assert_eq!(report.checked, 1);
    assert_eq!(report.problems.len(), 1, "{:?}", report.problems);
    // A type with no impl in the image fails, so a rename cannot go unseen.
    let report = run(
        &[("root", f(&[&entry(32), &RETW_N]))],
        &["root"],
        "root impl a::Gone\n",
    );
    assert!(
        report
            .problems
            .iter()
            .any(|p| p.contains("`impl a::Gone` matches no function"))
    );
}

#[test]
fn root_trait_names_the_methods_of_every_impl_of_a_trait_and_its_default_methods() {
    let one = "_ZN36_$LT$a..Type$u20$as$u20$a..T$GT$1m17h0123456789abcdefE";
    let two = "_ZN45_$LT$a..Gen$LT$X$GT$$u20$as$u20$a..T$GT$1m17h0123456789abcdefE";
    let longer = "_ZN40_$LT$a..Type$u20$as$u20$a..TT$GT$1m17h0123456789abcdefE";
    // A default method of the trait, which the vtable of an impl that does not
    // override it points at: the trait's path, then the method.
    let default = "_ZN1a1T3dft17h0123456789abcdefE";
    let nested = "_ZN1a1T3dft28_$u7b$$u7b$closure$u7d$$u7d$17h0123456789abcdefE";
    let other = "_ZN1a2TT3dft17h0123456789abcdefE";
    let p = |s: &str| crate::frames::path(s);
    assert!(trait_of(one, &p(one), "a::T") && trait_of(two, &p(two), "a::T"));
    assert!(trait_of(default, &p(default), "a::T") && trait_of(nested, &p(nested), "a::T"));
    assert!(
        !trait_of(longer, &p(longer), "a::T"),
        "a longer trait name is not the trait"
    );
    assert!(
        !trait_of(other, &p(other), "a::T"),
        "another trait's default method is not"
    );
    assert!(!trait_of("plain", "plain", "a::T"));
    let big = f(&[&entry(2032), &RETW_N]);
    let fine = f(&[&entry(32), &RETW_N]);
    let report = run(
        &[
            (one, big),
            (two, fine.clone()),
            (default, fine.clone()),
            (longer, fine),
        ],
        &[],
        "root trait a::T\n",
    );
    assert_eq!(report.checked, 3);
    assert_eq!(report.problems.len(), 1, "{:?}", report.problems);
    let report = run(
        &[("root", f(&[&entry(32), &RETW_N]))],
        &["root"],
        "root trait a::None\n",
    );
    assert!(
        report
            .problems
            .iter()
            .any(|p| p.contains("`trait a::None` matches no function"))
    );
}

#[test]
fn an_absent_trait_fails_once_the_image_has_code_of_it() {
    let fine = f(&[&entry(32), &RETW_N]);
    let root = ("root", fine.clone());
    // Nothing implements it: the line holds, and `absent` is no root.
    let report = run(
        std::slice::from_ref(&root),
        &["root"],
        "absent trait a::T no impl until the linked path lands\n",
    );
    assert!(report.problems.is_empty(), "{:?}", report.problems);
    // An impl method appears: it fails, by its path, with the reason.
    let one = "_ZN36_$LT$a..Type$u20$as$u20$a..T$GT$1m17h0123456789abcdefE";
    let report = run(
        &[root.clone(), (one, fine.clone())],
        &["root"],
        "absent trait a::T no impl until the linked path lands\n",
    );
    assert_eq!(report.problems.len(), 1, "{:?}", report.problems);
    assert!(report.problems[0].contains("has code in the image now"));
    assert!(report.problems[0].contains("no impl until the linked path lands"));
    // A default method of the trait counts as code of it.
    let default = "_ZN1a1T3dft17h0123456789abcdefE";
    let report = run(
        &[root, (default, fine)],
        &["root"],
        "absent trait a::T why\n",
    );
    assert_eq!(report.problems.len(), 1, "{:?}", report.problems);
    // A line with no reason is not a line of the budget.
    assert!(Budget::parse("root x\nabsent trait a::T\n").is_err());
}

/// A firmware whose functions are `(name, bytes)`, and `also` more names for
/// the code of the function at that index (folded or aliased code).
fn firmware_aliased(funcs: &[(&str, Vec<u8>)], also: &[(usize, &str)]) -> Vec<u8> {
    let mut code = Vec::new();
    let mut at = Vec::new();
    for (_, bytes) in funcs {
        while code.len() % 16 != 0 {
            code.push(0);
        }
        at.push((BASE + code.len() as u32, bytes.len() as u32));
        code.extend_from_slice(bytes);
    }
    let mut b = Builder::new(EM_XTENSA, ET_EXEC);
    let text = b.section(".text", 0x6, BASE, &code, 16);
    for ((name, _), (start, size)) in funcs.iter().zip(&at) {
        b.function(name, *start, *size, text as u16, false);
    }
    for (index, name) in also {
        let (start, size) = at[*index];
        b.function(name, start, size, text as u16, false);
    }
    b.build()
}

#[test]
fn a_root_or_trait_is_found_by_any_name_at_an_address_not_only_the_first() {
    let fine = f(&[&entry(32), &RETW_N]);
    let plain = "_ZN1a5plain17h0123456789abcdefE";
    let method = "_ZN36_$LT$a..Type$u20$as$u20$a..T$GT$1m17h0123456789abcdefE";
    // The code of `plain` also has the name of an impl method of `a::T` (the
    // linker folded two identical functions): the first name is `plain`.
    let bytes = firmware_aliased(&[("root", fine.clone()), (plain, fine)], &[(1, method)]);
    let elf = Elf::parse(&bytes).unwrap();
    let check_with = |text: &str| check(&elf, &Budget::parse(text).unwrap()).unwrap();
    // `absent` sees the trait's code under the second name.
    let report = check_with("root root\nabsent trait a::T folded\n");
    assert_eq!(report.problems.len(), 1, "{:?}", report.problems);
    assert!(report.problems[0].contains("has code in the image now"));
    // `root trait` roots it, `root impl` too, and the family by the first name.
    let report = check_with("root root\nroot trait a::T\n");
    assert!(report.problems.is_empty(), "{:?}", report.problems);
    assert_eq!(report.checked, 2);
    let report = check_with("root root\nroot impl a::Type\n");
    assert!(report.problems.is_empty(), "{:?}", report.problems);
    assert_eq!(report.checked, 2);
}

#[test]
fn a_frame_of_the_limit_in_a_function_reached_is_refused_and_one_not_reached_is_not() {
    let a = BASE;
    let root = f(&[&entry(32), &call(12, a + 3, a + 16), &RETW_N]);
    let big = f(&[&entry(2032), &RETW_N]); // 2,032 + 16 = 2,048
    let far = f(&[&entry(4000), &RETW_N]);
    let report = run(&[("root", root), ("big", big), ("far", far)], &["root"], "");
    assert_eq!(report.checked, 2);
    assert_eq!(report.problems.len(), 1, "{:?}", report.problems);
    assert!(report.problems[0].contains("`big`") && report.problems[0].contains("2048"));
    // Under the limit: 2,024 + 16 = 2,040 passes.
    let a = BASE;
    let root = f(&[&entry(32), &call(8, a + 3, a + 16), &RETW_N]);
    let ok = f(&[&entry(2024), &RETW_N]);
    assert!(
        run(&[("root", root), ("ok", ok)], &["root"], "")
            .problems
            .is_empty()
    );
}

#[test]
fn call0_call4_callx0_callx4_are_refused_and_call8_call12_callx8_callx12_are_not() {
    let a = BASE;
    for (n, bad) in [(0, true), (4, true), (8, false), (12, false)] {
        let root = f(&[&entry(32), &call(n, a + 3, a + 16), &RETW_N]);
        let target = f(&[&entry(16), &RETW_N]);
        let report = run(&[("root", root), ("t", target)], &["root"], "");
        assert_eq!(
            !report.problems.is_empty(),
            bad,
            "call{n}: {:?}",
            report.problems
        );
    }
    for (n, bad) in [(0, true), (4, true), (8, false), (12, false)] {
        let root = f(&[&entry(32), &callx(n), &RETW_N]);
        let report = run(
            &[("root", root)],
            &["root"],
            if bad {
                ""
            } else {
                "allow indirect root 1 vtable\n"
            },
        );
        assert_eq!(
            !report.problems.is_empty(),
            bad,
            "callx{n}: {:?}",
            report.problems
        );
        assert_eq!(report.indirect, u32::from(!bad), "callx{n}");
    }
}

#[test]
fn a_call4_deep_in_the_chain_is_found() {
    let a = BASE;
    let root = f(&[&entry(32), &call(8, a + 3, a + 16), &RETW_N]);
    let mid = f(&[&entry(32), &call(8, a + 19, a + 32), &RETW_N]);
    let leaf = f(&[&entry(32), &call(4, a + 35, a + 48), &RETW_N]);
    let end = f(&[&entry(16), &RETW_N]);
    let report = run(
        &[("root", root), ("mid", mid), ("leaf", leaf), ("end", end)],
        &["root"],
        "",
    );
    assert_eq!(report.problems.len(), 1, "{:?}", report.problems);
    assert!(report.problems[0].contains("`leaf`") && report.problems[0].contains("call4"));
}

#[test]
fn a_tail_jump_into_another_function_is_followed() {
    let a = BASE;
    let root = f(&[&entry(32), &jump(a + 3, a + 16)]);
    let other = f(&[&entry(2040), &RETW_N]);
    let report = run(&[("root", root), ("other", other)], &["root"], "");
    assert_eq!(report.problems.len(), 1, "{:?}", report.problems);
    assert!(report.problems[0].contains("`other`"));
}

#[test]
fn a_call_out_of_the_image_is_refused_unless_it_is_named() {
    let a = BASE;
    // The ROM's address space: far from the firmware's code.
    let rom = BASE + 0x1000;
    let root = f(&[&entry(32), &call(8, a + 3, rom), &RETW_N]);
    let report = run(&[("root", root.clone())], &["root"], "");
    assert_eq!(report.problems.len(), 1, "{:?}", report.problems);
    assert!(report.problems[0].contains("outside the image"));
    let allowed = run(
        &[("root", root)],
        &["root"],
        "allow call root -> 0x42001000 ROM memcpy, windowed call8, no frame of its own\n",
    );
    assert!(allowed.problems.is_empty(), "{:?}", allowed.problems);
    // Another caller, or another callee, is not covered by it.
    let other = run(
        &[("root", f(&[&entry(32), &call(8, a + 3, rom + 4), &RETW_N]))],
        &["root"],
        "allow call root -> 0x42001000 ROM memcpy\n",
    );
    assert!(
        other
            .problems
            .iter()
            .any(|p| p.contains("outside the image"))
    );
    assert!(other.problems.iter().any(|p| p.contains("matches nothing")));
}

#[test]
fn a_function_of_no_size_and_a_change_of_the_stack_pointer_are_refused() {
    let a = BASE;
    let root = f(&[&entry(32), &call(8, a + 3, a + 16), &RETW_N]);
    let asm = f(&[&entry(16), &RETW_N]);
    // Size 0: the symbol table gives the assembly no size.
    let (bytes, _) = {
        let mut b = Builder::new(EM_XTENSA, ET_EXEC);
        let code = f(&[&root, &[0; 8], &asm]);
        let text = b.section(".text", 0x6, BASE, &code, 16);
        b.function("root", BASE, root.len() as u32, text as u16, false);
        b.function("asm", BASE + 16, 0, text as u16, false);
        (b.build(), ())
    };
    let elf = Elf::parse(&bytes).unwrap();
    let r = check(&elf, &Budget::parse("root root\n").unwrap()).unwrap();
    assert_eq!(r.problems.len(), 1, "{:?}", r.problems);
    assert!(r.problems[0].contains("no size"));
    let r = check(
        &elf,
        &Budget::parse("root root\nallow nosize asm hand-written, 16 bytes read\n").unwrap(),
    )
    .unwrap();
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    // `movsp a1, a2` after `entry`: an alloca.
    let moves = f(&[&entry(32), &[0x10, 0x12, 0x00], &RETW_N]);
    let report = run(&[("root", moves)], &["root"], "");
    assert_eq!(report.problems.len(), 1, "{:?}", report.problems);
    assert!(report.problems[0].contains("stack pointer"));
    let moves = f(&[&entry(32), &[0x10, 0x12, 0x00], &RETW_N]);
    let report = run(
        &[("root", moves)],
        &["root"],
        "allow sp root alloca of a bounded size, read by hand\n",
    );
    assert!(report.problems.is_empty(), "{:?}", report.problems);
}

#[test]
fn indirect_calls_must_be_listed_exactly_and_an_unused_line_fails() {
    let two = f(&[&entry(32), &callx(8), &callx(12), &RETW_N]);
    let bare = run(&[("root", two.clone())], &["root"], "");
    assert_eq!(bare.indirect, 2);
    assert!(bare.problems[0].contains("2 call(s) through a register"));
    let listed = run(
        &[("root", two.clone())],
        &["root"],
        "allow indirect root 2 vtable of Storage, device test S4\n",
    );
    assert!(listed.problems.is_empty(), "{:?}", listed.problems);
    // The function gained one: the line no longer fits, and the new count fails.
    let three = f(&[&entry(32), &callx(8), &callx(12), &callx(8), &RETW_N]);
    let grown = run(
        &[("root", three)],
        &["root"],
        "allow indirect root 2 vtable of Storage, device test S4\n",
    );
    assert_eq!(grown.problems.len(), 2, "{:?}", grown.problems);
    // A line for a caller that has none is stale.
    let stale = run(
        &[("root", f(&[&entry(32), &RETW_N]))],
        &["root"],
        "allow indirect root 1 gone\n",
    );
    assert!(
        stale.problems[0].contains("matches nothing"),
        "{:?}",
        stale.problems
    );
}

#[test]
fn the_words_of_a_table_are_roots_and_a_slot_wired_later_is_one_too() {
    // Two slots and a function nothing points at; the table is data.
    let a = BASE;
    let slot1 = f(&[&entry(16), &RETW_N]);
    let slot2 = f(&[&entry(2040), &RETW_N]); // over the limit
    let unused = f(&[&entry(4000), &RETW_N]);
    let mut b = Builder::new(EM_XTENSA, ET_EXEC);
    let code = f(&[&slot1, &[0; 2], &slot2, &[0; 2], &unused]);
    let text = b.section(".text", 0x6, a, &code, 16);
    b.function("slot1", a, slot1.len() as u32, text as u16, false);
    let s2 = a + 8;
    b.function("slot2", s2, slot2.len() as u32, text as u16, false);
    b.function("unused", a + 16, unused.len() as u32, text as u16, false);
    let mut words = Vec::new();
    for w in [24u32, 0x0007_0001, a, s2, 0] {
        words.extend_from_slice(&w.to_le_bytes());
    }
    let rodata = b.section(".rodata", 0x2, 0x3c00_0000, &words, 4);
    b.object("HOST", 0x3c00_0000, words.len() as u32, rodata as u16);
    let bytes = b.build();
    let elf = Elf::parse(&bytes).unwrap();
    let report = check(&elf, &Budget::parse("table HOST\n").unwrap()).unwrap();
    assert_eq!(report.roots, (0, 2));
    assert_eq!(report.checked, 2);
    assert_eq!(report.problems.len(), 1, "{:?}", report.problems);
    assert!(report.problems[0].contains("`slot2`"));
    // A table name that matches nothing, or holds no pointer, fails.
    let missing = check(&elf, &Budget::parse("table NOPE\nroot slot1\n").unwrap()).unwrap();
    assert!(missing.problems[0].contains("matches no static"));
}

#[test]
fn a_root_that_matches_nothing_fails_and_a_family_matches_what_is_inside_it() {
    let root = f(&[&entry(32), &RETW_N]);
    let report = run(&[("root", root.clone())], &["nothing"], "");
    assert!(report.problems[0].contains("matches no function"));
    // A module's functions: the name has the module before it.
    let report = run(
        &[("_ZN6native4ppn52x45begin17h0123456789abcdefE", root)],
        &["native::ppn5"],
        "",
    );
    assert!(
        report.problems.is_empty() && report.checked == 1,
        "{report:?}"
    );
}

#[test]
fn the_budget_is_read_strictly() {
    assert!(Budget::parse("root a\nlimit 1024\nallow call a -> 0x4000 why\n").is_ok());
    assert!(Budget::parse("limit 1024\n").is_err(), "no root");
    assert!(Budget::parse("root a\nbogus 1\n").is_err());
    assert!(
        Budget::parse("root a\nallow call a -> 0x4000\n").is_err(),
        "an allow with no reason"
    );
    assert_eq!(Budget::parse("root a # c\n").unwrap().limit, 2048);
}

#[test]
fn a_form_the_scan_does_not_classify_fails_wide_or_narrow() {
    // A MAC16 word (op0 = 4) and an unassigned ST3 narrow form (op0 = 13,
    // r = 1): refused; a length the scan does not know (op0 = 14) too.
    let wide = f(&[&entry(32), &[0x04, 0x00, 0x00], &RETW_N]);
    let report = run(&[("root", wide)], &["root"], "");
    assert_eq!(report.problems.len(), 1, "{:?}", report.problems);
    assert!(report.problems[0].contains("does not classify"));
    let narrow = f(&[&entry(32), &[0x0d, 0x10], &RETW_N]);
    let report = run(&[("root", narrow)], &["root"], "");
    assert_eq!(report.problems.len(), 1, "{:?}", report.problems);
    assert!(report.problems[0].contains("does not classify"));
    // `rotw` rotates the window: a change of what `a1` names (`allow sp`).
    let rotw = f(&[&entry(32), &[0x20, 0x80, 0x40], &RETW_N]);
    let report = run(&[("root", rotw.clone())], &["root"], "");
    assert_eq!(report.problems.len(), 1, "{:?}", report.problems);
    assert!(report.problems[0].contains("stack pointer"));
    let listed = run(
        &[("root", rotw)],
        &["root"],
        "allow sp root spills the windows\n",
    );
    assert!(listed.problems.is_empty(), "{:?}", listed.problems);
    // The ordinary forms still pass: `nop.n`, `mov.n a2, a3`, `l32i.n`.
    let plain = f(&[
        &entry(32),
        &[0x3d, 0xf0],
        &[0x2d, 0x03],
        &[0x38, 0x02],
        &RETW_N,
    ]);
    assert!(run(&[("root", plain)], &["root"], "").problems.is_empty());
}

#[test]
fn an_entry_of_another_register_fails_and_a_jx_counts_as_indirect() {
    // `entry a2, 32`.
    let w = 0x36u32 | 2 << 8 | (32 / 8) << 12;
    let other = f(&[&[w as u8, (w >> 8) as u8, (w >> 16) as u8], &RETW_N]);
    let report = run(&[("root", other)], &["root"], "");
    assert_eq!(report.problems.len(), 1, "{:?}", report.problems);
    assert!(report.problems[0].contains("register other than a1"));
    // `jx a2`: not followed, counted; listed or it fails.
    let jx = f(&[&entry(32), &[0xa0, 0x02, 0x00]]);
    let bare = run(&[("root", jx.clone())], &["root"], "");
    assert_eq!(bare.indirect, 1);
    assert!(bare.problems[0].contains("through a register"));
    let listed = run(
        &[("root", jx)],
        &["root"],
        "allow indirect root 1 jump table\n",
    );
    assert!(listed.problems.is_empty(), "{:?}", listed.problems);
}

#[test]
fn the_limit_is_at_most_the_bands_bound() {
    assert!(Budget::parse("root a\nlimit 2048\n").is_ok());
    assert!(Budget::parse("root a\nlimit 2049\n").is_err());
    assert!(Budget::parse("root a\nlimit 4096\n").is_err());
    assert!(Budget::parse("root a\nlimit 0\n").is_err());
}

#[test]
fn a_table_word_inside_the_code_but_not_at_a_function_start_fails() {
    let a = BASE;
    let slot = f(&[&entry(16), &RETW_N]);
    let mut b = Builder::new(EM_XTENSA, ET_EXEC);
    let text = b.section(".text", 0x6, a, &slot, 16);
    b.function("slot", a, slot.len() as u32, text as u16, false);
    let mut words = Vec::new();
    // A slot at the start, and one in the middle of the function.
    for w in [a, a + 2] {
        words.extend_from_slice(&w.to_le_bytes());
    }
    let rodata = b.section(".rodata", 0x2, 0x3c00_0000, &words, 4);
    b.object("HOST", 0x3c00_0000, words.len() as u32, rodata as u16);
    let bytes = b.build();
    let elf = Elf::parse(&bytes).unwrap();
    let report = check(&elf, &Budget::parse("table HOST\n").unwrap()).unwrap();
    assert_eq!(report.problems.len(), 1, "{:?}", report.problems);
    assert!(report.problems[0].contains("not at the start of a function"));
}

#[test]
fn a_truncated_or_damaged_firmware_is_an_error_not_a_panic() {
    let a = BASE;
    let root = f(&[&entry(32), &call(8, a + 3, a + 16), &RETW_N]);
    let helper = f(&[&entry(16), &RETW_N]);
    let (bytes, _) = firmware(&[("root", root), ("helper", helper)]);
    let budget = Budget::parse("root root\n").unwrap();
    // Every prefix of the file, and each byte of it flipped in turn.
    for len in 0..bytes.len() {
        if let Ok(elf) = Elf::parse(&bytes[..len]) {
            let _ = check(&elf, &budget);
        }
    }
    for at in 0..bytes.len() {
        let mut damaged = bytes.clone();
        damaged[at] ^= 0xff;
        if let Ok(elf) = Elf::parse(&damaged) {
            let _ = check(&elf, &budget);
        }
    }
    assert!(Elf::parse(&[]).is_err());
    assert!(Elf::parse(b"not an elf file at all, no").is_err());
}

/// The privileged forms as the S3 toolchain's assembler makes them
/// (`xtensa-esp32s3-elf-as`, little endian): `(name, bytes, writes a1)`.
const FORM_WORDS: [(&str, [u8; 3], bool); 9] = [
    ("rsil", [0x20, 0x63, 0x00], false),   // rsil a2, 3
    ("waiti", [0x00, 0x70, 0x00], false),  // waiti 0
    ("wsr.ps", [0x20, 0xe6, 0x13], false), // wsr.ps a2
    ("xsr.ps", [0x20, 0xe6, 0x61], false), // xsr.ps a2
    ("xsr.ps", [0x10, 0xe6, 0x61], true),  // xsr.ps a1
    ("l32e", [0x20, 0xc3, 0x09], false),   // l32e a2, a3, -16
    ("l32e", [0x10, 0xc3, 0x09], true),    // l32e a1, a3, -16
    ("s32e", [0x20, 0xc3, 0x49], false),   // s32e a2, a3, -16
    ("rsil", [0x10, 0x63, 0x00], true),    // rsil a1, 3
];

#[test]
fn a_privileged_form_fails_unless_its_function_and_form_are_named() {
    for (name, word, writes_a1) in FORM_WORDS {
        let root = f(&[&entry(32), &word, &RETW_N]);
        let bare = run(&[("root", root.clone())], &["root"], "");
        assert_eq!(bare.problems.len(), 1, "{name}: {:?}", bare.problems);
        if writes_a1 {
            // A change of the stack pointer, whatever line is there.
            assert!(
                bare.problems[0].contains("stack pointer"),
                "{name}: {:?}",
                bare.problems
            );
            let line = format!("allow form root {name} a1 is written\n");
            let listed = run(&[("root", root)], &["root"], &line);
            assert!(
                listed.problems.iter().any(|p| p.contains("stack pointer")),
                "{name} a1: {:?}",
                listed.problems
            );
            continue;
        }
        assert!(
            bare.problems[0].contains(&format!("`{name}`"))
                && bare.problems[0].contains("allow form"),
            "{name}: {:?}",
            bare.problems
        );
        let line = format!("allow form root {name} leaves a1 alone\n");
        let listed = run(&[("root", root.clone())], &["root"], &line);
        assert!(listed.problems.is_empty(), "{name}: {:?}", listed.problems);
        // The line is for that function: another function's is not enough.
        let other = format!("allow form elsewhere {name} leaves a1 alone\n");
        let wrong = run(&[("root", root)], &["root"], &other);
        assert_eq!(wrong.problems.len(), 2, "{name}: {:?}", wrong.problems);
    }
}

#[test]
fn the_window_registers_are_a_stack_pointer_change_and_a_form_names_its_register() {
    // wsr/xsr of WINDOWBASE (72) and WINDOWSTART (73): the stack-pointer rule,
    // whatever `allow form` says; only `allow sp` passes them.
    for (name, word) in [
        ("wsr.windowbase", [0x20, 0x48, 0x13]),
        ("wsr.windowstart", [0x20, 0x49, 0x13]),
        ("xsr.windowbase", [0x20, 0x48, 0x61]),
        ("xsr.windowstart", [0x20, 0x49, 0x61]),
    ] {
        let root = f(&[&entry(32), &word, &RETW_N]);
        let bare = run(&[("root", root.clone())], &["root"], "");
        assert_eq!(bare.problems.len(), 1, "{name}: {:?}", bare.problems);
        assert!(bare.problems[0].contains("stack pointer"), "{name}");
        let kind = &name[..3];
        let line = format!("allow form root {kind}.scompare1 not enough\n");
        let listed = run(&[("root", root.clone())], &["root"], &line);
        assert!(
            listed.problems.iter().any(|p| p.contains("stack pointer")),
            "{name}: {:?}",
            listed.problems
        );
        let sp = run(&[("root", root)], &["root"], "allow sp root window moved\n");
        assert!(sp.problems.is_empty(), "{name}: {:?}", sp.problems);
    }
    // The form names its register: a line for one register does not cover
    // another (and is then unused), and a bare `wsr` or `xsr` is no form.
    for (kind, word, other) in [
        ("wsr", [0x20, 0xe6, 0x13], "scompare1"),
        ("xsr", [0x20, 0xe6, 0x61], "vecbase"),
    ] {
        let root = f(&[&entry(32), &word, &RETW_N]);
        let wrong = run(
            &[("root", root.clone())],
            &["root"],
            &format!("allow form root {kind}.{other} wrong register\n"),
        );
        assert_eq!(wrong.problems.len(), 2, "{kind}: {:?}", wrong.problems);
        let exact = run(
            &[("root", root)],
            &["root"],
            &format!("allow form root {kind}.ps restores the interrupt level\n"),
        );
        assert!(exact.problems.is_empty(), "{kind}.ps: {:?}", exact.problems);
        assert!(Budget::parse(&format!("root a\nallow form a {kind} bare\n")).is_err());
    }
    // VECBASE (231) and SCOMPARE1 (12) have their names; one not in the table is `sr<n>`.
    let vec = f(&[&entry(32), &[0x30, 0xe7, 0x13], &RETW_N]);
    let ok = run(
        &[("root", vec)],
        &["root"],
        "allow form root wsr.vecbase set\n",
    );
    assert!(ok.problems.is_empty(), "{:?}", ok.problems);
    let scompare = f(&[&entry(32), &[0x20, 0x0c, 0x13], &RETW_N]);
    let ok = run(
        &[("root", scompare)],
        &["root"],
        "allow form root wsr.scompare1 compare value\n",
    );
    assert!(ok.problems.is_empty(), "{:?}", ok.problems);
    // LITBASE (5) is no form: the literal resolver assumes it off, so a write is
    // refused and no line allows it; neither is the number of a named register.
    let litbase = f(&[&entry(32), &[0x20, 0x05, 0x13], &RETW_N]);
    let refused = run(&[("root", litbase)], &["root"], "");
    assert!(
        refused
            .problems
            .iter()
            .any(|p| p.contains("does not classify")),
        "{:?}",
        refused.problems
    );
    assert!(Budget::parse("root a\nallow form a wsr.litbase no\n").is_err());
    assert!(Budget::parse("root a\nallow form a wsr.sr12 no\n").is_err());
    assert!(Budget::parse("root a\nallow form a wsr.sr200 an unnamed register\n").is_ok());
}

#[test]
fn an_allow_form_line_that_matches_nothing_or_names_the_wrong_form_fails() {
    // A function with `rsil` only: a `waiti` line is unused and fails, and the
    // `rsil` is still unnamed.
    let root = f(&[&entry(32), &[0x20, 0x63, 0x00], &RETW_N]);
    let report = run(
        &[("root", root.clone())],
        &["root"],
        "allow form root waiti not here\n",
    );
    assert_eq!(report.problems.len(), 2, "{:?}", report.problems);
    // Both named: passes. One function with two forms needs both lines.
    let two = f(&[
        &entry(32),
        &[0x20, 0x63, 0x00],
        &[0x00, 0x70, 0x00],
        &RETW_N,
    ]);
    let one = run(
        &[("root", two.clone())],
        &["root"],
        "allow form root rsil ok\n",
    );
    assert_eq!(one.problems.len(), 1, "{:?}", one.problems);
    let both = run(
        &[("root", two)],
        &["root"],
        "allow form root rsil ok\nallow form root waiti ok\n",
    );
    assert!(both.problems.is_empty(), "{:?}", both.problems);
}

#[test]
fn forms_that_stay_unknown_cannot_be_allowed_and_the_line_is_read_strictly() {
    // `rfe`, `rfi 3` and `rfwo` have no line.
    for word in [[0x00u8, 0x30, 0x00], [0x10, 0x33, 0x00], [0x00, 0x34, 0x00]] {
        let root = f(&[&entry(32), &word, &RETW_N]);
        let report = run(&[("root", root)], &["root"], "");
        assert!(
            report
                .problems
                .iter()
                .any(|p| p.contains("does not classify")),
            "{word:?}: {:?}",
            report.problems
        );
    }
    for bad in [
        "root a\nallow form a rotw no\n",
        "root a\nallow form a rfe no\n",
        "root a\nallow form a rsil\n",
        "root a\nallow form a\n",
    ] {
        assert!(Budget::parse(bad).is_err(), "{bad}");
    }
    assert!(Budget::parse("root a\nallow form a rsil critical section\n").is_ok());
}

// ---- calls through a register that an `l32r` literal gives -----------------

/// `l32r a<reg>, <literal>` at `pc`.
fn l32r(reg: u32, pc: u32, literal: u32) -> [u8; 3] {
    let imm = ((i64::from(literal) - i64::from((pc + 3) & !3)) >> 2) as u32 & 0xFFFF;
    let w = 0x01 | reg << 4 | imm << 8;
    [w as u8, (w >> 8) as u8, (w >> 16) as u8]
}

/// `callx<n> a<reg>`.
fn callx_reg(n: u32, reg: u32) -> [u8; 3] {
    let w = (0xC + n / 4) << 4 | reg << 8;
    [w as u8, (w >> 8) as u8, (w >> 16) as u8]
}

/// `beqz a<reg>, <target>` at `pc`.
fn beqz(reg: u32, pc: u32, target: u32) -> [u8; 3] {
    let imm = (i64::from(target) - i64::from(pc) - 4) as u32 & 0xFFF;
    let w = 0x16 | reg << 8 | imm << 12;
    [w as u8, (w >> 8) as u8, (w >> 16) as u8]
}

/// The literal pool at `BASE` (words), the functions from `BASE + 0x40`, and a
/// data section at `BASE + 0x1_0000` (words).
fn firmware_lit(lits: &[u32], funcs: &[(&str, Vec<u8>)], data: &[u32]) -> Vec<u8> {
    firmware_lit_flags(lits, funcs, data, 0x6)
}

/// As [`firmware_lit`], with the flags of the text section (0x6: alloc and
/// exec; 0x7 adds write).
fn firmware_lit_flags(
    lits: &[u32],
    funcs: &[(&str, Vec<u8>)],
    data: &[u32],
    text_flags: u32,
) -> Vec<u8> {
    let mut b = Builder::new(EM_XTENSA, ET_EXEC);
    let mut code: Vec<u8> = lits.iter().flat_map(|w| w.to_le_bytes()).collect();
    code.resize(0x40, 0);
    let mut layout = Vec::new();
    for (name, bytes) in funcs {
        while !code.len().is_multiple_of(16) {
            code.push(0);
        }
        layout.push((*name, code.len() as u32, bytes.len() as u32));
        code.extend_from_slice(bytes);
    }
    let text = b.section(".text", text_flags, BASE, &code, 16);
    if !data.is_empty() {
        let bytes: Vec<u8> = data.iter().flat_map(|w| w.to_le_bytes()).collect();
        b.section(".rodata", 0x2, BASE + 0x1_0000, &bytes, 4);
    }
    for (name, at, size) in layout {
        b.function(name, BASE + at, size, text as u16, false);
    }
    b.build()
}

fn run_lit(lits: &[u32], funcs: &[(&str, Vec<u8>)], data: &[u32], extra: &str) -> Report {
    let bytes = firmware_lit(lits, funcs, data);
    let elf = Elf::parse(&bytes).unwrap();
    let text = format!("root root\n{extra}");
    check(&elf, &Budget::parse(&text).unwrap()).unwrap()
}

/// `root`: `entry`, `l32r a8, lit`, the `middle` bytes, `callx8 a8`, `retw.n`
/// at `BASE + 0x40`, and a `helper` of frame 64 after it (the literal 0 is the
/// helper's address).
fn root_and_helper(middle: &dyn Fn(u32) -> Vec<u8>) -> (Vec<u32>, Vec<(&'static str, Vec<u8>)>) {
    let root_at = BASE + 0x40;
    let mid = middle(root_at + 6);
    let root = f(&[
        &entry(32),
        &l32r(8, root_at + 3, BASE),
        &mid,
        &callx_reg(8, 8),
        &RETW_N,
    ]);
    let helper_at = (root_at + root.len() as u32 + 15) & !15;
    let helper = f(&[&entry(64), &RETW_N]);
    (vec![helper_at], vec![("root", root), ("helper", helper)])
}

#[test]
fn a_callx_through_an_l32r_literal_is_a_direct_call_to_the_literal() {
    for middle in [vec![], vec![0xad, 0x03] /* mov.n a10, a3 */] {
        let (lits, funcs) = root_and_helper(&|_| middle.clone());
        let report = run_lit(&lits, &funcs, &[], "");
        assert_eq!(report.problems, Vec::<String>::new(), "{middle:?}");
        assert_eq!((report.indirect, report.resolved), (0, 1));
        assert_eq!(report.checked, 2);
        assert_eq!(report.largest, Some((64 + 16, "helper".into())));
    }
}

#[test]
fn a_literal_out_of_the_image_is_a_call_out_of_the_image() {
    let root_at = BASE + 0x40;
    let root = f(&[
        &entry(32),
        &l32r(8, root_at + 3, BASE),
        &callx_reg(8, 8),
        &RETW_N,
    ]);
    let funcs = [("root", root)];
    let report = run_lit(&[0x4000_11f4], &funcs, &[], "");
    assert_eq!(report.problems.len(), 1, "{:?}", report.problems);
    assert!(report.problems[0].contains("0x400011f4") && report.problems[0].contains("allow call"));
    let named = run_lit(
        &[0x4000_11f4],
        &funcs,
        &[],
        "allow call root -> 0x400011f4 the ROM's memcpy\n",
    );
    assert!(named.problems.is_empty(), "{:?}", named.problems);
    assert_eq!((named.indirect, named.resolved), (0, 1));
}

#[test]
fn a_callx_whose_register_holds_no_literal_on_every_path_stays_indirect() {
    // Each is a call through a register the scan must not follow: the budget
    // lists it (`allow indirect`) or the scan fails.
    type Middle = Box<dyn Fn(u32) -> Vec<u8>>;
    let cases: Vec<(&str, Middle)> = vec![
        // The register is written again.
        ("a write of the register", Box::new(|_| vec![0x8d, 0x03])),
        // A call moves the window.
        (
            "a call in between",
            Box::new(|pc: u32| call(8, pc, BASE + 0x40).to_vec()),
        ),
        // A window register moves the registers.
        (
            "a write of WINDOWBASE",
            Box::new(|_| vec![0x20, 0x48, 0x13]),
        ),
    ];
    for (what, middle) in &cases {
        let (lits, funcs) = root_and_helper(&**middle);
        let bare = run_lit(&lits, &funcs, &[], "");
        assert_eq!(bare.resolved, 0, "{what}");
        assert_eq!(bare.indirect, 1, "{what}: {:?}", bare.problems);
        assert!(
            bare.problems
                .iter()
                .any(|p| p.contains("through a register")),
            "{what}: {:?}",
            bare.problems
        );
    }
    // The l32r loads another register than the one called.
    let root_at = BASE + 0x40;
    let root = f(&[
        &entry(32),
        &l32r(9, root_at + 3, BASE),
        &callx_reg(8, 8),
        &RETW_N,
    ]);
    let other = run_lit(&[root_at], &[("root", root)], &[], "");
    assert_eq!((other.resolved, other.indirect), (0, 1));
}

/// A function `root` of the instructions `parts` at `BASE + 0x40` (the first
/// literal is the helper's address, the second the same address again), and
/// the helper after it.
fn root_of(parts: &[&[u8]]) -> (Vec<u32>, Vec<(&'static str, Vec<u8>)>) {
    let root_at = BASE + 0x40;
    let root = f(parts);
    let helper_at = (root_at + root.len() as u32 + 15) & !15;
    let helper = f(&[&entry(64), &RETW_N]);
    (
        vec![helper_at, helper_at],
        vec![("root", root), ("helper", helper)],
    )
}

fn outcome(lits: &[u32], funcs: &[(&str, Vec<u8>)], data: &[u32]) -> (u32, u32) {
    let report = run_lit(lits, funcs, data, "");
    (report.resolved, report.indirect)
}

#[test]
fn the_literal_must_be_the_same_on_every_path_to_the_call() {
    let at = BASE + 0x40;
    // A register loaded before a loop and called inside it: a5 is kept by a
    // `call8` (the registers below its rotation are), a8 is not (it holds
    // what the callee left on the second time round).
    for (reg, want) in [(5, (1, 0)), (8, (0, 1))] {
        let (lits, funcs) = root_of(&[
            &entry(32),
            &l32r(reg, at + 3, BASE),
            &callx_reg(8, reg),
            &beqz(3, at + 9, at + 6),
            &RETW_N,
        ]);
        assert_eq!(outcome(&lits, &funcs, &[]), want, "a{reg} in a loop");
    }
    // A path that skips the `l32r`.
    let (lits, funcs) = root_of(&[
        &entry(32),
        &beqz(3, at + 3, at + 9),
        &l32r(8, at + 6, BASE),
        &callx_reg(8, 8),
        &RETW_N,
    ]);
    assert_eq!(outcome(&lits, &funcs, &[]), (0, 1), "a path without it");
    // The same literal on both arms of a branch resolves; two literals (even
    // of the same value) do not.
    for (second, want) in [(BASE, (1, 0)), (BASE + 4, (0, 1))] {
        let (lits, funcs) = root_of(&[
            &entry(32),
            &beqz(3, at + 3, at + 12),
            &l32r(8, at + 6, BASE),
            &jump(at + 9, at + 15),
            &l32r(8, at + 12, second),
            &callx_reg(8, 8),
            &RETW_N,
        ]);
        assert_eq!(
            outcome(&lits, &funcs, &[]),
            want,
            "second literal {second:#x}"
        );
    }
}

#[test]
fn an_address_the_image_points_at_is_an_entry_with_nothing_known() {
    let at = BASE + 0x40;
    // entry; l32r a8; mov.n; callx8 a8; retw.n: resolved on its own.
    let (lits, funcs) = root_and_helper(&|_| vec![0xad, 0x03]);
    assert_eq!(outcome(&lits, &funcs, &[]), (1, 0));
    // A word of data holding the address of the `callx8` (a jump table of
    // another function, or of this one): the call can be entered with nothing
    // loaded, so it is not resolved.
    let callx_at = at + 3 + 3 + 2;
    assert_eq!(outcome(&lits, &funcs, &[callx_at]), (0, 1));
    // At the `l32r` itself (it loads before the call on every path): resolved.
    assert_eq!(outcome(&lits, &funcs, &[at + 3]), (1, 0));
    // A word that is no address of this function changes nothing.
    assert_eq!(outcome(&lits, &funcs, &[0x1234_5678]), (1, 0));
    // An address inside an instruction: no jump target (the compiler's are
    // instruction starts), a number in the data that happens to be one: left
    // out, so where the build puts things does not decide whether the call
    // resolves. The same with a `jx` table that has a real entry too.
    assert_eq!(outcome(&lits, &funcs, &[at + 4]), (1, 0));
    assert_eq!(outcome(&lits, &funcs, &[at + 4, at + 3]), (1, 0));
    assert_eq!(outcome(&lits, &funcs, &[at + 4, callx_at]), (0, 1));
}

#[test]
fn a_jx_needs_an_address_to_jump_to_and_its_table_words_are_entries() {
    let at = BASE + 0x40;
    // entry; jx a3; l32r a8; callx8 a8; retw.n. Only a table gets to the
    // `l32r` or to the `callx8`.
    let code = [
        &entry(32)[..],
        &[0xa0, 0x03, 0x00],
        &l32r(8, at + 6, BASE),
        &callx_reg(8, 8),
        &RETW_N,
    ];
    let (lits, funcs) = root_of(&code);
    // No table: the `jx` cannot be placed, nothing is resolved (the `jx` is
    // counted as one indirect call too).
    assert_eq!(outcome(&lits, &funcs, &[]), (0, 2));
    // The table holds the `l32r`: reached, the literal loaded.
    assert_eq!(outcome(&lits, &funcs, &[at + 6]), (1, 1));
    // The table holds the `callx8`: reached with nothing loaded.
    assert_eq!(outcome(&lits, &funcs, &[at + 9]), (0, 2));
    // A `jx` after a resolvable call: with no table the whole function gives up.
    let code = [
        &entry(32)[..],
        &l32r(8, at + 3, BASE),
        &callx_reg(8, 8),
        &[0xa0, 0x03, 0x00],
        &RETW_N,
    ];
    let (lits, funcs) = root_of(&code);
    assert_eq!(outcome(&lits, &funcs, &[]), (0, 2));
    // A word that holds the function's own start (a vtable or a function
    // pointer) is no table: the function still gives up.
    assert_eq!(outcome(&lits, &funcs, &[at]), (0, 2));
}

/// `bf b0, <target>` (`r` 0) or `bt b0, <target>` (`r` 1) at `pc`.
fn bf_bt(r: u32, pc: u32, target: u32) -> [u8; 3] {
    let imm = (i64::from(target) - i64::from(pc) - 4) as u32 & 0xFF;
    let w = 0x76 | r << 12 | imm << 16;
    [w as u8, (w >> 8) as u8, (w >> 16) as u8]
}

/// `root`: `entry`, `l32r a8`, `mov.n a10, a3`, `callx8 a8`, then `tail` (at
/// `BASE + 0x40 + 11`), `retw.n`, and the helper the literal gives.
fn root_with_tail(tail: &dyn Fn(u32) -> Vec<u8>) -> (Vec<u32>, Vec<(&'static str, Vec<u8>)>) {
    let root_at = BASE + 0x40;
    let tail = tail(root_at + 11);
    let root = f(&[
        &entry(32),
        &l32r(8, root_at + 3, BASE),
        &[0xad, 0x03],
        &callx_reg(8, 8),
        &tail,
        &RETW_N,
    ]);
    let helper_at = (root_at + root.len() as u32 + 15) & !15;
    let helper = f(&[&entry(64), &RETW_N]);
    (vec![helper_at], vec![("root", root), ("helper", helper)])
}

#[test]
fn a_backward_bf_or_bt_is_a_label_and_an_unmodelled_branch_form_fails_closed() {
    let root_at = BASE + 0x40;
    // No branch after the call: resolved.
    let (lits, funcs) = root_with_tail(&|_| vec![0x3d, 0xf0]);
    let plain = run_lit(&lits, &funcs, &[], "");
    assert_eq!(
        (plain.resolved, plain.indirect),
        (1, 0),
        "{:?}",
        plain.problems
    );
    // A `bf`/`bt` after the call back to the `mov.n` between the `l32r` and the
    // call: the path round again has a8 from the callee, the call is not proven.
    for r in [0, 1] {
        let (lits, funcs) = root_with_tail(&|pc| bf_bt(r, pc, root_at + 6).to_vec());
        let report = run_lit(&lits, &funcs, &[], "");
        assert_eq!((report.resolved, report.indirect), (0, 1), "r = {r}");
    }
    // `bt` (r = 1) as the assembler makes it: 0xf61176 at 9 goes to 3.
    assert_eq!(
        super::regs::Insn {
            pc: 9,
            word: 0xf6_1176,
            wide: true
        }
        .targets(),
        Some([Some(3), None])
    );
    // A hardware loop (`loopnez a2` to the `retw.n`, 0x039276 as the assembler
    // makes it): its back edge is no instruction's, so no call is resolved.
    let (lits, funcs) =
        root_with_tail(&|_| f(&[&[0x76, 0x92, 0x03], &[0x3d, 0xf0], &[0x3d, 0xf0]]));
    assert_eq!(outcome(&lits, &funcs, &[]), (0, 1), "a loop");
    // B1 with an `r` that is no `bf`, `bt` or `loop*`: not modelled, so the
    // function's calls are not resolved at all, wherever the form is.
    let (lits, funcs) = root_with_tail(&|_| vec![0x76, 0x30, 0x00]);
    let report = run_lit(&lits, &funcs, &[], "");
    assert_eq!((report.resolved, report.indirect), (0, 1));
}

#[test]
fn a_branch_into_the_middle_of_an_instruction_leaves_the_functions_calls_unresolved() {
    let root_at = BASE + 0x40;
    // `beqz a3` to the second byte of the `retw.n`'s neighbour: not an
    // instruction start.
    let (lits, funcs) = root_with_tail(&|pc| beqz(3, pc, root_at + 7).to_vec());
    let report = run_lit(&lits, &funcs, &[], "");
    assert_eq!((report.resolved, report.indirect), (0, 1));
}

#[test]
fn a_write_of_the_window_registers_between_the_l32r_and_the_call_gives_up() {
    // wsr.windowbase, xsr.windowstart, wsr.ps: the registers are not the ones
    // the `l32r` loaded.
    for word in [[0x20, 0x48, 0x13], [0x20, 0x49, 0x61], [0x20, 0xe6, 0x13]] {
        let (lits, funcs) = root_and_helper(&|_| word.to_vec());
        let report = run_lit(&lits, &funcs, &[], "");
        assert_eq!((report.resolved, report.indirect), (0, 1), "{word:?}");
    }
}

#[test]
fn a_literal_in_a_writable_section_is_not_read() {
    let (lits, funcs) = root_and_helper(&|_| vec![]);
    let bytes = firmware_lit_flags(&lits, &funcs, &[], 0x7);
    let elf = Elf::parse(&bytes).unwrap();
    let report = check(&elf, &Budget::parse("root root\n").unwrap()).unwrap();
    assert_eq!((report.resolved, report.indirect), (0, 1));
}

#[test]
fn a_path_shared_by_functions_is_named_by_symbol_and_a_line_must_name_one() {
    // Two instantiations of one generic: the same path, two symbols (legacy
    // mangling drops the generics from the path, keeps the hash in the symbol).
    let (s1, s2) = (
        "_ZN4core3ptr13drop_in_place17h0000000000000001E",
        "_ZN4core3ptr13drop_in_place17h0000000000000002E",
    );
    let a = BASE;
    let root = f(&[
        &entry(32),
        &call(8, a + 3, a + 16),
        &call(8, a + 6, a + 32),
        &RETW_N,
    ]);
    let one = f(&[&entry(32), &callx_reg(8, 2), &RETW_N]);
    let funcs = [("root", root), (s1, one.clone()), (s2, one)];
    // Bare: both are reported by the symbol without its hash (the path alone
    // would not tell them from every other `core::ptr` function), one line.
    let stable_name = "_ZN4core3ptr13drop_in_place";
    let bare = run(&funcs, &["root"], "");
    assert_eq!(bare.problems.len(), 1, "{:?}", bare.problems);
    assert!(bare.problems[0].contains(stable_name) && !bare.problems[0].contains("17h"));
    // A line by the path names two functions: an error.
    let by_path = run(
        &funcs,
        &["root"],
        "allow indirect core::ptr::drop_in_place 1 both\n",
    );
    assert!(
        by_path
            .problems
            .iter()
            .any(|p| p.contains("names 2 functions") && p.contains(stable_name)),
        "{:?}",
        by_path.problems
    );
    // By the symbol without the hash: one line covers the instances that make
    // that many calls, and survives a rebuild that changes every hash.
    let by_stable = run(
        &funcs,
        &["root"],
        &format!("allow indirect {stable_name} 1 every instance\n"),
    );
    assert!(by_stable.problems.is_empty(), "{:?}", by_stable.problems);
    let rebuilt = [
        ("root", funcs[0].1.clone()),
        (
            "_ZN4core3ptr13drop_in_place17h00000000000000aaE.1322",
            funcs[1].1.clone(),
        ),
        (
            "_ZN4core3ptr13drop_in_place17h00000000000000bbE.77",
            funcs[2].1.clone(),
        ),
    ];
    let by_stable = run(
        &rebuilt,
        &["root"],
        &format!("allow indirect {stable_name} 1 every instance\n"),
    );
    assert!(by_stable.problems.is_empty(), "{:?}", by_stable.problems);
    // An instance with another count is not covered.
    let two = f(&[&entry(32), &callx_reg(8, 2), &callx_reg(8, 3), &RETW_N]);
    let uneven = [
        ("root", funcs[0].1.clone()),
        (s1, funcs[1].1.clone()),
        (s2, two),
    ];
    let uneven = run(
        &uneven,
        &["root"],
        &format!("allow indirect {stable_name} 1 one call\n"),
    );
    assert!(!uneven.problems.is_empty(), "{:?}", uneven.problems);
    // By raw symbol, one line each: passes too.
    let by_symbol = run(
        &funcs,
        &["root"],
        &format!("allow indirect {s1} 1 first\nallow indirect {s2} 1 second\n"),
    );
    assert!(by_symbol.problems.is_empty(), "{:?}", by_symbol.problems);
    // One of them named by symbol and the other by the path is still an error
    // (the path names both).
    let mixed = run(
        &funcs,
        &["root"],
        &format!("allow indirect {s1} 1 first\nallow indirect core::ptr::drop_in_place 1 second\n"),
    );
    assert!(!mixed.problems.is_empty());
}

#[test]
fn a_symbol_loses_its_hash_and_local_copy_suffix_and_nothing_else() {
    for (symbol, name) in [
        (
            "_ZN4core3ptr13drop_in_place17h0123456789abcdefE",
            "_ZN4core3ptr13drop_in_place",
        ),
        (
            "_ZN4core3ptr13drop_in_place17h0123456789abcdefE.1322",
            "_ZN4core3ptr13drop_in_place",
        ),
        (
            "_ZN4core3ptr13drop_in_place17h0123456789abcdefE.llvm.99",
            "_ZN4core3ptr13drop_in_place",
        ),
        (
            "_ZN3a..4m17hZZZZZZZZZZZZZZZZE",
            "_ZN3a..4m17hZZZZZZZZZZZZZZZZE",
        ),
        ("memcpy", "memcpy"),
        ("a.b", "a.b"),
    ] {
        assert_eq!(stable(symbol), name, "{symbol}");
    }
}

/// `_ZN<len>krate<len>name17h<hash>E`.
fn sym(krate: &str, name: &str, hash: u32) -> String {
    format!(
        "_ZN{}{krate}{}{name}17h{hash:016x}E",
        krate.len(),
        name.len()
    )
}

#[test]
fn an_allow_rom_line_covers_any_caller_of_that_routine_outside_the_image_only() {
    let a = BASE;
    let rom = BASE + 0x1000;
    let one = f(&[&entry(32), &call(8, a + 3, rom), &RETW_N]);
    let funcs = [("root", one)];
    let line = "allow rom 0x42001000 the ROM's memcpy\n";
    assert!(run(&funcs, &["root"], line).problems.is_empty());
    // Another routine, or none called, is not covered; a stale line fails.
    let other = f(&[&entry(32), &call(8, a + 3, rom + 4), &RETW_N]);
    let report = run(&[("root", other)], &["root"], line);
    assert!(
        report
            .problems
            .iter()
            .any(|p| p.contains("outside the image"))
    );
    assert!(
        report
            .problems
            .iter()
            .any(|p| p.contains("matches nothing"))
    );
}

#[test]
fn an_indirect_in_line_is_a_ceiling_for_the_crates_functions_and_must_match_one() {
    let a = BASE;
    let two = f(&[&entry(32), &callx(8), &callx(12), &RETW_N]);
    let root = f(&[
        &entry(32),
        &call(8, a + 3, a + 16),
        &call(8, a + 6, a + 32),
        &RETW_N,
    ]);
    let (x, y) = (
        sym("paperpaw_plugin_one", "run", 1),
        sym("paperpaw_plugin_two", "go", 2),
    );
    let funcs = [
        ("root", root),
        (x.as_str(), two.clone()),
        (y.as_str(), f(&[&entry(32), &callx(8), &RETW_N])),
    ];
    let why = " device test S4\n";
    // 3 sites in two crates: a prefix covers both, up to its ceiling.
    let both = run(
        &funcs,
        &["root"],
        &format!("allow indirect-in paperpaw_plugin_* 3{why}"),
    );
    assert!(both.problems.is_empty(), "{:?}", both.problems);
    let over = run(
        &funcs,
        &["root"],
        &format!("allow indirect-in paperpaw_plugin_* 2{why}"),
    );
    assert!(
        over.problems.iter().any(|p| p.contains("have 3 call(s)")),
        "{:?}",
        over.problems
    );
    // A crate by its exact name covers that crate only: the other is reported.
    let one = run(
        &funcs,
        &["root"],
        &format!("allow indirect-in paperpaw_plugin_one 2{why}"),
    );
    assert_eq!(one.problems.len(), 1, "{:?}", one.problems);
    assert!(one.problems[0].contains("1 call(s) through a register"));
    // A line no function matches is stale; a function's own line comes first.
    let stale = run(
        &funcs,
        &["root"],
        &format!("allow indirect-in paperpaw_other 9{why}"),
    );
    assert!(stale.problems.iter().any(|p| p.contains("matches nothing")));
    let own = run(
        &funcs,
        &["root"],
        &format!(
            "allow indirect-in paperpaw_plugin_* 1{why}allow indirect paperpaw_plugin_one::run 2{why}"
        ),
    );
    assert!(own.problems.is_empty(), "{:?}", own.problems);
}

#[test]
fn the_crate_of_a_function_is_read_from_its_path_or_the_type_of_its_impl() {
    use super::family::{crate_of_symbol, family_matches};
    assert_eq!(
        crate_of_symbol(&sym("paperpaw_sdk", "x", 1)).as_deref(),
        Some("paperpaw_sdk")
    );
    let imp = "_ZN62_$LT$paperpaw_plugin_update..native_plugin..NativeUpdate$u20$as$u20$a$GT$5event17h0000000000000001E";
    assert_eq!(
        crate_of_symbol(imp).as_deref(),
        Some("paperpaw_plugin_update")
    );
    assert_eq!(crate_of_symbol("memcpy"), None);
    assert!(family_matches(
        "paperpaw_plugin_*",
        "paperpaw_plugin_update"
    ));
    assert!(!family_matches("paperpaw_plugin_*", "paperpaw_sdk"));
    assert!(family_matches("core", "core") && !family_matches("core", "core_x"));
}

#[test]
fn an_include_line_brings_in_a_file_beside_the_budget_and_nothing_deeper() {
    let dir = std::env::temp_dir().join(format!("paperpaw-sign-include-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("shared.txt"), "root shared_root\n").unwrap();
    std::fs::write(dir.join("deep.txt"), "include shared.txt\n").unwrap();
    let base = dir.join("budget.txt");
    let text = expand_includes("root a\ninclude shared.txt # why\n", &base).unwrap();
    assert_eq!(Budget::parse(&text).unwrap().roots, ["a", "shared_root"]);
    assert!(expand_includes("include deep.txt\n", &base).is_err());
    assert!(expand_includes("include missing.txt\n", &base).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}
