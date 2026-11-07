//! `pack5` on small plugins made with the ELF builder (one thing broken in
//! each refusal), and on the real sample, linked and packed for the X4.

use paperpaw_native::{CODE_OFFSET, LINK_BASE};

use super::*;
use crate::elf::build::Builder;
use crate::elf::{SHF_ALLOC, SHF_WRITE, SHN_ABS, SHN_UNDEF};

/// The metadata record of the X4's profile, which the generator wrote
/// (`abi/fixtures/entry`).
fn record() -> Vec<u8> {
    let hex =
        include_str!("../../../../abi/fixtures/entry/esp32c3-riscv32imc-ilp32-soft-le.hex").trim();
    (0..hex.len() / 2)
        .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

const RODATA: u32 = 20 + 4 + 88;
const TEXT: u32 = LINK_BASE + CODE_OFFSET + RODATA;

/// A small plugin that packs: the entry at 0 (`make` at 16, the metadata at
/// 8), the record at 24, then 32 bytes of code.
struct Plugin {
    b: Builder,
    rodata: usize,
    text: usize,
    make: u32,
}

fn plugin(machine: u16) -> Plugin {
    plugin_with(machine, [0; 4])
}

/// [`plugin`] whose four spare bytes at 20, where a heap word may lie, hold
/// `spare`.
fn plugin_with(machine: u16, spare: [u8; 4]) -> Plugin {
    let record = record();
    let mut rodata = vec![0u8; RODATA as usize];
    rodata[20..24].copy_from_slice(&spare);
    for (at, value) in [
        (0, 20u32),
        (4, 7 | 1 << 16),
        (8, LINK_BASE + 24),
        (12, record.len() as u32),
        (16, TEXT),
    ] {
        rodata[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    rodata[24..24 + record.len()].copy_from_slice(&record);
    let mut b = Builder::new(machine, ET_EXEC);
    let ro = b.section(".rodata", SHF_ALLOC, LINK_BASE, &rodata, 16);
    let tx = b.section(".text", SHF_ALLOC | 4, TEXT, &[0x13; 32], 4);
    b.symbol(ENTRY_SYMBOL, LINK_BASE, 20, ro as u16, false);
    b.symbol(METADATA_SYMBOL, LINK_BASE + 24, 88, ro as u16, false);
    // The whole code is the function `make` (the packer reads each function
    // within its symbol's size and refuses code no function covers).
    let make = b.function("make", TEXT, 32, tx as u16, true);
    b.symbol("PAPERPAW_HOT", TEXT + 32, 0, tx as u16, false);
    b.symbol("PAPERPAW_HOT_END", TEXT + 32, 0, tx as u16, false);
    let meta = b.symbol("META", LINK_BASE + 24, 0, ro as u16, true);
    // The words to relocate: the metadata view and `make`.
    b.rela(ro, LINK_BASE + 8, meta, 1);
    b.rela(ro, LINK_BASE + 16, make, 1);
    Plugin {
        b,
        rodata: ro,
        text: tx,
        make,
    }
}

fn refused(p: &Plugin) -> String {
    pack5(&p.b.build()).unwrap_err()
}

#[test]
fn a_small_plugin_packs_to_a_file_the_loader_reads() {
    let packed = pack5(&plugin(EM_RISCV).b.build()).unwrap();
    let h = packed.header;
    assert_eq!(h.target, Chip::Esp32C3);
    assert_eq!((h.abi_major, h.needs_minor), (7, 1));
    assert_eq!((h.base, h.image_len, h.code_at), (LINK_BASE, 144, 112));
    assert_eq!((h.entry, h.meta_at, h.meta_len), (0, 24, 88));
    assert_eq!((h.relocs, h.align, h.patches), (2, 16, 0));
    assert_eq!(h.hot_len, 0);
    assert_eq!(packed.file.len() as u64, h.file_len());
    // The file reads back whole, at the base it is linked at and at another.
    let (back, signature) = check_file5(&packed.file).unwrap();
    assert_eq!((back, signature.is_none()), (h, true));
    // The words are the two offsets, in order, after the image.
    let tail: Vec<u32> = packed.file[HEADER_LEN + 144..]
        .chunks(4)
        .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
        .collect();
    assert_eq!(tail, [8, 16]);
    // Packing is deterministic.
    let again = pack5(&plugin(EM_RISCV).b.build()).unwrap();
    assert_eq!(again.file, packed.file);
}

#[test]
fn the_x4_pro_is_refused_until_an_xtensa_profile_exists() {
    let error = refused(&plugin(EM_XTENSA));
    assert!(error.contains("target 9"), "{error}");
    assert!(error.contains("Xtensa profile"), "{error}");
    let object = pack5(&Builder::new(EM_RISCV, ET_REL).build()).unwrap_err();
    assert!(object.contains("an object"), "{object}");
    let other = pack5(&Builder::new(3, ET_EXEC).build()).unwrap_err();
    assert!(other.contains("not RISC-V"), "{other}");
}

#[test]
fn the_entry_and_the_record_must_be_where_and_what_the_loader_reads() {
    // An entry of the other profile's length.
    let mut p = plugin(EM_RISCV);
    p.b.symbol(ENTRY_SYMBOL, LINK_BASE, 32, p.rodata as u16, false);
    // The later definition does not replace the first: build a plugin whose
    // entry symbol is the wrong size instead.
    let mut wrong = Builder::new(EM_RISCV, ET_EXEC);
    let ro = wrong.section(".rodata", SHF_ALLOC, LINK_BASE, &[0; 112], 16);
    wrong.section(".text", SHF_ALLOC | 4, TEXT, &[0x13; 32], 4);
    wrong.symbol(ENTRY_SYMBOL, LINK_BASE, 32, ro as u16, false);
    wrong.symbol(METADATA_SYMBOL, LINK_BASE + 24, 88, ro as u16, false);
    let error = pack5(&wrong.build()).unwrap_err();
    assert!(error.contains("not the 20 of the X4's entry"), "{error}");
    // No metadata symbol, an entry at an odd place, a record that is none.
    let missing = Builder::new(EM_RISCV, ET_EXEC);
    assert!(pack5(&missing.build()).is_err());
    for (entry_at, meta_at, expected) in [
        (4, 24, "not 8-aligned"),
        (0, 26, "not 4-aligned"),
        (0, 24, "not a metadata record"),
    ] {
        let mut broken = Builder::new(EM_RISCV, ET_EXEC);
        let mut data = vec![0u8; 128];
        data[24..24 + 88].copy_from_slice(&record());
        if expected == "not a metadata record" {
            data[24] = b'X';
        }
        let ro = broken.section(".rodata", SHF_ALLOC, LINK_BASE, &data, 16);
        broken.section(
            ".text",
            SHF_ALLOC | 4,
            LINK_BASE + CODE_OFFSET + 128,
            &[0x13; 32],
            4,
        );
        broken.symbol(ENTRY_SYMBOL, LINK_BASE + entry_at, 20, ro as u16, false);
        broken.symbol(METADATA_SYMBOL, LINK_BASE + meta_at, 88, ro as u16, false);
        let error = pack5(&broken.build()).unwrap_err();
        assert!(error.contains(expected), "{expected}: {error}");
    }
}

#[test]
fn a_record_of_another_interface_is_refused_by_what_it_says() {
    for (at, value, expected) in [(12, 8u8, "interface 8"), (18, 24, "entry of 24 bytes")] {
        let mut data = vec![0u8; 112];
        let mut record = record();
        record[at] = value;
        data[24..24 + 88].copy_from_slice(&record);
        let mut b = Builder::new(EM_RISCV, ET_EXEC);
        let ro = b.section(".rodata", SHF_ALLOC, LINK_BASE, &data, 16);
        b.section(".text", SHF_ALLOC | 4, TEXT, &[0x13; 32], 4);
        b.symbol(ENTRY_SYMBOL, LINK_BASE, 20, ro as u16, false);
        b.symbol(METADATA_SYMBOL, LINK_BASE + 24, 88, ro as u16, false);
        let error = pack5(&b.build()).unwrap_err();
        assert!(error.contains(expected), "{expected}: {error}");
    }
}

#[test]
fn an_exported_symbol_besides_the_two_is_refused() {
    let mut p = plugin(EM_RISCV);
    p.b.symbol("make_it", TEXT + 4, 4, p.text as u16, false);
    p.b.symbol("zzz", LINK_BASE + 100, 4, p.rodata as u16, false);
    let error = refused(&p);
    assert!(error.contains("`make_it`, `zzz`"), "{error}");
    assert!(error.contains("exports"), "{error}");
    // A symbol of the linker script outside the image, or a local, is no export.
    let mut p = plugin(EM_RISCV);
    p.b.symbol("ROM_FN", 0x4000_1234, 0, SHN_ABS, false);
    p.b.symbol("local_fn", TEXT + 4, 4, p.text as u16, true);
    assert!(pack5(&p.b.build()).is_ok());
}

#[test]
fn writable_data_other_sections_and_undefined_symbols_are_refused() {
    let mut p = plugin(EM_RISCV);
    let bss = p.b.nobits(".bss", SHF_ALLOC | SHF_WRITE, 0x3FC8_0000, 24);
    p.b.symbol("STATE", 0x3FC8_0000, 8, bss as u16, true);
    assert!(refused(&p).contains("no writable data"), "{}", refused(&p));
    let mut p = plugin(EM_RISCV);
    p.b.section(".init_array", SHF_ALLOC, 0x3C00_1000, &[0; 8], 4);
    assert!(refused(&p).contains("unexpected section .init_array"));
    let mut p = plugin(EM_RISCV);
    let undefined = p.b.symbol("memcpy", 0, 0, SHN_UNDEF, false);
    p.b.rela(p.text, TEXT, undefined, 18);
    assert!(refused(&p).contains("undefined symbol `memcpy`"));
}

#[test]
fn an_interface_6_table_word_is_no_interface_7_plugin() {
    let mut p = plugin(EM_RISCV);
    let elf = p.b.build();
    assert_eq!(is_interface7(&elf), Ok(true));
    p.b.symbol(API_SYMBOL, LINK_BASE + 100, 0, SHN_ABS, false);
    assert!(is_interface7(&p.b.build()).unwrap_err().contains("both"));
    let neither = Builder::new(EM_RISCV, ET_EXEC);
    assert!(
        is_interface7(&neither.build())
            .unwrap_err()
            .contains("neither")
    );
    // A family word read in the code: interface 7 has none.
    let mut p = plugin(EM_RISCV);
    let family = p.b.symbol(
        paperpaw_native::TABLE_SYMBOLS[0],
        LINK_BASE + 100,
        0,
        p.rodata as u16,
        true,
    );
    p.b.rela(p.text, TEXT, family, 18);
    let error = refused(&p);
    assert!(error.contains("a family of shared functions"), "{error}");
}

#[test]
fn a_word_that_points_outside_the_image_is_refused_by_the_packer() {
    let mut p = plugin(EM_RISCV);
    // The record's first word is no address.
    let record_word =
        p.b.symbol("RECORD", LINK_BASE + 24, 0, p.rodata as u16, true);
    p.b.rela(p.rodata, LINK_BASE + 24, record_word, 1);
    let error = refused(&p);
    assert!(error.contains("points outside the image"), "{error}");
    let _ = p.make;
}

#[test]
fn more_than_8192_patches_are_refused_as_the_loader_refuses_them() {
    assert_eq!(MAX_PATCHES, 8192);
    assert_eq!(patch_count(0), Ok(()));
    assert_eq!(patch_count(8192), Ok(()));
    let error = patch_count(8193).unwrap_err();
    assert!(error.contains("8193 patches"), "{error}");
    assert!(error.contains("more than the 8192"), "{error}");
}

/// The real sample, linked for the X4 by cargo and `paperpaw-plugin7.x`
/// (`samples/abi7-x4-min`), and the file the real packer made of it.
const SAMPLE_ELF: &[u8] =
    include_bytes!("../../../../crates/paperpaw-native/tests/data/abi7-x4-min-linked.bin");
const SAMPLE_PPN: &[u8] =
    include_bytes!("../../../../crates/paperpaw-native/tests/data/abi7-x4-min.ppn");

#[test]
fn the_real_sample_packs_to_the_committed_file() {
    assert_eq!(is_interface7(SAMPLE_ELF), Ok(true));
    let packed = pack5(SAMPLE_ELF).unwrap();
    assert_eq!(
        packed.file, SAMPLE_PPN,
        "run samples/abi7-x4-min/build.sh --update and review"
    );
    let h = packed.header;
    assert_eq!(
        (h.target, h.abi_major, h.needs_minor),
        (Chip::Esp32C3, 7, 1)
    );
    assert_eq!((h.image_len, h.code_at, h.entry), (236, 144, 0));
    assert_eq!((h.meta_at, h.meta_len, h.relocs, h.align), (20, 88, 3, 16));
    // One hot function, called from cold code: an `auipc` and its partner.
    assert_eq!((h.hot_at, h.hot_len, h.patches), (224, 12, 2));
}

#[test]
fn the_real_sample_is_refused_for_the_x4_pro_and_as_an_object() {
    let mut elf = SAMPLE_ELF.to_vec();
    elf[18..20].copy_from_slice(&EM_XTENSA.to_le_bytes());
    let error = pack5(&elf).unwrap_err();
    assert!(error.contains("target 9"), "{error}");
    let mut elf = SAMPLE_ELF.to_vec();
    elf[16..18].copy_from_slice(&ET_REL.to_le_bytes());
    assert!(pack5(&elf).unwrap_err().contains("an object"));
    // Cut short or changed anywhere, it is refused with a reason, never a panic.
    for len in (0..SAMPLE_ELF.len()).step_by(7) {
        let _ = pack5(&SAMPLE_ELF[..len]);
    }
    for at in (0..SAMPLE_ELF.len()).step_by(3) {
        let mut elf = SAMPLE_ELF.to_vec();
        elf[at] ^= 0x5A;
        let _ = pack5(&elf);
    }
}

#[test]
fn the_committed_file_is_read_as_the_loader_reads_it_and_no_change_passes() {
    let (header, signature) = check_file5(SAMPLE_PPN).unwrap();
    assert!(signature.is_none());
    assert_eq!(header.file_len(), SAMPLE_PPN.len() as u64);
    // Every byte changed in turn (the digest or a check refuses it), and
    // every cut short.
    for at in 0..SAMPLE_PPN.len() {
        let mut file = SAMPLE_PPN.to_vec();
        file[at] ^= 0x01;
        assert!(check_file5(&file).is_err(), "byte {at}");
    }
    for len in 0..SAMPLE_PPN.len() {
        assert!(check_file5(&SAMPLE_PPN[..len]).is_err(), "{len} bytes");
    }
}

#[test]
fn a_signed_sample_checks_and_a_changed_one_does_not() {
    let seed = [7u8; 32];
    let mut file = SAMPLE_PPN.to_vec();
    file.extend_from_slice(&paperpaw_native::signature::sign(&file, &seed).to_bytes());
    let (header, signature) = check_file5(&file).unwrap();
    assert!(header.signed);
    assert!(signature.is_some());
    // The same file with one byte of the signature changed.
    let last = file.len() - 1;
    file[last] ^= 1;
    assert!(check_file5(&file).unwrap_err().contains("signature"));
    // A byte of the image changed, signed again: the digest still refuses it.
    let mut other = SAMPLE_PPN.to_vec();
    other[HEADER_LEN + 3] ^= 1;
    other.extend_from_slice(&paperpaw_native::signature::sign(&other, &seed).to_bytes());
    assert!(check_file5(&other).is_err());
}

/// A plugin with `PAPERPAW_HEAP` of `size` bytes at `LINK_BASE + at`.
fn with_heap(spare: [u8; 4], at: u32, size: u32) -> Plugin {
    let mut p = plugin_with(EM_RISCV, spare);
    p.b.symbol(HEAP_SYMBOL, LINK_BASE + at, size, p.rodata as u16, false);
    p
}

#[test]
fn the_heap_word_is_found_by_its_symbol_and_recorded_in_the_header() {
    let plain = pack5(&plugin(EM_RISCV).b.build()).unwrap();
    assert_eq!(plain.header.heap_at, None);
    assert_eq!(&plain.file[12..16], &[0; 4]);
    assert_eq!(&plain.file[60..64], &[0; 4]);
    let packed = pack5(&with_heap([0; 4], 20, 4).b.build()).unwrap();
    assert_eq!(packed.header.heap_at, Some(20));
    // Flag bit 0 and `heap_at` are in the file, and the file reads back as
    // the loader reads it (the digest covers both).
    assert_eq!(&packed.file[12..16], &1u32.to_le_bytes());
    assert_eq!(&packed.file[60..64], &20u32.to_le_bytes());
    let (back, _) = check_file5(&packed.file).unwrap();
    assert_eq!(back, packed.header);
    // Nothing else of the image or the words differs.
    assert_eq!(packed.file[HEADER_LEN..], plain.file[HEADER_LEN..]);
    assert_ne!(packed.header.sha256, plain.header.sha256);
    // And it is no extra export.
    assert!(pack5(&with_heap([0; 4], 20, 4).b.build()).is_ok());
}

#[test]
fn a_heap_word_that_is_not_one_zero_read_only_aligned_word_is_refused() {
    let error = |p: Plugin| pack5(&p.b.build()).unwrap_err();
    // The wrong size.
    for size in [0, 2, 8] {
        let e = error(with_heap([0; 4], 20, size));
        assert!(
            e.contains("not the one word of the host heap"),
            "{size}: {e}"
        );
    }
    // Not 4-aligned.
    let e = error(with_heap([0; 4], 22, 4));
    assert!(e.contains("not 4-aligned"), "{e}");
    // A value in the file.
    let e = error(with_heap([1, 0, 0, 0], 20, 4));
    assert!(e.contains("holds a value in the file"), "{e}");
    // In the code, not in read-only data.
    let mut p = plugin(EM_RISCV);
    p.b.symbol(HEAP_SYMBOL, TEXT + 4, 4, p.text as u16, false);
    let e = error(p);
    assert!(e.contains("not in .rodata"), "{e}");
    // A word to relocate is refused either way: its value in the file points
    // into the image (so it is no 0), or it points outside it.
    let mut p = with_heap(LINK_BASE.wrapping_add(24).to_le_bytes(), 20, 4);
    let meta =
        p.b.symbol("META2", LINK_BASE + 24, 0, p.rodata as u16, true);
    p.b.rela(p.rodata, LINK_BASE + 20, meta, 1);
    let e = error(p);
    assert!(e.contains("holds a value in the file"), "{e}");
    let mut p = with_heap([0; 4], 20, 4);
    let meta =
        p.b.symbol("META2", LINK_BASE + 24, 0, p.rodata as u16, true);
    p.b.rela(p.rodata, LINK_BASE + 20, meta, 1);
    assert!(pack5(&p.b.build()).is_err());
}
