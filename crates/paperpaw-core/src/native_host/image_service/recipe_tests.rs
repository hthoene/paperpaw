#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! The picture recipe of `image_prepare`: the tone table, the EXIF turn and
//! the brightness grid (`docs/abi/document.md` 5).
extern crate std;
use super::*;
use crate::native_host::image_service::ImageTable;
use crate::native_host::image_service::prepare::{Env, READY};
use alloc::string::String;
use core::cell::{Cell, RefCell};
use paperpaw_platform::{DirEntry, StorageError};
use paperpaw_sdk::MemoryStorage;
use paperpaw_sdk::image::PngWriter;
use std::vec;

const WIDE: &[u8] = include_bytes!("fixtures/wide.jpg");
const A: u64 = 1;

/// The device with no X4 limits.
const ENV: Env = Env {
    screen: 480,
    colour: false,
    max_side: 4096,
    png_row_max: usize::MAX,
    turn_band: 1 << 20,
    turn_reads: usize::MAX,
};

/// A decoded gray picture file of `width` by `height` whose pixel at `(x, y)`
/// is `pixel(x, y)`.
fn gray_file(
    storage: &dyn Storage,
    path: &str,
    (width, height): (usize, usize),
    pixel: impl Fn(usize, usize) -> u8,
) {
    let mut bytes = std::vec::Vec::from(*b"PPGI");
    bytes.extend_from_slice(&(width as u32).to_le_bytes());
    bytes.extend_from_slice(&(height as u32).to_le_bytes());
    for y in 0..height {
        for x in 0..width {
            bytes.push(pixel(x, y));
        }
    }
    storage.write(path, &bytes).unwrap();
}

/// The picture file at `path`: its size and every pixel (`channels` bytes).
fn read_file(
    storage: &dyn Storage,
    path: &str,
    channels: usize,
) -> (usize, usize, std::vec::Vec<u8>) {
    let size = storage.size(path).unwrap() as usize;
    let mut bytes = vec![0_u8; size];
    assert!(read_all(storage, path, 0, &mut bytes));
    let width = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
    let height = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
    assert_eq!(size, 12 + width * height * channels, "{path} is whole");
    (width, height, bytes[12..].to_vec())
}

/// What a picture of `(w, h)` is turned to by EXIF orientation `o`, pixel by
/// pixel and by the definition of the orientations (not by the host's
/// method): `(w', h', pixels)`.
fn oracle(
    src: &[u8],
    (w, h): (usize, usize),
    channels: usize,
    o: u8,
) -> (usize, usize, std::vec::Vec<u8>) {
    // Where output pixel `(x, y)` of an output `ow` by `oh` comes from.
    let (ow, oh) = if (5..=8).contains(&o) { (h, w) } else { (w, h) };
    let mut out = std::vec::Vec::new();
    for y in 0..oh {
        for x in 0..ow {
            let (sx, sy) = match o {
                2 => (w - 1 - x, y),
                3 => (w - 1 - x, h - 1 - y),
                4 => (x, h - 1 - y),
                5 => (y, x),
                6 => (y, h - 1 - x),
                7 => (w - 1 - y, h - 1 - x),
                8 => (w - 1 - y, x),
                _ => (x, y),
            };
            let at = (sy * w + sx) * channels;
            out.extend_from_slice(&src[at..at + channels]);
        }
    }
    (ow, oh, out)
}

/// A storage that counts what is read and written: the longest read and the
/// longest append, and how many reads were made.
struct Counting {
    inner: MemoryStorage,
    longest_read: Cell<usize>,
    longest_append: Cell<usize>,
    reads: Cell<usize>,
    writes: RefCell<std::vec::Vec<String>>,
}

impl Counting {
    fn new(inner: MemoryStorage) -> Self {
        Self {
            inner,
            longest_read: Cell::new(0),
            longest_append: Cell::new(0),
            reads: Cell::new(0),
            writes: RefCell::new(std::vec::Vec::new()),
        }
    }
}

