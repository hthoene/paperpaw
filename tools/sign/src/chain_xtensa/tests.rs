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
    // `rotw`, `l32e` (RST0, op1 = 0, op2 = 4 `rotw`; RST0 `l32e` is op1 = 9).
    let rotw = f(&[&entry(32), &[0x20, 0x80, 0x40], &RETW_N]);
    let report = run(&[("root", rotw)], &["root"], "");
    assert!(
        report
            .problems
            .iter()
            .any(|p| p.contains("does not classify")),
        "{:?}",
        report.problems
    );
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
fn the_window_registers_are_a_stack_pointer_change_and_ps_is_its_own_form() {
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
        let line = format!("allow form root {kind} not enough\n");
        let listed = run(&[("root", root.clone())], &["root"], &line);
        assert!(
            listed.problems.iter().any(|p| p.contains("stack pointer")),
            "{name}: {:?}",
            listed.problems
        );
        let sp = run(&[("root", root)], &["root"], "allow sp root window moved\n");
        assert!(sp.problems.is_empty(), "{name}: {:?}", sp.problems);
    }
    // PS (230): `wsr.ps` and `xsr.ps` are forms of their own; the generic
    // `wsr` or `xsr` line does not cover them (and is then unused).
    for (kind, word) in [("wsr", [0x20, 0xe6, 0x13]), ("xsr", [0x20, 0xe6, 0x61])] {
        let root = f(&[&entry(32), &word, &RETW_N]);
        let generic = run(
            &[("root", root.clone())],
            &["root"],
            &format!("allow form root {kind} wrong register\n"),
        );
        assert_eq!(generic.problems.len(), 2, "{kind}: {:?}", generic.problems);
        let exact = run(
            &[("root", root)],
            &["root"],
            &format!("allow form root {kind}.ps restores the interrupt level\n"),
        );
        assert!(exact.problems.is_empty(), "{kind}.ps: {:?}", exact.problems);
    }
    // Another special register (VECBASE, 231) is the generic form.
    let vec = f(&[&entry(32), &[0x30, 0xe7, 0x13], &RETW_N]);
    let ok = run(&[("root", vec)], &["root"], "allow form root wsr vecbase\n");
    assert!(ok.problems.is_empty(), "{:?}", ok.problems);
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
    // `rotw 2`, `rfe`, `rfi 3` and `rfwo` have no line.
    for word in [
        [0x20u8, 0x80, 0x40],
        [0x00, 0x30, 0x00],
        [0x10, 0x33, 0x00],
        [0x00, 0x34, 0x00],
    ] {
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
