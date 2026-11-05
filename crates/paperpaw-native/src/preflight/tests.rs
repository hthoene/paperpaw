//! The ELF preflight on small files built here, byte by byte: one that is
//! accepted, and the same with each way to run code at load added.

extern crate std;

use alloc::vec::Vec;

use super::{Refusal, check_elf64_le};

const X86: u16 = 62;
const ARM: u16 = 183;

/// What goes into a built file, beyond the plain shared object.
#[derive(Default, Clone)]
struct Spec {
    machine: u16,
    needed: Vec<&'static str>,
    extra_dyn: Vec<(i64, u64)>,
    tls: bool,
    ifunc_symbol: bool,
    irelative: bool,
    irelative_in_jmprel: bool,
    gnu_hash: bool,
    /// With the GNU table, also a SysV one stating this many symbols.
    sysv_too: Option<u32>,
    /// A second loadable segment over the first one's addresses.
    overlapping_load: bool,
    /// The dynamic segment's file offset is not where its address maps.
    skewed_dynamic: bool,
    /// The jump-slot table claims `Rel` entries (DT_PLTREL = 17).
    pltrel_rel: bool,
}

fn put16(v: &mut Vec<u8>, x: u16) {
    v.extend_from_slice(&x.to_le_bytes());
}
fn put32(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_le_bytes());
}
fn put64(v: &mut Vec<u8>, x: u64) {
    v.extend_from_slice(&x.to_le_bytes());
}

/// The ELF header of a little-endian 64-bit shared object with `nph` program headers.
fn header(f: &mut Vec<u8>, machine: u16, nph: usize) {
    f.extend_from_slice(b"\x7fELF\x02\x01\x01\0");
    f.extend_from_slice(&[0; 8]);
    put16(f, 3); // ET_DYN
    put16(f, machine);
    put32(f, 1);
    put64(f, 0); // entry
    put64(f, 64); // phoff
    put64(f, 0); // shoff
    put32(f, 0);
    put16(f, 64);
    put16(f, 56);
    put16(f, nph as u16);
    put16(f, 64);
    put16(f, 0);
    put16(f, 0);
}

/// One program header whose memory address is its file offset.
fn phdr(f: &mut Vec<u8>, kind: u32, off: u64, size: u64) {
    phdr_at(f, kind, off, off, size);
}

/// One program header at file offset `off`, memory address `vaddr`.
fn phdr_at(f: &mut Vec<u8>, kind: u32, off: u64, vaddr: u64, size: u64) {
    put32(f, kind);
    put32(f, 5);
    put64(f, off);
    put64(f, vaddr);
    put64(f, vaddr);
    put64(f, size);
    put64(f, size);
    put64(f, 8);
}

/// The dynamic symbols: null, one function, one more (an IFUNC when asked).
fn symbols(f: &mut Vec<u8>, spec: &Spec, sym_name: u32) {
    f.extend_from_slice(&[0; 24]);
    for kind in [2u8, if spec.ifunc_symbol { 10 } else { 2 }] {
        put32(f, sym_name);
        f.push((1 << 4) | kind);
        f.push(0);
        put16(f, 1);
        put64(f, 0x100);
        put64(f, 4);
    }
}

/// The hash table(s) of the dynamic symbols: SysV, or GNU (with an optional SysV one after it).
fn hash_tables(f: &mut Vec<u8>, spec: &Spec, nsym: usize) {
    let sysv = |f: &mut Vec<u8>, nchain: u32| {
        put32(f, 1);
        put32(f, nchain);
        put32(f, 0);
        for _ in 0..nsym {
            put32(f, 0);
        }
    };
    if spec.gnu_hash {
        put32(f, 1); // nbuckets
        put32(f, 1); // symoffset: symbol 0 is not hashed
        put32(f, 1); // bloom size
        put32(f, 6); // bloom shift
        put64(f, !0); // bloom
        put32(f, 1); // bucket 0 starts at symbol 1
        put32(f, 0); // chain of symbol 1 (not last)
        put32(f, 1); // chain of symbol 2 (last)
        if let Some(nchain) = spec.sysv_too {
            sysv(f, nchain);
        }
    } else {
        sysv(f, nsym as u32);
    }
}

