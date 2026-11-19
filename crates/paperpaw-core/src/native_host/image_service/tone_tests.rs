#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! Parts of pictures (`canvas.draw_image_part`, `fmt.image.part`) and the
//! picture stage's answer kept with an image.
extern crate std;
use super::super::{ERR_INVALID, ERR_NOT_FOUND, GRAYS, STRETCH};
use super::*;
use crate::native_host::stages_wire::core_format_v1::FmtImage;
use paperpaw_graphics::Framebuffer;
use paperpaw_platform::{PixelFormat, Point};
use paperpaw_sdk::MemoryStorage;
use paperpaw_sdk::image::{GrayWriter, ImageSink};
use std::vec::Vec;

const A: u64 = 1;

/// A gray picture whose pixel `(x, y)` is `at(x, y)`.
fn picture(storage: &MemoryStorage, path: &str, (w, h): (u32, u32), at: impl Fn(u32, u32) -> u8) {
    let mut writer = GrayWriter::new(storage, path);
    writer.start(w, h).unwrap();
    for y in 0..h {
        let row: Vec<u8> = (0..w).map(|x| at(x, y)).collect();
        writer.row(&row).unwrap();
    }
    writer.finish().unwrap();
}

/// 8 by 4: the left half 10 with a corner of 2 by 2 of 90, the right half 200.
fn halves(storage: &MemoryStorage, path: &str) {
    picture(storage, path, (8, 4), |x, y| match (x, y) {
        (0..2, 0..2) => 90,
        (0..4, _) => 10,
        _ => 200,
    });
}

fn screen() -> Framebuffer {
    let mut fb = Framebuffer::new(40, 40, PixelFormat::Gray8).unwrap();
    fb.canvas().clear(paperpaw_platform::Color::WHITE);
    fb
}

fn gray(fb: &mut Framebuffer, x: i32, y: i32) -> u8 {
    fb.canvas().gray_at(Point::new(x, y)).unwrap()
}

fn document(index: u32) -> Origin {
    Origin::Document { document: 1, index }
}

fn words(part: [u32; 4]) -> Vec<u8> {
    part.iter().flat_map(|v| v.to_le_bytes()).collect()
}

#[test]
fn a_part_of_a_picture_is_drawn_alone_scaled_into_the_rectangle() {
    let storage = MemoryStorage::new();
    halves(&storage, "c/a.gray");
    let t = ImageTable::new();
    let id = t.add(A, document(0), "c/a.gray", 8, 4, false).unwrap();
    // The right half alone, stretched over 8 by 8 at (4, 4).
    let mut fb = screen();
    t.draw_part(
        A,
        id,
        &mut fb.canvas(),
        &storage,
        (4, 4, 8, 8),
        STRETCH,
        Some([4, 0, 4, 4]),
    )
    .unwrap();
    for (x, y, want) in [
        (4, 4, 200),
        (11, 11, 200),
        (3, 4, 255),
        (12, 4, 255),
        (4, 12, 255),
    ] {
        assert_eq!(gray(&mut fb, x, y), want, "({x}, {y})");
    }
    // The corner made larger: nothing of the pixels beside it comes in.
    let mut fb = screen();
    t.draw_part(
        A,
        id,
        &mut fb.canvas(),
        &storage,
        (0, 0, 10, 10),
        STRETCH,
        Some([0, 0, 2, 2]),
    )
    .unwrap();
    for (x, y) in [(0, 0), (9, 0), (0, 9), (9, 9), (5, 5)] {
        assert_eq!(gray(&mut fb, x, y), 90, "({x}, {y})");
    }
    // Without a flag the part keeps its shape and is centred, as a picture is.
    let mut fb = screen();
    t.draw_part(
        A,
        id,
        &mut fb.canvas(),
        &storage,
        (0, 0, 20, 10),
        0,
        Some([4, 0, 4, 4]),
    )
    .unwrap();
    for (x, want) in [(4, 255), (5, 200), (14, 200), (15, 255)] {
        assert_eq!(gray(&mut fb, x, 5), want, "column {x}");
    }
    // The whole picture as its own part is the picture.
    let mut fb = screen();
    t.draw_part(
        A,
        id,
        &mut fb.canvas(),
        &storage,
        (0, 0, 8, 4),
        0,
        Some([0, 0, 8, 4]),
    )
    .unwrap();
    assert_eq!(
        (
            gray(&mut fb, 0, 0),
            gray(&mut fb, 3, 3),
            gray(&mut fb, 4, 0)
        ),
        (90, 10, 200)
    );
}

