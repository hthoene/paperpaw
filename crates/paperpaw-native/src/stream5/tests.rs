//! `Relocator5` and `Tail` against the format-4 `Relocator` and
//! `check_relocations`, which they must agree with, and against a counting
//! source that shows nothing is read in proportion to the file.

use alloc::vec::Vec;
use core::cell::Cell;

use super::*;
use crate::header::tests::chip_header;
use crate::header::{Chip, Header, LINK_BASE};
use crate::header5::HEADER_LEN as HEADER5_LEN;
use crate::reloc::tests::hot_image;
use crate::{Relocator, check_relocations};

/// The format-5 header with the shape of a format-4 one.
fn as_five(h: &Header) -> Header5 {
    Header5 {
        target: Chip::Esp32C3,
        abi_major: 7,
        needs_minor: 1,
        base: h.base,
        image_len: h.image_len,
        code_at: h.code_at,
        entry: 0,
        meta_at: 0,
        meta_len: 0,
        relocs: h.relocs,
        align: h.align,
        hot_at: h.hot_at,
        hot_len: h.hot_len,
        patches: h.patches,
        sha256: [0; 32],
        signed: false,
    }
}

/// A format-5 file body: the header's room, the image, the words.
fn file(image: &[u8], words: &[u32]) -> Vec<u8> {
    let mut file = alloc::vec![0u8; HEADER5_LEN];
    file.extend_from_slice(image);
    for word in words {
        file.extend_from_slice(&word.to_le_bytes());
    }
    file
}

/// The image relocated by the format-4 relocator in pieces of `piece`.
fn oracle(h: &Header, image: &[u8], words: &[u32], base: u32, hot: u32, piece: usize) -> Vec<u8> {
    let mut image = image.to_vec();
    let mut relocator = Relocator::new(h, words, base, 0).unwrap();
    if hot != 0 {
        relocator = relocator.hot_at(hot).unwrap();
    }
    for (i, chunk) in image.chunks_mut(piece).enumerate() {
        relocator.apply((i * piece) as u32, chunk).unwrap();
    }
    image
}

fn relocated(h: &Header5, file: &[u8], image: &[u8], base: u32, hot: u32, piece: usize) -> Vec<u8> {
    let mut image = image.to_vec();
    let mut relocator = Relocator5::new(h, file, base).unwrap();
    if hot != 0 {
        relocator = relocator.hot_at(hot).unwrap();
    }
    for (i, chunk) in image.chunks_mut(piece).enumerate() {
        relocator.apply((i * piece) as u32, chunk).unwrap();
    }
    image
}

#[test]
fn words_and_patches_come_out_as_the_slice_relocator_has_them_in_any_pieces() {
    let (h4, image, words) = hot_image();
    let h5 = as_five(&h4);
    let file = file(&image, &words);
    for base in [LINK_BASE, 0x3C81_0000, 0x3C80_0040] {
        for hot in [0, 0x4039_0100] {
            for piece in [4, 8, 12, 20, 64, 128, 160] {
                let expected = oracle(&h4, &image, &words, base, hot, piece);
                let got = relocated(&h5, &file, &image, base, hot, piece);
                assert_eq!(got, expected, "base {base:#x}, hot {hot:#x}, piece {piece}");
            }
        }
    }
}

#[test]
fn the_check_agrees_with_the_slice_check_on_good_and_bad_words() {
    let (h4, image, words) = hot_image();
    let h5 = as_five(&h4);
    let mut cases = alloc::vec![words.clone()];
    // The same breakages as the slice check's own tests.
    for (i, record) in [
        (2, PatchKind::Hi20.record(128)),
        (1, PatchKind::Hi20.record(92)),
        (1, PatchKind::Hi20.record(129)),
        (2, PatchKind::Hi20.record(158)),
        (1, 4 << 28 | 128),
        (1, PatchKind::Hi20.record(PatchKind::SITES + 128)),
    ] {
        let mut changed = words.clone();
        changed[1 + 2 * i] = record;
        cases.push(changed);
    }
    let mut no_auipc = words.clone();
    no_auipc[1 + 2 * 3 + 1] = 98 + 2;
    cases.push(no_auipc);
    let mut in_hot = words.clone();
    in_hot[0] = 128 + 4;
    cases.push(in_hot);
    for case in cases {
        let file = file(&image, &case);
        let expected = check_relocations(&h4, &case);
        assert_eq!(
            Tail::new(&h5, file.as_slice()).check(&h5),
            expected,
            "{case:?}"
        );
    }
    // Words to relocate, on an image of 128 bytes with two.
    let h4 = chip_header(Chip::Esp32S3, 128, 2);
    let h5 = as_five(&h4);
    let image = alloc::vec![0u8; 128];
    for words in [[72, 68], [68, 68], [69, 72], [68, 128], [68, 126], [68, 72]] {
        let file = file(&image, &words);
        let expected = check_relocations(&h4, &words);
        assert_eq!(
            Tail::new(&h5, file.as_slice()).check(&h5),
            expected,
            "{words:?}"
        );
    }
}

