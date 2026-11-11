#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! Image ids and the band drawing of slots 36 and 37.
extern crate std;
use super::*;
use paperpaw_graphics::Framebuffer;
use paperpaw_platform::{PixelFormat, Point};
use paperpaw_sdk::MemoryStorage;
use paperpaw_sdk::image::{GrayWriter, ImageSink};

const A: u64 = 1;
const B: u64 = 2;

fn doc(document: u32, index: u32) -> Origin {
    Origin::Document { document, index }
}

fn add(t: &ImageTable, owner: u64, origin: Origin, path: &str) -> Result<u32, i32> {
    t.add(owner, origin, path, 8, 4, false)
}

/// A gray picture of `width` x `height`, every pixel `gray`.
fn gray_file(storage: &MemoryStorage, path: &str, width: u32, height: u32, gray: u8) {
    let mut writer = GrayWriter::new(storage, path);
    writer.start(width, height).unwrap();
    let row = std::vec![gray; width as usize];
    for _ in 0..height {
        writer.row(&row).unwrap();
    }
    writer.finish().unwrap();
}

fn screen() -> Framebuffer {
    let mut fb = Framebuffer::new(40, 40, PixelFormat::Gray8).unwrap();
    fb.canvas().clear(paperpaw_platform::Color::WHITE);
    fb
}

fn gray(canvas: &Canvas<'_>, x: u16, y: u16) -> u8 {
    canvas
        .gray_at(Point::new(i32::from(x), i32::from(y)))
        .unwrap()
}

#[test]
fn ids_are_per_plugin_checked_and_never_zero() {
    let t = ImageTable::new();
    let a = add(&t, A, doc(1, 0), "c/a.gray").unwrap();
    let b = add(&t, B, doc(2, 0), "c/b.gray").unwrap();
    assert!(a != 0 && b != 0 && a != b);
    assert_eq!(t.info(B, a), Err(ERR_NOT_FOUND), "another plugin's id");
    assert_eq!(t.info(A, 0), Err(ERR_NOT_FOUND));
    assert_eq!(t.info(A, 9999), Err(ERR_NOT_FOUND));
    assert_eq!(
        t.info(A, a),
        Ok(Info {
            image: a,
            index: 0,
            width: 8,
            height: 4,
            flags: 0
        })
    );
    let colour = t
        .add(A, doc(1, u32::MAX), "c/cover.gray", 3, 5, true)
        .unwrap();
    let info = t.info(A, colour).unwrap();
    assert_eq!((info.index, info.flags), (u32::MAX, 1), "the cover, colour");
    // The same picture of the same document keeps its id; another document's
    // is another picture.
    assert_eq!(add(&t, A, doc(1, 0), "c/a.gray"), Ok(a));
    assert_ne!(add(&t, A, doc(7, 0), "c/a.gray"), Ok(a));
    let mut inner = t.inner.borrow_mut();
    inner.next_id = u32::MAX;
    drop(inner);
    assert_eq!(
        add(&t, A, doc(1, 3), "c/x.gray"),
        Err(ERR_NO_SPACE),
        "spent"
    );
}

#[test]
fn a_hostile_picture_is_refused_before_it_is_kept() {
    let t = ImageTable::new();
    for (w, h) in [(0, 4), (4, 0), (DOC_SIDE_MAX + 1, 4), (4, DOC_SIDE_MAX + 1)] {
        assert_eq!(t.add(A, doc(1, 0), "p", w, h, false), Err(ERR_INVALID));
    }
    assert_eq!(t.add(A, doc(1, 0), "", 4, 4, false), Err(ERR_INVALID));
    let long = "p".repeat(PATH_MAX + 1);
    assert_eq!(t.add(A, doc(1, 0), &long, 4, 4, false), Err(ERR_INVALID));
    assert!(
        t.add(A, doc(1, 0), "p", DOC_SIDE_MAX, DOC_SIDE_MAX, false)
            .is_ok()
    );
    // A prepared image may be larger (the device's `max_side` limits it).
    assert!(t.add_prepared(A, 1, MAX_SIDE, MAX_SIDE, false).is_ok());
    assert_eq!(
        t.add_prepared(A, 2, MAX_SIDE + 1, 4, false),
        Err(ERR_INVALID)
    );
}

#[test]
fn the_ninth_document_image_waits_for_a_drawn_or_unused_one() {
    let t = ImageTable::new();
    let storage = MemoryStorage::new();
    let ids: Vec<u32> = (0..DOC_IMAGES as u32)
        .map(|n| {
            let path = std::format!("c/{n}.gray");
            gray_file(&storage, &path, 8, 4, 100);
            add(&t, A, doc(1, n), &path).unwrap()
        })
        .collect();
    // All eight handed out and none drawn: the ninth is busy, nothing changes.
    assert_eq!(add(&t, A, doc(1, 8), "c/8.gray"), Err(ERR_BUSY));
    assert!(ids.iter().all(|id| t.info(A, *id).is_ok()));
    // Another plugin has its own eight.
    assert!(add(&t, B, doc(2, 0), "c/b.gray").is_ok());
    // Draw the 3rd and then the 5th: the ninth takes the least recently drawn.
    let mut fb = screen();
    let mut canvas = fb.canvas();
    for n in [2, 4] {
        t.draw(A, ids[n], &mut canvas, &storage, 0, 0, 8, 4, 0)
            .unwrap();
    }
    let ninth = add(&t, A, doc(1, 8), "c/8.gray").unwrap();
    assert_eq!(
        t.info(A, ids[2]),
        Err(ERR_NOT_FOUND),
        "the oldest drawn went"
    );
    assert!(t.info(A, ids[4]).is_ok() && t.info(A, ninth).is_ok());
    // An ended id is a draw of NOT_FOUND, and asking again gives a new id.
    assert_eq!(
        t.draw(A, ids[2], &mut canvas, &storage, 0, 0, 8, 4, 0),
        Err(ERR_NOT_FOUND)
    );
}

