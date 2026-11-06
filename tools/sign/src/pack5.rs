//! A plugin file of format 5 (`.ppn` for interface 7 on the X4, specified in
//! `abi/ppn5.md`) from a plugin linked with `paperpaw-plugin7.x` and
//! `--emit-relocs`.
//!
//! The image is the same as format 4's (the linked `.rodata`, then `.text`),
//! and so are the words to relocate, the hot code and the patches that move
//! it ([`crate::pack::Image`], [`crate::hot`]). What differs is what the
//! file says: no fingerprint, no compiler, no table words and no family
//! revisions, but the entry (20 bytes on the ESP32-C3, `PAPERPAW_ENTRY`) and
//! the metadata record (`PAPERPAW_METADATA`) the generator wrote, whose
//! interface and minor are the header's. **There is no draft field**
//! (`abi/ppn5.md`, "Identity"): nothing in the file says what draft of the
//! interface it was built at.
//!
//! The pack-time half of the freestanding rule (`abi/ppn5.md`, "What #756's
//! freestanding rule becomes on the X4") is here: no undefined symbol (the
//! relocations show it), no symbol exported besides the two, no writable
//! data and nothing that would run before `make` (any section but `.rodata`
//! and `.text` that takes memory is refused), and no relocation kind outside
//! the short list. The loader sees none of that, only what the file shows.
//!
//! Before it returns, the packer reads the file as the loader will, at two
//! places: the header, the digest, every word to relocate and the entry and
//! metadata as they will lie once placed (`Relocator5::verify` and
//! `check_entry`). A file it makes is one the X4's loader takes.

use paperpaw_native::header5::{
    ABI_MAJOR, HEADER_LEN, Header5, Host, META_MAX, RecordFacts, X4_ENTRY_BYTES,
};
use paperpaw_native::host_glue::{ENTRY_SYMBOL, METADATA_SYMBOL, MINOR};
use paperpaw_native::preflight::PLUGIN_EXPORTS;
use paperpaw_native::stream5::{MAX_PATCHES, Relocator5};
use paperpaw_native::{API_SYMBOL, Chip, LINK_BASE, Signature};

use crate::elf::{EM_RISCV, EM_XTENSA, ET_EXEC, ET_REL, Elf, STT_SECTION, Symbol};
use crate::hot::Hot;
use crate::pack::{Image, Place};

/// The symbols the linker script defines for the hot code, which `hot.rs`
/// reads: defined by `paperpaw-plugin7.x`, not by the plugin.
const SCRIPT_SYMBOLS: [&str; 2] = ["PAPERPAW_HOT", "PAPERPAW_HOT_END"];
/// The host heap word (`abi/heap-word.md`): the one object a plugin may
/// export besides the entry and the metadata, and the only word the loader
/// writes into a placed image besides the relocated ones.
const HEAP_SYMBOL: &str = "PAPERPAW_HEAP";
/// `STT_FILE`, a symbol that names a source file.
const STT_FILE: u8 = 4;
/// Where the packer tries the file placed: as linked, and elsewhere.
const TRIAL_BASES: [u32; 2] = [LINK_BASE, LINK_BASE + 0x1_0000];
/// Where the packer tries hot code run, if there is some: in the arena.
const TRIAL_HOT: u32 = 0x4038_0000;

/// A format-5 plugin file made.
#[derive(Debug)]
pub struct Packed5 {
    pub header: Header5,
    /// The whole file, read back as the loader reads it.
    pub file: Vec<u8>,
}

/// Which interface the linked plugin is built for, by what it defines: the
/// metadata record of interface 7, or the table word of interface 6; a plugin
/// that has both or neither is no plugin of either.
pub fn is_interface7(elf: &[u8]) -> Result<bool, String> {
    let elf = Elf::parse(elf)?;
    let (_, symbols) = elf
        .symbols()?
        .ok_or("no symbol table: do not strip the plugin")?;
    let defines = |name: &str| {
        symbols
            .iter()
            .any(|s| s.name == name && s.shndx != crate::elf::SHN_UNDEF)
    };
    match (defines(METADATA_SYMBOL), defines(API_SYMBOL)) {
        (true, false) => Ok(true),
        (false, true) => Ok(false),
        (true, true) => Err(format!(
            "it defines both `{METADATA_SYMBOL}` (interface 7) and `{API_SYMBOL}` (interface 6): \
             link it with one of paperpaw-plugin7.x and paperpaw-plugin.x"
        )),
        (false, false) => Err(format!(
            "it defines neither `{METADATA_SYMBOL}` (interface 7) nor `{API_SYMBOL}` (interface 6): \
             link it with paperpaw-plugin7.x or paperpaw-plugin.x"
        )),
    }
}

