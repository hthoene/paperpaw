//! The pictures `core.format` provider on folders: the pages of a folder in
//! order from the picture opened, pictures decoded on their pages (the sizes and
//! bytes pinned from what the interface-6 reader made of the same files), and
//! what is done with files that are damaged, too large, or no pictures at all,
//! and the refusals a hostile call gets.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_plugin_format_images::CACHE;
use paperpaw_plugin_format_images::provider::{ImagesProvider, MAX_DOCUMENTS};
use paperpaw_sdk::native::Error;
use paperpaw_sdk::native::stage_schemas::core_format_v1 as stage;
use paperpaw_sdk::{DirEntry, MemoryStorage, Storage, StorageError};
use proptest::prelude::*;

/// The width, height, stored length and FNV-1a of each page of `folder()`.
const PINNED_PAGES: [(u32, u32, usize, u64); 3] = [
    (37, 23, 863, 5845503771251972455),
    (800, 1104, 883_212, 6540526257898697348),
    (160, 221, 35_372, 7765588397706990322),
];

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

/// A page as the host reads it: the block's kind, its text, its picture's size.
fn shown(p: &mut ImagesProvider, io: &dyn Storage, page: u32) -> (u32, String, Option<(u32, u32)>) {
    let reply = run(p, io, &call(8, 1, |c| c.section = page)).unwrap();
    let r = rows(&reply);
    let stage::Record::FmtBlock(block) = r[0] else {
        panic!("block row {r:?}")
    };
    let mut text = String::new();
    let mut size = None;
    for row in &r {
        match row {
            stage::Record::FmtTextPart(t) => text.push_str(t.text),
            stage::Record::FmtImage(i) => size = Some((i.width, i.height)),
            _ => {}
        }
    }
    (block.kind, text, size)
}

/// The names of the pages (the pictures that are none are shown as their names).
fn named(p: &mut ImagesProvider, io: &dyn Storage, total: u32) -> Vec<String> {
    (0..total).map(|i| shown(p, io, i).1).collect()
}

fn total(p: &mut ImagesProvider, io: &dyn Storage) -> u32 {
    let reply = run(p, io, &call(7, 1, |_| {})).unwrap();
    let r = rows(&reply);
    let Some(stage::Record::FmtEnd(end)) = r.last() else {
        panic!("end row {r:?}")
    };
    end.total
}

fn cache_files(io: &Roots, suffix: &str) -> usize {
    let mut count = 0;
    io.0.list("cache", &mut |entry| {
        count += usize::from(entry.name.ends_with(suffix));
    })
    .unwrap();
    count
}

