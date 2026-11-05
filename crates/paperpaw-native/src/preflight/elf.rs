//! The ELF part of the preflight: little-endian 64-bit shared objects.

use super::Refusal;

const ET_DYN: u16 = 3;
const PT_LOAD: u32 = 1;
const PT_DYNAMIC: u32 = 2;
const PT_TLS: u32 = 7;
const PHENT: usize = 56;
const DYN_ENT: usize = 16;
const SYM_ENT: usize = 24;
const STT_GNU_IFUNC: u8 = 10;

const DT_NULL: i64 = 0;
const DT_NEEDED: i64 = 1;
const DT_PLTRELSZ: i64 = 2;
const DT_HASH: i64 = 4;
const DT_STRTAB: i64 = 5;
const DT_SYMTAB: i64 = 6;
const DT_RELA: i64 = 7;
const DT_RELASZ: i64 = 8;
const DT_SYMENT: i64 = 11;
const DT_PLTREL: i64 = 20;
const DT_INIT: i64 = 12;
const DT_FINI: i64 = 13;
const DT_RPATH: i64 = 15;
const DT_TEXTREL: i64 = 22;
const DT_JMPREL: i64 = 23;
const DT_INIT_ARRAYSZ: i64 = 27;
const DT_FINI_ARRAYSZ: i64 = 28;
const DT_RUNPATH: i64 = 29;
const DT_FLAGS: i64 = 30;
const DT_PREINIT_ARRAYSZ: i64 = 33;
const DT_GNU_HASH: i64 = 0x6fff_fef5;
const DT_AUXILIARY: i64 = 0x7fff_fffd;
const DT_FILTER: i64 = 0x7fff_ffff;
const DF_TEXTREL: u64 = 0x4;
const DF_STATIC_TLS: u64 = 0x10;
/// The relocation kinds that call a resolver while loading.
const R_X86_64_IRELATIVE: u32 = 37;
const R_AARCH64_IRELATIVE: u32 = 1032;
/// The relocation kinds a plugin may have: absolute 64-bit, global data,
/// jump slot and base-relative. Any other kind is refused, not guessed at.
const X86_64_KINDS: [u32; 4] = [1, 6, 7, 8];
const AARCH64_KINDS: [u32; 4] = [257, 1025, 1026, 1027];
const RELA_ENT: u64 = 24;
const PF_W: u32 = 2;
const DT_RELAENT: i64 = 9;
const DT_REL: i64 = 17;
const DT_RELRSZ: i64 = 35;
const DT_RELR: i64 = 36;
/// Machines a host plugin may be built for.
const EM_X86_64: u16 = 62;
const EM_AARCH64: u16 = 183;

/// The libraries a host plugin may name: the C library and nothing else.
const ALLOWED_NEEDED: [&[u8]; 6] = [
    b"libc.so.6",
    b"libm.so.6",
    b"libdl.so.2",
    b"libc.so",
    b"libm.so",
    b"libdl.so",
];

fn u16_at(b: &[u8], at: usize) -> Result<u16, Refusal> {
    let s = b.get(at..at.checked_add(2).ok_or(Refusal::Truncated)?);
    s.map(|s| u16::from_le_bytes([s[0], s[1]]))
        .ok_or(Refusal::Truncated)
}

fn u32_at(b: &[u8], at: usize) -> Result<u32, Refusal> {
    let s = b.get(at..at.checked_add(4).ok_or(Refusal::Truncated)?);
    s.map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or(Refusal::Truncated)
}

fn u64_at(b: &[u8], at: usize) -> Result<u64, Refusal> {
    let s = b.get(at..at.checked_add(8).ok_or(Refusal::Truncated)?);
    s.map(|s| u64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]))
        .ok_or(Refusal::Truncated)
}

fn to_usize(v: u64) -> Result<usize, Refusal> {
    usize::try_from(v).map_err(|_| Refusal::Truncated)
}

/// One loadable segment: where it lies in memory and in the file.
#[derive(Clone, Copy)]
struct Load {
    flags: u32,
    vaddr: u64,
    offset: u64,
    filesz: u64,
    memsz: u64,
}

