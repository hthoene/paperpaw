//! A real target-9 file (the X4 Pro's and X4 Classic's, `abi/ppn5.md`, "Target
//! 9") through the loader's own code: the header, the lists, the write pass and
//! the entry check. `target9-aids.ppn` is `aids` packed by
//! `tools/plugin-files7/build.sh x4-pro`; the tests hold for any such file, so a
//! newer one may replace it (where it came from: `data/target9-aids.source.txt`).
//! The device's flash and PSRAM are the sink here. Nothing on the device itself
//! is shown by them.

#![cfg(feature = "kernel")]

use paperpaw_native::header5::{HEADER_LEN, Header5, Host, Refusal};
use paperpaw_native::stream5::{EntryCheck, Relocator5, Written};
use paperpaw_native::{Chip, Problem};

const FILE: &[u8] = include_bytes!("data/target9-aids.ppn");
/// The newest minor the file can ask for, as the S3 host knows it.
const MINOR: u16 = 255;
/// Where the loader places an image (any multiple of the file's alignment).
const BASE: u32 = 0x3C10_0000;
/// The address of the host's heap table, as the loader tells the plugin (the
/// heap word, `abi/heap-word.md`).
const TABLE: u32 = 0x3FC8_1230;

fn host() -> Host {
    Host::x4_pro(MINOR)
}

fn header_of(file: &[u8]) -> Result<Header5, Refusal> {
    Header5::read(file, file.len() as u64, &host())
}

/// What the loader does with a file: read it, relocate and write the image into
/// `placed`, check the entry. `Err` names where it was refused.
fn load(file: &[u8], placed: &mut Vec<u8>) -> Result<(), String> {
    let header = header_of(file).map_err(|e| format!("header: {e:?}"))?;
    let relocator = Relocator5::new(&header, file, BASE)
        .map_err(|e| format!("lists: {e:?}"))?
        .heap_word(TABLE);
    let mut block = [0u8; 256];
    placed.clear();
    placed.resize(header.image_len as usize, 0);
    relocator
        .write_image(&mut block, |at, piece| -> Result<(), ()> {
            let at = at as usize;
            let end = (at + piece.len()).min(placed.len());
            placed[at..end].copy_from_slice(&piece[..end - at]);
            Ok(())
        })
        .map_err(|e| format!("write: {e:?}"))?;
    let mut scratch = vec![0u8; 2 * header.meta_len as usize];
    relocator
        .check_entry(&host(), &mut scratch)
        .map_err(|e| format!("entry: {e:?}"))
}

#[test]
fn a_real_target_9_file_loads() {
    let header = header_of(FILE).expect("the S3 host reads it");
    assert_eq!(header.target, Chip::Esp32S3);
    assert_eq!(
        (header.hot_len, header.patches),
        (0, 0),
        "target 9: no hot code, no patches"
    );
    assert!(header.relocs > 0, "a real plugin has words to relocate");
    assert_eq!(header.entry % 4, 0);
    assert_eq!(FILE.len() as u64, header.file_len());
    let mut placed = Vec::new();
    assert_eq!(load(FILE, &mut placed), Ok(()));
    assert_eq!(placed.len() as u32, header.image_len);
    // The words were relocated: the image is not the file's bytes.
    assert_ne!(&placed[..], &FILE[HEADER_LEN..HEADER_LEN + placed.len()]);
}

/// The relocation is exactly this: each listed word of the file plus the
/// distance the image moved, the heap word the table's address, and every other
/// byte as the file has it.
#[test]
fn the_listed_words_move_with_the_base_the_heap_word_is_the_table_and_the_rest_is_unchanged() {
    let header = header_of(FILE).unwrap();
    let mut placed = Vec::new();
    assert_eq!(load(FILE, &mut placed), Ok(()));
    let word = |bytes: &[u8], at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    let from = header.relocs_at() as usize;
    let listed: Vec<usize> = (0..header.relocs as usize)
        .map(|i| word(FILE, from + 4 * i) as usize)
        .collect();
    assert_eq!(listed.len(), 125, "the fixture lists 125 words");
    let delta = BASE.wrapping_sub(header.base);
    let image = &FILE[HEADER_LEN..HEADER_LEN + header.image_len as usize];
    let heap = header.heap_at.expect("aids has a heap word") as usize;
    assert!(
        !listed.contains(&heap),
        "the heap word is no relocation word"
    );
    let mut expected = image.to_vec();
    for &at in &listed {
        let moved = word(image, at).wrapping_add(delta);
        expected[at..at + 4].copy_from_slice(&moved.to_le_bytes());
    }
    expected[heap..heap + 4].copy_from_slice(&TABLE.to_le_bytes());
    assert_eq!(
        word(&placed, heap),
        TABLE,
        "the heap word is the table's address"
    );
    for &at in &listed {
        assert_eq!(
            word(&placed, at),
            word(image, at).wrapping_add(delta),
            "word at {at}"
        );
    }
    assert_eq!(placed, expected, "every other byte is the file's");
}

