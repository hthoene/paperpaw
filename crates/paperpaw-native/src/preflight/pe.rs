//! The PE part of the preflight: PE32+ DLLs (Windows). Read from the COFF
//! header, the optional header and its data directories, as the Windows
//! loader reads them; the section names play no part.

use super::Refusal;
use super::elf::{to_usize, u16_at, u32_at};

const PE_SIGNATURE: &[u8] = b"PE\0\0";
const COFF: usize = 20;
const IMAGE_FILE_DLL: u16 = 0x2000;
const PE32_PLUS: u16 = 0x20b;
/// The optional header's fixed part before its data directories.
const OPTIONAL_FIXED: usize = 112;
const DIRECTORIES: usize = 16;
const SECTION: usize = 40;
/// The Windows loader's own limit.
const MAX_SECTIONS: u16 = 96;
const IMPORT: usize = 20;
/// More imported libraries than a plugin library has; a cap on the walk.
const MAX_IMPORTS: usize = 16;

const DIR_IMPORT: usize = 1;
const DIR_ARCHITECTURE: usize = 7;
const DIR_GLOBAL_PTR: usize = 8;
const DIR_TLS: usize = 9;
const DIR_DELAY_IMPORT: usize = 13;
const DIR_CLR: usize = 14;
const DIR_RESERVED: usize = 15;

/// The libraries a host plugin may import from: the system's own and the C
/// runtime, which every Windows process has loaded already (as the C library
/// on Linux), compared without case. `api-ms-win-crt-` names the C runtime's
/// API sets.
const ALLOWED_DLLS: [&[u8]; 4] = [
    b"kernel32.dll",
    b"ntdll.dll",
    b"ucrtbase.dll",
    b"vcruntime140.dll",
];
const ALLOWED_PREFIX: &[u8] = b"api-ms-win-crt-";

/// Checks `bytes`, a PE32+ DLL built for `machine` (`IMAGE_FILE_MACHINE_AMD64`
/// 0x8664 or `IMAGE_FILE_MACHINE_ARM64` 0xaa64), for every way to run code
/// or pull in more code while it loads: an entry point (`AddressOfEntryPoint`
/// must be zero, ADR 0034 section 6, without exception), a TLS directory
/// (its callbacks), delay-load imports, a CLR header, and an import of any
/// library outside the system's and the C runtime's.
///
/// # Errors
///
/// [`Refusal`] names the first problem; a data directory the check does not
/// know is refused ([`Refusal::DynamicTag`]), never skipped.
pub fn check_pe32plus(bytes: &[u8], machine: u16) -> Result<(), Refusal> {
    if bytes.get(..2) != Some(b"MZ") {
        return Err(Refusal::NotSupported);
    }
    let pe = to_usize(u64::from(u32_at(bytes, 0x3c)?))?;
    let coff = pe.checked_add(4).ok_or(Refusal::Truncated)?;
    if bytes.get(pe..coff) != Some(PE_SIGNATURE) {
        return Err(Refusal::NotSupported);
    }
    let sections = u16_at(bytes, coff + 2)?;
    let optional_size = usize::from(u16_at(bytes, coff + 16)?);
    if u16_at(bytes, coff + 18)? & IMAGE_FILE_DLL == 0 {
        return Err(Refusal::NotSupported);
    }
    let optional = coff.checked_add(COFF).ok_or(Refusal::Truncated)?;
    if u16_at(bytes, optional)? != PE32_PLUS {
        return Err(Refusal::NotSupported);
    }
    if u16_at(bytes, coff)? != machine {
        return Err(Refusal::OtherMachine);
    }
    if u32_at(bytes, optional + 16)? != 0 {
        return Err(Refusal::RunsCodeAtLoad("an entry point"));
    }
    let directories = to_usize(u64::from(u32_at(bytes, optional + 108)?))?;
    if directories > DIRECTORIES || OPTIONAL_FIXED + 8 * directories > optional_size {
        return Err(Refusal::DynamicTag);
    }
    if sections > MAX_SECTIONS {
        return Err(Refusal::Segments);
    }
    let table = optional
        .checked_add(optional_size)
        .ok_or(Refusal::Truncated)?;
    let table_end = table
        .checked_add(SECTION * usize::from(sections))
        .ok_or(Refusal::Truncated)?;
    let table = bytes.get(table..table_end).ok_or(Refusal::Truncated)?;
    for s in table.chunks_exact(SECTION) {
        let raw = to_usize(u64::from(u32_at(s, 16)?))?;
        let at = to_usize(u64::from(u32_at(s, 20)?))?;
        if raw != 0 && at.checked_add(raw).is_none_or(|end| end > bytes.len()) {
            return Err(Refusal::Truncated);
        }
    }
    for index in 0..directories {
        let at = optional + OPTIONAL_FIXED + 8 * index;
        let (rva, size) = (u32_at(bytes, at)?, u32_at(bytes, at + 4)?);
        if rva == 0 && size == 0 {
            continue;
        }
        match index {
            DIR_IMPORT => imports(bytes, table, rva)?,
            DIR_TLS => return Err(Refusal::RunsCodeAtLoad("a TLS directory")),
            DIR_DELAY_IMPORT => return Err(Refusal::RunsCodeAtLoad("delay-load imports")),
            DIR_CLR => return Err(Refusal::RunsCodeAtLoad("a CLR header")),
            DIR_ARCHITECTURE | DIR_GLOBAL_PTR | DIR_RESERVED => {
                return Err(Refusal::DynamicTag);
            }
            _ => {}
        }
    }
    Ok(())
}