/// The file offset of virtual address `vaddr`, if a loadable segment
/// holds it (and the `len` bytes from there).
fn offset_of(loads: &[Option<Load>], vaddr: u64, len: u64) -> Result<usize, Refusal> {
    for l in loads.iter().flatten() {
        let end = l.vaddr.checked_add(l.filesz).ok_or(Refusal::Truncated)?;
        let want = vaddr.checked_add(len).ok_or(Refusal::Truncated)?;
        if vaddr >= l.vaddr && want <= end {
            return to_usize(l.offset + (vaddr - l.vaddr));
        }
    }
    Err(Refusal::Truncated)
}

/// At most this many loadable segments are read; more is refused.
const MAX_LOADS: usize = 16;

/// Refuses loadable segments whose address ranges overlap.
fn check_no_overlap(loads: &[Option<Load>]) -> Result<(), Refusal> {
    for (i, a) in loads.iter().flatten().enumerate() {
        let a_end = a.vaddr.checked_add(a.memsz).ok_or(Refusal::Truncated)?;
        for b in loads.iter().flatten().skip(i + 1) {
            let b_end = b.vaddr.checked_add(b.memsz).ok_or(Refusal::Truncated)?;
            if a.vaddr < b_end && b.vaddr < a_end && a.memsz != 0 && b.memsz != 0 {
                return Err(Refusal::NotSupported);
            }
        }
    }
    Ok(())
}

/// Checks `bytes`, a little-endian 64-bit ELF shared object built for
/// `machine` (`EM_X86_64` 62 or `EM_AARCH64` 183), for every way to run
/// code or pull in more code while it loads: `DT_INIT`, `DT_INIT_ARRAY`,
/// `DT_PREINIT_ARRAY`, `DT_FINI*`, thread-local storage, `IFUNC` symbols
/// and relocations, text relocations, a search path, auxiliary and filter
/// libraries, and any needed library outside the C library.
///
/// # Errors
///
/// [`Refusal`] names the first problem; an unknown structure is refused,
/// never skipped.
pub fn check_elf64_le(bytes: &[u8], machine: u16) -> Result<(), Refusal> {
    if bytes.get(..4) != Some(b"\x7fELF") || bytes.get(4) != Some(&2) || bytes.get(5) != Some(&1) {
        return Err(Refusal::NotSupported);
    }
    if u16_at(bytes, 16)? != ET_DYN {
        return Err(Refusal::NotSupported);
    }
    let found = u16_at(bytes, 18)?;
    if machine != EM_X86_64 && machine != EM_AARCH64 {
        return Err(Refusal::NotSupported);
    }
    if found != machine {
        return Err(Refusal::OtherMachine);
    }
    let phoff = to_usize(u64_at(bytes, 32)?)?;
    if usize::from(u16_at(bytes, 54)?) != PHENT {
        return Err(Refusal::NotSupported);
    }
    let phnum = usize::from(u16_at(bytes, 56)?);
    let mut loads = [None; MAX_LOADS];
    let mut n_loads = 0;
    let mut dynamic: Option<(u64, u64, u64)> = None;
    for i in 0..phnum {
        let at = phoff.checked_add(i * PHENT).ok_or(Refusal::Truncated)?;
        let kind = u32_at(bytes, at)?;
        let flags = u32_at(bytes, at + 4)?;
        let offset = u64_at(bytes, at + 8)?;
        let vaddr = u64_at(bytes, at + 16)?;
        let filesz = u64_at(bytes, at + 32)?;
        let memsz = u64_at(bytes, at + 40)?;
        match kind {
            PT_LOAD => {
                if memsz < filesz {
                    return Err(Refusal::NotSupported);
                }
                let slot = loads.get_mut(n_loads).ok_or(Refusal::NotSupported)?;
                *slot = Some(Load {
                    flags,
                    vaddr,
                    offset,
                    filesz,
                    memsz,
                });
                n_loads += 1;
                let end = offset.checked_add(filesz).ok_or(Refusal::Truncated)?;
                if to_usize(end)? > bytes.len() {
                    return Err(Refusal::Truncated);
                }
            }
            PT_DYNAMIC => {
                if dynamic.is_some() {
                    return Err(Refusal::NotSupported);
                }
                dynamic = Some((offset, vaddr, filesz));
            }
            PT_TLS => return Err(Refusal::RunsCodeAtLoad("thread-local storage")),
            _ => {}
        }
    }
    // Two segments that share addresses would let the file say one thing
    // here and the system loader map another.
    check_no_overlap(&loads[..n_loads])?;
    // The dynamic section is found as the system loader finds it, through the
    // address mapping; the program header's own file offset must agree.
    let (dyn_offset, dyn_vaddr, dyn_filesz) = dynamic.ok_or(Refusal::NotSupported)?;
    let dyn_at = offset_of(&loads, dyn_vaddr, dyn_filesz)?;
    if dyn_at != to_usize(dyn_offset)? {
        return Err(Refusal::NotSupported);
    }
    let dyn_len = to_usize(dyn_filesz)?;
    let dyn_end = dyn_at.checked_add(dyn_len).ok_or(Refusal::Truncated)?;
    if dyn_end > bytes.len() {
        return Err(Refusal::Truncated);
    }
    let d = read_dynamic(bytes, dyn_at, dyn_end)?;
    // Needed libraries: the names lie in the string table.
    if d.needed_count > 0 || d.strtab.is_some() {
        check_needed(bytes, &loads, dyn_at, dyn_end, &d)?;
    }
    // The jump-slot table is read as `Rela` entries only where it says so.
    if d.jmprel.is_some() && d.pltrelsz != 0 && d.pltrel != Some(DT_RELA as u64) {
        return Err(Refusal::NotSupported);
    }
    let count = if d.symtab.is_some() {
        symbol_count(bytes, &loads, &d)?
    } else {
        0
    };
    let tables = Tables {
        loads: &loads[..n_loads],
        machine,
        symbols: count,
    };
    check_relocations(bytes, &tables, d.rela, d.relasz)?;
    check_relocations(bytes, &tables, d.jmprel, d.pltrelsz)?;
    check_symbols(bytes, &loads, &d, count)
}

