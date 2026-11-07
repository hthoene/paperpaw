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
    for (w, h) in [(0, 4), (4, 0), (MAX_SIDE + 1, 4), (4, MAX_SIDE + 1)] {
        assert_eq!(t.add(A, doc(1, 0), "p", w, h, false), Err(ERR_INVALID));
    }
    assert_eq!(t.add(A, doc(1, 0), "", 4, 4, false), Err(ERR_INVALID));
    let long = "p".repeat(PATH_MAX + 1);
    assert_eq!(t.add(A, doc(1, 0), &long, 4, 4, false), Err(ERR_INVALID));
    assert!(t.add(A, doc(1, 0), "p", MAX_SIDE, MAX_SIDE, false).is_ok());
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
    // A frame that draws nothing protects nothing less.
    t.end_frame(A).unwrap();
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
    for bad in [4, 8, 0x80, 0x1_0000, u32::MAX, FILL | 0x40] {
        assert_eq!(
            t.draw(A, id, &mut canvas, &storage, 0, 0, 8, 4, bad),
            Err(ERR_UNSUPPORTED)
        );
    }
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
        t.map_document(&storage, A, 7, CACHE, path, image, in_block)
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
    ] {
        assert_eq!(map(bad, &fmt_image(5, 8, 4), true), Ok(None), "{bad:?}");
    }
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
        )
    };
    assert_eq!(ninth(true), Ok(Some(0)), "in a block: image 0, no id yet");
    assert_eq!(ninth(false), Err(ERR_BUSY), "op 16/17/18: busy");
    // Another plugin has its own eight.
    assert!(matches!(
        t.map_document(&storage, B, 9, CACHE, "8.gray", &fmt_image(8, 8, 4), false),
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
        .map_fmt_image(&storage, A, 7, CACHE, &fmt, true)
        .unwrap()
        .unwrap();
    assert_ne!(doc.image, 0);
    assert_eq!((doc.index, doc.width, doc.height), (6, 8, 4));
    // A picture the card does not vouch for is no doc_image.
    let lying = FmtImage::new("a.gray", 6, 9, 4, 0);
    assert_eq!(
        t.map_fmt_image(&storage, A, 7, CACHE, &lying, true),
        Ok(None)
    );
}
