#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! Pictures kept turned a quarter clockwise (`turned`).
extern crate std;
use super::super::prepare::Within;
use super::super::recipe::{Recipe, needs_turn, then_quarter, turn_file};
use super::super::{ERR_FULL, ERR_NOT_FOUND, Origin, PREPARED_IMAGES};
use super::*;
use crate::native_host::stages_wire::core_format_v1::FmtImage;
use paperpaw_graphics::Framebuffer;
use paperpaw_platform::{PixelFormat, Point};
use paperpaw_sdk::MemoryStorage;
use paperpaw_sdk::image::{GrayWriter, ImageSink, PngWriter};
use std::vec::Vec;

const A: u64 = 1;
const ENV: Within = Within {
    screen: 480,
    colour: false,
    max_side: 4096,
    png_row_max: usize::MAX,
    turn_band: 1 << 20,
    turn_reads: usize::MAX,
};

/// The gray of pixel `(x, y)` of the 3 by 2 fixture.
fn value(x: u32, y: u32) -> u8 {
    (10 + 10 * x + 100 * y) as u8
}

/// 3 wide, 2 tall: `10 20 30` over `110 120 130`.
fn fixture(storage: &MemoryStorage, path: &str) {
    let mut writer = GrayWriter::new(storage, path);
    writer.start(3, 2).unwrap();
    for y in 0..2 {
        let row: Vec<u8> = (0..3).map(|x| value(x, y)).collect();
        writer.row(&row).unwrap();
    }
    writer.finish().unwrap();
}

/// The grays of the image `id` drawn pixel for pixel, row by row.
fn drawn(t: &ImageTable, storage: &MemoryStorage, id: u32, (w, h): (u32, u32)) -> Vec<u8> {
    let mut fb = Framebuffer::new(8, 8, PixelFormat::Gray8).unwrap();
    fb.canvas().clear(paperpaw_platform::Color::WHITE);
    t.draw(A, id, &mut fb.canvas(), storage, 0, 0, w, h, 0)
        .unwrap();
    let canvas = fb.canvas();
    (0..h as i32)
        .flat_map(|y| (0..w as i32).map(move |x| (x, y)))
        .map(|(x, y)| canvas.gray_at(Point::new(x, y)).unwrap())
        .collect()
}

/// The fixture a quarter clockwise: its left column, bottom first, is the
/// first row.
const QUARTER_CLOCKWISE: [u8; 6] = [110, 10, 120, 20, 130, 30];

#[test]
fn a_document_picture_is_turned_once_into_a_prepared_image_of_its_opener() {
    let storage = MemoryStorage::new();
    fixture(&storage, "c/a.gray");
    let t = ImageTable::new();
    let origin = Origin::Document {
        document: 1,
        index: 7,
    };
    let id = t.add(A, origin, "c/a.gray", 3, 2, false).unwrap();
    let turned = t.turned(&storage, A, id).unwrap();
    assert_eq!(
        (turned.width, turned.height, turned.index, turned.flags),
        (2, 3, 7, TURNED)
    );
    assert_ne!(turned.image, id);
    assert_eq!(drawn(&t, &storage, turned.image, (2, 3)), QUARTER_CLOCKWISE);
    // The page's own picture is as it was, and the turned one is the
    // plugin's: it does not end with the document.
    assert_eq!(drawn(&t, &storage, id, (3, 2)), [10, 20, 30, 110, 120, 130]);
    t.document_closed(1).unwrap();
    assert_eq!(t.info(A, id), Err(ERR_NOT_FOUND));
    assert_eq!(drawn(&t, &storage, turned.image, (2, 3)), QUARTER_CLOCKWISE);
    // Given back, its file goes with the next pass.
    let file = prepare::file_name(A, 0).unwrap();
    assert!(storage.exists(&file));
    t.release(A, turned.image).unwrap();
    t.prepare_step(&storage, ENV).unwrap();
    assert!(!storage.exists(&file));
    // Not another plugin's, not an id that ended.
    assert_eq!(t.turned(&storage, 2, id), Err(ERR_NOT_FOUND));
    assert_eq!(t.turned(&storage, A, turned.image), Err(ERR_NOT_FOUND));
}