/// The format-5 file of the linked plugin `elf`.
pub fn pack5(elf: &[u8]) -> Result<Packed5, String> {
    let elf = Elf::parse(elf)?;
    match elf.kind {
        ET_EXEC => {}
        ET_REL => return Err("an object, not a linked plugin: pack the plugin as linked".into()),
        _ => return Err("not a linked program".into()),
    }
    let chip = match elf.machine {
        EM_RISCV => Chip::Esp32C3,
        EM_XTENSA => {
            return Err(
                "made for the X4 Pro (target 9): refused until an Xtensa profile exists in \
                 abi/entry.md"
                    .into(),
            );
        }
        other => return Err(format!("made for machine {other}, not RISC-V")),
    };
    let (symtab, symbols) = elf
        .symbols()?
        .ok_or("no symbol table: do not strip the plugin")?;
    let image = Image::of(&elf, &symbols)?;
    exports(&image, &symbols)?;
    let (entry, meta_at, record) = locate(&image, &symbols)?;
    let heap_at = heap_word(&image, &symbols)?;
    let facts = RecordFacts::read(record).ok_or_else(|| {
        format!(
            "`{METADATA_SYMBOL}` is not a metadata record this PaperPaw reads (abi/metadata.md)"
        )
    })?;
    if facts.abi_major != ABI_MAJOR || facts.entry_bytes != X4_ENTRY_BYTES {
        return Err(format!(
            "its metadata says interface {} with an entry of {} bytes; the X4 reads interface \
             {ABI_MAJOR} with {X4_ENTRY_BYTES}",
            facts.abi_major, facts.entry_bytes
        ));
    }
    let mut used = [false; paperpaw_native::TABLE_SYMBOLS.len()];
    let words = image.words(&elf, chip, symtab, &symbols, &mut used)?;
    if let Some(n) = used.iter().position(|&u| u) {
        return Err(format!(
            "the code reads `{}`, a family of shared functions: interface 7 has none",
            paperpaw_native::TABLE_SYMBOLS[n]
        ));
    }
    let hot = Hot::of(&elf, &image, chip, &symbols, &words)?;
    let patches = records(&hot.patches)?;
    patch_count(patches.len())?;
    let align = [image.rodata.addralign, image.text.addralign, 4]
        .into_iter()
        .max()
        .unwrap_or(4)
        .min(4096);
    let mut header = Header5 {
        target: chip,
        abi_major: facts.abi_major,
        needs_minor: facts.needs_minor,
        base: LINK_BASE,
        image_len: image.len,
        code_at: image.code_at,
        entry,
        meta_at,
        meta_len: record.len() as u32,
        relocs: words.len() as u32,
        align,
        hot_at: hot.at,
        hot_len: hot.len,
        patches: patches.len() as u32,
        heap_at,
        sha256: [0; 32],
        signed: false,
    };
    let mut rest = image.bytes.clone();
    for word in words.iter().chain(patches.iter().flatten()) {
        rest.extend_from_slice(&word.to_le_bytes());
    }
    let mut sha = header.digest();
    sha.update(&rest);
    header.sha256 = sha.finish();
    let mut file = Vec::with_capacity(HEADER_LEN + rest.len());
    file.extend_from_slice(&header.to_bytes());
    file.extend_from_slice(&rest);
    let (read_back, _) =
        check_file5(&file).map_err(|e| format!("the file made does not hold: {e}"))?;
    if read_back != header {
        return Err("the file made reads back as another header".into());
    }
    Ok(Packed5 { header, file })
}

