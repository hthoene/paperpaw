use alloc::vec::Vec;

use super::*;

const HOST: Host = Host::x4(1);

/// A header that holds together; each test breaks one thing.
fn good() -> Header5 {
    Header5 {
        target: Chip::Esp32C3,
        abi_major: 7,
        needs_minor: 1,
        base: 0x3C00_0000,
        image_len: 4096,
        code_at: 1024,
        entry: 64,
        meta_at: 128,
        meta_len: 96,
        relocs: 10,
        align: 16,
        hot_at: 0,
        hot_len: 0,
        patches: 0,
        heap_at: None,
        sha256: [7; 32],
        signed: false,
    }
}

fn file_len(header: &Header5, signed: bool) -> u64 {
    header.file_len() + if signed { SIGNATURE_LEN as u64 } else { 0 }
}

fn read(bytes: &[u8], len: u64) -> Result<Header5, Refusal> {
    Header5::read(bytes, len, &HOST)
}

/// `good()` with `edit` applied to its bytes (the header, not the struct).
fn edited(edit: impl FnOnce(&mut [u8; HEADER_LEN])) -> Result<Header5, Refusal> {
    let header = good();
    let mut bytes = header.to_bytes();
    edit(&mut bytes);
    read(&bytes, file_len(&header, false))
}