/// What a relocation is checked against.
struct Tables<'a> {
    loads: &'a [Option<Load>],
    machine: u16,
    symbols: usize,
}

impl Tables<'_> {
    /// Whether the 8 bytes at `vaddr` lie in a segment that is mapped
    /// writable (the system loader writes there; elsewhere it faults).
    fn writable(&self, vaddr: u64) -> bool {
        let Some(end) = vaddr.checked_add(8) else {
            return false;
        };
        self.loads.iter().flatten().any(|l| {
            l.flags & PF_W != 0
                && vaddr >= l.vaddr
                && l.vaddr.checked_add(l.memsz).is_some_and(|e| end <= e)
        })
    }
}

/// Reads the dynamic section between `dyn_at` and `dyn_end`: every entry is
/// taken or refused, and the section must end with `DT_NULL`.
fn read_dynamic(bytes: &[u8], dyn_at: usize, dyn_end: usize) -> Result<Dyn, Refusal> {
    let mut d = Dyn::default();
    let mut ended = false;
    let mut at = dyn_at;
    while at + DYN_ENT <= dyn_end {
        let tag = u64_at(bytes, at)? as i64;
        let val = u64_at(bytes, at + 8)?;
        at += DYN_ENT;
        if tag == DT_NULL {
            ended = true;
            break;
        }
        d.take(tag, val)?;
    }
    if !ended {
        return Err(Refusal::Truncated);
    }
    d.check_arrays()?;
    Ok(d)
}

/// What the dynamic section says that matters.
#[derive(Default)]
struct Dyn {
    strtab: Option<u64>,
    symtab: Option<u64>,
    syment: Option<u64>,
    hash: Option<u64>,
    gnu_hash: Option<u64>,
    rela: Option<u64>,
    relasz: u64,
    jmprel: Option<u64>,
    pltrelsz: u64,
    pltrel: Option<u64>,
    init_arraysz: u64,
    fini_arraysz: u64,
    preinit_arraysz: u64,
    needed_count: usize,
}

