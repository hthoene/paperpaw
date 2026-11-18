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
/// The section index of an undefined (imported) symbol.
#[cfg(feature = "kernel")]
const SHN_UNDEF: u16 = 0;

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
const DT_STRSZ: i64 = 10;
const DT_PLTGOT: i64 = 3;
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
const PF_X: u32 = 1;
const PT_GNU_STACK: u32 = 0x6474_e551;
/// More distinct dynamic tags than this are refused (the list is shorter).
const MAX_TAGS: usize = 48;
const DT_RELAENT: i64 = 9;
const DT_REL: i64 = 17;
const DT_RELRSZ: i64 = 35;
const DT_RELR: i64 = 36;
const DT_RELACOUNT: i64 = 0x6fff_fff9;
const DT_RELCOUNT: i64 = 0x6fff_fffa;
/// Tags that are plain flags: they hold no address and name no table.
/// Every dynamic tag not read by [`Dyn::take`] and not listed here is
/// refused (default deny), including the symbol-version tags: a plugin
/// reaches the host through its table, not through versioned C symbols.
const FLAG_TAGS: [i64; 9] = [
    16,          // DT_SYMBOLIC
    24,          // DT_BIND_NOW
    25,          // DT_INIT_ARRAY (its size, DT_INIT_ARRAYSZ, is checked)
    26,          // DT_FINI_ARRAY
    32,          // DT_PREINIT_ARRAY
    0x6fff_fffb, // DT_FLAGS_1
    0x7000_0001, // DT_AARCH64_BTI_PLT
    0x7000_0003, // DT_AARCH64_PAC_PLT
    0x7000_0005, // DT_AARCH64_VARIANT_PCS
];
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
                return Err(Refusal::Segments);
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
    checked(bytes, machine).map(|_| ())
}

fn checked(bytes: &[u8], machine: u16) -> Result<([Option<Load>; MAX_LOADS], Dyn), Refusal> {
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
    let mut stack = false;
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
                    return Err(Refusal::Segments);
                }
                let slot = loads.get_mut(n_loads).ok_or(Refusal::Segments)?;
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
                    return Err(Refusal::DynamicSection);
                }
                dynamic = Some((offset, vaddr, filesz));
            }
            PT_TLS => return Err(Refusal::RunsCodeAtLoad("thread-local storage")),
            // An executable stack makes the system loader change the stack's
            // protection; a file that says nothing gets the platform's default.
            PT_GNU_STACK if flags & PF_X != 0 => return Err(Refusal::Segments),
            PT_GNU_STACK => stack = true,
            _ => {}
        }
    }
    // Two segments that share addresses would let the file say one thing
    // here and the system loader map another.
    if !stack {
        return Err(Refusal::Segments);
    }
    check_no_overlap(&loads[..n_loads])?;
    // The dynamic section is found as the system loader finds it, through the
    // address mapping; the program header's own file offset must agree.
    let (dyn_offset, dyn_vaddr, dyn_filesz) = dynamic.ok_or(Refusal::DynamicSection)?;
    let dyn_at = offset_of(&loads, dyn_vaddr, dyn_filesz)?;
    if dyn_at != to_usize(dyn_offset)? {
        return Err(Refusal::DynamicSection);
    }
    // The system loader adds the load address to the section's entries in place.
    if !writable_in(&loads, dyn_vaddr, dyn_filesz) {
        return Err(Refusal::DynamicSection);
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
    check_tables(bytes, &loads[..n_loads], machine, &d)?;
    Ok((loads, d))
}

/// Checks the relocation, jump-slot and symbol tables the dynamic section names.
fn check_tables(
    bytes: &[u8],
    loads: &[Option<Load>],
    machine: u16,
    d: &Dyn,
) -> Result<(), Refusal> {
    // The jump-slot table is read as `Rela` entries only where it says so.
    if d.jmprel.is_some() && d.pltrelsz != 0 && d.pltrel != Some(DT_RELA as u64) {
        return Err(Refusal::Relocation);
    }
    let count = if d.symtab.is_some() {
        symbol_count(bytes, loads, d)?
    } else {
        0
    };
    let tables = Tables {
        loads,
        machine,
        symbols: count,
    };
    // The jump-slot table's first three words are written by the loader, so a
    // file with jump slots names where they are.
    if d.jmprel.is_some() && d.pltrelsz != 0 && d.pltgot.is_none() {
        return Err(Refusal::Relocation);
    }
    if let Some(got) = d.pltgot
        && !(tables.writable(got) && tables.writable(got.saturating_add(16)))
    {
        return Err(Refusal::Relocation);
    }
    check_relocations(bytes, &tables, d.rela, d.relasz, d.relacount)?;
    check_relocations(bytes, &tables, d.jmprel, d.pltrelsz, 0)?;
    check_symbols(bytes, loads, d, count)
}

