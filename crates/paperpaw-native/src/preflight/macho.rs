//! The Mach-O part of the preflight: little-endian 64-bit dynamic libraries
//! (macOS). Read from the load commands and the section types dyld itself
//! uses, and from the section names as well, where a name alone makes dyld
//! or the Objective-C runtime run code.

use super::Refusal;
use super::elf::{to_usize, u32_at, u64_at};

const MH_MAGIC_64: u32 = 0xfeed_facf;
const MH_DYLIB: u32 = 6;
const HEADER: usize = 32;
/// More load commands than a plugin library has; a cap on the walk.
const MAX_COMMANDS: u32 = 128;
const MH_HAS_TLV_DESCRIPTORS: u32 = 0x0080_0000;

const LC_SEGMENT_64: u32 = 0x19;
const SEGMENT_64: usize = 72;
const SECTION_64: usize = 80;
const LC_LOAD_DYLIB: u32 = 0xc;
const LC_REQ_DYLD: u32 = 0x8000_0000;

/// Section types (`flags & SECTION_TYPE`) that run code or set up
/// thread-local storage while the library loads or unloads.
const SECTION_TYPE: u32 = 0xff;
const S_MOD_INIT_FUNC_POINTERS: u32 = 0x9;
const S_MOD_TERM_FUNC_POINTERS: u32 = 0xa;
const S_INTERPOSING: u32 = 0xd;
const S_THREAD_LOCAL_REGULAR: u32 = 0x11;
const S_THREAD_LOCAL_INIT_FUNCTION_POINTERS: u32 = 0x15;
const S_INIT_FUNC_OFFSETS: u32 = 0x16;

/// Load commands that hold data only: no code runs and no library is named
/// for them. Every command not listed here and not read by the check is
/// refused (default deny).
const PLAIN_COMMANDS: [u32; 18] = [
    0x2,                // LC_SYMTAB
    0xb,                // LC_DYSYMTAB
    0xd,                // LC_ID_DYLIB
    0x1b,               // LC_UUID
    0x1d,               // LC_CODE_SIGNATURE
    0x1e,               // LC_SEGMENT_SPLIT_INFO
    0x22,               // LC_DYLD_INFO
    0x22 | LC_REQ_DYLD, // LC_DYLD_INFO_ONLY
    0x24,               // LC_VERSION_MIN_MACOSX
    0x26,               // LC_FUNCTION_STARTS
    0x29,               // LC_DATA_IN_CODE
    0x2a,               // LC_SOURCE_VERSION
    0x2b,               // LC_DYLIB_CODE_SIGN_DRS
    0x2e,               // LC_LINKER_OPTIMIZATION_HINT
    0x32,               // LC_BUILD_VERSION
    0x33 | LC_REQ_DYLD, // LC_DYLD_EXPORTS_TRIE
    0x34 | LC_REQ_DYLD, // LC_DYLD_CHAINED_FIXUPS
    0x31,               // LC_NOTE
];

/// The one library a host plugin may name: the system library, macOS's C
/// library (as `libc.so.6` is on Linux).
const ALLOWED_DYLIB: &[u8] = b"/usr/lib/libSystem.B.dylib";

/// Checks `bytes`, a little-endian 64-bit Mach-O dynamic library built for
/// `cpu` (`CPU_TYPE_X86_64` 0x0100_0007 or `CPU_TYPE_ARM64` 0x0100_000c),
/// for every way to run code or pull in more code while it loads:
/// `LC_ROUTINES(_64)`, an entry point (`LC_MAIN`, `LC_(UNIX)THREAD`),
/// `__mod_init_func`, `__mod_term_func`, `__init_offsets` (by section type
/// and by name), Objective-C sections (`+load`), interposing, thread-local
/// storage, a search path or loader command, and any library besides
/// `libSystem`. A universal ("fat") file is refused: a loader hands over
/// the one slice it loads.
///
/// # Errors
///
/// [`Refusal`] names the first problem; an unknown load command is refused
/// ([`Refusal::DynamicTag`]), never skipped.
pub fn check_macho64(bytes: &[u8], cpu: u32) -> Result<(), Refusal> {
    if bytes.len() < HEADER || u32_at(bytes, 0)? != MH_MAGIC_64 {
        return Err(Refusal::NotSupported);
    }
    if u32_at(bytes, 12)? != MH_DYLIB {
        return Err(Refusal::NotSupported);
    }
    if u32_at(bytes, 4)? != cpu {
        return Err(Refusal::OtherMachine);
    }
    let count = u32_at(bytes, 16)?;
    let size = to_usize(u64::from(u32_at(bytes, 20)?))?;
    if u32_at(bytes, 24)? & MH_HAS_TLV_DESCRIPTORS != 0 {
        return Err(Refusal::RunsCodeAtLoad("thread-local storage"));
    }
    if count > MAX_COMMANDS {
        return Err(Refusal::DynamicTag);
    }
    let end = HEADER.checked_add(size).ok_or(Refusal::Truncated)?;
    let commands = bytes.get(HEADER..end).ok_or(Refusal::Truncated)?;
    let mut at = 0usize;
    for _ in 0..count {
        let cmd = u32_at(commands, at)?;
        let len = to_usize(u64::from(u32_at(commands, at + 4)?))?;
        if len < 8 || len % 8 != 0 {
            return Err(Refusal::Truncated);
        }
        let next = at.checked_add(len).ok_or(Refusal::Truncated)?;
        let body = commands.get(at..next).ok_or(Refusal::Truncated)?;
        command(bytes, cmd, body)?;
        at = next;
    }
    Ok(())
}

