//! The `core.format` provider against the interface-6 reader on the same
//! files: the same blocks, the same texts, runs and maps, whole or in
//! fragments; and the refusals a hostile call or book gets.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_plugin_format_txt::TxtDocument;
use paperpaw_plugin_format_txt::preparation::{Preparation, UTF8_CACHE};
use paperpaw_plugin_format_txt::provider::{Io, MAX_DOCUMENTS, TxtProvider};
use paperpaw_sdk::document::{Block, Document, Locator};
use paperpaw_sdk::native::Error;
use paperpaw_sdk::native::stage_schemas::core_format_v1 as stage;
use paperpaw_sdk::{DirEntry, MemoryStorage, Storage, StorageError};
use std::cell::RefCell;

/// Root 3 (the book) and root 6 (the cache) of one document.
struct Roots {
    book: Vec<u8>,
    cache: RefCell<Option<Vec<u8>>>,
}

impl Roots {
    fn new(book: impl Into<Vec<u8>>) -> Self {
        Self {
            book: book.into(),
            cache: RefCell::new(None),
        }
    }
}

impl Storage for Roots {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        let cache = self.cache.borrow();
        let bytes: &[u8] = match path {
            "" => &self.book,
            UTF8_CACHE => cache.as_deref().ok_or(StorageError::NotFound)?,
            _ => return Err(StorageError::NotFound),
        };
        let from = usize::try_from(offset).unwrap().min(bytes.len());
        let n = buf.len().min(bytes.len() - from).min(1000);
        buf[..n].copy_from_slice(&bytes[from..from + n]);
        Ok(n)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        match path {
            "" => Ok(self.book.len() as u64),
            UTF8_CACHE => self
                .cache
                .borrow()
                .as_ref()
                .map(|c| c.len() as u64)
                .ok_or(StorageError::NotFound),
            _ => Err(StorageError::NotFound),
        }
    }
    fn write(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }
    fn append(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }
    fn list(&self, _: &str, _: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }
    fn remove(&self, _: &str) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }
}

impl Io for Roots {
    fn convert(&self, preparation: &mut Preparation) -> Result<bool, Error> {
        preparation.advance(
            |offset, buf| {
                let n = self.read_at("", offset, buf).map_err(|_| Error::Io)?;
                Ok(n)
            },
            |first, bytes| {
                let mut cache = self.cache.borrow_mut();
                if first {
                    *cache = Some(Vec::new());
                }
                cache.as_mut().ok_or(Error::State)?.extend_from_slice(bytes);
                Ok(())
            },
        )
    }
}

/// What a reply said, owned.
#[derive(Debug, Default)]
struct Blockful {
    header: Option<(u32, u32, u32, u32, u32)>,
    text: String,
    runs: Vec<(u32, u32, u32)>,
    maps: Vec<(u32, u32)>,
    end: Option<(u32, u32, u32)>,
    rows: usize,
}