/// The file `file` (a format-5 plugin file, signed or not) read as the
/// loader reads it: the header, the digest, every word to relocate, and the
/// entry and the metadata as they will lie once placed, at the base it is
/// linked at and at another, with its hot code in the arena. And the author's
/// signature, if the file ends with one.
pub fn check_file5(file: &[u8]) -> Result<(Header5, Option<Signature>), String> {
    let host = Host::x4(MINOR);
    let header = Header5::read(file, file.len() as u64, &host)
        .map_err(|why| format!("not a format-5 file the X4 reads: {why:?}"))?;
    let end = header.file_len() as usize;
    let body = file.get(..end).ok_or("shorter than its header says")?;
    let mut scratch = vec![0u8; 4096];
    for base in TRIAL_BASES {
        let mut relocator =
            Relocator5::new(&header, body, base).map_err(|why| format!("at {base:#x}: {why}"))?;
        if header.hot_len > 0 {
            relocator = relocator
                .hot_at(TRIAL_HOT)
                .map_err(|why| format!("at {base:#x}: {why}"))?;
        }
        relocator
            .verify(&mut scratch)
            .map_err(|why| format!("at {base:#x}: {why}"))?;
        relocator
            .check_entry(&host, &mut scratch)
            .map_err(|why| format!("at {base:#x}: the entry: {why:?}"))?;
    }
    if !header.signed {
        return Ok((header, None));
    }
    let signature = Signature::read(&file[end..]).ok_or("its author's signature does not read")?;
    if signature.signed_len as usize != end || signature.holds(body).is_err() {
        return Err("its author's signature does not hold".into());
    }
    Ok((header, Some(signature)))
}

/// The patches of format 5 from the pairs `hot.rs` finds (the first word,
/// then a reference; upper parts first): each as `kind | site`, `from`, `to`,
/// sorted by site (`abi/ppn5.md`, "Decisions" 4). A lower part's `from` is the
/// site of its `auipc` and `to` what that `auipc`'s upper part reaches: the
/// loader looks nothing up, so **this is where it is guaranteed** that a lower
/// part's `auipc` is patched, and that no instruction is patched twice.
fn records(pairs: &[[u32; 2]]) -> Result<Vec<[u32; 3]>, String> {
    use paperpaw_native::PatchKind;
    let mut upper = std::collections::BTreeMap::new();
    for &[record, reference] in pairs {
        if let Some((PatchKind::Hi20, site)) = PatchKind::of(record) {
            upper.insert(site, reference);
        }
    }
    let mut out = Vec::with_capacity(pairs.len());
    for &[record, reference] in pairs {
        let (kind, site) = PatchKind::of(record).ok_or("a patch of no known kind")?;
        out.push(match kind {
            PatchKind::Hi20 => [record, site, reference],
            _ => {
                let to = upper.get(&reference).ok_or_else(|| {
                    format!(
                        "the instruction at {site:#x} uses the `auipc` at {reference:#x}, which \
                         reaches nothing the packer patches"
                    )
                })?;
                [record, reference, *to]
            }
        });
    }
    out.sort_by_key(|p| p[0] & 0x0FFF_FFFF);
    if let Some(pair) = out
        .windows(2)
        .find(|w| w[0][0] & 0x0FFF_FFFF == w[1][0] & 0x0FFF_FFFF)
    {
        return Err(format!(
            "the instruction at {:#x} is patched twice",
            pair[0][0] & 0x0FFF_FFFF
        ));
    }
    Ok(out)
}

/// Refuses more patches than a file may list (`abi/ppn5.md`, "Size caps"):
/// the loader refuses them too, so pack and load agree.
fn patch_count(patches: usize) -> Result<(), String> {
    if patches > MAX_PATCHES as usize {
        return Err(format!(
            "{patches} patches, more than the {MAX_PATCHES} a plugin file may have (every call \
             from other code into the hot code is two): make less code hot or call it less"
        ));
    }
    Ok(())
}