fn set16(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

fn set32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn malformed(header: Header5) -> bool {
    let len = header.file_len();
    matches!(read(&header.to_bytes(), len), Err(Refusal::Malformed(_)))
}

#[test]
fn a_good_header_round_trips_signed_or_not() {
    let header = good();
    for signed in [false, true] {
        let read = read(&header.to_bytes(), file_len(&header, signed)).unwrap();
        assert_eq!(read, Header5 { signed, ..header });
    }
    assert_eq!(HEADER_LEN, 96);
    assert_eq!(header.file_len(), 96 + 4096 + 40);
}

#[test]
fn the_first_checks_are_in_the_order_of_the_loader() {
    let header = good();
    let bytes = header.to_bytes();
    let len = file_len(&header, false);
    // Not a plugin: too short to say, or another magic.
    for head in [&[][..], &bytes[..3], b"NOPE-and-more"] {
        assert_eq!(read(head, len), Err(Refusal::NotAPlugin));
    }
    // The magic, but not the header.
    assert_eq!(read(&bytes[..95], len), Err(Refusal::Truncated));
    assert_eq!(read(&bytes[..4], len), Err(Refusal::Truncated));
    // Format.
    assert_eq!(edited(|b| set16(b, 4, 4)), Err(Refusal::OlderFormat));
    assert_eq!(edited(|b| set16(b, 4, 6)), Err(Refusal::NewerFormat));
    assert_eq!(edited(|b| set16(b, 4, 0)), Err(Refusal::OlderFormat));
    assert_eq!(edited(|b| set16(b, 4, 0xFFFF)), Err(Refusal::NewerFormat));
    // Chip: unknown, the X4 Pro's (no profile), and a host that has none.
    assert_eq!(edited(|b| set16(b, 6, 77)), Err(Refusal::UnknownChip));
    assert_eq!(edited(|b| set16(b, 6, 9)), Err(Refusal::NoProfile));
    let xtensa = Host {
        target: Chip::Esp32S3,
        ..HOST
    };
    assert_eq!(Header5::read(&bytes, len, &xtensa), Err(Refusal::NoProfile));
    // Interface major, then the needed minor.
    assert_eq!(edited(|b| set16(b, 8, 6)), Err(Refusal::OlderInterface));
    assert_eq!(edited(|b| set16(b, 8, 8)), Err(Refusal::NewerInterface));
    assert_eq!(edited(|b| set16(b, 8, 0)), Err(Refusal::OlderInterface));
    assert_eq!(edited(|b| set16(b, 10, 2)), Err(Refusal::NeedsNewerMinor));
    assert_eq!(
        edited(|b| set16(b, 10, 0xFFFF)),
        Err(Refusal::NeedsNewerMinor)
    );
    // A file for an older minor is taken.
    assert!(edited(|b| set16(b, 10, 0)).is_ok());
}

#[test]
fn a_refusal_by_the_first_sixteen_bytes_comes_before_anything_else_is_read() {
    // Garbage everywhere after byte 16: the format, chip, interface and
    // minor are still what is said first.
    let mut bytes = [0xFF; HEADER_LEN];
    bytes[..4].copy_from_slice(&MAGIC);
    set16(&mut bytes, 4, 4);
    assert_eq!(read(&bytes, 1), Err(Refusal::OlderFormat));
    set16(&mut bytes, 4, 5);
    assert_eq!(read(&bytes, 1), Err(Refusal::UnknownChip));
    set16(&mut bytes, 6, 5);
    set16(&mut bytes, 8, 9);
    assert_eq!(read(&bytes, 1), Err(Refusal::NewerInterface));
    set16(&mut bytes, 8, 7);
    set16(&mut bytes, 10, 9);
    assert_eq!(read(&bytes, 1), Err(Refusal::NeedsNewerMinor));
    set16(&mut bytes, 10, 1);
    assert_eq!(read(&bytes, 1), Err(Refusal::Reserved));
}

#[test]
fn only_the_heap_word_flag_is_known_and_the_reserved_word_is_its_offset() {
    // Bit 0 of the flags is the heap word; every other bit is a later
    // format's.
    for (at, bit) in [(12, 1u8 << 1), (12, 0x80), (13, 1), (14, 1), (15, 0x80)] {
        assert_eq!(
            edited(|b| b[at] = bit),
            Err(Refusal::Reserved),
            "flags {at} {bit:#x}"
        );
    }
    // A nonzero word at 60..64 without the flag is reserved.
    for at in 60..64 {
        assert_eq!(
            edited(|b| b[at] = 1),
            Err(Refusal::Reserved),
            "reserved {at}"
        );
    }
    // With the flag it is the offset, and 0 is an offset like any other.
    let with = |at: u32| {
        edited(|b| {
            set32(b, 12, FLAG_HEAP_WORD);
            set32(b, 60, at);
        })
    };
    assert_eq!(with(256).map(|h| h.heap_at), Ok(Some(256)));
    assert_eq!(with(0).map(|h| h.heap_at), Ok(Some(0)));
}

#[test]
fn a_header_with_the_heap_word_round_trips_and_its_digest_covers_it() {
    let header = Header5 {
        heap_at: Some(256),
        ..good()
    };
    assert_eq!(
        read(&header.to_bytes(), file_len(&header, false)),
        Ok(header)
    );
    // The flag and the offset are in the bytes the digest covers.
    let (with, without) = (header.digest_prefix(), good().digest_prefix());
    assert_eq!(&with[12..16], &1u32.to_le_bytes());
    assert_eq!(&with[60..64], &256u32.to_le_bytes());
    assert_ne!(with, without);
    assert_eq!(&without[12..16], &[0; 4]);
    assert_eq!(&without[60..64], &[0; 4]);
}

#[test]
fn the_heap_word_lies_in_the_read_only_data_outside_the_entry_and_the_metadata() {
    // good(): code_at 1024, entry 64..84, metadata 128..224.
    let with = |at: u32| {
        let header = Header5 {
            heap_at: Some(at),
            ..good()
        };
        read(&header.to_bytes(), file_len(&header, false))
    };
    let bad = |rule: &'static str| Err(Refusal::Malformed(rule));
    // Where it may lie, the edges included.
    for at in [0, 60, 84, 224, 256, 1020] {
        assert_eq!(with(at).map(|h| h.heap_at), Ok(Some(at)), "at {at}");
    }
    // 4-aligned.
    for at in [1, 2, 3, 258, 1021] {
        assert_eq!(with(at), bad("the heap word is not 4-aligned"), "at {at}");
    }
    // In the read-only data: [at, at + 4) <= code_at.
    for at in [1024, 1028, 4092, u32::MAX - 3, !3u32] {
        assert_eq!(
            with(at),
            bad("the heap word is not in the read-only data"),
            "at {at}"
        );
    }
    // Outside the entry (20 bytes at 64) and the metadata record.
    for at in [64, 68, 72, 80] {
        assert_eq!(with(at), bad("the heap word lies in the entry"), "at {at}");
    }
    for at in [128, 132, 220] {
        assert_eq!(
            with(at),
            bad("the heap word lies in the metadata record"),
            "at {at}"
        );
    }
}

#[test]
fn the_file_is_exactly_as_long_as_the_header_says() {
    let header = good();
    let bytes = header.to_bytes();
    let exact = header.file_len();
    assert_eq!(read(&bytes, exact).map(|h| h.signed), Ok(false));
    assert_eq!(
        read(&bytes, exact + SIGNATURE_LEN as u64).map(|h| h.signed),
        Ok(true)
    );
    for wrong in [
        0,
        exact - 1,
        exact + 1,
        exact + SIGNATURE_LEN as u64 - 1,
        exact + SIGNATURE_LEN as u64 + 1,
        exact + 2 * SIGNATURE_LEN as u64,
        u64::MAX,
    ] {
        assert_eq!(read(&bytes, wrong), Err(Refusal::Length), "{wrong}");
    }
    // Patches count 12 bytes each in the formula (`PATCH_BYTES`).
    let patched = Header5 {
        patches: 3,
        ..header
    };
    assert_eq!(patched.file_len(), exact + 36);
    assert!(read(&patched.to_bytes(), exact + 36).is_ok());
    assert_eq!(read(&patched.to_bytes(), exact), Err(Refusal::Length));
}

#[test]
fn parts_that_do_not_hold_together_are_malformed() {
    let g = good();
    let cases = [
        Header5 { image_len: 0, ..g },
        Header5 {
            image_len: MAX_IMAGE_LEN + 4,
            ..g
        },
        Header5 { code_at: 4100, ..g },
        Header5 { code_at: 1022, ..g },
        Header5 { align: 0, ..g },
        Header5 { align: 3, ..g },
        Header5 { align: 2, ..g },
        Header5 { align: 8192, ..g },
        Header5 {
            base: 0x3C00_0004,
            align: 16,
            ..g
        },
        Header5 {
            base: 0xFFFF_F000,
            align: 4096,
            ..g
        },
        Header5 { meta_len: 0, ..g },
        Header5 {
            meta_len: META_MAX + 1,
            ..g
        },
        Header5 { meta_at: 130, ..g },
        Header5 {
            meta_at: 1000,
            meta_len: 100,
            ..g
        },
        Header5 {
            meta_at: u32::MAX - 3,
            ..g
        },
        Header5 { entry: 60, ..g },
        Header5 { entry: 1016, ..g },
        Header5 {
            entry: u32::MAX - 7,
            ..g
        },
        Header5 { relocs: 1025, ..g },
        Header5 { hot_at: 1024, ..g },
        Header5 {
            hot_at: 1024,
            hot_len: 6,
            ..g
        },
        Header5 {
            hot_at: 1022,
            hot_len: 8,
            ..g
        },
        Header5 {
            hot_at: 4092,
            hot_len: 8,
            ..g
        },
        Header5 {
            hot_at: 1026,
            hot_len: 8,
            ..g
        },
        Header5 { patches: 1537, ..g },
    ];
    for (i, case) in cases.into_iter().enumerate() {
        assert!(malformed(case), "case {i}: {case:?}");
    }
}

#[test]
fn the_edges_that_hold_are_taken() {
    let g = good();
    // code_at at the end (no code is for the entry checks), the largest
    // record, hot code at HOT_MAX.
    for edge in [
        Header5 { code_at: 4096, ..g },
        Header5 {
            meta_len: META_MAX,
            meta_at: 128,
            code_at: 2176,
            ..g
        },
        Header5 { relocs: 1024, ..g },
        Header5 {
            hot_at: 1024,
            hot_len: HOT_MAX,
            image_len: 1024 + HOT_MAX,
            code_at: 1024,
            ..g
        },
        Header5 { patches: 1536, ..g },
    ] {
        assert!(
            read(&edge.to_bytes(), edge.file_len()).is_ok(),
            "{edge:?}: {:?}",
            read(&edge.to_bytes(), edge.file_len())
        );
    }
    // Hot code above the arena is refused even where it lies in the image.
    let big = Header5 {
        hot_at: 1024,
        hot_len: HOT_MAX + 4,
        image_len: 1024 + HOT_MAX + 4,
        ..g
    };
    assert!(malformed(big));
}

#[test]
fn big_numbers_never_overflow_or_allocate() {
    let g = good();
    let mut bytes = g.to_bytes();
    for at in (16..60).step_by(4) {
        let mut copy = bytes;
        set32(&mut copy, at, u32::MAX);
        assert!(read(&copy, u64::MAX).is_err(), "field at {at}");
        assert!(read(&copy, 0).is_err(), "field at {at}");
    }
    // All fields at once.
    for at in (16..60).step_by(4) {
        set32(&mut bytes, at, u32::MAX);
    }
    assert!(read(&bytes, u64::MAX).is_err());
}

#[test]
fn the_digest_covers_the_first_64_bytes_and_all_after() {
    let mut header = good();
    let body = [3u8; 100];
    let mut sha = header.digest();
    sha.update(&body);
    header.sha256 = sha.finish();
    let mut again = header.digest();
    again.update(&body);
    assert!(header.holds(again));
    let mut other = header.digest();
    other.update(&[4u8; 100]);
    assert!(!header.holds(other));
    // Changing a covered header field changes the digest prefix.
    let changed = Header5 {
        meta_len: header.meta_len + 4,
        ..header
    };
    assert_ne!(changed.digest_prefix(), header.digest_prefix());
    // The bytes the digest does not cover are the digest itself.
    assert_eq!(&header.to_bytes()[64..], &header.sha256);
}

#[test]
fn relocation_words_must_be_addresses_in_the_image_by_either_view() {
    let h = good();
    let code = h.base + CODE_OFFSET;
    // Data addresses and code addresses, the first byte, the last, and the
    // end (a slice may point there).
    let inside = [
        h.base,
        h.base + h.image_len - 1,
        h.base + h.image_len,
        h.base + 100,
        code,
        code + h.code_at,
        code + h.image_len,
    ];
    assert_eq!(h.check_reloc_words(inside), Ok(()));
    assert_eq!(h.check_reloc_words([]), Ok(()));
    for outside in [
        h.base - 1,
        h.base + h.image_len + 1,
        code - 1,
        code + h.image_len + 1,
        0,
        u32::MAX,
        h.base + h.image_len + CODE_OFFSET / 2,
    ] {
        assert_eq!(
            h.check_reloc_words([h.base, outside]),
            Err(EntryRefusal::RelocOutside),
            "{outside:#x}"
        );
    }
}

fn record_bytes(len: usize) -> Vec<u8> {
    (0..len).map(|i| i as u8).collect()
}

fn entry<'a>(h: &Header5, record: &'a [u8]) -> EntryFacts<'a> {
    EntryFacts {
        size: X4_ENTRY_BYTES,
        abi_major: h.abi_major,
        needs_minor: h.needs_minor,
        metadata_ptr: h.base + h.meta_at,
        metadata_len: h.meta_len,
        metadata: record,
        make_at: h.code_at + 16,
    }
}