#[test]
fn a_part_that_is_empty_or_outside_the_picture_is_invalid_and_draws_nothing() {
    let storage = MemoryStorage::new();
    halves(&storage, "c/a.gray");
    let t = ImageTable::new();
    let id = t.add(A, document(0), "c/a.gray", 8, 4, false).unwrap();
    for bad in [
        [0, 0, 0, 4],
        [0, 0, 4, 0],
        [5, 0, 4, 4],
        [0, 1, 8, 4],
        [0, 0, 9, 1],
        [u32::MAX, 0, 2, 2],
        [2, u32::MAX, 2, 2],
    ] {
        let mut fb = screen();
        assert_eq!(
            t.draw_part(
                A,
                id,
                &mut fb.canvas(),
                &storage,
                (0, 0, 8, 8),
                0,
                Some(bad)
            ),
            Err(ERR_INVALID),
            "{bad:?}"
        );
        assert_eq!(gray(&mut fb, 0, 0), 255);
    }
    assert_eq!(
        t.draw_part(
            A,
            id + 9,
            &mut screen().canvas(),
            &storage,
            (0, 0, 8, 8),
            0,
            None
        ),
        Err(ERR_NOT_FOUND)
    );
    // The slot's source is 16 bytes, no more and no fewer.
    assert_eq!(
        super::super::source_of(&words([1, 2, 3, 4])),
        Ok([1, 2, 3, 4])
    );
    assert_eq!(super::super::source_of(&[0; 15]), Err(ERR_INVALID));
    assert_eq!(super::super::source_of(&[0; 20]), Err(ERR_INVALID));
}

#[test]
fn a_formats_part_of_a_file_is_an_image_of_the_parts_size() {
    let storage = MemoryStorage::new();
    halves(&storage, "c/a.gray");
    let t = ImageTable::new();
    let map = |part: &[u8]| {
        let image = FmtImage::new("a.gray", 6, 8, 4, 0, part);
        t.map_fmt_image(&storage, A, 7, "c", &image, true, true)
    };
    // A panel: the right half. The opener gets a picture of its size.
    let right = map(&words([4, 0, 4, 4])).unwrap().unwrap();
    assert_eq!((right.width, right.height, right.index), (4, 4, 6));
    let info = t.info(A, right.image).unwrap();
    assert_eq!((info.width, info.height), (4, 4));
    let mut fb = screen();
    t.draw(A, right.image, &mut fb.canvas(), &storage, 0, 0, 8, 8, 0)
        .unwrap();
    assert_eq!((gray(&mut fb, 0, 0), gray(&mut fb, 7, 7)), (200, 200));
    // A part of a part is counted from the part's corner.
    let mut fb = screen();
    t.draw_part(
        A,
        right.image,
        &mut fb.canvas(),
        &storage,
        (0, 0, 4, 4),
        0,
        Some([0, 0, 2, 2]),
    )
    .unwrap();
    assert_eq!(gray(&mut fb, 0, 0), 200);
    assert_eq!(
        t.draw_part(
            A,
            right.image,
            &mut fb.canvas(),
            &storage,
            (0, 0, 4, 4),
            0,
            Some([3, 0, 2, 2])
        ),
        Err(ERR_INVALID),
        "outside the part, though inside the file"
    );
    // The same part again is the same image; another part and the whole
    // file are images of their own.
    assert_eq!(
        map(&words([4, 0, 4, 4])).unwrap().unwrap().image,
        right.image
    );
    let corner = map(&words([0, 0, 2, 2])).unwrap().unwrap();
    let whole = map(&[]).unwrap().unwrap();
    assert!(corner.image != right.image && whole.image != right.image);
    assert_eq!((whole.width, whole.height), (8, 4));
    let mut fb = screen();
    t.draw(A, corner.image, &mut fb.canvas(), &storage, 0, 0, 6, 6, 0)
        .unwrap();
    assert_eq!((gray(&mut fb, 0, 0), gray(&mut fb, 5, 5)), (90, 90));
    // A part that lies (not 16 bytes, empty, not inside the file) is no
    // picture: left out, never a failure of the stream.
    for lie in [
        words([4, 0, 5, 4]),
        words([0, 0, 0, 1]),
        words([0, 4, 1, 1]),
        words([u32::MAX, 0, 2, 2]),
        std::vec![0; 15],
        std::vec![0; 17],
    ] {
        assert_eq!(map(&lie), Ok(None), "{lie:?}");
    }
}