#[test]
fn a_frame_that_draws_another_picture_unprotects_the_ids_it_did_not_use() {
    let t = ImageTable::new();
    let storage = MemoryStorage::new();
    for n in 0..DOC_IMAGES {
        gray_file(&storage, &std::format!("c/{n}.gray"), 8, 4, 100);
    }
    let ids: Vec<u32> = (0..DOC_IMAGES as u32)
        .map(|n| add(&t, A, doc(1, n), &std::format!("c/{n}.gray")).unwrap())
        .collect();
    // A frame is not over until it ends: all eight are pinned meanwhile.
    assert_eq!(add(&t, A, doc(1, 8), "c/8.gray"), Err(ERR_BUSY));
    // A frame that drew only the first: the others are free to go.
    let mut fb = screen();
    t.draw(A, ids[0], &mut fb.canvas(), &storage, 0, 0, 8, 4, 0)
        .unwrap();
    t.end_frame(A).unwrap();
    let ninth = add(&t, A, doc(1, 8), "c/8.gray").unwrap();
    assert!(t.info(A, ninth).is_ok());
    assert!(t.info(A, ids[0]).is_ok(), "the drawn one is the newest");
    let gone = ids.iter().filter(|id| t.info(A, **id).is_err()).count();
    assert_eq!(gone, 1);
    // Another plugin's frames do not touch this one's protection.
    t.end_frame(B).unwrap();
}

#[test]
fn a_text_only_frame_ends_the_protection_of_abandoned_ids() {
    let t = ImageTable::new();
    for n in 0..DOC_IMAGES {
        add(&t, A, doc(1, n as u32), &std::format!("c/{n}.gray")).unwrap();
    }
    assert_eq!(add(&t, A, doc(1, 8), "c/8.gray"), Err(ERR_BUSY));
    // The reader moved on and draws a frame of text: nothing pins the ids.
    t.end_frame(A).unwrap();
    assert!(add(&t, A, doc(1, 8), "c/8.gray").is_ok());
    // Another plugin's text-only frame does not free this one's new id.
    t.end_frame(B).unwrap();
    assert_eq!(
        t.lock()
            .unwrap()
            .entries
            .iter()
            .filter(|e| e.protected)
            .count(),
        1
    );
}

#[test]
fn prepared_images_have_their_own_eight_and_are_never_evicted() {
    let t = ImageTable::new();
    let own: Vec<u32> = (0..PREPARED_IMAGES)
        .map(|n| add(&t, A, Origin::Prepared, &std::format!("p/{n}")).unwrap())
        .collect();
    assert_eq!(add(&t, A, Origin::Prepared, "p/9"), Err(ERR_FULL));
    // Document images do not count against them, and do not evict them.
    for n in 0..DOC_IMAGES as u32 {
        add(&t, A, doc(1, n), &std::format!("c/{n}")).unwrap();
    }
    assert_eq!(add(&t, A, doc(1, 99), "c/99"), Err(ERR_BUSY));
    assert!(own.iter().all(|id| t.info(A, *id).is_ok()));
    // image_release frees one, once; only the owner's.
    assert_eq!(t.release(B, own[0]), Err(ERR_NOT_FOUND));
    t.release(A, own[0]).unwrap();
    assert_eq!(t.release(A, own[0]), Err(ERR_NOT_FOUND));
    assert!(add(&t, A, Origin::Prepared, "p/9").is_ok());
}

#[test]
fn documents_and_plugins_ending_end_their_images() {
    let t = ImageTable::new();
    let d1 = add(&t, A, doc(1, 0), "c/1").unwrap();
    let d2 = add(&t, A, doc(2, 0), "c/2").unwrap();
    let own = add(&t, A, Origin::Prepared, "p/1").unwrap();
    let other = add(&t, B, doc(1, 0), "c/1").unwrap();
    t.document_closed(1).unwrap();
    assert_eq!(t.info(A, d1), Err(ERR_NOT_FOUND));
    assert_eq!(t.info(B, other), Err(ERR_NOT_FOUND), "any opener's");
    assert!(t.info(A, d2).is_ok() && t.info(A, own).is_ok());
    t.owner_stopped(A).unwrap();
    assert_eq!(t.info(A, d2), Err(ERR_NOT_FOUND));
    assert_eq!(t.info(A, own), Err(ERR_NOT_FOUND));
    t.owner_stopped(A).unwrap();
}

#[test]
fn a_picture_fits_its_rectangle_keeping_its_shape_or_covers_it() {
    // A wide 8x4 picture in a 20x20 square.
    let fit = place(5, 5, 20, 20, 8, 4, false).unwrap().unwrap();
    assert_eq!((fit.left, fit.top, fit.width, fit.height), (5, 10, 20, 10));
    let cover = place(5, 5, 20, 20, 8, 4, true).unwrap().unwrap();
    assert_eq!(
        (cover.left, cover.top, cover.width, cover.height),
        (-5, 5, 40, 20),
        "centred, wider than the rectangle"
    );
    // A tall picture in a wide rectangle.
    let tall = place(0, 0, 30, 10, 4, 8, false).unwrap().unwrap();
    assert_eq!(
        (tall.left, tall.top, tall.width, tall.height),
        (12, 0, 5, 10)
    );
    assert_eq!(place(0, 0, 0, 10, 4, 8, false), Ok(None));
    assert_eq!(place(0, 0, 10, 0, 4, 8, true), Ok(None));
    // A wide rectangle is fine while the drawn picture is small; one that
    // would need a picture beyond 65,535 pixels is refused, nothing wraps.
    assert!(place(0, 0, 70_000, 10, 4, 4, false).unwrap().is_some());
    assert_eq!(place(0, 0, 70_000, 10, 4, 4, true), Err(ERR_INVALID));
    assert!(place(i32::MIN, i32::MAX, 100, 100, 4, 4, false).is_ok());
}