const RECORD: RecordFacts = RecordFacts {
    abi_major: 7,
    needs_minor: 1,
    entry_bytes: X4_ENTRY_BYTES,
};

#[test]
fn a_matching_entry_is_taken() {
    let h = good();
    let record = record_bytes(h.meta_len as usize);
    assert_eq!(
        h.check_entry(h.base, &HOST, &RECORD, &record, &entry(&h, &record)),
        Ok(())
    );
}

/// An image placed somewhere else than it was linked: the relocated
/// pointer is the placed base's, not the linked base's.
#[test]
fn the_metadata_view_is_checked_against_the_placed_base() {
    let h = good();
    let record = record_bytes(h.meta_len as usize);
    let placed = h.base + 0x1_0000;
    let moved = EntryFacts {
        metadata_ptr: placed + h.meta_at,
        ..entry(&h, &record)
    };
    // Placed elsewhere and relocated: taken.
    assert_eq!(
        h.check_entry(placed, &HOST, &RECORD, &record, &moved),
        Ok(())
    );
    // The same facts against the linked base: the pointer is not there.
    assert_eq!(
        h.check_entry(h.base, &HOST, &RECORD, &record, &moved),
        Err(EntryRefusal::MetadataView)
    );
    // An unrelocated pointer (still the linked address) in a moved image.
    let unrelocated = entry(&h, &record);
    assert_eq!(unrelocated.metadata_ptr, h.base + h.meta_at);
    assert_eq!(
        h.check_entry(placed, &HOST, &RECORD, &record, &unrelocated),
        Err(EntryRefusal::MetadataView)
    );
    // A placed base that cannot hold the offset is no view either.
    assert_eq!(
        h.check_entry(u32::MAX, &HOST, &RECORD, &record, &moved),
        Err(EntryRefusal::MetadataView)
    );
}