impl Storage for Counting {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.reads.set(self.reads.get() + 1);
        self.longest_read
            .set(self.longest_read.get().max(buf.len()));
        self.inner.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.inner.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.writes.borrow_mut().push(path.into());
        self.inner.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.longest_append
            .set(self.longest_append.get().max(data.len()));
        self.inner.append(path, data)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.inner.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.inner.remove(path)
    }
}

/// A sink that keeps what it is given.
#[derive(Default)]
struct Keep {
    size: (u32, u32),
    rows: std::vec::Vec<std::vec::Vec<u8>>,
    colour: bool,
    rgb: std::vec::Vec<std::vec::Vec<u8>>,
}

impl ImageSink for Keep {
    fn start(&mut self, width: u32, height: u32) -> Result<(), ImageError> {
        self.size = (width, height);
        Ok(())
    }
    fn row(&mut self, gray: &[u8]) -> Result<(), ImageError> {
        self.rows.push(gray.to_vec());
        Ok(())
    }
    fn wants_color(&self) -> bool {
        self.colour
    }
    fn row_rgb(&mut self, rgb: &[u8]) -> Result<(), ImageError> {
        self.rgb.push(rgb.to_vec());
        Ok(())
    }
}

fn inverted() -> std::vec::Vec<u8> {
    (0..=255_u8).map(|v| 255 - v).collect()
}

#[test]
fn the_tone_table_maps_gray_values_and_every_colour_channel() {
    let table = inverted();
    let mut keep = Keep {
        colour: true,
        ..Keep::default()
    };
    let mut tone = ToneSink::new(&mut keep, &table);
    tone.start(3, 2).unwrap();
    assert!(
        tone.wants_color(),
        "what the sink behind wants is passed on"
    );
    tone.row(&[0, 100, 255]).unwrap();
    tone.row_rgb(&[0, 10, 20, 255, 254, 253, 1, 2, 3]).unwrap();
    assert_eq!(keep.size, (3, 2));
    assert_eq!(keep.rows, [[255, 155, 0]]);
    assert_eq!(keep.rgb, [[255, 245, 235, 0, 1, 2, 254, 253, 252]]);
}

#[test]
fn a_short_table_leaves_the_values_it_does_not_reach() {
    // The host checks the length before; the sink never indexes past it.
    let table = [9_u8, 8];
    let mut keep = Keep::default();
    let mut tone = ToneSink::new(&mut keep, &table);
    tone.start(4, 1).unwrap();
    tone.row(&[0, 1, 2, 200]).unwrap();
    assert_eq!(keep.rows, [[9, 8, 2, 200]]);
}

/// A JPEG head: SOI, then an APP1 `Exif` block of `tiff`, then the start of
/// a scan (no frame header: only the head is read).
fn jpeg_with(tiff: &[u8]) -> std::vec::Vec<u8> {
    let mut bytes = vec![0xff, 0xd8, 0xff, 0xe1];
    let len = (2 + 6 + tiff.len()) as u16;
    bytes.extend_from_slice(&len.to_be_bytes());
    bytes.extend_from_slice(b"Exif\0\0");
    bytes.extend_from_slice(tiff);
    bytes.extend_from_slice(&[0xff, 0xda, 0, 2]);
    bytes
}

/// A TIFF block of one IFD of the given entries `(tag, type, count, value)`.
fn tiff(big: bool, entries: &[(u16, u16, u32, u16)]) -> std::vec::Vec<u8> {
    let s = |v: u16| {
        if big {
            v.to_be_bytes()
        } else {
            v.to_le_bytes()
        }
    };
    let l = |v: u32| {
        if big {
            v.to_be_bytes()
        } else {
            v.to_le_bytes()
        }
    };
    let mut bytes = std::vec::Vec::from(if big { *b"MM" } else { *b"II" });
    bytes.extend_from_slice(&s(42));
    bytes.extend_from_slice(&l(8));
    bytes.extend_from_slice(&s(entries.len() as u16));
    for &(tag, kind, count, value) in entries {
        bytes.extend_from_slice(&s(tag));
        bytes.extend_from_slice(&s(kind));
        bytes.extend_from_slice(&l(count));
        bytes.extend_from_slice(&s(value));
        bytes.extend_from_slice(&[0, 0]);
    }
    bytes
}