#[test]
fn drawing_is_banded_clipped_and_checked() {
    let t = ImageTable::new();
    let storage = MemoryStorage::new();
    gray_file(&storage, "c/p.gray", 8, 4, 40);
    let id = add(&t, A, doc(1, 0), "c/p.gray").unwrap();
    let mut fb = screen();
    let mut canvas = fb.canvas();
    // Fit: 16x8 centred in 20x20 at (10, 10): rows 16..24.
    t.draw(A, id, &mut canvas, &storage, 10, 10, 20, 20, 0)
        .unwrap();
    assert_eq!(gray(&canvas, 20, 20), 40, "inside the picture");
    assert_eq!(gray(&canvas, 20, 12), 255, "above it, in the rectangle");
    assert_eq!(gray(&canvas, 5, 20), 255, "left of the rectangle");
    // Fill covers the whole rectangle.
    let mut fb = screen();
    let mut canvas = fb.canvas();
    t.draw(A, id, &mut canvas, &storage, 10, 10, 20, 20, FILL)
        .unwrap();
    assert_eq!(gray(&canvas, 20, 12), 40);
    assert_eq!(gray(&canvas, 9, 20), 255, "but never outside it");
    assert_eq!(gray(&canvas, 30, 20), 255);
    // Partly (or wholly) off the canvas: clipped, never a panic or an error.
    let mut fb = screen();
    let mut canvas = fb.canvas();
    for (x, y) in [
        (-10, -10),
        (30, 30),
        (-100, 5),
        (i32::MAX - 3, 0),
        (i32::MIN, i32::MIN),
    ] {
        assert_eq!(
            t.draw(A, id, &mut canvas, &storage, x, y, 20, 20, 0),
            Ok(())
        );
    }
    assert_eq!(
        gray(&canvas, 2, 2),
        40,
        "the corner draw reaches the canvas"
    );
    assert_eq!(gray(&canvas, 20, 20), 255, "between the draws, untouched");
    assert_eq!(t.draw(A, id, &mut canvas, &storage, 0, 0, 0, 20, 0), Ok(()));
    // Not this plugin's, or ended.
    assert_eq!(
        t.draw(B, id, &mut canvas, &storage, 0, 0, 8, 4, 0),
        Err(ERR_NOT_FOUND)
    );
    // Reserved flag bits are unsupported, never silently ignored.
    for bad in [0x20, 0x40, 0x80, 0x1_0000, u32::MAX, FILL | 0x40] {
        assert_eq!(
            t.draw(A, id, &mut canvas, &storage, 0, 0, 8, 4, bad),
            Err(ERR_UNSUPPORTED)
        );
    }
}

#[test]
fn stretch_fills_the_rectangle_and_drops_the_shape() {
    let t = ImageTable::new();
    let storage = MemoryStorage::new();
    gray_file(&storage, "c/p.gray", 8, 4, 40);
    let id = add(&t, A, doc(1, 0), "c/p.gray").unwrap();
    let mut fb = screen();
    let mut canvas = fb.canvas();
    // 8x4 into 20x20 at (10, 10): every pixel of the rectangle, none outside.
    t.draw(A, id, &mut canvas, &storage, 10, 10, 20, 20, STRETCH)
        .unwrap();
    for (x, y) in [(10, 10), (29, 10), (10, 29), (29, 29), (20, 12)] {
        assert_eq!(gray(&canvas, x, y), 40, "({x}, {y})");
    }
    for (x, y) in [(9, 20), (30, 20), (20, 9), (20, 30)] {
        assert_eq!(gray(&canvas, x, y), 255, "({x}, {y})");
    }
    // An empty rectangle draws nothing; one over 65,535 pixels is invalid.
    assert_eq!(
        t.draw(A, id, &mut canvas, &storage, 0, 0, 0, 20, STRETCH),
        Ok(())
    );
    assert_eq!(
        t.draw(A, id, &mut canvas, &storage, 0, 0, 65_536, 20, STRETCH),
        Err(ERR_INVALID)
    );
}

#[test]
fn flags_that_choose_two_ways_at_once_are_invalid_before_anything_else() {
    let t = ImageTable::new();
    let storage = MemoryStorage::new();
    gray_file(&storage, "c/p.gray", 8, 4, 40);
    let id = add(&t, A, doc(1, 0), "c/p.gray").unwrap();
    let mut fb = screen();
    let mut canvas = fb.canvas();
    for bad in [FILL | STRETCH, DOTS | GRAYS, FILL | STRETCH | SMALL | DOTS] {
        assert_eq!(
            t.draw(A, id, &mut canvas, &storage, 0, 0, 8, 4, bad),
            Err(ERR_INVALID),
            "{bad:#x}"
        );
        assert_eq!(
            t.draw(B, id, &mut canvas, &storage, 0, 0, 8, 4, bad),
            Err(ERR_INVALID),
            "{bad:#x}: invalid whoever asks"
        );
    }
    assert_eq!(gray(&canvas, 3, 2), 255, "nothing was drawn");
    // Each alone, and with the others that go with it, is fine.
    for good in [
        FILL,
        STRETCH,
        DOTS,
        GRAYS,
        STRETCH | DOTS | SMALL,
        FILL | GRAYS,
    ] {
        assert_eq!(
            t.draw(A, id, &mut canvas, &storage, 0, 0, 8, 4, good),
            Ok(()),
            "{good:#x}"
        );
    }
}