#[test]
fn every_entry_check_refuses_on_its_own() {
    let h = good();
    let record = record_bytes(h.meta_len as usize);
    let ok = entry(&h, &record);
    let check = |record_facts: &RecordFacts, bytes: &[u8], facts: &EntryFacts<'_>| {
        h.check_entry(h.base, &HOST, record_facts, bytes, facts)
    };
    // The record against the header.
    for facts in [
        RecordFacts {
            abi_major: 8,
            ..RECORD
        },
        RecordFacts {
            needs_minor: 0,
            ..RECORD
        },
        RecordFacts {
            entry_bytes: 24,
            ..RECORD
        },
    ] {
        assert_eq!(
            check(&facts, &record, &ok),
            Err(EntryRefusal::RecordDisagrees)
        );
    }
    assert_eq!(
        check(&RECORD, &record[1..], &ok),
        Err(EntryRefusal::RecordLength)
    );
    // The metadata view: where it is, how long, what it shows.
    for facts in [
        EntryFacts {
            metadata_ptr: ok.metadata_ptr + 4,
            ..ok
        },
        EntryFacts {
            metadata_ptr: ok.metadata_ptr - 4,
            ..ok
        },
        EntryFacts {
            metadata_ptr: 0,
            ..ok
        },
        EntryFacts {
            metadata_len: ok.metadata_len + 1,
            ..ok
        },
        EntryFacts {
            metadata_len: ok.metadata_len - 1,
            ..ok
        },
        EntryFacts {
            metadata: &record[1..],
            ..ok
        },
    ] {
        assert_eq!(
            check(&RECORD, &record, &facts),
            Err(EntryRefusal::MetadataView)
        );
    }
    let mut other = record.clone();
    other[40] ^= 1;
    assert_eq!(
        check(
            &RECORD,
            &record,
            &EntryFacts {
                metadata: &other,
                ..ok
            }
        ),
        Err(EntryRefusal::MetadataBytes)
    );
}