/// A shared object whose single `PT_LOAD` maps the file at its own offsets.
fn build(spec: &Spec) -> Vec<u8> {
    let nph: usize = 2 + usize::from(spec.tls) + usize::from(spec.overlapping_load);
    let after_ph = 64 + nph * 56;
    // string table: "\0" + needed names + "sym\0"
    let mut strtab = std::vec![0u8];
    let mut name_at = Vec::new();
    for n in &spec.needed {
        name_at.push(strtab.len() as u64);
        strtab.extend_from_slice(n.as_bytes());
        strtab.push(0);
    }
    let sym_name = strtab.len() as u32;
    strtab.extend_from_slice(b"sym\0");
    let strtab_at = after_ph;
    let symtab_at = (strtab_at + strtab.len() + 7) & !7;
    let nsym = 3usize;
    let hash_at = symtab_at + nsym * 24;
    let gnu_len = 16 + 8 + 4 + 4 * 2;
    let sysv_len = 8 + 4 + 4 * nsym;
    let hash_len = if spec.gnu_hash {
        gnu_len + if spec.sysv_too.is_some() { sysv_len } else { 0 }
    } else {
        sysv_len
    };
    let rela_at = (hash_at + hash_len + 7) & !7;
    let rela_len = if spec.irelative { 24 } else { 0 };
    let jmprel_at = rela_at + rela_len;
    let jmprel_len = if spec.irelative_in_jmprel { 24 } else { 0 };
    let dyn_at = (jmprel_at + jmprel_len + 7) & !7;

    let mut dynv: Vec<(i64, u64)> = Vec::new();
    for at in &name_at {
        dynv.push((1, *at));
    }
    dynv.push((5, strtab_at as u64));
    dynv.push((6, symtab_at as u64));
    dynv.push((11, 24));
    dynv.push((if spec.gnu_hash { 0x6fff_fef5 } else { 4 }, hash_at as u64));
    if spec.sysv_too.is_some() {
        dynv.push((4, (hash_at + gnu_len) as u64));
    }
    if spec.irelative {
        dynv.push((7, rela_at as u64));
        dynv.push((8, 24));
    }
    if spec.irelative_in_jmprel {
        dynv.push((23, jmprel_at as u64));
        dynv.push((2, 24));
        dynv.push((20, if spec.pltrel_rel { 17 } else { 7 }));
    }
    dynv.extend(spec.extra_dyn.iter().copied());
    dynv.push((0, 0));
    let total = dyn_at + dynv.len() * 16;

    let mut f = Vec::new();
    header(&mut f, spec.machine, nph);
    phdr(&mut f, 1, 0, total as u64); // PT_LOAD over the whole file
    if spec.skewed_dynamic {
        phdr_at(
            &mut f,
            2,
            dyn_at as u64,
            dyn_at as u64 - 16,
            (dynv.len() * 16) as u64,
        );
    } else {
        phdr(&mut f, 2, dyn_at as u64, (dynv.len() * 16) as u64);
    }
    if spec.overlapping_load {
        phdr(&mut f, 1, 0, 64);
    }
    if spec.tls {
        phdr(&mut f, 7, 0, 8);
    }
    assert_eq!(f.len(), after_ph);
    f.extend_from_slice(&strtab);
    f.resize(symtab_at, 0);
    symbols(&mut f, spec, sym_name);
    assert_eq!(f.len(), hash_at);
    hash_tables(&mut f, spec, nsym);
    f.resize(rela_at, 0);
    let irel = if spec.machine == X86 { 37u64 } else { 1032 };
    if spec.irelative {
        put64(&mut f, 0x200);
        put64(&mut f, irel);
        put64(&mut f, 0);
    }
    if spec.irelative_in_jmprel {
        put64(&mut f, 0x200);
        put64(&mut f, irel);
        put64(&mut f, 0);
    }
    f.resize(dyn_at, 0);
    for (tag, val) in dynv {
        put64(&mut f, tag as u64);
        put64(&mut f, val);
    }
    assert_eq!(f.len(), total);
    f
}

fn plain(machine: u16) -> Spec {
    Spec {
        machine,
        ..Spec::default()
    }
}

fn refused(spec: &Spec, what: &'static str) {
    let machine = spec.machine;
    assert_eq!(
        check_elf64_le(&build(spec), machine),
        Err(Refusal::RunsCodeAtLoad(what))
    );
}

#[test]
fn a_plain_shared_object_is_accepted() {
    for machine in [X86, ARM] {
        for gnu_hash in [false, true] {
            let spec = Spec {
                gnu_hash,
                needed: std::vec!["libc.so.6", "libm.so.6"],
                ..plain(machine)
            };
            assert_eq!(
                check_elf64_le(&build(&spec), machine),
                Ok(()),
                "{machine} gnu_hash={gnu_hash}"
            );
        }
    }
}