/// Whether the `len` bytes at `vaddr` lie in one segment that is mapped
/// writable (the system loader writes there; elsewhere it faults).
fn writable_in(loads: &[Option<Load>], vaddr: u64, len: u64) -> bool {
    let Some(end) = vaddr.checked_add(len) else {
        return false;
    };
    loads.iter().flatten().any(|l| {
        l.flags & PF_W != 0
            && vaddr >= l.vaddr
            && l.vaddr.checked_add(l.memsz).is_some_and(|e| end <= e)
    })
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
        writable_in(self.loads, vaddr, 8)
    }
}

/// Reads the dynamic section between `dyn_at` and `dyn_end`: every entry is
/// taken or refused, and the section must end with `DT_NULL`.
fn read_dynamic(bytes: &[u8], dyn_at: usize, dyn_end: usize) -> Result<Dyn, Refusal> {
    let mut d = Dyn::default();
    let (mut seen, mut n_seen) = ([0i64; MAX_TAGS], 0);
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
        // A tag twice would be read one way here and another by the loader.
        if tag != DT_NEEDED {
            if seen[..n_seen].contains(&tag) {
                return Err(Refusal::DynamicTag);
            }
            *seen.get_mut(n_seen).ok_or(Refusal::DynamicTag)? = tag;
            n_seen += 1;
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
    strsz: u64,
    pltgot: Option<u64>,
    symtab: Option<u64>,
    syment: Option<u64>,
    hash: Option<u64>,
    gnu_hash: Option<u64>,
    rela: Option<u64>,
    relasz: u64,
    relacount: u64,
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
            DT_RELAENT if val != RELA_ENT => return Err(Refusal::DynamicTag),
            DT_RELAENT | DT_FLAGS => {}
            DT_STRSZ => self.strsz = val,
            DT_PLTGOT => self.pltgot = Some(val),
            // Tables this check does not read are refused, not skipped.
            DT_REL | DT_RELCOUNT | DT_RELR | DT_RELRSZ => return Err(Refusal::DynamicTag),
            DT_RELACOUNT => self.relacount = val,
            DT_JMPREL => self.jmprel = Some(val),
            DT_PLTRELSZ => self.pltrelsz = val,
            DT_PLTREL => self.pltrel = Some(val),
            DT_INIT_ARRAYSZ => self.init_arraysz = val,
            DT_FINI_ARRAYSZ => self.fini_arraysz = val,
            DT_PREINIT_ARRAYSZ => self.preinit_arraysz = val,
            t if FLAG_TAGS.contains(&t) => {}
            _ => return Err(Refusal::DynamicTag),
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
    // The string table lies in the file, is not empty and ends with a NUL, so
    // no name the system loader reads runs past it.
    let size = to_usize(d.strsz)?;
    let base = offset_of(loads, strtab, d.strsz)?;
    let last = base.checked_add(size).ok_or(Refusal::Truncated)?;
    if size == 0 || bytes.get(last - 1) != Some(&0) {
        return Err(Refusal::Strings);
    }
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
        if to_usize(val)? >= size {
            return Err(Refusal::Strings);
        }
        let start = base.checked_add(to_usize(val)?).ok_or(Refusal::Truncated)?;
        let rest = bytes.get(start..last).ok_or(Refusal::Truncated)?;
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
    relative_prefix: u64,
) -> Result<(), Refusal> {
    let Some(table) = table else {
        return if relative_prefix == 0 {
            Ok(())
        } else {
            Err(Refusal::Relocation)
        };
    };
    if size == 0 {
        return if relative_prefix == 0 {
            Ok(())
        } else {
            Err(Refusal::Relocation)
        };
    }
    if !size.is_multiple_of(RELA_ENT) {
        return Err(Refusal::Relocation);
    }
    let at = offset_of(t.loads, table, size)?;
    let (irelative, kinds) = if t.machine == EM_X86_64 {
        (R_X86_64_IRELATIVE, &X86_64_KINDS)
    } else {
        (R_AARCH64_IRELATIVE, &AARCH64_KINDS)
    };
    let relative = kinds[3];
    let count = to_usize(size / RELA_ENT)?;
    // The system loader trusts `DT_RELACOUNT`: it applies that many entries
    // as base-relative ones without looking at them.
    if relative_prefix > size / RELA_ENT {
        return Err(Refusal::Relocation);
    }
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
            return Err(Refusal::Relocation);
        }
        if (i as u64) < relative_prefix && (kind != relative || sym != 0) {
            return Err(Refusal::Relocation);
        }
        if sym != 0 && sym >= t.symbols {
            return Err(Refusal::Relocation);
        }
    }
    Ok(())
}