fn orientation_of(bytes: &[u8]) -> u8 {
    let storage = MemoryStorage::new();
    storage.write("a.jpg", bytes).unwrap();
    exif_orientation(&storage, "a.jpg")
}

#[test]
fn the_exif_orientation_is_read_in_both_byte_orders_after_other_tags() {
    for big in [false, true] {
        for wanted in 1..=8_u16 {
            let block = tiff(big, &[(0x010f, 2, 4, 0), (0x0112, 3, 1, wanted)]);
            assert_eq!(orientation_of(&jpeg_with(&block)), wanted as u8);
        }
    }
}

#[test]
fn an_orientation_out_of_range_or_odd_is_as_it_is() {
    for value in [0_u16, 9, 255] {
        let block = tiff(false, &[(0x0112, 3, 1, value)]);
        assert_eq!(orientation_of(&jpeg_with(&block)), 1);
    }
    // A long, or two values: not a single short.
    assert_eq!(
        orientation_of(&jpeg_with(&tiff(false, &[(0x0112, 4, 1, 6)]))),
        1
    );
    assert_eq!(
        orientation_of(&jpeg_with(&tiff(false, &[(0x0112, 3, 2, 6)]))),
        1
    );
    // None at all, other tags only.
    assert_eq!(
        orientation_of(&jpeg_with(&tiff(true, &[(0x010f, 2, 4, 0)]))),
        1
    );
    assert_eq!(orientation_of(&jpeg_with(&tiff(true, &[]))), 1);
}

#[test]
fn a_hostile_exif_block_is_as_it_is_and_reads_nothing_unbounded() {
    // Not TIFF, wrong magic, an IFD outside the block, a count past the block,
    // a block shorter than its header says.
    let mut bad = tiff(false, &[(0x0112, 3, 1, 6)]);
    bad[0] = b'X';
    assert_eq!(orientation_of(&jpeg_with(&bad)), 1);
    let mut bad = tiff(false, &[(0x0112, 3, 1, 6)]);
    bad[2] = 41;
    assert_eq!(orientation_of(&jpeg_with(&bad)), 1);
    let mut bad = tiff(false, &[(0x0112, 3, 1, 6)]);
    bad[4..8].copy_from_slice(&0x00ff_ffff_u32.to_le_bytes());
    assert_eq!(orientation_of(&jpeg_with(&bad)), 1);
    let mut bad = tiff(false, &[(0x010f, 2, 4, 0)]);
    bad[8..10].copy_from_slice(&60_000_u16.to_le_bytes());
    assert_eq!(
        orientation_of(&jpeg_with(&bad)),
        1,
        "the entries run past the block"
    );
    let cut = jpeg_with(&tiff(false, &[(0x0112, 3, 1, 6)]));
    assert_eq!(orientation_of(&cut[..cut.len() - 14]), 1);
    // A JPEG with no APP1, one that ends at once, and segment lengths that
    // never reach the end.
    assert_eq!(orientation_of(&[0xff, 0xd8]), 1);
    assert_eq!(orientation_of(&[0xff, 0xd8, 0xff, 0xda, 0, 2]), 1);
    let mut loops = vec![0xff, 0xd8];
    for _ in 0..100 {
        loops.extend_from_slice(&[0xff, 0xe0, 0, 2]);
    }
    assert_eq!(orientation_of(&loops), 1);
    assert_eq!(orientation_of(&[]), 1);
}

#[test]
fn every_turn_of_a_gray_picture_is_the_orientation_it_names_whatever_the_band() {
    let (w, h) = (7, 5);
    let pixel = |x: usize, y: usize| (y * w + x) as u8 * 3 + 1;
    for orientation in 2..=8_u8 {
        // One column at a time, a few, and all of it.
        for band in [1, 2 * h, 3 * h + 1, 1 << 20] {
            let storage = MemoryStorage::new();
            gray_file(&storage, "s.img", (w, h), pixel);
            turn_file(
                &storage,
                ("s.img", "t.img"),
                orientation,
                1,
                (band, usize::MAX),
            )
            .unwrap();
            let (_, _, src) = read_file(&storage, "s.img", 1);
            let want = oracle(&src, (w, h), 1, orientation);
            let got = read_file(&storage, "t.img", 1);
            assert_eq!(got, want, "orientation {orientation}, band {band}");
        }
    }
}

