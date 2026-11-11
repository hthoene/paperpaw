//! The XTC `core.format` provider against the interface-6 reader on the same
//! books: the same pages, chapters and pictures, and the refusals a hostile
//! call or book gets.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_plugin_format_xtc::XtcDocument;
use paperpaw_plugin_format_xtc::provider::{MAX_DOCUMENTS, XtcProvider};
use paperpaw_sdk::document::{Document, Locator};
use paperpaw_sdk::native::Error;
use paperpaw_sdk::native::stage_schemas::core_format_v1 as stage;
use paperpaw_sdk::{DirEntry, MemoryStorage, Storage, StorageError};

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
#[allow(dead_code, reason = "the parts a test of damage would patch")]
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

/// Root 3 (the book, path `""`) and root 6 (any other path).
struct Roots(MemoryStorage);

impl Roots {
    fn new(book: Vec<u8>) -> Self {
        Self(MemoryStorage::new().with("book", book))
    }
    fn map(path: &str) -> String {
        if path.is_empty() {
            "book".into()
        } else {
            format!("cache/{path}")
        }
    }
}

impl Storage for Roots {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.0.read_at(&Self::map(path), offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.0.size(&Self::map(path))
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        if path.is_empty() {
            return Err(StorageError::Unsupported);
        }
        self.0.write(&Self::map(path), data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        if path.is_empty() {
            return Err(StorageError::Unsupported);
        }
        self.0.append(&Self::map(path), data)
    }
    fn list(&self, _: &str, _: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.0.remove(&Self::map(path))
    }
}

fn call(op: u32, document: u32, f: impl FnOnce(&mut stage::FmtCall<'static>)) -> Vec<u8> {
    let mut c = stage::FmtCall::new(op, document, 0, 0, 0, 0, 0, 0, 0, 0, 7, "", &[]);
    f(&mut c);
    let mut buf = vec![0u8; 4096];
    let n = stage::input_encode(&[stage::Record::FmtCall(c)], &mut buf).unwrap();
    buf.truncate(n);
    buf
}

fn run(p: &mut XtcProvider, io: &dyn Storage, input: &[u8]) -> Result<Vec<u8>, Error> {
    let mut out = vec![0u8; 3840];
    let n = p.call(io, input, &mut out)?;
    out.truncate(n);
    Ok(out)
}

fn rows(reply: &[u8]) -> Vec<stage::Record<'_>> {
    stage::reply_decode(reply)
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn pages() -> Vec<Vec<u8>> {
    (0..5).map(|n| white(16 + n, 16)).collect()
}

fn open(p: &mut XtcProvider, io: &dyn Storage) -> (u32, i32, u32) {
    let reply = run(p, io, &call(2, 1, |_| {})).unwrap();
    let stage::Record::FmtState(s) = rows(&reply)[0] else {
        panic!("state")
    };
    (s.state, s.status, s.flags)
}

/// Whether `end` closes the last page of a list, after checking what `document.md`
/// section 3 asks of a list end: bit 0 the only flag, and a generation that is never 0.
fn last_page(end: &stage::FmtEnd) -> bool {
    assert_eq!(end.generation, 1, "never 0");
    assert_eq!(end.flags & !1, 0, "bit 0 is the only flag of a list");
    end.flags & 1 == 1
}

#[test]
fn a_book_opens_with_whole_pages_and_chapters_and_reads_like_interface_6() {
    let (data, _) = book(&pages());
    let io = Roots::new(data.clone());
    let mut p = XtcProvider::new();
    assert_eq!(open(&mut p, &io), (0, 0, 3));
    let old_io = MemoryStorage::new().with("b.xtc", data);
    let old = XtcDocument::open(&old_io, "b.xtc").unwrap();
    // Sections.
    let reply = run(&mut p, &io, &call(7, 1, |_| {})).unwrap();
    let r = rows(&reply);
    assert_eq!(r.len(), 6);
    assert!(
        matches!(r[5], stage::Record::FmtEnd(e) if e.total == 5 && e.flags == 1 && e.generation == 1)
    );
    // Blocks and pictures, page by page, equal in size and in pixels.
    for page in 0..5u32 {
        let expected = old
            .block_at(&old_io, Locator::new(page, 0))
            .unwrap()
            .unwrap();
        let reply = run(&mut p, &io, &call(8, 1, |c| c.section = page)).unwrap();
        let r = rows(&reply);
        let stage::Record::FmtImage(image) = r[1] else {
            panic!("image row {r:?}")
        };
        let wanted = expected.image.unwrap();
        assert_eq!(
            (image.width, image.height, image.index),
            (wanted.width, wanted.height, page)
        );
        let (mut a, mut b) = (vec![0u8; 100_000], vec![0u8; 100_000]);
        let n = io.read_at(image.path, 0, &mut a).unwrap();
        let m = old_io.read_at(&wanted.path, 0, &mut b).unwrap();
        assert_eq!(a[..n], b[..m]);
        assert!(matches!(r[2], stage::Record::FmtEnd(e) if e.flags == 1));
        // Op 18 names the same picture.
        let reply = run(&mut p, &io, &call(18, 1, |c| c.index = page)).unwrap();
        assert!(matches!(rows(&reply)[0], stage::Record::FmtImage(i) if i.path == image.path));
    }
    // The cover, the chapters and where they go.
    let reply = run(&mut p, &io, &call(17, 1, |_| {})).unwrap();
    assert!(
        matches!(rows(&reply)[0], stage::Record::FmtImage(i) if i.index == u32::MAX && i.width == 16)
    );
    let reply = run(&mut p, &io, &call(9, 1, |_| {})).unwrap();
    let r = rows(&reply);
    assert_eq!(r.len(), 6);
    assert!(matches!(r[2], stage::Record::FmtToc(t) if t.title == "Chapter 3" && t.section == 2));
    let reply = run(&mut p, &io, &call(10, 1, |c| c.index = 3)).unwrap();
    assert!(matches!(rows(&reply)[0], stage::Record::FmtAt(a) if a.section == 3));
    assert!(matches!(
        run(&mut p, &io, &call(10, 1, |c| c.index = 9)),
        Err(Error::NotFound)
    ));
    // The metadata.
    let reply = run(&mut p, &io, &call(5, 1, |_| {})).unwrap();
    assert!(
        matches!(rows(&reply)[0], stage::Record::FmtMeta(m) if m.title == "Title" && m.sections == 5 && m.authors == 0)
    );
    let reply = run(
        &mut p,
        &io,
        &call(6, 1, |c| {
            c.flags = 0;
        }),
    )
    .unwrap();
    assert!(matches!(rows(&reply)[0], stage::Record::FmtText(t) if t.text == "Title"));
}

#[test]
fn long_lists_come_in_pages() {
    let many: Vec<Vec<u8>> = (0..150).map(|_| white(8, 8)).collect();
    let (data, _) = book(&many);
    let io = Roots::new(data);
    let mut p = XtcProvider::new();
    open(&mut p, &io);
    for (op, rows_per_page) in [(7u32, 63usize), (9, 0)] {
        let (mut cursor, mut seen) = (0, 0);
        loop {
            let reply = run(&mut p, &io, &call(op, 1, |c| c.cursor = cursor)).unwrap();
            let r = rows(&reply);
            let stage::Record::FmtEnd(end) = r[r.len() - 1] else {
                panic!("end")
            };
            seen += r.len() - 1;
            assert!(rows_per_page == 0 || r.len() - 1 <= rows_per_page);
            assert_eq!(end.total, 150);
            if last_page(&end) {
                break;
            }
            assert!(end.next > cursor);
            cursor = end.next;
        }
        assert_eq!(seen, 150, "op {op}");
    }
}

#[test]
fn sniffing_and_refusals() {
    let io = Roots::new(book(&pages()).0);
    let mut p = XtcProvider::new();
    let sniff = |arg: &'static [u8]| {
        let mut c = stage::FmtCall::new(1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, "", arg);
        let mut buf = vec![0u8; 4096];
        let n = stage::input_encode(
            &[stage::Record::FmtCall(std::mem::replace(
                &mut c,
                stage::FmtCall::new(1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, "", arg),
            ))],
            &mut buf,
        )
        .unwrap();
        buf.truncate(n);
        buf
    };
    assert_eq!(
        rows(&run(&mut p, &io, &sniff(b"XTCH....")).unwrap()).len(),
        1
    );
    assert_eq!(rows(&run(&mut p, &io, &sniff(b"XTC\0")).unwrap()).len(), 1);
    assert_eq!(
        rows(&run(&mut p, &io, &sniff(b"PK\x03\x04")).unwrap()).len(),
        0
    );
    assert!(matches!(
        run(&mut p, &io, &call(5, 1, |_| {})),
        Err(Error::NotFound)
    ));
    assert!(matches!(run(&mut p, &io, &[9]), Err(Error::Invalid)));
    open(&mut p, &io);
    assert!(matches!(
        run(&mut p, &io, &call(2, 1, |_| {})),
        Err(Error::Invalid)
    ));
    for op in [3, 19, 99] {
        assert!(
            matches!(
                run(&mut p, &io, &call(op, 1, |_| {})),
                Err(Error::Unsupported)
            ),
            "op {op}"
        );
    }
    for op in [11, 12, 13, 15] {
        assert!(
            matches!(run(&mut p, &io, &call(op, 1, |_| {})), Err(Error::NotFound)),
            "op {op}"
        );
    }
    for op in [14, 16] {
        let reply = run(&mut p, &io, &call(op, 1, |_| {})).unwrap();
        assert!(
            matches!(rows(&reply)[..], [stage::Record::FmtEnd(e)] if e.total == 0 && e.flags == 1 && e.generation == 1),
            "op {op}"
        );
    }
    assert!(matches!(
        run(&mut p, &io, &call(8, 1, |c| c.section = 5)),
        Err(Error::NotFound)
    ));
    assert!(matches!(
        run(&mut p, &io, &call(18, 1, |c| c.index = 5)),
        Err(Error::NotFound)
    ));
    assert!(matches!(
        run(&mut p, &io, &call(8, 1, |c| c.cursor = 1)),
        Err(Error::Invalid)
    ));
    let mut small = [0u8; 8];
    assert!(matches!(
        p.call(&io, &call(5, 1, |_| {}), &mut small),
        Err(Error::NoSpace)
    ));
    run(&mut p, &io, &call(4, 1, |_| {})).unwrap();
    run(&mut p, &io, &call(4, 1, |_| {})).unwrap();
    for id in 1..=MAX_DOCUMENTS as u32 {
        run(&mut p, &io, &call(2, id, |_| {})).unwrap();
    }
    assert!(matches!(
        run(&mut p, &io, &call(2, 99, |_| {})),
        Err(Error::Full)
    ));
}

#[test]
fn a_damaged_book_fails_the_document_and_a_damaged_page_is_named() {
    for bad in [Vec::new(), b"XTC\0".to_vec(), vec![7u8; 400]] {
        let io = Roots::new(bad);
        let mut p = XtcProvider::new();
        let (state, status, _) = open(&mut p, &io);
        assert_eq!(state, 2);
        assert_ne!(status, 0);
        assert!(matches!(
            run(&mut p, &io, &call(5, 1, |_| {})),
            Err(Error::State)
        ));
        run(&mut p, &io, &call(4, 1, |_| {})).unwrap();
    }
    // A page that is damaged is shown as its number, not as an error.
    let mut cut = pages();
    cut[1][..4].copy_from_slice(b"ZZZZ");
    let io = Roots::new(book(&cut).0);
    let mut p = XtcProvider::new();
    assert_eq!(open(&mut p, &io).0, 0);
    let reply = run(&mut p, &io, &call(8, 1, |c| c.section = 1)).unwrap();
    let r = rows(&reply);
    assert!(
        matches!(r[0], stage::Record::FmtBlock(b) if b.kind == 0 && b.text_bytes == 1),
        "{r:?}"
    );
    assert!(matches!(r[1], stage::Record::FmtTextPart(t) if t.text == "2"));
}