#[test]
fn zero_sized_arrays_run_nothing_and_are_accepted() {
    let spec = Spec {
        extra_dyn: std::vec![(27, 0), (28, 0), (33, 0)],
        ..plain(X86)
    };
    assert_eq!(check_elf64_le(&build(&spec), X86), Ok(()));
}

#[test]
fn every_way_to_run_code_at_load_is_refused() {
    let with = |tag: i64, val: u64| Spec {
        extra_dyn: std::vec![(tag, val)],
        ..plain(X86)
    };
    refused(&with(12, 0x100), "DT_INIT");
    refused(&with(13, 0x100), "DT_FINI");
    refused(&with(27, 8), "DT_INIT_ARRAY");
    refused(&with(28, 8), "DT_FINI_ARRAY");
    refused(&with(33, 8), "DT_PREINIT_ARRAY");
    refused(&with(22, 0), "text relocations");
    refused(&with(30, 0x4), "text relocations");
    refused(&with(30, 0x10), "static thread-local storage");
    refused(&with(15, 0), "a library search path");
    refused(&with(29, 0), "a library search path");
    refused(&with(0x7fff_fffd, 0), "an auxiliary or filter library");
    refused(&with(0x7fff_ffff, 0), "an auxiliary or filter library");
    refused(
        &Spec {
            tls: true,
            ..plain(X86)
        },
        "thread-local storage",
    );
    for machine in [X86, ARM] {
        for gnu_hash in [false, true] {
            refused(
                &Spec {
                    ifunc_symbol: true,
                    gnu_hash,
                    ..plain(machine)
                },
                "an IFUNC symbol",
            );
        }
        refused(
            &Spec {
                irelative: true,
                ..plain(machine)
            },
            "an IFUNC relocation",
        );
        refused(
            &Spec {
                irelative_in_jmprel: true,
                ..plain(machine)
            },
            "an IFUNC relocation",
        );
    }
}

#[test]
fn files_the_checker_and_the_system_loader_could_read_differently_are_refused() {
    let machine = X86;
    for (what, spec) in [
        (
            "overlapping segments",
            Spec {
                overlapping_load: true,
                ..plain(machine)
            },
        ),
        (
            "a dynamic segment away from its address",
            Spec {
                skewed_dynamic: true,
                ..plain(machine)
            },
        ),
        (
            "jump slots that are not Rela",
            Spec {
                irelative_in_jmprel: true,
                pltrel_rel: true,
                ..plain(machine)
            },
        ),
    ] {
        assert_eq!(
            check_elf64_le(&build(&spec), machine),
            Err(Refusal::NotSupported),
            "{what}"
        );
    }
}

#[test]
fn two_hash_tables_that_disagree_are_refused_and_the_ifunc_cannot_hide() {
    for machine in [X86, ARM] {
        // Both tables state the three symbols: read, and the IFUNC is found.
        let honest = Spec {
            ifunc_symbol: true,
            gnu_hash: true,
            sysv_too: Some(3),
            ..plain(machine)
        };
        assert_eq!(
            check_elf64_le(&build(&honest), machine),
            Err(Refusal::RunsCodeAtLoad("an IFUNC symbol"))
        );
        // The SysV table says one symbol, the GNU table three: the symbols the
        // system loader reads through the GNU table must not escape the check.
        let lying = Spec {
            ifunc_symbol: true,
            gnu_hash: true,
            sysv_too: Some(1),
            ..plain(machine)
        };
        assert_eq!(
            check_elf64_le(&build(&lying), machine),
            Err(Refusal::NotSupported)
        );
        // Agreement on a clean file is accepted.
        let clean = Spec {
            gnu_hash: true,
            sysv_too: Some(3),
            ..plain(machine)
        };
        assert_eq!(check_elf64_le(&build(&clean), machine), Ok(()));
    }
}

#[test]
fn a_library_other_than_the_c_library_is_refused() {
    let spec = Spec {
        needed: std::vec!["libc.so.6", "libevil.so"],
        ..plain(X86)
    };
    refused(&spec, "a library other than the C library");
}