/// One little-endian word at virtual address `vaddr`, found through the
/// segment map like every other table (never as a raw file offset).
fn word_at(bytes: &[u8], loads: &[Option<Load>], vaddr: u64) -> Result<u32, Refusal> {
    u32_at(bytes, offset_of(loads, vaddr, 4)?)
}

/// The symbol count the SysV hash table (`DT_HASH`) states. The whole table
/// (header, buckets and chains) must lie in a loadable segment.
fn sysv_count(bytes: &[u8], loads: &[Option<Load>], hash: u64) -> Result<usize, Refusal> {
    let buckets = u64::from(word_at(bytes, loads, hash)?);
    let chains = u64::from(word_at(
        bytes,
        loads,
        hash.checked_add(4).ok_or(Refusal::Truncated)?,
    )?);
    let size = buckets
        .checked_add(chains)
        .and_then(|n| n.checked_mul(4))
        .and_then(|n| n.checked_add(8))
        .ok_or(Refusal::Truncated)?;
    offset_of(loads, hash, size)?;
    to_usize(chains)
}

/// The symbol count the GNU hash table (`DT_GNU_HASH`) implies: the end of
/// the chain of its highest bucket. The header, the bloom words and the
/// buckets must lie in a loadable segment, and so must each chain word read.
fn gnu_count(bytes: &[u8], loads: &[Option<Load>], gnu: u64) -> Result<usize, Refusal> {
    let word = |i: u64| word_at(bytes, loads, gnu.checked_add(i).ok_or(Refusal::Truncated)?);
    let nbuckets = u64::from(word(0)?);
    let symoffset = u64::from(word(4)?);
    let bloom = u64::from(word(8)?);
    let buckets = bloom
        .checked_mul(8)
        .and_then(|x| x.checked_add(16))
        .ok_or(Refusal::Truncated)?;
    let chains = nbuckets
        .checked_mul(4)
        .and_then(|x| x.checked_add(buckets))
        .ok_or(Refusal::Truncated)?;
    offset_of(loads, gnu, chains)?;
    let mut last = 0u64;
    for i in 0..nbuckets {
        last = last.max(u64::from(word(buckets + i * 4)?));
    }
    if last < symoffset {
        return to_usize(symoffset);
    }
    // Walk the chain of the highest bucket to its end.
    let mut sym = last;
    loop {
        let h = word(chains + (sym - symoffset) * 4)?;
        sym += 1;
        if h & 1 == 1 {
            return to_usize(sym);
        }
        if sym - symoffset > (bytes.len() / 4) as u64 {
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
        (Some(a), Some(b)) if a != b => Err(Refusal::Symbols),
        (Some(n), _) | (_, Some(n)) => Ok(n),
        (None, None) => Err(Refusal::Symbols),
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
        return Err(Refusal::Symbols);
    }
    let total = u64::try_from(count.checked_mul(SYM_ENT).ok_or(Refusal::Truncated)?)
        .map_err(|_| Refusal::Truncated)?;
    let at = offset_of(loads, symtab, total)?;
    // Names are read from the string table, which was checked above.
    if d.strtab.is_none() {
        return Err(Refusal::Strings);
    }
    for i in 0..count {
        if to_usize(u64::from(u32_at(bytes, at + i * SYM_ENT)?))? >= to_usize(d.strsz)? {
            return Err(Refusal::Strings);
        }
        let info = *bytes.get(at + i * SYM_ENT + 4).ok_or(Refusal::Truncated)?;
        if info & 0xf == STT_GNU_IFUNC {
            return Err(Refusal::RunsCodeAtLoad("an IFUNC symbol"));
        }
    }
    Ok(())
}

/// Why [`check_freestanding`] refuses a library.
#[cfg(feature = "kernel")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FreestandingRefusal {
    /// The full ELF preflight ([`check_elf64_le`]) refused it.
    Preflight(Refusal),
    /// The library names another library it needs (`DT_NEEDED`); a
    /// freestanding plugin needs none.
    NeededLibrary,
    /// The library names a symbol it does not define (an import, strong or
    /// weak).
    UndefinedImport,
    /// The library defines a dynamic symbol that is not one of the entry
    /// symbols its caller allows.
    UnexpectedExport,
}