/// One load command, `body` its bytes (from `cmd` on).
fn command(file: &[u8], cmd: u32, body: &[u8]) -> Result<(), Refusal> {
    match cmd {
        LC_SEGMENT_64 => segment(file, body),
        LC_LOAD_DYLIB => {
            let offset = to_usize(u64::from(u32_at(body, 8)?))?;
            let rest = body.get(offset..).ok_or(Refusal::Truncated)?;
            let name = &rest[..rest
                .iter()
                .position(|&c| c == 0)
                .ok_or(Refusal::Truncated)?];
            if name == ALLOWED_DYLIB {
                Ok(())
            } else {
                Err(Refusal::RunsCodeAtLoad("a library other than libSystem"))
            }
        }
        // LC_LOAD_WEAK_DYLIB, LC_REEXPORT_DYLIB, LC_LAZY_LOAD_DYLIB,
        // LC_LOAD_UPWARD_DYLIB.
        0x8000_0018 | 0x8000_001f | 0x20 | 0x8000_0023 => {
            Err(Refusal::RunsCodeAtLoad("a library other than libSystem"))
        }
        // LC_ROUTINES, LC_ROUTINES_64.
        0x11 | 0x1a => Err(Refusal::RunsCodeAtLoad("LC_ROUTINES")),
        // LC_THREAD, LC_UNIXTHREAD, LC_MAIN.
        0x4 | 0x5 | 0x8000_0028 => Err(Refusal::RunsCodeAtLoad("an entry point")),
        // LC_RPATH.
        0x8000_001c => Err(Refusal::RunsCodeAtLoad("a library search path")),
        // LC_LOAD_DYLINKER, LC_ID_DYLINKER, LC_DYLD_ENVIRONMENT.
        0xe | 0xf | 0x27 => Err(Refusal::RunsCodeAtLoad("a dynamic loader command")),
        c if PLAIN_COMMANDS.contains(&c) => Ok(()),
        _ => Err(Refusal::DynamicTag),
    }
}

/// An `LC_SEGMENT_64` and its sections: within the file, and none that runs
/// code at load.
fn segment(file: &[u8], body: &[u8]) -> Result<(), Refusal> {
    let fileoff = u64_at(body, 40)?;
    let filesize = u64_at(body, 48)?;
    let seg_end = fileoff.checked_add(filesize).ok_or(Refusal::Truncated)?;
    if to_usize(seg_end)? > file.len() {
        return Err(Refusal::Truncated);
    }
    if objc(body.get(8..24).ok_or(Refusal::Truncated)?) {
        return Err(Refusal::RunsCodeAtLoad("Objective-C metadata"));
    }
    let sections = to_usize(u64::from(u32_at(body, 64)?))?;
    let want = sections
        .checked_mul(SECTION_64)
        .and_then(|s| s.checked_add(SEGMENT_64))
        .ok_or(Refusal::Truncated)?;
    if want != body.len() {
        return Err(Refusal::Segments);
    }
    for s in body[SEGMENT_64..].as_chunks::<SECTION_64>().0 {
        section(s)?;
    }
    Ok(())
}