#[test]
fn dots_are_black_and_white_where_the_panel_has_grays_and_grays_are_the_panels() {
    let t = ImageTable::new();
    let storage = MemoryStorage::new();
    gray_file(&storage, "c/p.gray", 16, 16, 128);
    let id = t.add(A, doc(1, 0), "c/p.gray", 16, 16, false).unwrap();
    let count = |flags| {
        let mut fb = screen();
        let mut canvas = fb.canvas();
        t.draw(A, id, &mut canvas, &storage, 0, 0, 16, 16, flags)
            .unwrap();
        let mut seen = [0_u32; 3];
        for y in 0..16 {
            for x in 0..16 {
                match gray(&canvas, x, y) {
                    0 => seen[0] += 1,
                    255 => seen[1] += 1,
                    128 => seen[2] += 1,
                    other => panic!("an unexpected gray {other}"),
                }
            }
        }
        seen
    };
    // The panel's own rule on a Gray8 frame: the gray as it is.
    assert_eq!(count(0), [0, 0, 256]);
    assert_eq!(count(GRAYS), [0, 0, 256]);
    // Dots: only black and white, about half each.
    let [black, white, mid] = count(DOTS);
    assert_eq!(mid, 0);
    assert_eq!(black + white, 256);
    assert!((96..=160).contains(&white), "{white} white of 256");
}

fn alpha_file(storage: &MemoryStorage, path: &str, width: u32, height: u32, gray: u8, alpha: u8) {
    let mut writer = paperpaw_sdk::image::AlphaWriter::new(storage, path);
    writer.start(width, height).unwrap();
    let row: std::vec::Vec<u8> = (0..width).flat_map(|_| [gray, alpha]).collect();
    for _ in 0..height {
        writer.row_alpha(&row).unwrap();
    }
    writer.finish().unwrap();
}

#[test]
fn transparency_small_and_alpha_pictures_blend_over_what_is_below() {
    let t = ImageTable::new();
    let storage = MemoryStorage::new();
    gray_file(&storage, "c/p.gray", 8, 4, 0);
    let id = add(&t, A, doc(1, 0), "c/p.gray").unwrap();
    // Opaque black, then transparency 255 draws nothing, 128 shows both.
    let mut fb = screen();
    let mut canvas = fb.canvas();
    let half = 128 << TRANSPARENCY_SHIFT;
    t.draw(
        A,
        id,
        &mut canvas,
        &storage,
        0,
        0,
        8,
        4,
        255 << TRANSPARENCY_SHIFT,
    )
    .unwrap();
    assert_eq!(gray(&canvas, 2, 2), 255, "fully transparent: nothing");
    t.draw(A, id, &mut canvas, &storage, 0, 0, 8, 4, half)
        .unwrap();
    let mixed = gray(&canvas, 2, 2);
    assert!((100..160).contains(&mixed), "half: {mixed}");
    // A small picture does not count towards the ghost refresh, a large does.
    let mut fb = screen();
    {
        let mut canvas = fb.canvas();
        t.draw(A, id, &mut canvas, &storage, 0, 0, 8, 4, SMALL)
            .unwrap();
    }
    assert_eq!(fb.take_pictured(), 0);
    {
        let mut canvas = fb.canvas();
        t.draw(A, id, &mut canvas, &storage, 0, 0, 8, 4, 0).unwrap();
    }
    assert!(fb.take_pictured() > 0);
    // An alpha cache image: clear parts show what is below.
    alpha_file(&storage, "c/a.alpha", 8, 4, 0, 0);
    let clear = t.add(
        A,
        Origin::Document {
            document: 1,
            index: 1,
        },
        "c/a.alpha",
        8,
        4,
        false,
    );
    let clear = clear.unwrap();
    let mut fb = screen();
    let mut canvas = fb.canvas();
    t.draw(A, clear, &mut canvas, &storage, 0, 0, 8, 4, 0)
        .unwrap();
    assert_eq!(gray(&canvas, 2, 2), 255, "clear alpha: paper stays");
    alpha_file(&storage, "c/b.alpha", 8, 4, 0, 255);
    let solid = t
        .add(
            A,
            Origin::Document {
                document: 1,
                index: 2,
            },
            "c/b.alpha",
            8,
            4,
            false,
        )
        .unwrap();
    t.draw(A, solid, &mut canvas, &storage, 0, 0, 8, 4, 0)
        .unwrap();
    assert_eq!(gray(&canvas, 2, 2), 0, "solid alpha: black");
}

#[test]
fn a_file_the_card_cannot_give_or_that_lies_draws_nothing() {
    let t = ImageTable::new();
    let storage = MemoryStorage::new();
    gray_file(&storage, "c/real.gray", 8, 4, 40);
    gray_file(&storage, "c/other.gray", 6, 6, 40);
    let missing = add(&t, A, doc(1, 0), "c/none.gray").unwrap();
    let wrong = add(&t, A, doc(1, 1), "c/other.gray").unwrap();
    let real = add(&t, A, doc(1, 2), "c/real.gray").unwrap();
    let mut fb = screen();
    let mut canvas = fb.canvas();
    assert_eq!(
        t.draw(A, missing, &mut canvas, &storage, 0, 0, 8, 4, 0),
        Err(ERR_IO)
    );
    // Its header says 6x6, the id says 8x4.
    assert_eq!(
        t.draw(A, wrong, &mut canvas, &storage, 0, 0, 8, 4, 0),
        Err(ERR_INVALID)
    );
    // Garbage of any length is not a picture.
    let junk = MemoryStorage::new().with("c/real.gray", "not a picture at all");
    assert!(matches!(
        t.draw(A, real, &mut canvas, &junk, 0, 0, 8, 4, 0),
        Err(ERR_IO | ERR_INVALID)
    ));
    let short = MemoryStorage::new().with("c/real.gray", "PPGR");
    assert!(t.draw(A, real, &mut canvas, &short, 0, 0, 8, 4, 0).is_err());
    for pixel in 0..40u16 {
        assert_eq!(gray(&canvas, pixel, 1), 255, "nothing was drawn");
    }
}

