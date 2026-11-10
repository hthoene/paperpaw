//! Hostile XTC and XTCH books: headers whose counts and offsets point
//! outside the file, pages cut short or claiming sizes their data does
//! not have, damaged books at random. Opening and reading must fail
//! cleanly or show what can be shown; decoding must stay bounded.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind};

use paperpaw_plugin_format_xtc::XtcDocument;
use paperpaw_sdk::document::{BlockKind, Document, Locator};
use paperpaw_sdk::{DirEntry, MemoryStorage, Storage, StorageError};
use proptest::prelude::*;

const HEADER: usize = 48;
const METADATA: usize = 256;
const CHAPTER: usize = 96;

/// A page: magic, width, height, data.
fn page(magic: &[u8; 4], width: u16, height: u16, data: &[u8]) -> Vec<u8> {
    let mut out = magic.to_vec();
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(&[0, 0]);
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&[0; 8]);
    out.extend_from_slice(data);
    out
}

/// A 1-bit page `width` by `height`, all white.
fn white(width: u16, height: u16) -> Vec<u8> {
    let stride = usize::from(width).div_ceil(8);
    page(
        b"XTG\0",
        width,
        height,
        &vec![0xff; stride * usize::from(height)],
    )
}

/// The offsets of the parts of a book built by [`book`].
struct Parts {
    metadata: usize,
    chapters: usize,
    index: usize,
}

/// An XTC book of `pages` with metadata and a chapter per page.
fn book(pages: &[Vec<u8>]) -> (Vec<u8>, Parts) {
    let count = pages.len();
    let metadata = HEADER;
    let chapters = metadata + METADATA;
    let index = chapters + CHAPTER * count;
    let data = index + 16 * count;
    let mut out = b"XTC\0".to_vec();
    out.extend_from_slice(&0x0100u16.to_le_bytes());
    out.extend_from_slice(&(count as u16).to_le_bytes());
    out.extend_from_slice(&[0, 1, 0, 1]);
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(metadata as u64).to_le_bytes());
    out.extend_from_slice(&(index as u64).to_le_bytes());
    out.extend_from_slice(&(data as u64).to_le_bytes());
    out.extend_from_slice(&0u64.to_le_bytes());
    let mut meta = vec![0u8; METADATA];
    meta[..5].copy_from_slice(b"Title");
    meta[246..248].copy_from_slice(&(count as u16).to_le_bytes());
    out.extend_from_slice(&meta);
    for i in 0..count {
        let mut c = vec![0u8; CHAPTER];
        let name = format!("Chapter {}", i + 1);
        c[..name.len()].copy_from_slice(name.as_bytes());
        c[80..82].copy_from_slice(&(i as u16).to_le_bytes());
        out.extend_from_slice(&c);
    }
    let mut at = data as u64;
    for p in pages {
        out.extend_from_slice(&at.to_le_bytes());
        out.extend_from_slice(&(p.len() as u32).to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        at += p.len() as u64;
    }
    for p in pages {
        out.extend_from_slice(p);
    }
    (
        out,
        Parts {
            metadata,
            chapters,
            index,
        },
    )
}

fn put_u16(data: &mut [u8], at: usize, value: u16) {
    data[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u64(data: &mut [u8], at: usize, value: u64) {
    data[at..at + 8].copy_from_slice(&value.to_le_bytes());
}

/// A storage in memory with the book as root 3 (path `""`) and the cache as
/// root 6 (any other path), that refuses to let a file grow past `limit`
/// bytes, and notes the most any file was asked to hold.
#[derive(Debug)]
struct Limited {
    inner: MemoryStorage,
    limit: u64,
    largest: Cell<u64>,
}

impl Limited {
    fn new(data: Vec<u8>, limit: u64) -> Self {
        Self {
            inner: MemoryStorage::new().with("b.xtc", data),
            limit,
            largest: Cell::new(0),
        }
    }

    /// Where a path of the roots is kept in `inner`.
    fn map(path: &str) -> String {
        if path.is_empty() {
            String::from("b.xtc")
        } else {
            format!("cache/{path}")
        }
    }

    fn note(&self, size: u64) -> Result<(), StorageError> {
        self.largest.set(self.largest.get().max(size));
        if size > self.limit {
            Err(StorageError::Io)
        } else {
            Ok(())
        }
    }
}

impl Storage for Limited {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.inner.read_at(&Self::map(path), offset, buf)
    }

    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.inner.size(&Self::map(path))
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        if path.is_empty() {
            return Err(StorageError::Unsupported);
        }
        self.note(data.len() as u64)?;
        self.inner.write(&Self::map(path), data)
    }

    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        if path.is_empty() {
            return Err(StorageError::Unsupported);
        }
        self.note(self.inner.size(&Self::map(path)).unwrap_or(0) + data.len() as u64)?;
        self.inner.append(&Self::map(path), data)
    }

    fn list(&self, _: &str, _: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }

    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.inner.remove(&Self::map(path))
    }
}