#[test]
fn every_turn_of_a_colour_picture_keeps_its_pixels_whole() {
    let (w, h) = (6, 4);
    let mut bytes = std::vec::Vec::from(*b"PPCI");
    bytes.extend_from_slice(&(w as u32).to_le_bytes());
    bytes.extend_from_slice(&(h as u32).to_le_bytes());
    let mut pixels = std::vec::Vec::new();
    for i in 0..w * h {
        pixels.extend_from_slice(&[i as u8, 100 + i as u8, 200 - i as u8]);
    }
    bytes.extend_from_slice(&pixels);
    for orientation in 2..=8_u8 {
        for band in [1, 3 * h, 1 << 20] {
            let storage = MemoryStorage::new();
            storage.write("s.img", &bytes).unwrap();
            turn_file(
                &storage,
                ("s.img", "t.img"),
                orientation,
                3,
                (band, usize::MAX),
            )
            .unwrap();
            assert_eq!(
                read_file(&storage, "t.img", 3),
                oracle(&pixels, (w, h), 3, orientation),
                "orientation {orientation}, band {band}"
            );
        }
    }
}

#[test]
fn a_turn_of_one_pixel_a_row_or_a_column_is_whole() {
    for (w, h) in [(1, 1), (1, 9), (9, 1)] {
        for orientation in 2..=8_u8 {
            let storage = MemoryStorage::new();
            gray_file(&storage, "s.img", (w, h), |x, y| (x * 7 + y * 13) as u8);
            turn_file(
                &storage,
                ("s.img", "t.img"),
                orientation,
                1,
                (1, usize::MAX),
            )
            .unwrap();
            let (_, _, src) = read_file(&storage, "s.img", 1);
            assert_eq!(
                read_file(&storage, "t.img", 1),
                oracle(&src, (w, h), 1, orientation),
                "{w}x{h} orientation {orientation}"
            );
        }
    }
}

#[test]
fn a_turn_refuses_a_file_that_is_no_picture_or_has_no_orientation() {
    let storage = MemoryStorage::new();
    assert_eq!(
        turn_file(&storage, ("none.img", "t.img"), 6, 1, (1024, usize::MAX)),
        Err(ImageError::Io)
    );
    storage.write("junk.img", b"PPGI\0\0\0\0\0\0\0\0").unwrap();
    assert!(matches!(
        turn_file(&storage, ("junk.img", "t.img"), 6, 1, (1024, usize::MAX)),
        Err(ImageError::Invalid(_))
    ));
    gray_file(&storage, "s.img", (2, 2), |_, _| 1);
    assert!(matches!(
        turn_file(&storage, ("s.img", "t.img"), 1, 1, (1024, usize::MAX)),
        Err(ImageError::Invalid(_))
    ));
    // A source cut short fails as the card does, with no whole file made.
    let mut cut = storage.size("s.img").unwrap() as usize;
    cut -= 1;
    let mut bytes = vec![0_u8; cut];
    assert!(read_all(&storage, "s.img", 0, &mut bytes));
    storage.write("cut.img", &bytes).unwrap();
    assert_eq!(
        turn_file(&storage, ("cut.img", "t.img"), 6, 1, (1024, usize::MAX)),
        Err(ImageError::Io)
    );
}

