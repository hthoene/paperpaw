//! The pictures `core.format` provider against the interface-6 reader on the
//! same folders: the same pages in the same order, the same decoded pictures,
//! and the refusals a hostile call or file gets.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_plugin_format_images::provider::{ImagesProvider, MAX_DOCUMENTS};
use paperpaw_plugin_format_images::{CACHE, ImagesDocument};
use paperpaw_sdk::document::{Document, Locator};
use paperpaw_sdk::native::Error;
use paperpaw_sdk::native::stage_schemas::core_format_v1 as stage;
use paperpaw_sdk::{DirEntry, MemoryStorage, OpenOptions, Storage, StorageError};

const PNG: &[u8] = include_bytes!("../../../crates/paperpaw-image/tests/data/gray.png");
const JPG: &[u8] = include_bytes!("../../../crates/paperpaw-image/tests/data/alice-cover.jpg");
const GIF: &[u8] = include_bytes!("../../../crates/paperpaw-image/tests/data/alice-cover.gif");

/// Root 3 (the book's folder) and root 6 (paths that start with `cache:`).
struct Roots(MemoryStorage);

impl Roots {
    fn new(files: &[(&str, &[u8])]) -> Self {
        let mut mem = MemoryStorage::new();
        for (name, data) in files {
            mem = mem.with(&format!("book/{name}"), data.to_vec());
        }
        Self(mem)
    }
    fn map(path: &str) -> String {
        match path.strip_prefix(CACHE) {
            Some(name) => format!("cache/{name}"),
            None => format!("book/{path}"),
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
        if !path.starts_with(CACHE) {
            return Err(StorageError::Unsupported);
        }
        self.0.write(&Self::map(path), data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        if !path.starts_with(CACHE) {
            return Err(StorageError::Unsupported);
        }
        self.0.append(&Self::map(path), data)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.0.list(
            &format!(
                "book{}",
                if dir.is_empty() { "" } else { "/" }.to_owned() + dir
            ),
            visit,
        )
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

fn run(p: &mut ImagesProvider, io: &dyn Storage, input: &[u8]) -> Result<Vec<u8>, Error> {
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

fn open(p: &mut ImagesProvider, io: &dyn Storage, name: &'static str) -> (u32, i32, u32) {
    let reply = run(p, io, &call(2, 1, |c| c.text = name)).unwrap();
    let stage::Record::FmtState(s) = rows(&reply)[0] else {
        panic!("state")
    };
    (s.state, s.status, s.flags)
}

fn folder() -> Roots {
    Roots::new(&[
        ("1.png", PNG),
        ("2.jpg", JPG),
        ("10.gif", GIF),
        ("readme.txt", b"hello"),
    ])
}

#[test]
fn a_folder_reads_like_interface_6_page_by_page() {
    let io = folder();
    let mut p = ImagesProvider::new();
    assert_eq!(open(&mut p, &io, "1.png"), (0, 0, 1));
    let old_io = MemoryStorage::new()
        .with("d/1.png", PNG.to_vec())
        .with("d/2.jpg", JPG.to_vec())
        .with("d/10.gif", GIF.to_vec())
        .with("d/readme.txt", b"hello".to_vec());
    let old = ImagesDocument::open(&old_io, "d/1.png", &OpenOptions::default()).unwrap();
    assert_eq!(old.sections(), 3);
    let reply = run(&mut p, &io, &call(7, 1, |_| {})).unwrap();
    let r = rows(&reply);
    assert_eq!(r.len(), 4);
    assert!(
        matches!(r[3], stage::Record::FmtEnd(e) if e.total == 3 && e.flags == 1 && e.generation == 1)
    );
    for page in 0..3u32 {
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
        assert!(!image.path.contains(':'), "a root 6 path: {}", image.path);
        let (mut a, mut b) = (vec![0u8; 1 << 20], vec![0u8; 1 << 20]);
        let n = io
            .read_at(&format!("{CACHE}{}", image.path), 0, &mut a)
            .unwrap();
        let m = old_io.read_at(&wanted.path, 0, &mut b).unwrap();
        assert_eq!(a[..n], b[..m], "page {page}");
        let reply = run(&mut p, &io, &call(18, 1, |c| c.index = page)).unwrap();
        assert!(matches!(rows(&reply)[0], stage::Record::FmtImage(i) if i.path == image.path));
    }
    let reply = run(&mut p, &io, &call(17, 1, |c| c.cover_side = 120)).unwrap();
    assert!(matches!(rows(&reply)[0], stage::Record::FmtImage(i) if i.index == u32::MAX));
    let reply = run(&mut p, &io, &call(5, 1, |_| {})).unwrap();
    assert!(
        matches!(rows(&reply)[0], stage::Record::FmtMeta(m) if m.title == "1" && m.sections == 3)
    );
}

#[test]
fn a_picture_later_in_the_folder_starts_there() {
    let io = folder();
    let mut p = ImagesProvider::new();
    assert_eq!(open(&mut p, &io, "2.jpg").0, 0);
    let reply = run(&mut p, &io, &call(7, 1, |_| {})).unwrap();
    assert!(
        matches!(rows(&reply)[2], stage::Record::FmtEnd(e) if e.total == 2 && e.flags == 1 && e.generation == 1)
    );
}

#[test]
fn what_is_no_picture_is_named_and_bad_names_fail_the_document() {
    let io = Roots::new(&[("a.png", b"not a picture at all"), ("b.png", PNG)]);
    let mut p = ImagesProvider::new();
    assert_eq!(open(&mut p, &io, "a.png").0, 0);
    let reply = run(&mut p, &io, &call(8, 1, |_| {})).unwrap();
    let r = rows(&reply);
    assert!(matches!(r[0], stage::Record::FmtBlock(b) if b.kind == 0));
    assert!(matches!(r[1], stage::Record::FmtTextPart(t) if t.text == "a.png"));
    for (id, name) in [
        (2, ""),
        (3, "readme.txt"),
        (4, "../a.png"),
        (5, "gone.png"),
        (6, "cache:1.png"),
    ] {
        let reply = run(&mut p, &io, &call(2, id, |c| c.text = name)).unwrap();
        let stage::Record::FmtState(s) = rows(&reply)[0] else {
            panic!("state")
        };
        assert_eq!(s.state, 2, "{name}");
        assert_ne!(s.status, 0);
        assert!(matches!(
            run(&mut p, &io, &call(8, id, |_| {})),
            Err(Error::State)
        ));
    }
}

#[test]
fn refusals() {
    let io = folder();
    let mut p = ImagesProvider::new();
    assert!(matches!(
        run(&mut p, &io, &call(5, 1, |_| {})),
        Err(Error::NotFound)
    ));
    assert!(matches!(run(&mut p, &io, &[9]), Err(Error::Invalid)));
    open(&mut p, &io, "1.png");
    assert!(matches!(
        run(&mut p, &io, &call(2, 1, |c| c.text = "1.png")),
        Err(Error::Invalid)
    ));
    for op in [1, 3, 19, 99] {
        assert!(
            matches!(
                run(&mut p, &io, &call(op, 1, |_| {})),
                Err(Error::Unsupported)
            ),
            "op {op}"
        );
    }
    for op in [10, 11, 12, 13, 15] {
        assert!(
            matches!(run(&mut p, &io, &call(op, 1, |_| {})), Err(Error::NotFound)),
            "op {op}"
        );
    }
    for op in [9, 14, 16] {
        let reply = run(&mut p, &io, &call(op, 1, |_| {})).unwrap();
        assert!(
            matches!(rows(&reply)[..], [stage::Record::FmtEnd(e)] if e.total == 0 && e.flags == 1 && e.generation == 1),
            "op {op}"
        );
    }
    assert!(matches!(
        run(&mut p, &io, &call(8, 1, |c| c.section = 3)),
        Err(Error::NotFound)
    ));
    assert!(matches!(
        run(&mut p, &io, &call(18, 1, |c| c.index = 3)),
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
        run(&mut p, &io, &call(2, id, |c| c.text = "1.png")).unwrap();
    }
    assert!(matches!(
        run(&mut p, &io, &call(2, 99, |c| c.text = "1.png")),
        Err(Error::Full)
    ));
}