fn open(data: Vec<u8>) -> (Limited, XtcDocument) {
    let storage = Limited::new(data, u64::MAX);
    let doc = XtcDocument::open_native(&storage).unwrap();
    (storage, doc)
}

/// Every page's block, read as a reader does.
fn read_all(doc: &XtcDocument, storage: &dyn Storage) -> Vec<(BlockKind, String)> {
    (0..doc.sections())
        .map(|s| {
            let block = doc.block_at(storage, Locator::new(s, 0)).unwrap().unwrap();
            assert!(doc.block_at(storage, Locator::new(s, 1)).unwrap().is_none());
            (block.kind, block.text)
        })
        .collect()
}

#[test]
fn files_that_are_no_books_are_refused() {
    let (good, _) = book(&[white(8, 8)]);
    let mut no_pages = good.clone();
    put_u16(&mut no_pages, 6, 0);
    let mut magic = good.clone();
    magic[..4].copy_from_slice(b"XTX\0");
    for data in [
        Vec::new(),
        b"XTC\0".to_vec(),
        good[..HEADER - 1].to_vec(),
        no_pages,
        magic,
    ] {
        let storage = Limited::new(data, u64::MAX);
        assert!(XtcDocument::open_native(&storage).is_err());
    }
}

#[test]
fn metadata_and_chapters_pointing_outside_the_file_fail_cleanly() {
    let (good, parts) = book(&[white(8, 8), white(8, 8)]);
    // Metadata past the end: an error, not a panic.
    let mut far = good.clone();
    put_u64(&mut far, 16, u64::MAX - 10);
    let storage = Limited::new(far, u64::MAX);
    assert!(XtcDocument::open_native(&storage).is_err());
    // More chapters than there is room for before the index: as many
    // as fit.
    let mut many = good.clone();
    put_u16(&mut many, parts.metadata + 246, u16::MAX);
    let (storage, doc) = open(many);
    assert_eq!(doc.toc_len(&storage), 2);
    // Chapters starting past the last page are left out; the cover too.
    let mut late = good.clone();
    put_u16(&mut late, parts.chapters + CHAPTER + 80, 7);
    put_u16(&mut late, parts.metadata + 244, 9);
    let (storage, doc) = open(late);
    assert_eq!(doc.toc_len(&storage), 1);
    let cover = doc.cover(&storage).unwrap().unwrap();
    assert_eq!((cover.width, cover.height), (8, 8));
    // No chapters flag, but a count: none.
    let mut none = good;
    none[11] = 0;
    let (storage, doc) = open(none);
    assert_eq!(doc.toc_len(&storage), 0);
}

#[test]
fn pages_outside_the_file_or_damaged_are_named_by_number() {
    let (mut data, parts) = book(&[white(8, 8), white(8, 8), white(8, 8), white(8, 8)]);
    // Page 2 far past the end, page 3 of an unknown kind, page 4 empty.
    put_u64(&mut data, parts.index + 16, 1 << 40);
    let third = u64::from_le_bytes(data[parts.index + 32..parts.index + 40].try_into().unwrap());
    data[third as usize..third as usize + 4].copy_from_slice(b"XXX\0");
    let fourth = u64::from_le_bytes(data[parts.index + 48..parts.index + 56].try_into().unwrap());
    put_u16(&mut data, fourth as usize + 4, 0);
    let (storage, doc) = open(data);
    assert_eq!(
        read_all(&doc, &storage),
        [
            (BlockKind::Image, String::new()),
            (BlockKind::Paragraph, "2".into()),
            (BlockKind::Paragraph, "3".into()),
            (BlockKind::Paragraph, "4".into()),
        ]
    );
    assert_eq!(doc.block_at(&storage, Locator::new(4, 0)).unwrap(), None);
    assert_eq!(
        doc.block_at(&storage, Locator::new(u32::MAX, 0)).unwrap(),
        None
    );
}

#[test]
fn an_index_past_the_end_is_refused() {
    // Without its index no page can be found: the book does not open.
    let (mut data, _) = book(&[white(8, 8), white(8, 8)]);
    put_u64(&mut data, 24, 1 << 40);
    let storage = Limited::new(data, u64::MAX);
    assert!(XtcDocument::open_native(&storage).is_err());
}