#[test]
fn a_large_picture_is_turned_within_the_x4_band_and_never_whole_in_memory() {
    // The longest a prepared picture gets on the X4: 1,600 by 1,200 gray
    // (1.9 MB of pixels) turned a quarter, in tiles of at most 32 KiB.
    let (w, h) = (1_600, 1_200);
    let counted = Counting::new(MemoryStorage::new());
    gray_file(&counted, "s.img", (w, h), |x, y| (x ^ y) as u8);
    let band = 32 * 1024;
    turn_file(&counted, ("s.img", "t.img"), 6, 1, (band, usize::MAX)).unwrap();
    // What the turn did, before the check below reads the file whole.
    let (longest_read, longest_append, reads) = (
        counted.longest_read.get(),
        counted.longest_append.get(),
        counted.reads.get(),
    );
    let (ow, oh, out) = read_file(&counted, "t.img", 1);
    assert_eq!((ow, oh), (h, w));
    // Spot checks of the oracle: the first and last output pixels and one
    // from the middle (`6`: `out(x, y) = src(y, h - 1 - x)`).
    let src = |x: usize, y: usize| (x ^ y) as u8;
    for (x, y) in [(0, 0), (h - 1, w - 1), (600, 800), (1_199, 0), (0, 1_599)] {
        assert_eq!(out[y * ow + x], src(y, h - 1 - x), "({x}, {y})");
    }
    // The tile is `columns * h` bytes, `columns` = band / h = 27: at most the
    // band; nothing read or appended is more than that or a flush chunk.
    assert!(longest_read <= band, "{}", longest_read);
    assert!(longest_append <= 8192.max(oh), "{}", longest_append);
    // 27 columns a tile: `h * ceil(w / 27)` reads of 27 bytes, and no more.
    let tiles = w.div_ceil(band / h);
    assert!(reads <= h * tiles + 8, "{}", reads);
}

#[test]
fn the_brightness_grid_is_the_mean_of_eight_by_eight_cells() {
    let storage = MemoryStorage::new();
    // Left half black, right half white; top half lighter by 10.
    gray_file(&storage, "g.img", (16, 16), |x, y| {
        let base = if x < 8 { 0 } else { 200 };
        base + if y < 8 { 10 } else { 0 }
    });
    let grid = luma_grid(&storage, "g.img", (16, 16, 1)).unwrap();
    for cell_y in 0..8 {
        for cell_x in 0..8 {
            let want = if cell_x < 4 { 0 } else { 200 } + if cell_y < 4 { 10 } else { 0 };
            assert_eq!(grid[cell_y * 8 + cell_x], want, "({cell_x}, {cell_y})");
        }
    }
}

#[test]
fn the_grid_of_a_small_picture_takes_the_pixel_at_each_cell() {
    let storage = MemoryStorage::new();
    gray_file(&storage, "g.img", (3, 2), |x, y| (x * 80 + y * 5) as u8);
    let grid = luma_grid(&storage, "g.img", (3, 2, 1)).unwrap();
    assert_eq!(grid.len(), 64);
    // Every cell is a real pixel's value (none is zero by a divide).
    let values: std::vec::Vec<u8> = (0..2)
        .flat_map(|y| (0..3).map(move |x| (x * 80 + y * 5) as u8))
        .collect();
    assert!(grid.iter().all(|v| values.contains(v)), "{grid:?}");
    assert_eq!(grid[0], 0);
    assert_eq!(grid[7], 160);
    assert_eq!(grid[56], 5);
}

#[test]
fn the_grid_of_a_colour_picture_is_its_luma_and_a_cut_file_has_none() {
    let storage = MemoryStorage::new();
    let mut bytes = std::vec::Vec::from(*b"PPCI");
    bytes.extend_from_slice(&8_u32.to_le_bytes());
    bytes.extend_from_slice(&8_u32.to_le_bytes());
    for _ in 0..64 {
        bytes.extend_from_slice(&[255, 0, 0]);
    }
    storage.write("c.img", &bytes).unwrap();
    let grid = luma_grid(&storage, "c.img", (8, 8, 3)).unwrap();
    let red = luma(255, 0, 0);
    assert!(grid.iter().all(|&v| v == red), "{grid:?}");
    storage.write("cut.img", &bytes[..bytes.len() - 5]).unwrap();
    assert_eq!(luma_grid(&storage, "cut.img", (8, 8, 3)), None);
}

/// The prepared file of the table's only image.
fn prepared(table: &ImageTable) -> String {
    let inner = table.lock().unwrap();
    super::super::prepare::file_name(inner.entries[0].owner, inner.entries[0].seq).unwrap()
}