/// The file offset of `rva`, from the section that holds it in the file.
fn offset_of(table: &[u8], rva: u32) -> Result<usize, Refusal> {
    for s in table.chunks_exact(SECTION) {
        let (address, raw, at) = (u32_at(s, 12)?, u32_at(s, 16)?, u32_at(s, 20)?);
        if rva >= address && rva - address < raw {
            return to_usize(u64::from(at) + u64::from(rva - address));
        }
    }
    Err(Refusal::Truncated)
}

/// The import directory at `rva`: every library it names is allowed.
fn imports(bytes: &[u8], table: &[u8], rva: u32) -> Result<(), Refusal> {
    let mut at = offset_of(table, rva)?;
    for _ in 0..MAX_IMPORTS {
        let entry = bytes.get(at..at + IMPORT).ok_or(Refusal::Truncated)?;
        if entry.iter().all(|&b| b == 0) {
            return Ok(());
        }
        let name = offset_of(table, u32_at(entry, 12)?)?;
        let rest = bytes.get(name..).ok_or(Refusal::Truncated)?;
        let name = &rest[..rest
            .iter()
            .position(|&c| c == 0)
            .ok_or(Refusal::Truncated)?];
        let allowed = ALLOWED_DLLS.iter().any(|d| d.eq_ignore_ascii_case(name))
            || name
                .get(..ALLOWED_PREFIX.len())
                .is_some_and(|p| p.eq_ignore_ascii_case(ALLOWED_PREFIX));
        if !allowed {
            return Err(Refusal::RunsCodeAtLoad(
                "a library other than the system's and the C runtime",
            ));
        }
        at += IMPORT;
    }
    Err(Refusal::DynamicTag)
}

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;

    use super::{Refusal, check_pe32plus};

    const AMD64: u16 = 0x8664;
    const ARM64: u16 = 0xaa64;
    /// Where the one section (`.rdata`, RVA 0x1000) lies in the file.
    const RAW: usize = 0x200;

    /// A plugin-shaped DLL: no entry point, one section, `directories`
    /// set to `(index, rva, size)`, importing `dlls` (if any).
    fn build(
        machine: u16,
        entry: u32,
        directories: &[(usize, u32, u32)],
        dlls: &[&str],
    ) -> Vec<u8> {
        let mut f = alloc::vec![0u8; RAW + 0x200];
        f[..2].copy_from_slice(b"MZ");
        f[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        f[0x80..0x84].copy_from_slice(b"PE\0\0");
        let coff = 0x84;
        f[coff..coff + 2].copy_from_slice(&machine.to_le_bytes());
        f[coff + 2..coff + 4].copy_from_slice(&1u16.to_le_bytes());
        f[coff + 16..coff + 18].copy_from_slice(&240u16.to_le_bytes());
        f[coff + 18..coff + 20].copy_from_slice(&0x2022u16.to_le_bytes());
        let opt = coff + 20;
        f[opt..opt + 2].copy_from_slice(&0x20bu16.to_le_bytes());
        f[opt + 16..opt + 20].copy_from_slice(&entry.to_le_bytes());
        f[opt + 108..opt + 112].copy_from_slice(&16u32.to_le_bytes());
        let mut dirs = directories.to_vec();
        if !dlls.is_empty() {
            // Descriptors at RVA 0x1000, names from RVA 0x1100.
            let mut name_rva = 0x1100u32;
            for (i, dll) in dlls.iter().enumerate() {
                let d = RAW + 20 * i;
                f[d + 12..d + 16].copy_from_slice(&name_rva.to_le_bytes());
                f[d + 16..d + 20].copy_from_slice(&0x1180u32.to_le_bytes());
                let n = RAW + (name_rva - 0x1000) as usize;
                f[n..n + dll.len()].copy_from_slice(dll.as_bytes());
                name_rva += 0x20;
            }
            dirs.push((1, 0x1000, 20 * (dlls.len() as u32 + 1)));
        }
        for (index, rva, size) in dirs {
            let d = opt + 112 + 8 * index;
            f[d..d + 4].copy_from_slice(&rva.to_le_bytes());
            f[d + 4..d + 8].copy_from_slice(&size.to_le_bytes());
        }
        let s = opt + 240;
        f[s..s + 6].copy_from_slice(b".rdata");
        f[s + 8..s + 12].copy_from_slice(&0x200u32.to_le_bytes());
        f[s + 12..s + 16].copy_from_slice(&0x1000u32.to_le_bytes());
        f[s + 16..s + 20].copy_from_slice(&0x200u32.to_le_bytes());
        f[s + 20..s + 24].copy_from_slice(&(RAW as u32).to_le_bytes());
        f
    }

    #[test]
    fn a_dll_without_entry_point_is_accepted_for_its_machine_only() {
        assert_eq!(check_pe32plus(&build(AMD64, 0, &[], &[]), AMD64), Ok(()));
        assert_eq!(
            check_pe32plus(&build(ARM64, 0, &[(0, 0x1000, 40)], &[]), ARM64),
            Ok(())
        );
        let system = build(
            AMD64,
            0,
            &[],
            &["KERNEL32.dll", "api-ms-win-crt-runtime-l1-1-0.dll"],
        );
        assert_eq!(check_pe32plus(&system, AMD64), Ok(()));
        assert_eq!(
            check_pe32plus(&build(ARM64, 0, &[], &[]), AMD64),
            Err(Refusal::OtherMachine)
        );
    }

    #[test]
    fn every_way_to_run_code_at_load_is_refused() {
        let at_load = |f: Vec<u8>, what| {
            assert_eq!(
                check_pe32plus(&f, AMD64),
                Err(Refusal::RunsCodeAtLoad(what))
            )
        };
        at_load(build(AMD64, 0x1000, &[], &[]), "an entry point");
        at_load(build(AMD64, 0, &[(9, 0x1000, 40)], &[]), "a TLS directory");
        at_load(
            build(AMD64, 0, &[(13, 0x1000, 64)], &[]),
            "delay-load imports",
        );
        at_load(build(AMD64, 0, &[(14, 0x1000, 72)], &[]), "a CLR header");
        at_load(
            build(AMD64, 0, &[], &["kernel32.dll", "user32.dll"]),
            "a library other than the system's and the C runtime",
        );
    }

    #[test]
    fn other_files_and_short_files_are_refused() {
        let mut exe = build(AMD64, 0, &[], &[]);
        exe[0x84 + 19] = 0; // no IMAGE_FILE_DLL
        assert_eq!(check_pe32plus(&exe, AMD64), Err(Refusal::NotSupported));
        let mut pe32 = build(AMD64, 0, &[], &[]);
        pe32[0x98] = 0x0b;
        pe32[0x99] = 0x01;
        assert_eq!(check_pe32plus(&pe32, AMD64), Err(Refusal::NotSupported));
        let global = build(AMD64, 0, &[(8, 0x1000, 8)], &[]);
        assert_eq!(check_pe32plus(&global, AMD64), Err(Refusal::DynamicTag));
        let whole = build(AMD64, 0, &[], &["kernel32.dll"]);
        for cut in [0, 0x40, 0x90, 0x150, whole.len() - 1] {
            assert!(
                check_pe32plus(&whole[..cut], AMD64).is_err(),
                "cut at {cut}"
            );
        }
    }
}