#[test]
fn a_folder_reads_page_by_page() {
    let io = folder();
    let mut p = ImagesProvider::new();
    assert_eq!(open(&mut p, &io, "1.png"), (0, 0, 1));
    let reply = run(&mut p, &io, &call(7, 1, |_| {})).unwrap();
    let r = rows(&reply);
    assert_eq!(r.len(), 4);
    assert!(matches!(r[3], stage::Record::FmtEnd(e) if e.total == 3 && e.flags == 0));
    // The sizes and the decoded bytes (a gray file: FNV-1a of what is stored)
    // the interface-6 reader made of these files.
    for (page, (width, height, length, digest)) in PINNED_PAGES.into_iter().enumerate() {
        let page = page as u32;
        let reply = run(&mut p, &io, &call(8, 1, |c| c.section = page)).unwrap();
        let r = rows(&reply);
        let stage::Record::FmtImage(image) = r[1] else {
            panic!("image row {r:?}")
        };
        assert_eq!(
            (image.width, image.height, image.index),
            (width, height, page)
        );
        assert!(!image.path.contains(':'), "a root 6 path: {}", image.path);
        let mut stored = vec![0u8; 1 << 20];
        let n = io
            .read_at(&format!("{CACHE}{}", image.path), 0, &mut stored)
            .unwrap();
        assert_eq!(
            (n, paperpaw_sdk::digest::fnv1a64(&stored[..n])),
            (length, digest),
            "page {page}"
        );
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
fn a_picture_is_a_book_of_one_page() {
    let io = Roots::new(&[("a b.png", PNG)]);
    let mut p = ImagesProvider::new();
    assert_eq!(open(&mut p, &io, "a b.png"), (0, 0, 1));
    let reply = run(&mut p, &io, &call(5, 1, |_| {})).unwrap();
    assert!(
        matches!(rows(&reply)[0], stage::Record::FmtMeta(m) if m.title == "a b" && m.sections == 1)
    );
    assert_eq!(total(&mut p, &io), 1);
    let (kind, text, size) = shown(&mut p, &io, 0);
    let (width, height) = size.expect("a picture");
    assert_eq!((kind, text.as_str()), (3, ""));
    assert!(width > 0 && height > 0);
    for (section, offset) in [(1, 0), (0, 1)] {
        let found = run(
            &mut p,
            &io,
            &call(8, 1, |c| (c.section, c.offset) = (section, offset)),
        );
        assert!(matches!(found, Err(Error::NotFound)), "{section} {offset}");
    }
    let reply = run(&mut p, &io, &call(17, 1, |_| {})).unwrap();
    assert!(matches!(rows(&reply)[0], stage::Record::FmtImage(_)));
}

#[test]
fn every_kind_of_picture_is_shown() {
    let io = Roots::new(&[("1.png", PNG), ("2.jpg", JPG), ("3.gif", GIF)]);
    let mut p = ImagesProvider::new();
    open(&mut p, &io, "1.png");
    assert_eq!(total(&mut p, &io), 3);
    for page in 0..3 {
        let (kind, _, size) = shown(&mut p, &io, page);
        assert!(kind == 3 && size.is_some(), "page {page}");
    }
}

#[test]
fn a_picture_is_fit_to_the_side_asked() {
    let io = Roots::new(&[("a.jpg", JPG)]);
    let mut p = ImagesProvider::new();
    run(
        &mut p,
        &io,
        &call(2, 1, |c| (c.text, c.image_side) = ("a.jpg", 64)),
    )
    .unwrap();
    let (_, _, size) = shown(&mut p, &io, 0);
    let (width, height) = size.unwrap();
    assert!(width.max(height) <= 64, "{width} x {height}");
}

#[test]
fn the_pages_are_the_pictures_from_the_opened_one_on_in_natural_order() {
    let mut files: Vec<(String, &[u8])> = [
        "10.gif",
        "2.png",
        "1.jpg",
        "02b.jpg",
        "x/9.png",
        "other/3.png",
    ]
    .iter()
    .map(|name| (name.to_string(), b"not a picture".as_slice()))
    .collect();
    files.extend([
        ("notes.txt".to_string(), b"text".as_slice()),
        (".hidden.jpg".to_string(), b"x".as_slice()),
        ("._1.jpg".to_string(), b"x".as_slice()),
    ]);
    let files: Vec<(&str, &[u8])> = files.iter().map(|(n, d)| (n.as_str(), *d)).collect();
    let io = Roots::new(&files);
    let mut p = ImagesProvider::new();
    open(&mut p, &io, "1.jpg");
    assert_eq!(total(&mut p, &io), 4);
    assert_eq!(
        named(&mut p, &io, 4),
        ["1.jpg", "2.png", "02b.jpg", "10.gif"]
    );
    // Opened in the middle, it starts there.
    run(&mut p, &io, &call(4, 1, |_| {})).unwrap();
    open(&mut p, &io, "2.png");
    assert_eq!(total(&mut p, &io), 3);
    assert_eq!(named(&mut p, &io, 3), ["2.png", "02b.jpg", "10.gif"]);
    run(&mut p, &io, &call(4, 1, |_| {})).unwrap();
    open(&mut p, &io, "10.gif");
    assert_eq!(total(&mut p, &io), 1);
}

#[test]
fn a_file_that_is_no_picture_is_a_page_with_its_name() {
    let io = Roots::new(&[("a.jpg", b"definitely not a jpeg")]);
    let mut p = ImagesProvider::new();
    open(&mut p, &io, "a.jpg");
    assert_eq!(shown(&mut p, &io, 0), (0, "a.jpg".to_string(), None));
    assert!(matches!(
        run(&mut p, &io, &call(17, 1, |_| {})),
        Err(Error::NotFound)
    ));
    // It is marked, and not read again: the page and the cover.
    assert_eq!(cache_files(&io, ".bad"), 2);
}

#[test]
fn a_file_too_large_is_named_not_decoded() {
    let mut big = PNG.to_vec();
    big.resize(25 << 20, 0);
    let io = Roots::new(&[("a.png", &big)]);
    let mut p = ImagesProvider::new();
    open(&mut p, &io, "a.png");
    assert_eq!(shown(&mut p, &io, 0), (0, "a.png".to_string(), None));
}

#[test]
fn a_folder_of_a_thousand_pictures_has_the_first_three_hundred() {
    let names: Vec<String> = (0..1000).rev().map(|n| format!("{n:04}.png")).collect();
    let files: Vec<(&str, &[u8])> = names
        .iter()
        .map(|n| (n.as_str(), b"x".as_slice()))
        .collect();
    let io = Roots::new(&files);
    let mut p = ImagesProvider::new();
    open(&mut p, &io, "0000.png");
    assert_eq!(total(&mut p, &io), 300);
    assert_eq!(shown(&mut p, &io, 299).1, "0299.png");
    // From the middle: the next three hundred.
    run(&mut p, &io, &call(4, 1, |_| {})).unwrap();
    open(&mut p, &io, "0500.png");
    assert_eq!(total(&mut p, &io), 300);
    assert_eq!(shown(&mut p, &io, 0).1, "0500.png");
    assert_eq!(shown(&mut p, &io, 299).1, "0799.png");
}

#[test]
fn the_picture_is_decoded_once() {
    let io = Roots::new(&[("a.png", PNG)]);
    let mut p = ImagesProvider::new();
    open(&mut p, &io, "a.png");
    let first = shown(&mut p, &io, 0);
    assert!(first.2.is_some());
    // The cache has it: the file may change under the same name and size
    // and the page is the same, from the cache.
    io.0.write("book/a.png", &vec![0u8; PNG.len()]).unwrap();
    run(&mut p, &io, &call(4, 1, |_| {})).unwrap();
    open(&mut p, &io, "a.png");
    assert_eq!(shown(&mut p, &io, 0), first);
    // Another size is another picture.
    io.0.write("book/a.png", b"short").unwrap();
    run(&mut p, &io, &call(4, 1, |_| {})).unwrap();
    open(&mut p, &io, "a.png");
    assert_eq!(shown(&mut p, &io, 0), (0, "a.png".to_string(), None));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn damaged_pictures_do_no_harm(
        which in 0usize..3,
        hits in proptest::collection::vec((0usize..40_000, any::<u8>()), 1..24),
        cut in proptest::option::of(0usize..40_000),
    ) {
        let mut bytes = [PNG, JPG, GIF][which].to_vec();
        for (at, value) in hits {
            let at = at % bytes.len();
            bytes[at] = value;
        }
        if let Some(cut) = cut {
            bytes.truncate(cut % bytes.len());
        }
        let name = ["a.png", "a.jpg", "a.gif"][which];
        let io = Roots::new(&[(name, &bytes)]);
        let mut p = ImagesProvider::new();
        open(&mut p, &io, name);
        let (kind, text, size) = shown(&mut p, &io, 0);
        prop_assert!((kind == 3 && size.is_some()) || text == name);
        let _ = run(&mut p, &io, &call(17, 1, |_| {}));
    }

    #[test]
    fn random_bytes_are_named(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let io = Roots::new(&[("a.png", &bytes)]);
        let mut p = ImagesProvider::new();
        open(&mut p, &io, "a.png");
        let (kind, text, size) = shown(&mut p, &io, 0);
        prop_assert!((kind == 3 && size.is_some()) || text == "a.png");
    }
}

/// The CRC-32 of PNG chunks.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

#[test]
fn a_picture_claiming_a_huge_size_is_no_harm() {
    // A PNG of 100,000 x 100,000 pixels in the header of a tiny one, with
    // the header's checksum made right.
    let mut png = PNG.to_vec();
    png[16..20].copy_from_slice(&100_000u32.to_be_bytes());
    png[20..24].copy_from_slice(&100_000u32.to_be_bytes());
    let crc = crc32(&png[12..29]);
    png[29..33].copy_from_slice(&crc.to_be_bytes());
    // A JPEG of 65,535 x 65,535.
    let mut jpg = JPG.to_vec();
    let sof = jpg
        .windows(2)
        .position(|w| w == [0xff, 0xc0] || w == [0xff, 0xc2])
        .expect("a frame header");
    jpg[sof + 5..sof + 9].copy_from_slice(&[0xff, 0xff, 0xff, 0xff]);
    for (name, bytes) in [("a.png", png), ("a.jpg", jpg)] {
        let io = Roots::new(&[(name, &bytes)]);
        let mut p = ImagesProvider::new();
        open(&mut p, &io, name);
        // Shown at the size the screen takes, or named: never a crash.
        match shown(&mut p, &io, 0) {
            (3, _, Some((width, height))) => {
                assert!(width <= 1600 && height <= 1600, "{width} x {height}");
            }
            (_, text, _) => assert_eq!(text, name),
        }
    }
}

#[test]
fn a_picture_later_in_the_folder_starts_there() {
    let io = folder();
    let mut p = ImagesProvider::new();
    assert_eq!(open(&mut p, &io, "2.jpg").0, 0);
    let reply = run(&mut p, &io, &call(7, 1, |_| {})).unwrap();
    assert!(matches!(rows(&reply)[2], stage::Record::FmtEnd(e) if e.total == 2));
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
        (7, "noextension"),
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
            matches!(rows(&reply)[..], [stage::Record::FmtEnd(e)] if e.total == 0),
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