fn prepare_jpeg(
    storage: &dyn Storage,
    orientation: Option<u16>,
    side: u32,
    recipe: &Recipe,
    env: Env,
) -> (ImageTable, super::super::prepare::Ready) {
    // The fixture with an EXIF block of `orientation` after the SOI.
    let mut bytes = WIDE.to_vec();
    if let Some(value) = orientation {
        let block = jpeg_with(&tiff(false, &[(0x0112, 3, 1, value)]));
        // `jpeg_with` ends with a scan start: only its APP1 is taken.
        let app1_len = 2 + usize::from(u16::from_be_bytes([block[4], block[5]]));
        let mut with = bytes[..2].to_vec();
        with.extend_from_slice(&block[2..2 + app1_len]);
        with.extend_from_slice(&bytes[2..]);
        bytes = with;
    }
    storage.write("p.jpg", &bytes).unwrap();
    let table = ImageTable::new();
    table.prepare(A, "p.jpg", side, 1, false, recipe).unwrap();
    table.prepare_step(storage, env).unwrap();
    let mut ready = None;
    table
        .take_ready(&mut |r| {
            ready = Some(r.clone());
            Ok(())
        })
        .unwrap();
    (table, ready.unwrap())
}

#[test]
fn a_jpeg_is_turned_as_its_exif_says_and_the_size_told_is_the_size_after_the_turn() {
    let storage = MemoryStorage::new();
    // The fixture is 400 by 250, not turned and no EXIF: the plain decode.
    let (plain, ready) = prepare_jpeg(&storage, None, 0, &Recipe::default(), ENV);
    assert_eq!((ready.status, ready.width, ready.height), (READY, 400, 250));
    let (w, h, src) = read_file(&storage, &prepared(&plain), 1);
    assert_eq!((w, h), (400, 250));
    for orientation in 2..=8_u16 {
        let storage = MemoryStorage::new();
        let recipe = Recipe {
            turn: true,
            tone: None,
        };
        let (table, ready) = prepare_jpeg(&storage, Some(orientation), 0, &recipe, ENV);
        let (want_w, want_h, want) = oracle(&src, (400, 250), 1, orientation as u8);
        assert_eq!(
            (ready.status, ready.width as usize, ready.height as usize),
            (READY, want_w, want_h),
            "orientation {orientation}"
        );
        let got = read_file(&storage, &prepared(&table), 1);
        assert_eq!(got, (want_w, want_h, want), "orientation {orientation}");
        // No half-way file is left behind.
        let mut names = std::vec::Vec::new();
        storage
            .list(".paperpaw/cache/images", &mut |e| {
                names.push(String::from(e.name))
            })
            .unwrap();
        assert_eq!(names.len(), 1, "{names:?}");
    }
}

#[test]
fn a_jpeg_is_left_as_it_is_without_the_turn_flag_or_an_orientation_to_apply() {
    let storage = MemoryStorage::new();
    // EXIF says 6 but `turn` is not asked for: decoded as it is.
    let (_, ready) = prepare_jpeg(&storage, Some(6), 0, &Recipe::default(), ENV);
    assert_eq!((ready.width, ready.height), (400, 250));
    // `turn` asked for, orientation 1: as it is, no second file.
    let turn = Recipe {
        turn: true,
        tone: None,
    };
    let (table, ready) = prepare_jpeg(&storage, Some(1), 0, &turn, ENV);
    assert_eq!((ready.width, ready.height), (400, 250));
    assert!(storage.exists(&prepared(&table)));
    // And a PNG is never turned.
    let png_storage = MemoryStorage::new();
    let mut writer = PngWriter::new(&png_storage, "a.png", 40, 20).unwrap();
    for _ in 0..20 {
        writer.row(&png_storage, "a.png", &[90; 40]).unwrap();
    }
    let table = ImageTable::new();
    table.prepare(A, "a.png", 0, 1, false, &turn).unwrap();
    table.prepare_step(&png_storage, ENV).unwrap();
    let mut ready = None;
    table
        .take_ready(&mut |r| {
            ready = Some(r.clone());
            Ok(())
        })
        .unwrap();
    let ready = ready.unwrap();
    assert_eq!((ready.width, ready.height), (40, 20));
}