/// Refuses a symbol the plugin defines and exports besides the two entry
/// symbols (and the two the linker script defines): the loader gives
/// a plugin no name to bind to, so another global is a sign of code that
/// expects one.
fn exports(image: &Image<'_>, symbols: &[Symbol]) -> Result<(), String> {
    let mut extra: Vec<&str> = symbols
        .iter()
        .filter(|s| s.info >> 4 != 0 && s.kind() != STT_SECTION && s.kind() != STT_FILE)
        .filter(|s| image.place(Some(s)) == Place::Image)
        .map(|s| s.name.as_str())
        .filter(|name| {
            !PLUGIN_EXPORTS.contains(name) && !SCRIPT_SYMBOLS.contains(name) && *name != HEAP_SYMBOL
        })
        .collect();
    extra.sort_unstable();
    extra.dedup();
    if extra.is_empty() {
        return Ok(());
    }
    Err(format!(
        "it exports {} besides `{ENTRY_SYMBOL}` and `{METADATA_SYMBOL}`: a plugin exports \
         nothing else (drop `#[no_mangle]` and `pub extern`, or make it local)",
        extra
            .iter()
            .take(5)
            .map(|name| format!("`{name}`"))
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

/// Where `PAPERPAW_HEAP` lies in the image, if the plugin has one: four
/// bytes of read-only data, 4-aligned, holding 0 (the loader writes the
/// address of the host's heap table into the placed image). Anything else
/// is refused, so a plugin cannot ship a value the loader would overwrite
/// or a word the loader reads as a relocation.
fn heap_word(image: &Image<'_>, symbols: &[Symbol]) -> Result<Option<u32>, String> {
    let Some(symbol) = symbols
        .iter()
        .find(|s| s.name == HEAP_SYMBOL && s.shndx != crate::elf::SHN_UNDEF)
    else {
        return Ok(None);
    };
    if symbol.size != 4 {
        return Err(format!(
            "`{HEAP_SYMBOL}` is {} bytes, not the one word of the host heap",
            symbol.size
        ));
    }
    let at = image.in_rodata(symbols, HEAP_SYMBOL, 4).map_err(|why| {
        format!("{why}: the heap word is read-only data the loader fills, never writable data")
    })?;
    if !at.is_multiple_of(4) {
        return Err(format!("`{HEAP_SYMBOL}` is not 4-aligned"));
    }
    if image.bytes[at as usize..at as usize + 4] != [0; 4] {
        return Err(format!(
            "`{HEAP_SYMBOL}` holds a value in the file: it holds 0, the loader writes it"
        ));
    }
    Ok(Some(at))
}

/// Where the entry and the metadata record lie in the image, and the record.
fn locate<'i>(image: &'i Image<'_>, symbols: &[Symbol]) -> Result<(u32, u32, &'i [u8]), String> {
    let sized = |name: &str| {
        symbols
            .iter()
            .find(|s| s.name == name && s.shndx != crate::elf::SHN_UNDEF)
            .map(|s| s.size)
            .ok_or_else(|| format!("no `{name}`: a plugin's entry and metadata come from its SDK"))
    };
    let entry_size = sized(ENTRY_SYMBOL)?;
    if entry_size != X4_ENTRY_BYTES {
        return Err(format!(
            "`{ENTRY_SYMBOL}` is {entry_size} bytes, not the {X4_ENTRY_BYTES} of the X4's entry"
        ));
    }
    let entry = image.in_rodata(symbols, ENTRY_SYMBOL, entry_size)?;
    if !entry.is_multiple_of(8) {
        return Err(format!("`{ENTRY_SYMBOL}` is not 8-aligned"));
    }
    let len = sized(METADATA_SYMBOL)?;
    if len == 0 || len > META_MAX {
        return Err(format!(
            "`{METADATA_SYMBOL}` is {len} bytes, not 1..={META_MAX}"
        ));
    }
    let meta_at = image.in_rodata(symbols, METADATA_SYMBOL, len)?;
    if !meta_at.is_multiple_of(4) {
        return Err(format!("`{METADATA_SYMBOL}` is not 4-aligned"));
    }
    let record = &image.bytes[meta_at as usize..(meta_at + len) as usize];
    Ok((entry, meta_at, record))
}

#[cfg(test)]
mod tests;