#[test]
fn info_is_one_doc_image_record_that_must_fit() {
    let info = Info {
        image: 3,
        index: 9,
        width: 100,
        height: 200,
        flags: 1,
    };
    let mut out = [0u8; 64];
    let used = encode_info(info, &mut out).unwrap();
    assert!(used > 0 && used <= 64);
    let Record::ServiceDocImage(row) = wire::record_decode_one(&out[..used]).unwrap() else {
        panic!()
    };
    assert_eq!(
        (row.image, row.index, row.width, row.height, row.flags),
        (3, 9, 100, 200, 1)
    );
    let mut small = [0u8; 4];
    assert_eq!(encode_info(info, &mut small), Err(ERR_NO_SPACE));
    assert_eq!(small, [0; 4]);
}

fn fmt_image(index: u32, width: u32, height: u32) -> wire::ServiceDocImage {
    wire::ServiceDocImage {
        image: 0,
        index,
        width,
        height,
        flags: 0,
    }
}

const CACHE: &str = ".paperpaw/cache/k/f/c";

#[test]
fn a_format_picture_becomes_an_image_id_after_the_cache_check() {
    let t = ImageTable::new();
    let storage = MemoryStorage::new();
    gray_file(&storage, ".paperpaw/cache/k/f/c/a.gray", 8, 4, 40);
    let map = |path: &str, image: &wire::ServiceDocImage, in_block| {
        t.map_document(&storage, A, 7, CACHE, path, image, in_block, true)
    };
    let id = map("a.gray", &fmt_image(3, 8, 4), true).unwrap().unwrap();
    assert_ne!(id, 0);
    // The same path in the same document is the same id.
    assert_eq!(map("a.gray", &fmt_image(3, 8, 4), true), Ok(Some(id)));
    assert_eq!(t.info(A, id).unwrap().index, 3);
    // Left out of the stream, never failing it: wrong size, missing file,
    // and every path the root-2 rules refuse or reserve.
    assert_eq!(map("a.gray", &fmt_image(3, 9, 4), true), Ok(None));
    assert_eq!(map("none.gray", &fmt_image(4, 8, 4), true), Ok(None));
    for bad in [
        "",
        "/a.gray",
        "../a.gray",
        "x/../a.gray",
        "x//a.gray",
        "a\\b",
        "a\0b",
        ".complete",
        ".host/a.gray",
        "./a.gray",
        "./.host/a.gray",
        ".HOST/a.gray",
        ".Host/x/a.gray",
        "HOST~1/a.gray",
        "host~12/a.gray",
        ".COMPLETE",
        "COMPLE~1",
        "a:b.gray",
        "a.gray.",
        "nul.gray",
    ] {
        assert_eq!(map(bad, &fmt_image(5, 8, 4), true), Ok(None), "{bad:?}");
    }
    // Names that only look alike are fine.
    gray_file(&storage, ".paperpaw/cache/k/f/c/hostile/a.gray", 8, 4, 40);
    assert!(
        map("hostile/a.gray", &fmt_image(7, 8, 4), true)
            .unwrap()
            .is_some()
    );
    // A cover (`u32::MAX`) is a document image like the others.
    gray_file(&storage, ".paperpaw/cache/k/f/c/cover.gray", 6, 6, 9);
    assert!(map("cover.gray", &fmt_image(u32::MAX, 6, 6), false).is_ok());
}

#[test]
fn the_ninth_unseen_picture_is_busy_or_a_zero_id_in_a_block() {
    let t = ImageTable::new();
    let storage = MemoryStorage::new();
    for n in 0..9 {
        gray_file(&storage, &std::format!("{CACHE}/{n}.gray"), 8, 4, 40);
    }
    for n in 0..8 {
        let r = t.map_document(
            &storage,
            A,
            7,
            CACHE,
            &std::format!("{n}.gray"),
            &fmt_image(n, 8, 4),
            false,
            true,
        );
        assert!(matches!(r, Ok(Some(id)) if id != 0));
    }
    let ninth = |in_block| {
        t.map_document(
            &storage,
            A,
            7,
            CACHE,
            "8.gray",
            &fmt_image(8, 8, 4),
            in_block,
            true,
        )
    };
    assert_eq!(ninth(true), Ok(Some(0)), "in a block: image 0, no id yet");
    assert_eq!(ninth(false), Err(ERR_BUSY), "op 16/17/18: busy");
    // Another plugin has its own eight.
    assert!(matches!(
        t.map_document(&storage, B, 9, CACHE, "8.gray", &fmt_image(8, 8, 4), false, true),
        Ok(Some(id)) if id != 0
    ));
}

#[test]
fn a_fmt_image_record_becomes_the_openers_doc_image() {
    use super::super::stages_wire::core_format_v1::FmtImage;
    let t = ImageTable::new();
    let storage = MemoryStorage::new();
    gray_file(&storage, ".paperpaw/cache/k/f/c/a.gray", 8, 4, 40);
    let fmt = FmtImage::new("a.gray", 6, 8, 4, 0);
    let doc = t
        .map_fmt_image(&storage, A, 7, CACHE, &fmt, true, true)
        .unwrap()
        .unwrap();
    assert_ne!(doc.image, 0);
    assert_eq!((doc.index, doc.width, doc.height), (6, 8, 4));
    assert_eq!(doc.flags, 0, "decoded: no flag");
    // `fmt.image` bit 0 (not decoded yet) is `doc_image` bit 1; bit 0 there is
    // colour, which a format's record does not set; other bits are dropped.
    let pending = FmtImage::new("a.gray", 6, 8, 4, 0b101);
    let doc = t
        .map_fmt_image(&storage, A, 7, CACHE, &pending, true, true)
        .unwrap()
        .unwrap();
    assert_eq!(doc.flags, 0b10);
    let other = FmtImage::new("a.gray", 6, 8, 4, 0b110);
    let doc = t
        .map_fmt_image(&storage, A, 7, CACHE, &other, true, true)
        .unwrap()
        .unwrap();
    assert_eq!(doc.flags, 0);
    // A colour cache file: bit 0 where the screen keeps colour (as
    // `image.info` says), never on a gray screen.
    let mut writer = paperpaw_sdk::image::ColorWriter::new(&storage, ".paperpaw/cache/k/f/c/c.rgb");
    writer.start(8, 4).unwrap();
    for _ in 0..4 {
        writer.row_rgb(&[9; 8 * 3]).unwrap();
    }
    writer.finish().unwrap();
    let colour = FmtImage::new("c.rgb", 7, 8, 4, 1);
    let shown = t
        .map_fmt_image(&storage, A, 7, CACHE, &colour, true, true)
        .unwrap()
        .unwrap();
    assert_eq!(shown.flags, 0b11, "colour, not decoded yet");
    assert_eq!(t.info(A, shown.image).unwrap().flags, 1);
    let t = ImageTable::new();
    let gray_screen = t
        .map_fmt_image(&storage, A, 7, CACHE, &colour, true, false)
        .unwrap()
        .unwrap();
    assert_eq!(gray_screen.flags, 0b10, "no colour on a gray screen");
    assert_eq!(t.info(A, gray_screen.image).unwrap().flags, 0);
    // A picture the card does not vouch for is no doc_image.
    let lying = FmtImage::new("a.gray", 6, 9, 4, 0);
    assert_eq!(
        t.map_fmt_image(&storage, A, 7, CACHE, &lying, true, true),
        Ok(None)
    );
}