#[test]
fn a_turn_that_cannot_be_made_leaves_nothing_behind() {
    let storage = MemoryStorage::new();
    fixture(&storage, "c/a.gray");
    let t = ImageTable::new();
    let origin = |index| Origin::Document { document: 1, index };
    let id = t.add(A, origin(0), "c/a.gray", 3, 2, false).unwrap();
    // A part of a file (a comic's panel) is not turned.
    let part: Vec<u8> = [1_u32, 0, 2, 2]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let panel = FmtImage::new("a.gray", 3, 3, 2, 0, &part);
    let panel = t
        .map_fmt_image(&storage, A, 1, "c", &panel, true, true)
        .unwrap()
        .unwrap();
    assert_eq!(t.turned(&storage, A, panel.image), Err(ERR_UNSUPPORTED));
    // A file that is not what the id says.
    let lying = t.add(A, origin(1), "c/b.gray", 3, 2, false).unwrap();
    assert_eq!(t.turned(&storage, A, lying), Err(ERR_IO));
    // Eight prepared images are out: full, and no file is made for a ninth.
    let ids: Vec<u32> = (0..PREPARED_IMAGES)
        .map(|_| t.turned(&storage, A, id).unwrap().image)
        .collect();
    assert_eq!(t.turned(&storage, A, id), Err(ERR_FULL));
    let mut files = 0;
    storage
        .list(prepare::DIR, &mut |entry| {
            files += usize::from(!entry.is_dir)
        })
        .unwrap();
    assert_eq!(files, PREPARED_IMAGES);
    t.release(A, ids[0]).unwrap();
    assert!(t.turned(&storage, A, id).is_ok());
}

#[test]
fn a_quarter_after_an_orientation_is_one_turn() {
    // `then_quarter(o)` is the turn `o` and then a quarter clockwise.
    let storage = MemoryStorage::new();
    fixture(&storage, "src");
    let limits = (1 << 20, usize::MAX);
    let read = |path: &str| paperpaw_sdk::read_file(&storage, path, 1 << 10).unwrap();
    for orientation in 1..=8_u8 {
        let first = if needs_turn(orientation) {
            turn_file(&storage, ("src", "first"), orientation, 1, limits).unwrap();
            "first"
        } else {
            "src"
        };
        turn_file(&storage, (first, "twice"), QUARTER, 1, limits).unwrap();
        let composed = then_quarter(orientation);
        let once = if needs_turn(composed) {
            turn_file(&storage, ("src", "once"), composed, 1, limits).unwrap();
            "once"
        } else {
            "src"
        };
        assert_eq!(read("twice"), read(once), "orientation {orientation}");
    }
    // The quarter itself is clockwise.
    turn_file(&storage, ("src", "q"), QUARTER, 1, limits).unwrap();
    assert_eq!(read("q")[12..], QUARTER_CLOCKWISE);
}

#[test]
fn a_prepared_picture_is_kept_turned_when_asked() {
    let storage = MemoryStorage::new();
    let mut writer = PngWriter::new(&storage, "p.png", 3, 2).unwrap();
    for y in 0..2 {
        let row: Vec<u8> = (0..3).map(|x| value(x, y)).collect();
        writer.row(&storage, "p.png", &row).unwrap();
    }
    let t = ImageTable::new();
    let ready = |turned: bool, cookie: u32| {
        let recipe = Recipe {
            turned,
            ..Recipe::default()
        };
        t.prepare(A, "p.png", 0, cookie, false, &recipe).unwrap();
        assert!(t.prepare_step(&storage, ENV).unwrap());
        let mut got = None;
        t.take_ready(&mut |r| {
            got = Some(r.clone());
            Ok(())
        })
        .unwrap();
        got.unwrap()
    };
    let plain = ready(false, 1);
    assert_eq!(
        (plain.status, plain.width, plain.height),
        (prepare::READY, 3, 2)
    );
    // Any format, not only a JPEG with an orientation; the size told is the
    // turned one and no first file is left.
    let turned = ready(true, 2);
    assert_eq!(
        (turned.status, turned.width, turned.height),
        (prepare::READY, 2, 3)
    );
    assert_eq!(drawn(&t, &storage, turned.image, (2, 3)), QUARTER_CLOCKWISE);
    let mut files = 0;
    storage
        .list(prepare::DIR, &mut |entry| {
            files += usize::from(!entry.is_dir)
        })
        .unwrap();
    assert_eq!(files, 2);
}