#[test]
fn a_file_that_is_not_the_asked_machine_or_not_an_elf_is_refused() {
    assert_eq!(
        check_elf64_le(&build(&plain(ARM)), X86),
        Err(Refusal::OtherMachine)
    );
    assert_eq!(
        check_elf64_le(&build(&plain(X86)), 3),
        Err(Refusal::NotSupported)
    );
    assert_eq!(check_elf64_le(b"MZ", X86), Err(Refusal::NotSupported));
    assert_eq!(check_elf64_le(&[], X86), Err(Refusal::NotSupported));
    let mut big_endian = build(&plain(X86));
    big_endian[5] = 2;
    assert_eq!(check_elf64_le(&big_endian, X86), Err(Refusal::NotSupported));
    let mut class32 = build(&plain(X86));
    class32[4] = 1;
    assert_eq!(check_elf64_le(&class32, X86), Err(Refusal::NotSupported));
}

#[test]
fn every_truncation_and_header_edit_is_refused_or_survives_without_a_crash() {
    let good = build(&Spec {
        needed: std::vec!["libc.so.6"],
        ..plain(X86)
    });
    // Cut anywhere: never a panic, and never accepted but for the full file.
    for len in 0..good.len() {
        assert_ne!(check_elf64_le(&good[..len], X86), Ok(()), "cut at {len}");
    }
    // One byte changed anywhere: never a panic.
    for at in 0..good.len() {
        for value in [0u8, 0xff, 0x80, 1] {
            let mut bad = good.clone();
            bad[at] = value;
            let _ = check_elf64_le(&bad, X86);
        }
    }
}

/// Real libraries built by the toolchain on this machine (Linux hosts).
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod real {
    extern crate std;

    use std::path::Path;
    use std::process::Command;
    use std::string::String;
    use std::{format, fs};

    use super::{ARM, X86};
    use crate::preflight::{Refusal, check_elf64_le};

    const MACHINE: u16 = if cfg!(target_arch = "x86_64") {
        X86
    } else {
        ARM
    };

    const PLAIN: &str = r#"#![no_std]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { loop {} }
#[unsafe(no_mangle)]
pub extern "C" fn plugin_entry() -> u32 { 7 }
"#;

    // An initialiser that creates a marker file the moment the library is loaded.
    const WITH_INIT: &str = r#"#![no_std]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { loop {} }
unsafe extern "C" { fn creat(path: *const u8, mode: u32) -> i32; }
extern "C" fn make_marker() { unsafe { creat(b"marker-must-not-exist\0".as_ptr(), 0o600); } }
#[used]
#[unsafe(link_section = ".init_array")]
static INIT: extern "C" fn() = make_marker;
#[unsafe(no_mangle)]
pub extern "C" fn plugin_entry() -> u32 { 7 }
"#;

    fn build(dir: &Path, name: &str, source: &str) -> Option<std::vec::Vec<u8>> {
        let src = dir.join(format!("{name}.rs"));
        let out = dir.join(format!("lib{name}.so"));
        fs::write(&src, source).unwrap();
        let rustc = std::env::var("RUSTC").unwrap_or_else(|_| String::from("rustc"));
        let status = Command::new(rustc)
            .args([
                "--edition",
                "2024",
                "--crate-type",
                "cdylib",
                "-C",
                "panic=abort",
                "-C",
                "link-arg=-nostartfiles",
                "-O",
                "-o",
            ])
            .arg(&out)
            .arg(&src)
            .current_dir(dir)
            .status();
        // No compiler to run: the byte-built tests stand. A compiler that runs
        // and fails is a failure of the test, never a silent pass.
        let status = status.ok()?;
        assert!(
            status.success(),
            "the fixture library {name} did not compile"
        );
        Some(fs::read(out).unwrap())
    }

    #[test]
    fn a_real_library_is_accepted_and_one_with_an_initialiser_is_refused_without_loading() {
        let dir = std::env::temp_dir().join(std::format!("pp-preflight-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let (Some(plain), Some(init)) =
            (build(&dir, "plain", PLAIN), build(&dir, "init", WITH_INIT))
        else {
            // No usable compiler in this environment: the synthetic tests stand.
            std::eprintln!("preflight: no rustc to build real libraries; skipped");
            return;
        };
        assert_eq!(
            check_elf64_le(&plain, MACHINE),
            Ok(()),
            "a plain no_std cdylib"
        );
        assert!(
            matches!(
                check_elf64_le(&init, MACHINE),
                Err(Refusal::RunsCodeAtLoad(_))
            ),
            "a library with an .init_array entry: {:?}",
            check_elf64_le(&init, MACHINE)
        );
        // Nothing was loaded, so nothing ran.
        assert!(!dir.join("marker-must-not-exist").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