/// One `section_64`.
fn section(s: &[u8]) -> Result<(), Refusal> {
    let name = &s[..16];
    let kind = u32_at(s, 64)? & SECTION_TYPE;
    let named = |n: &[u8]| name.starts_with(n) && name.get(n.len()).is_none_or(|&c| c == 0);
    if kind == S_MOD_INIT_FUNC_POINTERS || named(b"__mod_init_func") {
        return Err(Refusal::RunsCodeAtLoad("__mod_init_func"));
    }
    if kind == S_MOD_TERM_FUNC_POINTERS || named(b"__mod_term_func") {
        return Err(Refusal::RunsCodeAtLoad("__mod_term_func"));
    }
    if kind == S_INIT_FUNC_OFFSETS || named(b"__init_offsets") {
        return Err(Refusal::RunsCodeAtLoad("__init_offsets"));
    }
    if kind == S_INTERPOSING || named(b"__interpose") {
        return Err(Refusal::RunsCodeAtLoad("interposing"));
    }
    if (S_THREAD_LOCAL_REGULAR..=S_THREAD_LOCAL_INIT_FUNCTION_POINTERS).contains(&kind) {
        return Err(Refusal::RunsCodeAtLoad("thread-local storage"));
    }
    if objc(name) || objc(&s[16..32]) {
        return Err(Refusal::RunsCodeAtLoad("Objective-C metadata"));
    }
    Ok(())
}

/// Whether a segment or section name is one the Objective-C runtime reads
/// when an image loads (class lists, `+load`): `__OBJC` or `__objc_*`.
fn objc(name: &[u8]) -> bool {
    name.starts_with(b"__objc_") || name.starts_with(b"__OBJC")
}

#[cfg(test)]
mod tests {
    extern crate std;

    use alloc::vec::Vec;

    use super::{LC_REQ_DYLD, MH_HAS_TLV_DESCRIPTORS, Refusal, check_macho64};

    const X86: u32 = 0x0100_0007;
    const ARM: u32 = 0x0100_000c;

    fn name16(n: &str) -> [u8; 16] {
        let mut out = [0u8; 16];
        out[..n.len()].copy_from_slice(n.as_bytes());
        out
    }

    fn dylib(cmd: u32, path: &str) -> Vec<u8> {
        let mut c = Vec::new();
        let len = (24 + path.len() + 1).next_multiple_of(8);
        c.extend_from_slice(&cmd.to_le_bytes());
        c.extend_from_slice(&(len as u32).to_le_bytes());
        c.extend_from_slice(&24u32.to_le_bytes());
        c.extend_from_slice(&[0; 12]);
        c.extend_from_slice(path.as_bytes());
        c.resize(len, 0);
        c
    }

    fn plain(cmd: u32, len: usize) -> Vec<u8> {
        let mut c = Vec::new();
        c.extend_from_slice(&cmd.to_le_bytes());
        c.extend_from_slice(&(len as u32).to_le_bytes());
        c.resize(len, 0);
        c
    }

    /// An `LC_SEGMENT_64` named `seg` over the whole file, with sections
    /// `(name, flags)`.
    fn segment(seg: &str, sections: &[(&str, u32)], file_len: u64) -> Vec<u8> {
        let mut c = Vec::new();
        c.extend_from_slice(&0x19u32.to_le_bytes());
        c.extend_from_slice(&((72 + 80 * sections.len()) as u32).to_le_bytes());
        c.extend_from_slice(&name16(seg));
        c.extend_from_slice(&0u64.to_le_bytes()); // vmaddr
        c.extend_from_slice(&0x4000u64.to_le_bytes()); // vmsize
        c.extend_from_slice(&0u64.to_le_bytes()); // fileoff
        c.extend_from_slice(&file_len.to_le_bytes()); // filesize
        c.extend_from_slice(&[5, 0, 0, 0, 5, 0, 0, 0]); // maxprot, initprot
        c.extend_from_slice(&(sections.len() as u32).to_le_bytes());
        c.extend_from_slice(&0u32.to_le_bytes());
        for (name, flags) in sections {
            c.extend_from_slice(&name16(name));
            c.extend_from_slice(&name16(seg));
            c.extend_from_slice(&[0; 32]); // addr, size, offset, align, reloff, nreloc
            c.extend_from_slice(&flags.to_le_bytes());
            c.extend_from_slice(&[0; 12]);
        }
        c
    }