impl Dyn {
    /// Records one entry, or refuses it.
    fn take(&mut self, tag: i64, val: u64) -> Result<(), Refusal> {
        match tag {
            DT_INIT => return Err(Refusal::RunsCodeAtLoad("DT_INIT")),
            DT_FINI => return Err(Refusal::RunsCodeAtLoad("DT_FINI")),
            DT_TEXTREL => return Err(Refusal::RunsCodeAtLoad("text relocations")),
            DT_RPATH | DT_RUNPATH => return Err(Refusal::RunsCodeAtLoad("a library search path")),
            DT_AUXILIARY | DT_FILTER => {
                return Err(Refusal::RunsCodeAtLoad("an auxiliary or filter library"));
            }
            DT_FLAGS if val & DF_TEXTREL != 0 => {
                return Err(Refusal::RunsCodeAtLoad("text relocations"));
            }
            DT_FLAGS if val & DF_STATIC_TLS != 0 => {
                return Err(Refusal::RunsCodeAtLoad("static thread-local storage"));
            }
            DT_NEEDED => self.needed_count += 1,
            DT_STRTAB => self.strtab = Some(val),
            DT_SYMTAB => self.symtab = Some(val),
            DT_SYMENT => self.syment = Some(val),
            DT_HASH => self.hash = Some(val),
            DT_GNU_HASH => self.gnu_hash = Some(val),
            DT_RELA => self.rela = Some(val),
            DT_RELASZ => self.relasz = val,
            DT_RELAENT if val != RELA_ENT => return Err(Refusal::NotSupported),
            // Tables this check does not read are refused, not skipped.
            DT_REL | DT_RELR | DT_RELRSZ => return Err(Refusal::NotSupported),
            DT_JMPREL => self.jmprel = Some(val),
            DT_PLTRELSZ => self.pltrelsz = val,
            DT_PLTREL => self.pltrel = Some(val),
            DT_INIT_ARRAYSZ => self.init_arraysz = val,
            DT_FINI_ARRAYSZ => self.fini_arraysz = val,
            DT_PREINIT_ARRAYSZ => self.preinit_arraysz = val,
            _ => {}
        }
        Ok(())
    }

    /// An initialiser, finaliser or pre-initialiser array that has a
    /// size is refused: its entries run while the library loads or unloads.
    fn check_arrays(&self) -> Result<(), Refusal> {
        for (size, what) in [
            (self.init_arraysz, "DT_INIT_ARRAY"),
            (self.fini_arraysz, "DT_FINI_ARRAY"),
            (self.preinit_arraysz, "DT_PREINIT_ARRAY"),
        ] {
            if size != 0 {
                return Err(Refusal::RunsCodeAtLoad(what));
            }
        }
        Ok(())
    }
}

fn check_needed(
    bytes: &[u8],
    loads: &[Option<Load>],
    dyn_at: usize,
    dyn_end: usize,
    d: &Dyn,
) -> Result<(), Refusal> {
    let strtab = d.strtab.ok_or(Refusal::Truncated)?;
    let base = offset_of(loads, strtab, 1)?;
    let mut at = dyn_at;
    while at + DYN_ENT <= dyn_end {
        let tag = u64_at(bytes, at)? as i64;
        let val = u64_at(bytes, at + 8)?;
        at += DYN_ENT;
        if tag == DT_NULL {
            break;
        }
        if tag != DT_NEEDED {
            continue;
        }
        let start = base.checked_add(to_usize(val)?).ok_or(Refusal::Truncated)?;
        let rest = bytes.get(start..).ok_or(Refusal::Truncated)?;
        let end = rest
            .iter()
            .position(|&c| c == 0)
            .ok_or(Refusal::Truncated)?;
        let name = &rest[..end];
        if !ALLOWED_NEEDED.contains(&name) {
            return Err(Refusal::RunsCodeAtLoad(
                "a library other than the C library",
            ));
        }
    }
    Ok(())
}

/// Reads the `Rela` table at `table`: refuses a call to a resolver, a kind
/// not allowed, a destination the system loader could not write, and a
/// symbol that is not in the symbol table. The system loader does none of
/// these checks: it faults.
fn check_relocations(
    bytes: &[u8],
    t: &Tables<'_>,
    table: Option<u64>,
    size: u64,
) -> Result<(), Refusal> {
    let Some(table) = table else { return Ok(()) };
    if size == 0 {
        return Ok(());
    }
    if !size.is_multiple_of(RELA_ENT) {
        return Err(Refusal::NotSupported);
    }
    let at = offset_of(t.loads, table, size)?;
    let (irelative, kinds) = if t.machine == EM_X86_64 {
        (R_X86_64_IRELATIVE, &X86_64_KINDS)
    } else {
        (R_AARCH64_IRELATIVE, &AARCH64_KINDS)
    };
    let count = to_usize(size / RELA_ENT)?;
    for i in 0..count {
        let entry = at + i * 24;
        let place = u64_at(bytes, entry)?;
        let info = u64_at(bytes, entry + 8)?;
        let kind = (info & 0xffff_ffff) as u32;
        let sym = to_usize(info >> 32)?;
        if kind == irelative {
            return Err(Refusal::RunsCodeAtLoad("an IFUNC relocation"));
        }
        if !kinds.contains(&kind) || !t.writable(place) {
            return Err(Refusal::NotSupported);
        }
        if sym != 0 && sym >= t.symbols {
            return Err(Refusal::NotSupported);
        }
    }
    Ok(())
}