fn call(op: u32, document: u32, f: impl FnOnce(&mut stage::FmtCall<'static>)) -> Vec<u8> {
    let mut c = stage::FmtCall::new(op, document, 0, 0, 0, 0, 0, 0, 0, 0, 7, "", &[]);
    f(&mut c);
    let mut buf = vec![0u8; 4096];
    let n = stage::input_encode(&[stage::Record::FmtCall(c)], &mut buf).unwrap();
    buf.truncate(n);
    buf
}

fn run(p: &mut TxtProvider, io: &dyn Io, input: &[u8]) -> Result<Vec<u8>, Error> {
    run_in(p, io, input, 3840)
}

/// [`run`] with a reply buffer of `size` bytes.
fn run_in(p: &mut TxtProvider, io: &dyn Io, input: &[u8], size: usize) -> Result<Vec<u8>, Error> {
    let mut out = vec![0u8; size];
    let n = p.call(io, input, &mut out)?;
    out.truncate(n);
    Ok(out)
}

fn state(reply: &[u8]) -> (u32, u32, i32, u32) {
    let mut rows = stage::reply_decode(reply).unwrap();
    let Some(Ok(stage::Record::FmtState(s))) = rows.next() else {
        panic!("no state");
    };
    (s.state, s.progress_ppm, s.status, s.flags)
}

fn fold(reply: &[u8], into: &mut Blockful) {
    for row in stage::reply_decode(reply).unwrap() {
        into.rows += 1;
        match row.unwrap() {
            stage::Record::FmtBlock(b) => {
                into.header = Some((b.kind, b.level, b.start, b.end, b.text_bytes));
            }
            stage::Record::FmtTextPart(t) => into.text.push_str(t.text),
            stage::Record::FmtRun(r) => into.runs.push((r.start, r.end, r.style)),
            stage::Record::FmtMap(m) => into.maps.push((m.text, m.source)),
            stage::Record::FmtEnd(e) => into.end = Some((e.flags, e.generation, e.next)),
            other => panic!("unexpected {other:?}"),
        }
    }
}

/// One block as the provider gives it, joined from its fragments.
fn block(p: &mut TxtProvider, io: &dyn Io, offset: u32) -> Result<(Blockful, usize), Error> {
    block_in(p, io, offset, 3840)
}

/// [`block`] with reply buffers of `size` bytes.
fn block_in(
    p: &mut TxtProvider,
    io: &dyn Io,
    offset: u32,
    size: usize,
) -> Result<(Blockful, usize), Error> {
    let mut whole = Blockful::default();
    let mut fragments = 0;
    let (mut cursor, mut generation) = (0, 0);
    loop {
        let reply = run_in(
            p,
            io,
            &call(8, 1, |c| {
                c.offset = offset;
                c.cursor = cursor;
                c.generation = generation;
            }),
            size,
        )?;
        fold(&reply, &mut whole);
        fragments += 1;
        let (flags, g, next) = whole.end.unwrap();
        if flags & 2 == 0 {
            return Ok((whole, fragments));
        }
        (cursor, generation) = (next, g);
        assert!(fragments < 100);
        whole.end = None;
    }
}

fn same(new: &Blockful, old: &Block) {
    assert_eq!(new.text, old.text);
    assert_eq!(
        new.header.map(|h| (h.2, h.3, h.4)),
        Some((old.start, old.end, old.text.len() as u32))
    );
    let runs: Vec<_> = old
        .runs
        .iter()
        .map(|r| (r.start, r.end, r.style.bits()))
        .collect();
    assert_eq!(new.runs, runs);
    let maps: Vec<_> = old.map.checkpoints().collect();
    assert_eq!(new.maps, maps);
}

fn sample() -> Vec<u8> {
    let mut text = String::from("*** START OF THE PROJECT GUTENBERG EBOOK ***\n\nCHAPTER I\n\n");
    for n in 0..60 {
        text.push_str(&format!(
            "Paragraph {n}: Größe café, some words that wrap around\nthe line.\n\n"
        ));
    }
    text.push_str(&"long ".repeat(1500));
    text.push_str("\n\n*** END OF THE PROJECT GUTENBERG EBOOK ***\nlicence\n");
    text.into_bytes()
}

fn open(p: &mut TxtProvider, io: &dyn Io) -> (u32, u32, i32, u32) {
    state(&run(p, io, &call(2, 1, |_| {})).unwrap())
}

/// Every block of the book, new and old, side by side.
fn walk(book: Vec<u8>) -> usize {
    let io = Roots::new(book.clone());
    let mut p = TxtProvider::new();
    let (mut s, mut ppm, status, _) = open(&mut p, &io);
    assert_eq!(status, 0);
    while s == 1 {
        assert!(ppm <= 1_000_000);
        (s, ppm, _, _) = state(&run(&mut p, &io, &call(3, 1, |_| {})).unwrap());
    }
    assert_eq!(s, 0);
    // The interface-6 reader on the same bytes (a UTF-16 book from its copy).
    let old_io = MemoryStorage::new().with("b.txt", book);
    let old = TxtDocument::open(&old_io, "b.txt").unwrap();
    let mut at = Locator::START;
    let mut count = 0;
    while let Ok(Some(expected)) = old.block_at(&old_io, at) {
        let (got, _) = block(&mut p, &io, at.offset).unwrap();
        same(&got, &expected);
        let (flags, _, _) = got.end.unwrap_or((0, 0, 0));
        let last = old
            .block_at(&old_io, Locator::new(0, expected.end))
            .unwrap()
            .is_none();
        assert_eq!(flags & 1 != 0, last, "last-block bit at {at:?}");
        at = Locator::new(0, expected.end);
        count += 1;
    }
    assert!(matches!(
        block(&mut p, &io, at.offset),
        Err(Error::NotFound)
    ));
    count
}

#[test]
fn the_blocks_of_a_utf_8_book_are_the_interface_6_blocks() {
    assert!(walk(sample()) > 60);
}

#[test]
fn a_windows_1252_book_reads_the_same_way() {
    let mut book = b"Gr\xf6\xdfe caf\xe9 text\n\n".repeat(40);
    book.extend_from_slice(b"end\n");
    assert!(walk(book) > 10);
}

#[test]
fn utf_16_books_are_prepared_then_read_like_their_utf_8_text() {
    let text = String::from_utf8(sample()).unwrap();
    for big in [false, true] {
        let mut book = if big {
            vec![0xfe, 0xff]
        } else {
            vec![0xff, 0xfe]
        };
        for unit in text.encode_utf16() {
            book.extend_from_slice(&if big {
                unit.to_be_bytes()
            } else {
                unit.to_le_bytes()
            });
        }
        let io = Roots::new(book);
        let mut p = TxtProvider::new();
        let (s, _, _, flags) = open(&mut p, &io);
        assert_eq!((s, flags), (1, 0), "preparing first");
        // Reading before it is done is refused.
        assert!(matches!(
            run(&mut p, &io, &call(5, 1, |_| {})),
            Err(Error::Busy)
        ));
        let mut steps = 0;
        let (mut s, mut last) = (1, 0);
        while s == 1 {
            let (next, ppm, _, flags) = state(&run(&mut p, &io, &call(3, 1, |_| {})).unwrap());
            assert!(ppm >= last || next != 1);
            (s, last) = (next, ppm);
            steps += 1;
            assert_eq!(next == 0, flags & 64 != 0);
        }
        assert!(steps > 1);
        let utf8 = Roots::new(text.as_bytes().to_vec());
        let mut q = TxtProvider::new();
        assert_eq!(open(&mut q, &utf8).0, 0);
        for offset in [0, 100, 1000] {
            let a = block(&mut p, &io, offset).unwrap().0;
            let b = block(&mut q, &utf8, offset).unwrap().0;
            assert_eq!(a.text, b.text);
        }
        // Opened again with the cache there: ready without a step.
        let mut again = TxtProvider::new();
        assert_eq!(open(&mut again, &io).0, 0);
    }
}

#[test]
fn a_long_block_comes_in_fragments_that_join_whole() {
    // One 4,000 byte paragraph with many odd bytes: more records than a reply holds.
    let mut book = Vec::new();
    for _ in 0..450 {
        book.extend_from_slice(b"ab\xe4\xf6 ");
    }
    let (got, fragments) = {
        let io = Roots::new(book.clone());
        let mut p = TxtProvider::new();
        open(&mut p, &io);
        block(&mut p, &io, 0).unwrap()
    };
    let old_io = MemoryStorage::new().with("b.txt", book);
    let old = TxtDocument::open(&old_io, "b.txt").unwrap();
    let expected = old.block_at(&old_io, Locator::START).unwrap().unwrap();
    same(&got, &expected);
    assert!(fragments > 1, "{fragments} fragments, {} rows", got.rows);
}

/// A reply buffer of any size from the smallest that holds the block's longest
/// record to the most a reply may be: the fragments join to the same block. One
/// that holds not even the first record is refused, not cut.
#[test]
fn fragments_at_any_cap_join_to_the_same_block() {
    let mut book = Vec::new();
    for _ in 0..450 {
        book.extend_from_slice(b"ab\xe4\xf6 ");
    }
    let io = Roots::new(book);
    let mut p = TxtProvider::new();
    open(&mut p, &io);
    let (whole, fragments) = block(&mut p, &io, 0).unwrap();
    assert!(fragments > 1);
    // The size of the first fragment at the most a reply holds: the cap itself.
    let first = run(&mut p, &io, &call(8, 1, |c| c.offset = 0)).unwrap();
    // From the smallest reply that holds this block's longest record (a text part
    // of about 3 KiB) to one byte short of the most, and exactly the first fragment.
    let sizes = (3200..3840)
        .step_by(32)
        .chain([3839, first.len(), first.len() - 1])
        .filter(|size| *size >= 3200);
    for size in sizes {
        let (got, _) = block_in(&mut p, &io, 0, size).unwrap();
        assert_eq!(got.header, whole.header, "size {size}");
        assert_eq!(got.text, whole.text, "size {size}");
        assert_eq!(got.maps, whole.maps, "size {size}");
        assert_eq!(got.runs, whole.runs, "size {size}");
    }
    // Not even the first record fits: refused, not cut.
    assert!(matches!(block_in(&mut p, &io, 0, 16), Err(Error::NoSpace)));
}

#[test]
fn a_fragment_of_another_block_or_conversion_is_refused() {
    let io = Roots::new("b".repeat(5000).into_bytes());
    let mut p = TxtProvider::new();
    open(&mut p, &io);
    let first = run(&mut p, &io, &call(8, 1, |_| {})).unwrap();
    let mut seen = Blockful::default();
    fold(&first, &mut seen);
    let (flags, generation, next) = seen.end.unwrap();
    if flags & 2 != 0 {
        let wrong = run(
            &mut p,
            &io,
            &call(8, 1, |c| {
                c.cursor = next;
                c.generation = generation ^ 1;
            }),
        );
        assert!(matches!(wrong, Err(Error::State)));
        let past = run(
            &mut p,
            &io,
            &call(8, 1, |c| {
                c.cursor = 9999;
                c.generation = generation;
            }),
        );
        assert!(matches!(past, Err(Error::Invalid)));
    }
}

#[test]
fn metadata_sections_progress_and_texts() {
    let io = Roots::new(
        b"Title: A Tale\nAuthor: Some One\nLanguage: German\n\n*** START OF THE PROJECT GUTENBERG EBOOK ***\nHello there.\n\nBye.\n*** END OF THE PROJECT GUTENBERG EBOOK ***\n".to_vec(),
    );
    let mut p = TxtProvider::new();
    open(&mut p, &io);
    let reply = run(&mut p, &io, &call(5, 1, |_| {})).unwrap();
    let Some(Ok(stage::Record::FmtMeta(m))) = stage::reply_decode(&reply).unwrap().next() else {
        panic!("meta");
    };
    assert_eq!(
        (m.title, m.language, m.sections, m.authors, m.subjects),
        ("A Tale", "de", 1, 1, 0)
    );
    let author = run(
        &mut p,
        &io,
        &call(6, 1, |c| {
            c.flags = 1;
            c.index = 0;
            c.offset = 5;
        }),
    )
    .unwrap();
    let Some(Ok(stage::Record::FmtText(t))) = stage::reply_decode(&author).unwrap().next() else {
        panic!("text");
    };
    assert_eq!((t.kind, t.total, t.text), (1, 8, "One"));
    for (flags, index, error) in [
        (1, 1, Error::NotFound),
        (2, 0, Error::NotFound),
        (9, 0, Error::Invalid),
        (0, 1, Error::NotFound),
    ] {
        let result = run(
            &mut p,
            &io,
            &call(6, 1, |c| {
                c.flags = flags;
                c.index = index;
            }),
        );
        assert_eq!(result.err(), Some(error), "kind {flags} index {index}");
    }
    let sections = run(&mut p, &io, &call(7, 1, |_| {})).unwrap();
    let rows: Vec<_> = stage::reply_decode(&sections)
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert!(matches!(rows[0], stage::Record::FmtSection(s) if s.index == 0 && s.length > 0));
    assert!(matches!(rows[1], stage::Record::FmtEnd(e) if e.total == 1 && e.flags == 0));
    let none = run(&mut p, &io, &call(7, 1, |c| c.cursor = 1)).unwrap();
    assert_eq!(stage::reply_decode(&none).unwrap().len(), 1);
    let mut ppm = |offset| {
        let reply = run(&mut p, &io, &call(19, 1, |c| c.offset = offset)).unwrap();
        let Some(Ok(stage::Record::FmtProgress(p))) = stage::reply_decode(&reply).unwrap().next()
        else {
            panic!("progress");
        };
        p.progress_ppm
    };
    assert_eq!(ppm(0), 0);
    assert_eq!(ppm(u32::MAX), 1_000_000);
}

#[test]
fn bad_calls_get_their_refusals() {
    let io = Roots::new(sample());
    let mut p = TxtProvider::new();
    assert!(matches!(
        run(&mut p, &io, &call(5, 1, |_| {})),
        Err(Error::NotFound)
    ));
    assert!(matches!(run(&mut p, &io, &[1, 2, 3]), Err(Error::Invalid)));
    assert!(matches!(run(&mut p, &io, &[]), Err(Error::Invalid)));
    open(&mut p, &io);
    assert!(matches!(
        run(&mut p, &io, &call(2, 1, |_| {})),
        Err(Error::Invalid)
    ));
    for op in [20, 99] {
        assert!(
            matches!(
                run(&mut p, &io, &call(op, 1, |_| {})),
                Err(Error::Unsupported)
            ),
            "op {op}"
        );
    }
    // Nothing a text has: no labels, XPointers, links to follow, cover or pictures.
    for op in [10, 11, 12, 13, 15, 17, 18] {
        assert!(
            matches!(run(&mut p, &io, &call(op, 1, |_| {})), Err(Error::NotFound)),
            "op {op}"
        );
    }
    // Nothing to list: contents, links, fonts; and not a sniffer.
    for op in [9, 14, 16] {
        let reply = run(&mut p, &io, &call(op, 1, |_| {})).unwrap();
        let rows: Vec<_> = stage::reply_decode(&reply)
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert!(
            matches!(rows[..], [stage::Record::FmtEnd(e)] if e.total == 0 && e.flags == 0),
            "op {op}"
        );
    }
    let none = run(&mut p, &io, &call(1, 0, |_| {})).unwrap();
    assert_eq!(stage::reply_decode(&none).unwrap().len(), 0);
    let mut small = [0u8; 8];
    assert!(matches!(
        p.call(&io, &call(5, 1, |_| {}), &mut small),
        Err(Error::NoSpace)
    ));
    // Closing frees the id, and closing twice is harmless.
    run(&mut p, &io, &call(4, 1, |_| {})).unwrap();
    run(&mut p, &io, &call(4, 1, |_| {})).unwrap();
    assert_eq!(p.documents(), 0);
    for id in 1..=MAX_DOCUMENTS as u32 {
        run(&mut p, &io, &call(2, id, |_| {})).unwrap();
    }
    assert!(matches!(
        run(&mut p, &io, &call(2, 99, |_| {})),
        Err(Error::Full)
    ));
}

#[test]
fn an_empty_or_unreadable_book_fails_the_document_not_the_provider() {
    for book in [Vec::new(), vec![0xff, 0xfe], vec![0xff, 0xfe, 0x41]] {
        let io = Roots::new(book);
        let mut p = TxtProvider::new();
        let (s, _, status, _) = open(&mut p, &io);
        if s == 2 {
            assert_ne!(status, 0);
            assert!(matches!(
                run(&mut p, &io, &call(8, 1, |_| {})),
                Err(Error::State)
            ));
        }
        run(&mut p, &io, &call(4, 1, |_| {})).unwrap();
    }
}

#[test]
fn any_bytes_read_through_the_provider_without_panics() {
    let mut x = 0x2545_f491_4f6c_dd1d_u64;
    for seed in 0..200u64 {
        let mut next = || {
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            x.wrapping_mul(0x2545_f491_4f6c_dd1d)
        };
        let mut data: Vec<u8> = (0..(next() % 6000)).map(|_| next() as u8).collect();
        if seed % 4 == 0 {
            data.splice(0..0, [0xff, 0xfe]);
        }
        let io = Roots::new(data);
        let mut p = TxtProvider::new();
        let (mut s, ..) = open(&mut p, &io);
        let mut guard = 0;
        while s == 1 && guard < 100_000 {
            s = state(&run(&mut p, &io, &call(3, 1, |_| {})).unwrap()).0;
            guard += 1;
        }
        if s != 0 {
            continue;
        }
        let mut offset = 0;
        for _ in 0..10_000 {
            let Ok((got, _)) = block(&mut p, &io, offset) else {
                break;
            };
            let end = got.header.unwrap().3;
            assert!(end > offset);
            if got.end.unwrap().0 & 1 != 0 {
                break;
            }
            offset = end;
        }
    }
}