/// The rule for an interface-7 (freestanding) host plugin, in one place for
/// every caller (the loader, and the packager's check) so that they cannot
/// disagree. It runs the full [`check_elf64_le`] preflight and then refuses,
/// from the bytes alone and before the file is loaded:
///
/// 1. **Any `DT_NEEDED`** ([`FreestandingRefusal::NeededLibrary`]). A plugin reaches the
///    host only through its table (PR #133, ADR 0034 section 6), so it needs
///    no library, not even the C library the interface-6 path tolerates.
/// 2. **Any undefined dynamic symbol** ([`FreestandingRefusal::UndefinedImport`]): every
///    symbol after the null symbol whose section index is `SHN_UNDEF`,
///    whatever its binding. *Weak* undefined symbols are refused too: a weak
///    import binds to the process's own definition when there is one, so it
///    is the same hole as a strong import, and a freestanding library has no
///    reason to name a symbol it does not define. There is no exception.
/// 3. **Any defined dynamic symbol not named in `exports`**
///    ([`FreestandingRefusal::UnexpectedExport`]): the library exports the entry symbols
///    and nothing else, so it cannot interpose on or add to what the process
///    sees. An empty `exports` allows none. Compiler-runtime helpers the
///    library needs (`memcpy`, `rust_eh_personality`) must be defined with
///    hidden visibility so they never reach the dynamic symbol table.
///
/// # Link recipe for authors
///
/// The linker may add dynamic symbols a source file never named (`_end`,
/// `__bss_start`, `_edata`, `_init`, `_fini`, local or section symbols with
/// an empty name). They are refused as [`FreestandingRefusal::UnexpectedExport`]
/// like any other. Keep them out of the dynamic symbol table by building with
/// a version script that makes everything but the entry symbols local (for
/// example `-Wl,--version-script=exports.map` with
/// `{ global: PAPERPAW_ENTRY; PAPERPAW_METADATA; local: *; };`), with
/// `-Wl,--exclude-libs,ALL` for symbols from static libraries, and by
/// defining runtime helpers with `.hidden` visibility (for instance
/// `global_asm!(".globl memcpy", ".hidden memcpy", ".set memcpy, {f}", f = sym copy)`).
/// `nm -D --defined-only` must list exactly the two entry symbols and
/// `nm -D --undefined-only` nothing; `readelf -d` shows no `NEEDED`.
///
/// `check_elf64_le` itself is unchanged: the interface-6 path keeps accepting
/// the C library it was built on.
///
/// # Errors
///
/// The [`FreestandingRefusal`] named above; the full preflight's refusals are
/// passed on as [`FreestandingRefusal::Preflight`]. All
/// reads are bounded by the checked tables; a malformed symbol table or name
/// is refused, never followed out of the file.
#[cfg(feature = "kernel")]
pub fn check_freestanding(
    bytes: &[u8],
    machine: u16,
    exports: &[&str],
) -> Result<(), FreestandingRefusal> {
    freestanding(bytes, machine, exports).map_err(|refusal| match refusal {
        Inner::Preflight(refusal) => FreestandingRefusal::Preflight(refusal),
        Inner::Needed => FreestandingRefusal::NeededLibrary,
        Inner::Import => FreestandingRefusal::UndefinedImport,
        Inner::Export => FreestandingRefusal::UnexpectedExport,
    })
}

#[cfg(feature = "kernel")]
enum Inner {
    Preflight(Refusal),
    Needed,
    Import,
    Export,
}

#[cfg(feature = "kernel")]
impl From<Refusal> for Inner {
    fn from(refusal: Refusal) -> Self {
        Self::Preflight(refusal)
    }
}