#[test]
fn the_longest_side_is_the_one_before_the_turn_and_the_turn_swaps_the_axes() {
    let storage = MemoryStorage::new();
    let turn = Recipe {
        turn: true,
        tone: None,
    };
    let (_, ready) = prepare_jpeg(&storage, Some(6), 100, &turn, ENV);
    assert_eq!(ready.status, READY);
    assert_eq!(ready.width.max(ready.height), 100);
    assert!(
        ready.height > ready.width,
        "now upright: {}x{}",
        ready.width,
        ready.height
    );
}

#[test]
fn a_tone_table_is_applied_while_the_picture_is_decoded() {
    let storage = MemoryStorage::new();
    let mut writer = PngWriter::new(&storage, "a.png", 16, 16).unwrap();
    for _ in 0..16 {
        writer.row(&storage, "a.png", &[90; 16]).unwrap();
    }
    let table = ImageTable::new();
    let recipe = Recipe {
        turn: false,
        tone: Some(inverted()),
    };
    table.prepare(A, "a.png", 0, 1, false, &recipe).unwrap();
    table.prepare_step(&storage, ENV).unwrap();
    let mut ready = None;
    table
        .take_ready(&mut |r| {
            ready = Some(r.clone());
            Ok(())
        })
        .unwrap();
    let ready = ready.unwrap();
    assert_eq!(ready.status, READY);
    let (_, _, pixels) = read_file(&storage, &prepared(&table), 1);
    assert!(pixels.iter().all(|&p| p == 165), "255 - 90");
    // The grid is of the prepared picture, after the tone.
    assert_eq!(ready.luma, Some([165; 64]));
}

#[test]
fn only_a_ready_picture_has_a_grid() {
    let storage = MemoryStorage::new();
    storage.write("junk.png", b"not a picture").unwrap();
    let table = ImageTable::new();
    table
        .prepare(A, "junk.png", 0, 1, false, &Recipe::default())
        .unwrap();
    table.prepare_step(&storage, ENV).unwrap();
    let mut ready = None;
    table
        .take_ready(&mut |r| {
            ready = Some(r.clone());
            Ok(())
        })
        .unwrap();
    let ready = ready.unwrap();
    assert_ne!(ready.status, READY);
    assert_eq!(ready.luma, None);
}

#[test]
fn a_recipe_is_copied_for_the_waiting_request_and_the_copy_is_its_own() {
    let recipe = Recipe {
        turn: true,
        tone: Some(inverted()),
    };
    let copy = recipe.try_copy().unwrap();
    assert_eq!(copy, recipe);
    assert_eq!(Recipe::default().try_copy().unwrap(), Recipe::default());
}

#[test]
fn a_quarter_turn_over_the_read_budget_is_too_large_and_writes_nothing() {
    // The X4's budget (16,384 short reads, 32 KiB tiles): the screen's
    // 800 by 600 gray is turned (about 9,000 reads), a 1,600 by 1,200 one
    // (about 72,000) is refused before a byte is written; a half turn and a
    // mirror are never counted.
    let (band, reads) = (32 * 1024, 16_384);
    let storage = MemoryStorage::new();
    gray_file(&storage, "screen.img", (800, 600), |x, y| (x ^ y) as u8);
    turn_file(&storage, ("screen.img", "a.img"), 6, 1, (band, reads)).unwrap();
    assert_eq!(read_file(&storage, "a.img", 1).0, 600);
    gray_file(&storage, "big.img", (1_600, 1_200), |x, y| (x ^ y) as u8);
    assert_eq!(
        turn_file(&storage, ("big.img", "b.img"), 6, 1, (band, reads)),
        Err(ImageError::TooLarge)
    );
    assert!(storage.size("b.img").is_err());
    turn_file(&storage, ("big.img", "c.img"), 3, 1, (band, reads)).unwrap();
}