#[test]
fn it_loads_the_same_wherever_it_is_placed_and_moves_with_the_base() {
    let header = header_of(FILE).unwrap();
    let place = |base: u32| {
        let relocator = Relocator5::new(&header, FILE, base).unwrap();
        let mut image = vec![0u8; header.image_len as usize];
        relocator
            .write_image(&mut [0u8; 512], |at, piece| -> Result<(), ()> {
                let at = at as usize;
                let end = (at + piece.len()).min(image.len());
                image[at..end].copy_from_slice(&piece[..end - at]);
                Ok(())
            })
            .unwrap();
        image
    };
    let (a, b) = (place(BASE), place(BASE + 0x1_0000));
    assert_ne!(a, b, "the words follow the base");
    assert_eq!(a, place(BASE), "and the same base gives the same bytes");
    assert_eq!(a.len(), b.len());
}

#[test]
fn the_wrong_chip_is_refused_at_the_header_both_ways() {
    assert_eq!(
        Header5::read(FILE, FILE.len() as u64, &Host::x4(MINOR)),
        Err(Refusal::OtherChip)
    );
    // Target 5's file on the S3 host: the chip byte changed (the digest would
    // refuse it too; the chip comes first).
    let mut c3 = FILE.to_vec();
    c3[6] = 5;
    assert_eq!(header_of(&c3), Err(Refusal::OtherChip));
}

#[test]
fn an_older_host_minor_and_an_other_major_are_refused() {
    let header = header_of(FILE).unwrap();
    if header.needs_minor > 0 {
        assert_eq!(
            Header5::read(
                FILE,
                FILE.len() as u64,
                &Host::x4_pro(header.needs_minor - 1)
            ),
            Err(Refusal::NeedsNewerMinor)
        );
    }
    let mut older = FILE.to_vec();
    older[8] = 6;
    assert_eq!(header_of(&older), Err(Refusal::OlderInterface));
    let mut newer = FILE.to_vec();
    newer[8] = 8;
    assert_eq!(header_of(&newer), Err(Refusal::NewerInterface));
}

#[test]
fn a_file_cut_short_or_with_a_tail_is_refused_without_a_panic() {
    for len in [0, 1, 4, 8, 95, 96, 97, FILE.len() / 2, FILE.len() - 1] {
        let cut = &FILE[..len];
        assert!(
            Header5::read(cut, FILE.len() as u64 - 1, &host()).is_err()
                || Header5::read(cut, cut.len() as u64, &host()).is_err(),
            "{len} bytes were taken"
        );
        let mut placed = Vec::new();
        assert!(load(cut, &mut placed).is_err(), "{len} bytes were loaded");
    }
    let mut longer = FILE.to_vec();
    longer.push(0);
    assert_eq!(header_of(&longer), Err(Refusal::Length));
}

#[test]
fn any_single_changed_byte_is_refused() {
    // Every byte of the header, the entry and its record, and a stride across
    // the rest: the header's rules, the lists' checks or the digest stop each.
    let header = header_of(FILE).unwrap();
    let dense = HEADER_LEN + 4 * 1024;
    let tail = header.relocs_at() as usize;
    let mut placed = Vec::new();
    let mut at = 0;
    while at < FILE.len() {
        let mut changed = FILE.to_vec();
        changed[at] ^= 0x40;
        assert!(
            load(&changed, &mut placed).is_err(),
            "a byte changed at {at} was loaded"
        );
        at += if at < dense || at >= tail { 1 } else { 53 };
    }
}

#[test]
fn a_sink_that_has_no_room_stops_the_write_and_says_so() {
    let header = header_of(FILE).unwrap();
    let relocator = Relocator5::new(&header, FILE, BASE).unwrap();
    for refuse_at in [0u32, 256, header.image_len / 2] {
        let mut calls = 0;
        let result = relocator.write_image(&mut [0u8; 256], |at, _| {
            calls += 1;
            if at >= refuse_at { Err("full") } else { Ok(()) }
        });
        assert_eq!(result, Err(Written::Sink("full")), "refused at {refuse_at}");
        assert!(calls >= 1);
    }
}

#[test]
fn a_placement_that_cannot_hold_the_image_is_refused() {
    let header = header_of(FILE).unwrap();
    // Not aligned as the file asks; and past the end of the address space.
    assert_eq!(
        Relocator5::new(&header, FILE, BASE + 1).err(),
        Some(Problem::Malformed)
    );
    assert_eq!(
        Relocator5::new(&header, FILE, u32::MAX - header.align + 1).err(),
        Some(Problem::Malformed)
    );
}

#[test]
fn the_wrong_profile_in_the_record_is_refused_at_the_entry() {
    // The X4's host reading the same record would call it another target: the
    // check compares the record's target name with the host's profile.
    let header = header_of(FILE).unwrap();
    let relocator = Relocator5::new(&header, FILE, BASE).unwrap();
    let mut scratch = vec![0u8; 2 * header.meta_len as usize];
    let mut c3_like = host();
    c3_like.profile_target = Host::x4(MINOR).profile_target;
    assert!(matches!(
        relocator.check_entry(&c3_like, &mut scratch),
        Err(EntryCheck::File(_) | EntryCheck::Entry(_))
    ));
}