#[test]
fn a_picture_not_decoded_yet_comes_without_an_id_and_its_index() {
    use super::super::stages_wire::core_format_v1::FmtImage;
    let t = ImageTable::new();
    // No cache file yet: the format decodes it when the opener asks (op 16).
    let storage = MemoryStorage::new();
    let pending = FmtImage::new("img/12.gray", 12, 300, 200, 1);
    let edge = FmtImage::new("img/13.gray", 13, 1_600, 1, 1);
    assert!(matches!(
        t.map_fmt_image(&storage, A, 7, CACHE, &edge, true, true),
        Ok(Some(doc)) if doc.image == 0 && doc.width == 1_600
    ));
    assert_eq!(
        t.map_fmt_image(&storage, A, 7, CACHE, &pending, true, true),
        Ok(Some(wire::ServiceDocImage {
            image: 0,
            index: 12,
            width: 300,
            height: 200,
            flags: 0b10,
        }))
    );
    // Nothing was handed out for it.
    assert!(t.info(A, 1).is_err());
    // A decoded picture whose file is not there is still left out.
    let decoded = FmtImage::new("img/12.gray", 12, 300, 200, 0);
    assert_eq!(
        t.map_fmt_image(&storage, A, 7, CACHE, &decoded, true, true),
        Ok(None)
    );
    // A pending one is still checked for what can be checked without the
    // file: the path and a size a document image can have (1 to 1,600).
    for (n, bad) in [
        FmtImage::new("../x.gray", 12, 300, 200, 1),
        FmtImage::new(".host/x.gray", 12, 300, 200, 1),
        FmtImage::new("img/12.gray", 12, 0, 200, 1),
        FmtImage::new("img/12.gray", 12, 300, 1_601, 1),
        FmtImage::new("img/12.gray", 12, u32::MAX, 200, 1),
    ]
    .iter()
    .enumerate()
    {
        assert_eq!(
            t.map_fmt_image(&storage, A, 7, CACHE, bad, true, true),
            Ok(None),
            "case {n}"
        );
    }
}

#[test]
fn a_picture_not_decoded_yet_is_checked_at_the_edges() {
    use super::super::stages_wire::core_format_v1::FmtImage;
    let t = ImageTable::new();
    let storage = MemoryStorage::new();
    // A file there whose header says another size does not vouch for the
    // record: no id, the record's own size, as for a file not there yet.
    gray_file(&storage, ".paperpaw/cache/k/f/c/a.gray", 8, 4, 40);
    let other_size = FmtImage::new("a.gray", 6, 9, 4, 1);
    assert_eq!(
        t.map_fmt_image(&storage, A, 7, CACHE, &other_size, true, true),
        Ok(Some(wire::ServiceDocImage {
            image: 0,
            index: 6,
            width: 9,
            height: 4,
            flags: 0b10,
        }))
    );
    assert!(t.info(A, 1).is_err(), "no id handed out");
    // One pixel wide, 1,600 high: the narrowest and tallest box.
    let tall = FmtImage::new("img/14.gray", 14, 1, 1_600, 1);
    assert!(matches!(
        t.map_fmt_image(&storage, A, 7, CACHE, &tall, true, true),
        Ok(Some(doc)) if doc.image == 0 && (doc.width, doc.height) == (1, 1_600)
    ));
    // The cache folder, `/` and the path: 512 bytes pass, 513 do not.
    let folders = std::vec!["d".repeat(98); 4].join("/");
    let room = PATH_MAX - CACHE.len() - 1 - folders.len() - 1;
    for (name, kept) in [("n".repeat(room), true), ("n".repeat(room + 1), false)] {
        let path = std::format!("{folders}/{name}");
        let image = FmtImage::new(&path, 15, 300, 200, 1);
        let mapped = t.map_fmt_image(&storage, A, 7, CACHE, &image, true, true);
        assert_eq!(mapped.map(|doc| doc.is_some()), Ok(kept), "{}", path.len());
    }
}

/// A card whose reads stop working after a number of them.
struct Flaky {
    inner: MemoryStorage,
    reads_left: core::cell::Cell<usize>,
}