#[test]
fn the_entry_itself_is_checked_for_size_versions_and_make() {
    let h = good();
    let record = record_bytes(h.meta_len as usize);
    let ok = entry(&h, &record);
    let check = |record_facts: &RecordFacts, bytes: &[u8], facts: &EntryFacts<'_>| {
        h.check_entry(h.base, &HOST, record_facts, bytes, facts)
    };
    // The entry's own size and versions.
    for size in [0, 16, 24, 32, u32::MAX] {
        assert_eq!(
            check(&RECORD, &record, &EntryFacts { size, ..ok }),
            Err(EntryRefusal::EntrySize)
        );
    }
    assert_eq!(
        check(&RECORD, &record, &EntryFacts { abi_major: 6, ..ok }),
        Err(EntryRefusal::EntryVersion)
    );
    assert_eq!(
        check(
            &RECORD,
            &record,
            &EntryFacts {
                needs_minor: 0,
                ..ok
            }
        ),
        Err(EntryRefusal::EntryVersion)
    );
    // `make` in [code_at, image_len), 2-aligned.
    for make_at in [
        0,
        h.code_at - 2,
        h.image_len,
        h.image_len + 2,
        h.code_at + 1,
        u32::MAX,
    ] {
        assert_eq!(
            check(&RECORD, &record, &EntryFacts { make_at, ..ok }),
            Err(EntryRefusal::Make),
            "{make_at}"
        );
    }
    for make_at in [h.code_at, h.code_at + 2, h.image_len - 2] {
        assert_eq!(
            check(&RECORD, &record, &EntryFacts { make_at, ..ok }),
            Ok(()),
            "{make_at}"
        );
    }
}