    /// A plugin-shaped dylib: its id, `__TEXT,__text`, libSystem, a symbol
    /// table, and `extra` commands after them.
    fn build(cpu: u32, flags: u32, sections: &[(&str, u32)], extra: &[Vec<u8>]) -> Vec<u8> {
        let mut cmds: Vec<Vec<u8>> = std::vec![
            dylib(0xd, "@rpath/libplugin.dylib"),
            dylib(0xc, "/usr/lib/libSystem.B.dylib"),
            plain(0x2, 24),
            plain(0x32, 24),
        ];
        let mut text = std::vec![("__text", 0x8000_0400u32)];
        text.extend_from_slice(sections);
        cmds.extend(extra.iter().cloned());
        let seg_len = 72 + 80 * text.len();
        let size: usize = cmds.iter().map(Vec::len).sum::<usize>() + seg_len;
        let file_len = (32 + size) as u64;
        cmds.insert(0, segment("__TEXT", &text, file_len));
        let mut out = Vec::new();
        for w in [
            0xfeed_facfu32,
            cpu,
            3,
            6,
            cmds.len() as u32,
            size as u32,
            flags,
            0,
        ] {
            out.extend_from_slice(&w.to_le_bytes());
        }
        for c in cmds {
            out.extend_from_slice(&c);
        }
        out
    }

    #[test]
    fn a_plain_dylib_is_accepted_for_its_cpu_only() {
        assert_eq!(check_macho64(&build(X86, 0, &[], &[]), X86), Ok(()));
        assert_eq!(check_macho64(&build(ARM, 0, &[], &[]), ARM), Ok(()));
        assert_eq!(
            check_macho64(&build(ARM, 0, &[], &[]), X86),
            Err(Refusal::OtherMachine)
        );
    }

    /// Added sections, header flags, an added command, and what is refused.
    type Case = (&'static [(&'static str, u32)], u32, Vec<u8>, &'static str);

    #[test]
    fn every_way_to_run_code_at_load_is_refused() {
        let cases: [Case; 14] = [
            (
                &[("__mod_init_func", 0x9)],
                0,
                Vec::new(),
                "__mod_init_func",
            ),
            (&[("__other", 0x9)], 0, Vec::new(), "__mod_init_func"),
            (&[("__mod_init_func", 0)], 0, Vec::new(), "__mod_init_func"),
            (
                &[("__mod_term_func", 0xa)],
                0,
                Vec::new(),
                "__mod_term_func",
            ),
            (&[("__init_offsets", 0x16)], 0, Vec::new(), "__init_offsets"),
            (
                &[("__objc_nlclslist", 0)],
                0,
                Vec::new(),
                "Objective-C metadata",
            ),
            (&[("__interpose", 0xd)], 0, Vec::new(), "interposing"),
            (
                &[("__thread_vars", 0x13)],
                0,
                Vec::new(),
                "thread-local storage",
            ),
            (
                &[],
                MH_HAS_TLV_DESCRIPTORS,
                Vec::new(),
                "thread-local storage",
            ),
            (&[], 0, plain(0x1a, 152), "LC_ROUTINES"),
            (&[], 0, plain(0x28 | LC_REQ_DYLD, 24), "an entry point"),
            (
                &[],
                0,
                dylib(0xc, "/usr/lib/libobjc.A.dylib"),
                "a library other than libSystem",
            ),
            (
                &[],
                0,
                dylib(0x18 | LC_REQ_DYLD, "/usr/lib/libSystem.B.dylib"),
                "a library other than libSystem",
            ),
            (
                &[],
                0,
                plain(0x1c | LC_REQ_DYLD, 16),
                "a library search path",
            ),
        ];
        for (sections, flags, extra, what) in cases {
            let extra: &[Vec<u8>] = if extra.is_empty() {
                &[]
            } else {
                core::slice::from_ref(&extra)
            };
            let file = build(X86, flags, sections, extra);
            assert_eq!(
                check_macho64(&file, X86),
                Err(Refusal::RunsCodeAtLoad(what)),
                "{what}"
            );
        }
    }

    #[test]
    fn unknown_commands_other_files_and_short_files_are_refused() {
        let unknown = build(X86, 0, &[], &[plain(0x7777, 8)]);
        assert_eq!(check_macho64(&unknown, X86), Err(Refusal::DynamicTag));
        let mut executable = build(X86, 0, &[], &[]);
        executable[12] = 2; // MH_EXECUTE
        assert_eq!(check_macho64(&executable, X86), Err(Refusal::NotSupported));
        let mut fat = build(X86, 0, &[], &[]);
        fat[..4].copy_from_slice(&0xcafe_babeu32.to_be_bytes());
        assert_eq!(check_macho64(&fat, X86), Err(Refusal::NotSupported));
        let whole = build(X86, 0, &[], &[]);
        for cut in [0, 16, 40, whole.len() - 1] {
            assert!(check_macho64(&whole[..cut], X86).is_err(), "cut at {cut}");
        }
    }
}