#[test]
fn a_file_cut_short_is_damaged_never_a_panic() {
    let (h4, image, words) = hot_image();
    let h5 = as_five(&h4);
    let whole = file(&image, &words);
    for len in [0, 10, HEADER5_LEN, whole.len() - 4, whole.len() - 1] {
        let cut = &whole[..len];
        assert_eq!(
            Tail::new(&h5, cut).check(&h5),
            Err(Problem::Damaged),
            "{len}"
        );
        assert!(Relocator5::new(&h5, cut, LINK_BASE).is_err());
    }
}

#[test]
fn more_patches_than_the_cap_and_a_place_not_aligned_are_refused() {
    let (h4, image, words) = hot_image();
    let h5 = Header5 {
        patches: MAX_PATCHES + 1,
        ..as_five(&h4)
    };
    let file = file(&image, &words);
    assert_eq!(
        Tail::new(&h5, file.as_slice()).check(&h5),
        Err(Problem::Relocation)
    );
    let h5 = as_five(&h4);
    assert_eq!(
        Relocator5::new(&h5, file.as_slice(), LINK_BASE + 8).unwrap_err(),
        Problem::Malformed
    );
    assert_eq!(
        Relocator5::new(&h5, file.as_slice(), u32::MAX - 16).unwrap_err(),
        Problem::Malformed
    );
}

#[test]
fn a_word_with_no_address_or_in_moved_hot_code_is_refused_when_applied() {
    let (h4, image, words) = hot_image();
    let h5 = as_five(&h4);
    let file = file(&image, &words);
    let mut bad = image.clone();
    bad[68..72].copy_from_slice(&0x4000_0000u32.to_le_bytes());
    let relocator = Relocator5::new(&h5, file.as_slice(), LINK_BASE).unwrap();
    assert_eq!(relocator.apply(0, &mut bad), Err(Problem::Relocation));
    // An odd offset.
    assert_eq!(relocator.apply(2, &mut bad[2..]), Err(Problem::Relocation));
    // A data address in hot code that moves.
    let mut into_hot = alloc::vec![0u8; 160];
    into_hot[68..72].copy_from_slice(&(LINK_BASE + 128).to_le_bytes());
    assert!(relocator.apply(0, &mut into_hot.clone()).is_ok());
    let moved = Relocator5::new(&h5, file.as_slice(), LINK_BASE)
        .unwrap()
        .hot_at(0x4039_0000)
        .unwrap();
    assert_eq!(moved.apply(0, &mut into_hot), Err(Problem::Relocation));
}

/// Counts what is read.
struct Counting<'a> {
    bytes: &'a [u8],
    reads: Cell<usize>,
    largest: Cell<usize>,
}

impl Source for Counting<'_> {
    fn read_at(&self, at: u64, buf: &mut [u8]) -> Result<(), ReadError> {
        self.reads.set(self.reads.get() + 1);
        self.largest.set(self.largest.get().max(buf.len()));
        self.bytes.read_at(at, buf)
    }
}