#[cfg(feature = "kernel")]
fn freestanding(bytes: &[u8], machine: u16, exports: &[&str]) -> Result<(), Inner> {
    let (loads, d) = checked(bytes, machine)?;
    if d.needed_count > 0 {
        return Err(Inner::Needed);
    }
    let Some(symtab) = d.symtab else {
        return Ok(());
    };
    let count = symbol_count(bytes, &loads, &d)?;
    let total = u64::try_from(count.checked_mul(SYM_ENT).ok_or(Refusal::Truncated)?)
        .map_err(|_| Refusal::Truncated)?;
    let at = offset_of(&loads, symtab, total)?;
    let strings = offset_of(&loads, d.strtab.ok_or(Refusal::Strings)?, d.strsz)?;
    let strings = bytes
        .get(
            strings
                ..strings
                    .checked_add(to_usize(d.strsz)?)
                    .ok_or(Refusal::Truncated)?,
        )
        .ok_or(Refusal::Strings)?;
    // Entry 0 is the reserved null symbol, whose index means "no symbol".
    for i in 1..count {
        let sym = at + i * SYM_ENT;
        if u16_at(bytes, sym + 6)? == SHN_UNDEF {
            return Err(Inner::Import);
        }
    }
    // Imports are decided first, for every symbol, so the reason reported
    // does not depend on the order of the table.
    for i in 1..count {
        let sym = at + i * SYM_ENT;
        let start = to_usize(u64::from(u32_at(bytes, sym)?))?;
        let tail = strings.get(start..).ok_or(Refusal::Strings)?;
        let end = tail.iter().position(|&b| b == 0).ok_or(Refusal::Strings)?;
        if !exports.iter().any(|name| name.as_bytes() == &tail[..end]) {
            return Err(Inner::Export);
        }
    }
    Ok(())
}

/// A defined global data export, found through the dynamic tables.
#[derive(Debug)]
#[non_exhaustive]
pub struct DataExport<'a> {
    /// Unrelocated bytes from the checked file.
    pub bytes: &'a [u8],
    /// Virtual address before the system loader adds its base.
    pub address: u64,
}

/// Finds one exact global, default-visible object export after full preflight.
/// Read-only exports must lie in a non-writable, non-executable segment.
/// Section headers are never used. Missing and duplicate exports are refused.
pub fn data_export<'a>(
    bytes: &'a [u8],
    machine: u16,
    name: &str,
    readonly: bool,
) -> Result<DataExport<'a>, Refusal> {
    let (loads, d) = checked(bytes, machine)?;
    let count = symbol_count(bytes, &loads, &d)?;
    let sym = offset_of(
        &loads,
        d.symtab.ok_or(Refusal::Symbols)?,
        (count.checked_mul(SYM_ENT).ok_or(Refusal::Truncated)?) as u64,
    )?;
    let strings = offset_of(&loads, d.strtab.ok_or(Refusal::Strings)?, d.strsz)?;
    let strings = bytes
        .get(strings..strings + to_usize(d.strsz)?)
        .ok_or(Refusal::Strings)?;
    let mut found = None;
    for i in 0..count {
        let at = sym + i * SYM_ENT;
        let start = to_usize(u64::from(u32_at(bytes, at)?))?;
        let tail = strings.get(start..).ok_or(Refusal::Strings)?;
        let end = tail.iter().position(|&b| b == 0).ok_or(Refusal::Strings)?;
        if &tail[..end] != name.as_bytes() {
            continue;
        }
        // STB_GLOBAL, STT_OBJECT, STV_DEFAULT, a defined ordinary section.
        if bytes[at + 4] != 0x11
            || bytes[at + 5] != 0
            || !(1..0xff00).contains(&u16_at(bytes, at + 6)?)
            || found.is_some()
        {
            return Err(Refusal::Symbols);
        }
        let address = u64_at(bytes, at + 8)?;
        let len = u64_at(bytes, at + 16)?;
        let offset = offset_of(&loads, address, len)?;
        let segment = loads
            .iter()
            .flatten()
            .find(|l| {
                address >= l.vaddr
                    && address
                        .checked_add(len)
                        .is_some_and(|end| end <= l.vaddr + l.filesz)
            })
            .ok_or(Refusal::Segments)?;
        if len == 0 || segment.flags & PF_X != 0 || (readonly && segment.flags & PF_W != 0) {
            return Err(Refusal::Segments);
        }
        found = Some(DataExport {
            bytes: bytes
                .get(offset..offset + to_usize(len)?)
                .ok_or(Refusal::Truncated)?,
            address,
        });
    }
    found.ok_or(Refusal::Symbols)
}