/// The symbol count the SysV hash table (`DT_HASH`) states.
fn sysv_count(bytes: &[u8], loads: &[Option<Load>], hash: u64) -> Result<usize, Refusal> {
    let at = offset_of(loads, hash, 8)?;
    to_usize(u64::from(u32_at(
        bytes,
        at.checked_add(4).ok_or(Refusal::Truncated)?,
    )?))
}

/// The symbol count the GNU hash table (`DT_GNU_HASH`) implies: the end of
/// the chain of its highest bucket.
fn gnu_count(bytes: &[u8], loads: &[Option<Load>], gnu: u64) -> Result<usize, Refusal> {
    let at = offset_of(loads, gnu, 16)?;
    let word = |i: usize| -> Result<usize, Refusal> {
        to_usize(u64::from(u32_at(
            bytes,
            at.checked_add(i).ok_or(Refusal::Truncated)?,
        )?))
    };
    let nbuckets = word(0)?;
    let symoffset = word(4)?;
    let bloom = word(8)?;
    let buckets = at
        .checked_add(16)
        .and_then(|x| x.checked_add(bloom.checked_mul(8)?))
        .ok_or(Refusal::Truncated)?;
    let chains = nbuckets
        .checked_mul(4)
        .and_then(|x| buckets.checked_add(x))
        .ok_or(Refusal::Truncated)?;
    let mut last = 0usize;
    for i in 0..nbuckets {
        let at = buckets.checked_add(i * 4).ok_or(Refusal::Truncated)?;
        last = last.max(to_usize(u64::from(u32_at(bytes, at)?))?);
    }
    if last < symoffset {
        return Ok(symoffset);
    }
    // Walk the chain of the highest bucket to its end.
    let mut sym = last;
    loop {
        let at = (sym - symoffset)
            .checked_mul(4)
            .and_then(|x| chains.checked_add(x))
            .ok_or(Refusal::Truncated)?;
        let h = u32_at(bytes, at)?;
        sym += 1;
        if h & 1 == 1 {
            return Ok(sym);
        }
        if sym - symoffset > bytes.len() / 4 {
            return Err(Refusal::Truncated);
        }
    }
}

/// How many symbols the dynamic symbol table has, from the hash tables. The
/// system loader prefers `DT_GNU_HASH` and falls back to `DT_HASH`; a file
/// that has both must state the same count in both, or it is refused (a
/// count that differs would let a symbol hide where this check does not look).
fn symbol_count(bytes: &[u8], loads: &[Option<Load>], d: &Dyn) -> Result<usize, Refusal> {
    let sysv = d.hash.map(|h| sysv_count(bytes, loads, h)).transpose()?;
    let gnu = d.gnu_hash.map(|g| gnu_count(bytes, loads, g)).transpose()?;
    match (sysv, gnu) {
        (Some(a), Some(b)) if a != b => Err(Refusal::NotSupported),
        (Some(n), _) | (_, Some(n)) => Ok(n),
        (None, None) => Err(Refusal::NotSupported),
    }
}

fn check_symbols(
    bytes: &[u8],
    loads: &[Option<Load>],
    d: &Dyn,
    count: usize,
) -> Result<(), Refusal> {
    let Some(symtab) = d.symtab else {
        return Ok(());
    };
    if d.syment.is_some_and(|e| e != SYM_ENT as u64) {
        return Err(Refusal::NotSupported);
    }
    let total = u64::try_from(count.checked_mul(SYM_ENT).ok_or(Refusal::Truncated)?)
        .map_err(|_| Refusal::Truncated)?;
    let at = offset_of(loads, symtab, total)?;
    for i in 0..count {
        let info = *bytes.get(at + i * SYM_ENT + 4).ok_or(Refusal::Truncated)?;
        if info & 0xf == STT_GNU_IFUNC {
            return Err(Refusal::RunsCodeAtLoad("an IFUNC symbol"));
        }
    }
    Ok(())
}