impl paperpaw_platform::Storage for Flaky {
    fn read_at(
        &self,
        path: &str,
        offset: u64,
        buf: &mut [u8],
    ) -> Result<usize, paperpaw_platform::StorageError> {
        let left = self.reads_left.get();
        if left == 0 {
            return Err(paperpaw_platform::StorageError::Io);
        }
        self.reads_left.set(left - 1);
        self.inner.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, paperpaw_platform::StorageError> {
        self.inner.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), paperpaw_platform::StorageError> {
        self.inner.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), paperpaw_platform::StorageError> {
        self.inner.append(path, data)
    }
    fn list(
        &self,
        dir: &str,
        visit: &mut dyn FnMut(paperpaw_platform::DirEntry<'_>),
    ) -> Result<(), paperpaw_platform::StorageError> {
        self.inner.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), paperpaw_platform::StorageError> {
        self.inner.remove(path)
    }
}

fn unseen(t: &ImageTable) -> bool {
    let inner = t.lock().unwrap();
    inner.entries[0].protected && inner.entries[0].drawn == 0
}

#[test]
fn a_truncated_file_is_refused_with_nothing_drawn() {
    let t = ImageTable::new();
    let storage = MemoryStorage::new();
    gray_file(&storage, "c/p.gray", 8, 4, 40);
    let id = add(&t, A, doc(1, 0), "c/p.gray").unwrap();
    // The header still says 8 x 4, the payload is cut short.
    let size = storage.size("c/p.gray").unwrap();
    let mut cut = std::vec![0_u8; usize::try_from(size).unwrap() - 5];
    assert_eq!(storage.read_at("c/p.gray", 0, &mut cut), Ok(cut.len()));
    storage.write("c/p.gray", &cut).unwrap();
    let mut fb = screen();
    let mut canvas = fb.canvas();
    let r = t.draw(A, id, &mut canvas, &storage, 0, 0, 8, 4, 0);
    assert_eq!(r, Err(ERR_IO));
    assert_eq!(gray(&canvas, 2, 2), 255, "nothing drawn");
    assert!(unseen(&t), "and not counted as drawn");
}

#[test]
fn a_card_that_stops_reading_ends_the_draw_with_io_and_is_not_a_draw() {
    let t = ImageTable::new();
    let inner = MemoryStorage::new();
    gray_file(&inner, "c/p.gray", 8, 40, 40);
    let id = t.add(A, doc(1, 0), "c/p.gray", 8, 40, false).unwrap();
    let card = Flaky {
        inner,
        // The header probe reads once; the rows come in one read each.
        reads_left: core::cell::Cell::new(1),
    };
    let mut fb = screen();
    let mut canvas = fb.canvas();
    let r = t.draw(A, id, &mut canvas, &card, 0, 0, 8, 40, 0);
    assert_eq!(r, Err(ERR_IO));
    assert_eq!(gray(&canvas, 2, 2), 255, "no band before the failed read");
    assert!(unseen(&t));
    // The same card, working again, draws it.
    card.reads_left.set(usize::MAX);
    assert_eq!(t.draw(A, id, &mut canvas, &card, 0, 0, 8, 40, 0), Ok(()));
    assert_eq!(gray(&canvas, 2, 2), 40);
}

#[test]
fn every_allocation_of_a_draw_may_fail_and_the_canvas_stays_as_it_was() {
    use crate::native_host::shell::tests::FAIL_NTH;
    let t = ImageTable::new();
    let storage = MemoryStorage::new();
    gray_file(&storage, "c/p.gray", 8, 4, 40);
    let id = add(&t, A, doc(1, 0), "c/p.gray").unwrap();
    let mut failed = 0;
    for nth in 1..=40 {
        let mut fb = screen();
        let mut canvas = fb.canvas();
        FAIL_NTH.with(|n| n.set(nth));
        let r = t.draw(A, id, &mut canvas, &storage, 0, 0, 16, 8, 0);
        FAIL_NTH.with(|n| n.set(0));
        match r {
            Ok(()) => assert_eq!(gray(&canvas, 8, 4), 40, "nth {nth}"),
            Err(error) => {
                failed += 1;
                assert_eq!(error, ERR_NO_MEMORY, "nth {nth}");
                assert_eq!(gray(&canvas, 8, 4), 255, "nth {nth}: nothing drawn");
            }
        }
    }
    assert!(failed > 0, "the draw does allocate, fallibly");
    // And it still draws.
    let mut fb = screen();
    let mut canvas = fb.canvas();
    assert_eq!(t.draw(A, id, &mut canvas, &storage, 0, 0, 16, 8, 0), Ok(()));
}

#[test]
fn a_rectangle_many_times_the_screen_costs_only_the_columns_on_it() {
    use crate::native_host::shell::tests::FAIL_NTH;
    let t = ImageTable::new();
    let storage = MemoryStorage::new();
    gray_file(&storage, "c/p.gray", 8, 4, 40);
    let id = add(&t, A, doc(1, 0), "c/p.gray").unwrap();
    let mut fb = screen();
    let mut canvas = fb.canvas();
    // 60,000 columns wide, nearly all of them off the 40-pixel canvas.
    assert_eq!(
        t.draw(A, id, &mut canvas, &storage, -29_980, 0, 60_000, 40, FILL),
        Ok(())
    );
    assert_eq!(gray(&canvas, 20, 20), 40);
    // Every allocation the draw makes may fail, none is sized by the rectangle.
    for nth in 1..=30 {
        let mut fb = screen();
        let mut canvas = fb.canvas();
        FAIL_NTH.with(|n| n.set(nth));
        let r = t.draw(A, id, &mut canvas, &storage, -29_980, 0, 60_000, 40, FILL);
        FAIL_NTH.with(|n| n.set(0));
        match r {
            Ok(()) => assert_eq!(gray(&canvas, 20, 20), 40),
            Err(e) => {
                assert_eq!(e, ERR_NO_MEMORY, "nth {nth}");
                assert_eq!(gray(&canvas, 20, 20), 255);
            }
        }
    }
}

#[test]
fn a_file_longer_than_its_header_or_not_a_picture_is_invalid_not_io() {
    let t = ImageTable::new();
    let storage = MemoryStorage::new();
    gray_file(&storage, "c/p.gray", 8, 4, 40);
    let id = add(&t, A, doc(1, 0), "c/p.gray").unwrap();
    let mut fb = screen();
    let mut canvas = fb.canvas();
    storage.append("c/p.gray", b"extra").unwrap();
    assert_eq!(
        t.draw(A, id, &mut canvas, &storage, 0, 0, 8, 4, 0),
        Err(ERR_INVALID)
    );
    storage.write("c/p.gray", &[b'X'; 44]).unwrap();
    assert_eq!(
        t.draw(A, id, &mut canvas, &storage, 0, 0, 8, 4, 0),
        Err(ERR_INVALID)
    );
    assert_eq!(gray(&canvas, 2, 2), 255);
}

#[test]
fn a_dithered_draw_keeps_nothing_and_every_allocation_of_it_may_fail() {
    use crate::native_host::shell::tests::FAIL_NTH;
    let t = ImageTable::new();
    let storage = MemoryStorage::new();
    gray_file(&storage, "c/p.gray", 8, 40, 100);
    let id = t.add(A, doc(1, 0), "c/p.gray", 8, 40, false).unwrap();
    let mono = || {
        let mut fb = Framebuffer::new(40, 40, PixelFormat::Mono1).unwrap();
        fb.canvas().clear(paperpaw_platform::Color::WHITE);
        fb
    };
    // Whole, then in two bands: the same picture, as nothing carries over.
    let mut whole = mono();
    t.draw(A, id, &mut whole.canvas(), &storage, 0, 0, 40, 40, FILL)
        .unwrap();
    let mut banded = mono();
    for (top, bottom) in [(0_u16, 17_u16), (17, 40)] {
        let mut canvas = banded.canvas();
        let mut band = canvas.clipped(Rect::new(0, top, 40, bottom - top));
        t.draw(A, id, &mut band, &storage, 0, 0, 40, 40, FILL)
            .unwrap();
    }
    let at = |fb: &mut Framebuffer| {
        let canvas = fb.canvas();
        let mut v = std::vec::Vec::new();
        for y in 0..40 {
            for x in 0..40 {
                v.push(gray(&canvas, x, y));
            }
        }
        v
    };
    assert_eq!(at(&mut whole), at(&mut banded));
    for nth in 1..=40 {
        let mut fb = mono();
        FAIL_NTH.with(|n| n.set(nth));
        let r = t.draw(A, id, &mut fb.canvas(), &storage, 0, 0, 40, 40, FILL);
        FAIL_NTH.with(|n| n.set(0));
        match r {
            Ok(()) => assert_eq!(at(&mut fb), at(&mut whole), "nth {nth}"),
            Err(e) => {
                assert_eq!(e, ERR_NO_MEMORY, "nth {nth}");
                assert!(at(&mut fb).iter().all(|&g| g == 255), "nth {nth}");
            }
        }
    }
}

#[test]
fn every_allocation_of_a_fmt_image_mapping_may_fail_and_keeps_no_id() {
    use super::super::stages_wire::core_format_v1::FmtImage;
    use crate::native_host::shell::tests::FAIL_NTH;
    let storage = MemoryStorage::new();
    gray_file(&storage, ".paperpaw/cache/k/f/c/a.gray", 8, 4, 40);
    let fmt = FmtImage::new("a.gray", 6, 8, 4, 0);
    let mut failed = 0;
    for nth in 1..=20 {
        let t = ImageTable::new();
        FAIL_NTH.with(|n| n.set(nth));
        let r = t.map_fmt_image(&storage, A, 7, CACHE, &fmt, true, true);
        FAIL_NTH.with(|n| n.set(0));
        match r {
            Ok(Some(doc)) => assert_ne!(doc.image, 0, "nth {nth}"),
            Ok(None) => panic!("nth {nth}: a refused allocation is not a left-out picture"),
            Err(error) => {
                failed += 1;
                assert_eq!(error, ERR_NO_MEMORY, "nth {nth}");
                assert!(t.lock().unwrap().entries.is_empty(), "nth {nth}");
            }
        }
    }
    assert!(failed > 0);
}

#[test]
fn the_draw_scratch_is_the_devices_32_kib_on_the_x4_and_more_elsewhere() {
    use paperpaw_sdk::image::ColorWriter;
    let storage = MemoryStorage::new();
    // A prepared 1,600 px colour picture (the widest there is), drawn taller
    // and see-through: it needs about 14 KiB of scratch.
    let path = prepare::file_name(A, 1).unwrap();
    let mut writer = ColorWriter::new(&storage, &path);
    writer.start(1600, 2).unwrap();
    for _ in 0..2 {
        writer.row_rgb(&std::vec![0_u8; 1600 * 3]).unwrap();
    }
    writer.finish().unwrap();
    let flags = FILL | (55 << TRANSPARENCY_SHIFT);
    let t = ImageTable::new();
    let id = t.add_prepared(A, 1, 1600, 2, true).unwrap();
    // The X4's cap (the default) holds the widest picture.
    assert_eq!(t.scratch_max.get(), SCRATCH_X4);
    let mut fb = screen();
    let mut canvas = fb.canvas();
    assert_eq!(
        t.draw(A, id, &mut canvas, &storage, 0, 0, 40, 30, flags),
        Ok(())
    );
    assert!(gray(&canvas, 20, 15) < 255);
    // A device whose cap is lower is refused before anything is drawn.
    t.set_scratch_max(8 * 1024);
    let mut fb = screen();
    let mut canvas = fb.canvas();
    assert_eq!(
        t.draw(A, id, &mut canvas, &storage, 0, 0, 40, 30, flags),
        Err(ERR_NO_MEMORY)
    );
    assert_eq!(gray(&canvas, 20, 15), 255);
    // A device with memory to spare draws it.
    t.set_scratch_max(SCRATCH_LARGE);
    assert_eq!(
        t.draw(A, id, &mut canvas, &storage, 0, 0, 40, 30, flags),
        Ok(())
    );
    assert!(gray(&canvas, 20, 15) < 255);
}