#[test]
fn the_stage_is_asked_once_for_a_prepared_picture_and_its_curve_is_drawn_through() {
    let storage = MemoryStorage::new();
    let t = ImageTable::new();
    assert!(!t.tone_pending());
    let path = prepare::file_name(A, 5).unwrap();
    picture(&storage, &path, (8, 4), |_, _| 100);
    let id = t.add_prepared(A, 5, 8, 4, false).unwrap();
    assert!(t.tone_pending());
    let wanted = t.tone_wanted().unwrap();
    assert_eq!(
        (wanted.owner, wanted.id, wanted.width, wanted.height),
        (A, id, 8, 4)
    );
    assert_eq!(wanted.path, path);
    // Asked once: nothing waits any more.
    assert!(t.tone_wanted().is_none());
    assert!(!t.tone_pending());
    // Every pixel is gray 100: the seventh bin is the fullest, the others empty.
    let (bins, flags) = histogram(&storage, &wanted.path).unwrap();
    let mut expected = [0_u8; BINS];
    expected[6] = 255;
    assert_eq!((bins, flags), (expected, 0));
    assert!(histogram(&storage, "none").is_none());
    // An answer that is none keeps nothing.
    assert!(!t.tone_answer(A, id, &[0; 255], 0));
    assert!(!t.tone_answer(A, id, &[], 3));
    assert!(!t.tone_answer(A, id, &[], 4));
    assert!(!t.tone_answer(A, id + 1, &[], 1));
    let mut fb = screen();
    t.draw(A, id, &mut fb.canvas(), &storage, 0, 0, 8, 4, 0)
        .unwrap();
    assert_eq!(gray(&mut fb, 3, 2), 100);
    // A curve: gray 100 is drawn as 30, at every draw, parts too.
    let mut curve = [0_u8; TONE_BYTES];
    for (value, slot) in curve.iter_mut().enumerate() {
        *slot = value as u8;
    }
    curve[100] = 30;
    assert!(t.tone_answer(A, id, &curve, 0));
    for _ in 0..2 {
        let mut fb = screen();
        t.draw(A, id, &mut fb.canvas(), &storage, 0, 0, 8, 4, 0)
            .unwrap();
        assert_eq!((gray(&mut fb, 0, 0), gray(&mut fb, 7, 3)), (30, 30));
    }
    let mut fb = screen();
    t.draw_part(
        A,
        id,
        &mut fb.canvas(),
        &storage,
        (0, 0, 4, 4),
        0,
        Some([2, 1, 2, 2]),
    )
    .unwrap();
    assert_eq!(gray(&mut fb, 1, 1), 30);
    // Dots where the draw names neither; the draw's own choice wins.
    assert!(t.tone_answer(A, id, &[], 1));
    let mut fb = screen();
    t.draw(A, id, &mut fb.canvas(), &storage, 0, 0, 8, 4, 0)
        .unwrap();
    for (x, y) in [(0, 0), (3, 1), (7, 3)] {
        assert!(matches!(gray(&mut fb, x, y), 0 | 255), "dots at ({x}, {y})");
    }
    let mut fb = screen();
    t.draw(A, id, &mut fb.canvas(), &storage, 0, 0, 8, 4, GRAYS)
        .unwrap();
    assert_eq!(gray(&mut fb, 3, 2), 100);
    // A document's picture is not asked for here; without a subscriber
    // nothing waits.
    t.add(A, document(0), "c/a.gray", 8, 4, false).unwrap();
    assert!(!t.tone_pending());
    t.add_prepared(A, 6, 8, 4, false).unwrap();
    assert!(t.tone_pending());
    t.tones_skip();
    assert!(!t.tone_pending() && t.tone_wanted().is_none());
}
