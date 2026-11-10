//! The comic book `core.format` provider against the interface-6 reader on the
//! same archive: the same sections and page blocks, the cover and a page's
//! picture, and the refusals a hostile call gets. The archive is root 3 (named
//! by the call's `text`), the converted pages root 6.
//
//! the refusals a hostile call gets. The book is root 3 (named by the call's
//! `text`), the converted page root 6.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_epub_family::{CbzDocument, cbz_provider};
use paperpaw_sdk::document::{BlockKind, Document, Locator};
use paperpaw_sdk::native::Error;
use paperpaw_sdk::native::stage_schemas::core_format_v1 as stage;
use paperpaw_sdk::{DirEntry, Env, MemoryStorage, Storage, StorageError};
use paperpaw_zip::testing::ZipBuilder;

/// Root 3 (the folder of the book) and root 6 (below the plugin's folder).
struct Roots {
    mem: MemoryStorage,
    prefix: String,
}

impl Roots {
    fn new(name: &str, book: &[u8]) -> Self {
        Self {
            mem: MemoryStorage::new().with(&format!("book/{name}"), book.to_vec()),
            prefix: Env::plugin_dir("format-epub"),
        }
    }
    fn map(&self, path: &str) -> String {
        match path.strip_prefix(&self.prefix) {
            Some(rest) => format!("cache{rest}"),
            None => format!("book/{path}"),
        }
    }
}

impl Storage for Roots {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.mem.read_at(&self.map(path), offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.mem.size(&self.map(path))
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        if !path.starts_with(&self.prefix) {
            return Err(StorageError::Unsupported);
        }
        self.mem.write(&self.map(path), data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        if !path.starts_with(&self.prefix) {
            return Err(StorageError::Unsupported);
        }
        self.mem.append(&self.map(path), data)
    }
    fn list(&self, _: &str, _: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.mem.remove(&self.map(path))
    }
}

fn call(op: u32, f: impl FnOnce(&mut stage::FmtCall<'static>)) -> Vec<u8> {
    let mut c = stage::FmtCall::new(op, 1, 0, 0, 0, 0, 0, 1, 0, 0, 7, "", &[]);
    f(&mut c);
    let mut buf = vec![0u8; 4096];
    let n = stage::input_encode(&[stage::Record::FmtCall(c)], &mut buf).unwrap();
    buf.truncate(n);
    buf
}

fn run(
    p: &mut paperpaw_epub_family::provider::FormatProvider,
    io: &dyn Storage,
    input: &[u8],
) -> Result<Vec<u8>, Error> {
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

const PNG: &[u8] = include_bytes!("../../../crates/paperpaw-image/tests/data/gray.png");

fn comic() -> Vec<u8> {
    ZipBuilder::new()
        .stored("p10.png", PNG.to_vec())
        .stored("p2.png", PNG.to_vec())
        .stored("p1.png", PNG.to_vec())
        .stored("readme.txt", b"not a page".to_vec())
        .build()
}

fn opened() -> (paperpaw_epub_family::provider::FormatProvider, Roots) {
    let io = Roots::new("c.cbz", &comic());
    let mut p = cbz_provider();
    let reply = run(&mut p, &io, &call(2, |c| c.text = "c.cbz")).unwrap();
    let stage::Record::FmtState(s) = rows(&reply)[0] else {
        panic!("state")
    };
    assert_eq!((s.state, s.status), (0, 0), "{s:?}");
    (p, io)
}

#[test]
fn a_comic_reads_like_interface_6_page_by_page() {
    let (mut p, io) = opened();
    let old_io = MemoryStorage::new().with("c.cbz", comic());
    let old = CbzDocument::open(&old_io, "c.cbz").unwrap();
    assert_eq!(old.sections(), 3, "pages in natural order, no readme");
    let reply = run(&mut p, &io, &call(7, |_| {})).unwrap();
    let sections: Vec<_> = rows(&reply)
        .into_iter()
        .filter_map(|r| match r {
            stage::Record::FmtSection(s) => Some((s.index, s.length, s.flags)),
            _ => None,
        })
        .collect();
    assert_eq!(sections.len(), 3);
    for (i, (index, length, whole)) in sections.iter().enumerate() {
        assert_eq!(*index, i as u32);
        assert_eq!(*length, old.section_len(i as u32));
        assert_eq!(*whole, 1, "whole pages");
        let expected = old
            .block_at(&old_io, Locator::new(i as u32, 0))
            .unwrap()
            .unwrap();
        let reply = run(
            &mut p,
            &io,
            &call(8, |c| {
                c.section = i as u32;
                c.offset = 0;
            }),
        )
        .unwrap();
        let kind = rows(&reply)
            .into_iter()
            .find_map(|r| match r {
                stage::Record::FmtBlock(b) => Some(b.kind),
                _ => None,
            })
            .expect("a block row");
        assert_eq!(
            kind == 3,
            matches!(expected.kind, BlockKind::Image),
            "page {i}"
        );
    }
    // The cover is the first page.
    let reply = run(&mut p, &io, &call(17, |_| {})).unwrap();
    assert!(matches!(rows(&reply)[0], stage::Record::FmtImage(_)));
    assert!(matches!(
        run(&mut p, &io, &call(18, |c| c.index = 999)),
        Err(Error::NotFound)
    ));
    // Each page's picture is asked for by its number (its section), with no
    // section on the call: page 2 gives page 2's, not page 1's.
    let mut paths = Vec::new();
    for page in 0..3u32 {
        let block = run(&mut p, &io, &call(8, |c| c.section = page)).unwrap();
        let from_block = rows(&block)
            .into_iter()
            .find_map(|r| match r {
                stage::Record::FmtImage(i) => Some((i.path.to_string(), i.index)),
                _ => None,
            })
            .expect("an image row");
        assert_eq!(from_block.1, page, "the number of page {page}");
        let reply = run(&mut p, &io, &call(18, |c| c.index = page)).unwrap();
        match rows(&reply)[0] {
            stage::Record::FmtImage(i) => {
                assert_eq!((i.path.to_string(), i.index), from_block, "page {page}");
                paths.push(from_block.0);
            }
            other => panic!("not an image: {other:?}"),
        }
    }
    paths.sort();
    paths.dedup();
    assert_eq!(paths.len(), 3, "three different pictures");
}

#[test]
fn bad_calls_get_their_refusals() {
    let (mut p, io) = opened();
    assert!(matches!(run(&mut p, &io, &[7]), Err(Error::Invalid)));
    assert!(matches!(
        run(&mut p, &io, &call(99, |_| {})),
        Err(Error::Unsupported)
    ));
    assert!(matches!(
        run(&mut p, &io, &call(8, |c| c.section = 3)),
        Err(Error::NotFound) | Err(Error::Invalid)
    ));
    // An archive with no page fails the document, not the provider.
    let empty = ZipBuilder::new().stored("a.txt", b"x".to_vec()).build();
    let io2 = Roots::new("e.cbz", &empty);
    let reply = run(
        &mut p,
        &io2,
        &call(2, |c| {
            c.document = 2;
            c.text = "e.cbz";
        }),
    )
    .unwrap();
    let stage::Record::FmtState(s) = rows(&reply)[0] else {
        panic!("state")
    };
    assert_eq!(s.state, 2);
}

#[test]
#[ignore = "waits for Core todo 208 (fallible shared parser): a refused allocation aborts the process while the parser allocates infallibly"]
fn every_allocation_of_a_read_may_be_refused() {
    panic!(
        "acceptance case: refuse each allocation of open, sections and block; NoMemory, never a panic"
    );
}