/// The generated metadata record of the X4 profile
/// (`abi/fixtures/entry`), the bytes the prefix offsets are pinned by.
pub(crate) fn x4_record() -> Vec<u8> {
    let hex = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../abi/fixtures/entry/esp32c3-riscv32imc-ilp32-soft-le.hex"
    ))
    .trim();
    (0..hex.len() / 2)
        .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

#[test]
fn the_record_prefix_reads_as_the_generated_fixture_says() {
    let record = x4_record();
    assert_eq!(
        RecordFacts::read(&record),
        Some(RecordFacts {
            abi_major: generated_major(),
            needs_minor: 1,
            entry_bytes: X4_ENTRY_BYTES,
        })
    );
}

fn generated_major() -> u16 {
    let abi = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../abi/generated/paperpaw_abi.rs"
    ));
    let line = abi
        .lines()
        .find(|l| l.starts_with("pub const MAJOR: u16 = "))
        .unwrap();
    line.trim_end_matches(';')
        .rsplit(' ')
        .next()
        .unwrap()
        .parse()
        .unwrap()
}

#[test]
fn a_record_that_is_not_the_first_layout_is_not_read() {
    let record = x4_record();
    assert_eq!(record.len(), 0x58);
    // Short, long, or a total that is not its length.
    for bad in [&record[..31], &record[..record.len() - 1], &[][..]] {
        assert_eq!(RecordFacts::read(bad), None);
    }
    let mut long = record.clone();
    long.push(0);
    assert_eq!(RecordFacts::read(&long), None, "trailing byte");
    let too_big = alloc::vec![0u8; META_MAX as usize + 1];
    assert_eq!(RecordFacts::read(&too_big), None);
    // Each prefix field broken on its own: magic, layout, prefix length,
    // total, entry layout, flags.
    for (at, value) in [(0, 0x4E), (4, 3), (6, 33), (8, 0x59), (16, 2), (20, 1)] {
        let mut changed = record.clone();
        changed[at] = value;
        assert_eq!(RecordFacts::read(&changed), None, "byte {at}");
    }
    // The version fields are facts, not checks: step 9 compares them.
    let mut other = record.clone();
    other[12] = 8;
    other[14] = 9;
    other[18] = 24;
    assert_eq!(
        RecordFacts::read(&other),
        Some(RecordFacts {
            abi_major: 8,
            needs_minor: 9,
            entry_bytes: 24
        })
    );
}