#[test]
fn an_index_offset_near_the_largest_number_does_not_panic() {
    let (mut data, _) = book(&[white(8, 8), white(8, 8)]);
    put_u64(&mut data, 24, u64::MAX - 8);
    let storage = Limited::new(data, u64::MAX);
    let opened = catch_unwind(AssertUnwindSafe(|| {
        XtcDocument::open_native(&storage).is_err()
    }));
    assert_eq!(opened.ok(), Some(true));
}

#[test]
fn pages_cut_short_are_named_by_number() {
    // The file ends halfway through the last page: it is not decoded from
    // bits that are not there, it is named by its number like any other
    // damaged page.
    let xtg = white(16, 16);
    // Two planes of 4 columns of 2 bytes.
    let xth = page(b"XTH\0", 4, 9, &[0xff; 16]);
    for (full, cut) in [(&xtg, xtg.len() / 2), (&xth, xth.len() - 2)] {
        let (data, _) = book(&[white(8, 8), full.clone()]);
        let (storage, doc) = open(data);
        assert_eq!(read_all(&doc, &storage)[1].0, BlockKind::Image);
        let (data, _) = book(&[white(8, 8), full[..cut].to_vec()]);
        let (storage, doc) = open(data);
        assert_eq!(
            read_all(&doc, &storage),
            [
                (BlockKind::Image, String::new()),
                (BlockKind::Paragraph, "2".into())
            ]
        );
    }
}

#[test]
fn page_sizes_the_data_cannot_fill_are_refused() {
    // 65535 by 65535 pixels from a few bytes: decoding would write 4 GB
    // and take minutes. The storage refuses files past 64 MB here, so the
    // test ends quickly; it notes how large a file was asked for.
    let (data, _) = book(&[page(b"XTG\0", u16::MAX, u16::MAX, &[0xff; 16])]);
    let storage = Limited::new(data, 64 << 20);
    let doc = XtcDocument::open_native(&storage).unwrap();
    let block = doc.block_at(&storage, Locator::new(0, 0)).unwrap().unwrap();
    assert_eq!(block.kind, BlockKind::Paragraph);
    assert!(
        storage.largest.get() < 1 << 20,
        "{} bytes asked for",
        storage.largest.get()
    );
}

#[test]
fn metadata_text_of_any_bytes_is_read() {
    let (mut data, parts) = book(&[white(8, 8)]);
    // Title without its terminating zero, invalid UTF-8 in the author.
    data[parts.metadata..parts.metadata + 128].fill(b'T');
    data[parts.metadata + 128..parts.metadata + 132].copy_from_slice(&[0xff, 0xfe, b'A', 0]);
    let (_, doc) = open(data);
    assert_eq!(doc.metadata().title.as_deref().map(str::len), Some(128));
    assert_eq!(doc.metadata().authors, ["\u{fffd}\u{fffd}A"]);
}

/// `cases` cases, or as many as `PROPTEST_CASES` says, for longer runs.
fn cases(cases: u32) -> ProptestConfig {
    let cases = std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(cases);
    ProptestConfig::with_cases(cases)
}

proptest! {
    #![proptest_config(cases(128))]

    #[test]
    fn damaged_books_never_panic(
        damage in prop::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 1..10),
        aim in prop::sample::select(&[0usize, 1, 2][..]),
    ) {
        let (mut data, parts) = book(&[white(16, 8), page(b"XTH\0", 8, 8, &[0x5a; 16]), white(8, 3)]);
        // Damage aimed at the header, the index and pages, or anywhere.
        let (from, to) = match aim {
            0 => (0, HEADER),
            1 => (parts.index, parts.index + 48),
            _ => (0, data.len()),
        };
        for (at, byte) in damage {
            let at = from + at.index(to - from);
            data[at] = byte;
        }
        // A page's header may claim any size: past 4 MB a file is
        // refused, which ends its decoding.
        let storage = Limited::new(data, 4 << 20);
        let Ok(doc) = XtcDocument::open_native(&storage) else {
            return Ok(());
        };
        prop_assert!(doc.sections() >= 1);
        let index_overflows = u64::from_le_bytes(
            storage.inner.get("b.xtc").unwrap()[24..32].try_into().unwrap(),
        ) > u64::MAX - (1 << 20);
        if index_overflows {
            // See `an_index_offset_near_the_largest_number_does_not_panic`.
            return Ok(());
        }
        for s in 0..doc.sections().min(8) {
            let _ = doc.block_at(&storage, Locator::new(s, 0));
        }
        for i in 0..doc.toc_len(&storage) {
            let entry = doc.toc_entry(&storage, i).unwrap();
            prop_assert!(entry.at.section < doc.sections());
        }
        let _ = doc.cover(&storage);
    }
}