#[test]
fn a_piece_reads_only_the_words_it_needs_in_a_fixed_buffer() {
    // 4 KiB image with a word to relocate every 8 bytes from 128 on.
    let len = 4096u32;
    let count = (len - 128) / 8;
    let at = |i: u32| 128 + 8 * i;
    let mut image = alloc::vec![0u8; len as usize];
    let words: Vec<u32> = (0..count).map(at).collect();
    for &w in &words {
        image[w as usize..w as usize + 4].copy_from_slice(&(LINK_BASE + w).to_le_bytes());
    }
    let h4 = chip_header(Chip::Esp32C3, len, count);
    let h5 = as_five(&h4);
    let file = file(&image, &words);
    let base = 0x3C81_0000;
    // The whole image, a 512-byte piece at a time, as the oracle has it.
    assert_eq!(
        relocated(&h5, &file, &image, base, 0, 512),
        oracle(&h4, &image, &words, base, 0, 512)
    );
    // One piece in the middle: a binary search and a short run, never the
    // 500 words.
    let counting = Counting {
        bytes: &file,
        reads: Cell::new(0),
        largest: Cell::new(0),
    };
    let relocator = Relocator5::new(&h5, &counting, base).unwrap();
    let after_new = counting.reads.get();
    assert!(
        after_new <= (count as usize).div_ceil(32) + 4,
        "new reads the words once: {after_new}"
    );
    counting.reads.set(0);
    let mut piece = image[2048..2048 + 512].to_vec();
    relocator.apply(2048, &mut piece).unwrap();
    assert!(
        counting.reads.get() <= 12 + 2 + 512 / 8 / 32,
        "a piece reads {} times for {count} words",
        counting.reads.get()
    );
    assert!(counting.largest.get() <= 128, "a fixed buffer");
}

/// The header sealed over its file as the packer does: the digest of the
/// header's first 64 bytes and everything after the header.
fn sealed(mut header: Header5, file: &[u8]) -> Header5 {
    let mut sha = header.digest();
    sha.update(&file[HEADER5_LEN..]);
    header.sha256 = sha.finish();
    header
}

fn verified(h: &Header5, file: &[u8], base: u32, hot: u32, buf: usize) -> Result<(), Problem> {
    let mut relocator = Relocator5::new(h, file, base)?;
    if hot != 0 {
        relocator = relocator.hot_at(hot)?;
    }
    relocator.verify(&mut alloc::vec![0u8; buf])
}

#[test]
fn a_file_is_verified_whole_in_any_buffer_and_changed_in_no_byte() {
    let (h4, image, words) = hot_image();
    let file = file(&image, &words);
    let h5 = sealed(as_five(&h4), &file);
    for base in [LINK_BASE, 0x3C81_0000] {
        for hot in [0, 0x4039_0100] {
            for buf in [4, 12, 64, 100, 4096] {
                assert_eq!(verified(&h5, &file, base, hot, buf), Ok(()), "{buf}");
            }
        }
    }
    // Any byte after the header changed: the digest refuses it, or the
    // relocation does where the byte is one it reads.
    for at in HEADER5_LEN..file.len() {
        let mut changed = file.clone();
        changed[at] ^= 0x01;
        let got = verified(&h5, &changed, LINK_BASE, 0, 64);
        assert!(
            matches!(got, Err(Problem::Damaged | Problem::Relocation)),
            "byte {at}: {got:?}"
        );
    }
    // A header changed after sealing: its other digest.
    let other = Header5 {
        needs_minor: 2,
        ..h5
    };
    assert_eq!(
        verified(&other, &file, LINK_BASE, 0, 64),
        Err(Problem::Damaged)
    );
    // A buffer that holds no word.
    assert_eq!(
        verified(&h5, &file, LINK_BASE, 0, 3),
        Err(Problem::Malformed)
    );
    // A file shorter than its header says: a read that fails.
    assert_eq!(
        verified(&h5, &file[..file.len() - 1], LINK_BASE, 0, 64),
        Err(Problem::Damaged)
    );
}

#[test]
fn verifying_reads_the_file_once_in_a_fixed_buffer() {
    let (h4, image, words) = hot_image();
    let file = file(&image, &words);
    let h5 = sealed(as_five(&h4), &file);
    let counting = Counting {
        bytes: file.as_slice(),
        reads: Cell::new(0),
        largest: Cell::new(0),
    };
    let relocator = Relocator5::new(&h5, &counting, LINK_BASE).unwrap();
    counting.largest.set(0);
    relocator.verify(&mut [0u8; 64]).unwrap();
    assert!(counting.largest.get() <= 64, "no read past the buffer");
}