#[test]
fn the_raw_entry_reads_in_the_order_of_the_generated_struct() {
    let mut bytes = Vec::new();
    for word in [20u32, 7 | 1 << 16, 0x3C00_0080, 96, 0x4200_0410] {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    assert_eq!(bytes.len() as u32, X4_ENTRY_BYTES);
    assert_eq!(
        RawEntry::read(&bytes),
        Some(RawEntry {
            size: 20,
            abi_major: 7,
            needs_minor: 1,
            metadata_ptr: 0x3C00_0080,
            metadata_len: 96,
            make: 0x4200_0410,
        })
    );
    assert_eq!(RawEntry::read(&bytes[..19]), None);
    bytes.push(0xEE);
    assert!(RawEntry::read(&bytes).is_some(), "more is not read");
}

#[test]
fn make_maps_back_to_its_image_offset_in_flash_or_in_the_hot_arena() {
    let h = Header5 {
        hot_at: 2048,
        hot_len: 512,
        ..good()
    };
    let placed = h.base + 0x1_0000;
    let flash = placed + CODE_OFFSET;
    // In the image: its offset, wherever it was linked.
    assert_eq!(h.make_offset(flash + 1030, placed, 0), 1030);
    assert_eq!(h.make_offset(flash + 4095, placed, 0), 4095);
    for outside in [flash - 1, flash + 4096, 0, u32::MAX, placed + 1030] {
        assert_eq!(h.make_offset(outside, placed, 0), u32::MAX, "{outside:#x}");
    }
    // In hot code, copied to the arena: the offset of its place in the image.
    let hot = 0x4038_1000;
    assert_eq!(h.make_offset(hot + 6, placed, hot), 2048 + 6);
    assert_eq!(h.make_offset(hot + 511, placed, hot), 2048 + 511);
    // Past the hot code, the flash address still counts; the arena's does not.
    assert_eq!(h.make_offset(hot + 512, placed, hot), u32::MAX);
    assert_eq!(h.make_offset(flash + 1030, placed, hot), 1030);
    // Without hot code copied, the arena's addresses mean nothing.
    assert_eq!(h.make_offset(hot + 6, placed, 0), u32::MAX);
    // The flash copy of the hot code is patched for the arena: with the hot
    // code copied it is no place for `make`; without, it is the code.
    assert_eq!(h.make_offset(flash + 2048, placed, hot), u32::MAX);
    assert_eq!(h.make_offset(flash + 2048 + 511, placed, hot), u32::MAX);
    assert_eq!(h.make_offset(flash + 2048 + 512, placed, hot), 2048 + 512);
    assert_eq!(h.make_offset(flash + 2048 - 1, placed, hot), 2048 - 1);
    assert_eq!(h.make_offset(flash + 2048 + 6, placed, 0), 2048 + 6);
    // The offset is what check_entry takes.
    let record = record_bytes(h.meta_len as usize);
    let entry = EntryFacts {
        make_at: h.make_offset(hot + 6, placed, hot),
        metadata_ptr: placed + h.meta_at,
        ..entry(&h, &record)
    };
    assert_eq!(
        h.check_entry(placed, &HOST, &RECORD, &record, &entry),
        // Hot code lies in the image's code, so it is a `make`.
        Ok(())
    );
}

/// A file the real packer made (`samples/abi7-x4-min`, `build.sh --update`).
const PACKED: &[u8] = include_bytes!("../../tests/data/abi7-x4-min.ppn");

#[test]
fn the_packer_made_file_reads_and_its_record_is_the_generated_one() {
    let h = read_packed();
    assert_eq!(
        (h.target, h.abi_major, h.needs_minor, h.base),
        (Chip::Esp32C3, 7, 1, 0x3C00_0000)
    );
    assert_eq!((h.entry, h.meta_at, h.meta_len, h.align), (0, 20, 88, 16));
    assert!(!h.signed);
    assert_eq!(PACKED.len() as u64, h.file_len());
    // The record in the image is the generated fixture, byte for byte.
    let at = HEADER_LEN + h.meta_at as usize;
    let record = &PACKED[at..at + h.meta_len as usize];
    assert_eq!(record, x4_record());
    assert_eq!(
        RecordFacts::read(record),
        Some(RecordFacts {
            abi_major: 7,
            needs_minor: 1,
            entry_bytes: X4_ENTRY_BYTES
        })
    );
    // As the loader checks the file's length, against the signed form too.
    assert_eq!(read(&PACKED[..HEADER_LEN], h.file_len()), Ok(h));
    assert_eq!(
        read(&PACKED[..HEADER_LEN], h.file_len() + SIGNATURE_LEN as u64),
        Ok(Header5 { signed: true, ..h })
    );
    assert_eq!(
        read(&PACKED[..HEADER_LEN], h.file_len() + 1),
        Err(Refusal::Length)
    );
    // Another host: a newer minor is no problem, an older one is one.
    assert!(Header5::read(PACKED, PACKED.len() as u64, &Host::x4(2)).is_ok());
    assert_eq!(
        Header5::read(PACKED, PACKED.len() as u64, &Host::x4(0)),
        Err(Refusal::NeedsNewerMinor)
    );
}

fn read_packed() -> Header5 {
    Header5::read(PACKED, PACKED.len() as u64, &HOST).unwrap()
}
