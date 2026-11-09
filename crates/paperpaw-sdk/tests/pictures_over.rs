//! Pictures drawn over what is drawn ([`ui::draw_image_over`]): opaque
//! ones as [`ui::draw_image`] draws them, see-through ones mixed with
//! what is below, pictures with transparency showing it through their
//! clear parts, in their area only; and a PNG's transparency kept by
//! [`image::picture_with_alpha`].

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod support;

use paperpaw_image::{AlphaWriter, ColorWriter, GrayWriter, ImageSink};
use paperpaw_sdk::document::ImageRef;
use paperpaw_sdk::{Color, MemoryStorage, Rect, Storage, image, ui};
use proptest::prelude::*;

use support::{PAPER, drawn_outside, frame};

/// A picture of one `gray` all over: gray, colour, or with transparency
/// (clear in its left half).
fn flat(storage: &MemoryStorage, kind: u8, gray: u8) -> ImageRef {
    let (width, height) = (8, 8);
    let path = ["p.gray", "p.rgb", "p.alpha"][usize::from(kind)];
    match kind {
        0 => {
            let mut writer = GrayWriter::new(storage, path);
            writer.start(width, height).unwrap();
            for _ in 0..height {
                writer.row(&[gray; 8]).unwrap();
            }
            writer.finish().unwrap();
        }
        1 => {
            let mut writer = ColorWriter::new(storage, path);
            writer.start(width, height).unwrap();
            for _ in 0..height {
                writer.row_rgb(&[gray; 24]).unwrap();
            }
            writer.finish().unwrap();
        }
        _ => {
            let mut writer = AlphaWriter::new(storage, path);
            writer.start(width, height).unwrap();
            let row: Vec<u8> = (0..8u8)
                .flat_map(|x| [gray, if x < 4 { 0 } else { 255 }])
                .collect();
            for _ in 0..height {
                writer.row_alpha(&row).unwrap();
            }
            writer.finish().unwrap();
        }
    }
    ImageRef {
        path: path.into(),
        width,
        height,
    }
}

fn pixel(frame: &paperpaw_graphics::Framebuffer, x: u16, y: u16) -> Color {
    frame.view().pixel(x, y)
}

#[test]
fn opaque_pictures_are_drawn_as_draw_image_draws_them() {
    for kind in 0..2 {
        let storage = MemoryStorage::new();
        let image = flat(&storage, kind, 200);
        let area = Rect::new(4, 4, 16, 16);
        let mut once = frame(24, 24);
        ui::draw_image(&mut once.canvas(), &storage, &image, area);
        let mut over = frame(24, 24);
        ui::draw_image_over(&mut over.canvas(), &storage, &image, area, 255);
        assert_eq!(once.view().data, over.view().data, "kind {kind}");
    }
}

#[test]
fn see_through_pictures_mix_with_what_is_below() {
    for kind in 0..3 {
        let storage = MemoryStorage::new();
        let image = flat(&storage, kind, 255);
        let area = Rect::new(0, 0, 16, 16);
        let mut frame = frame(16, 16);
        ui::draw_image_over(&mut frame.canvas(), &storage, &image, area, 0);
        assert_eq!(drawn_outside(&mut frame, Rect::new(0, 0, 0, 0)), None);
        ui::draw_image_over(&mut frame.canvas(), &storage, &image, area, 128);
        // White half over the paper (77), in the opaque part.
        let mixed = ((255u32 * 128 + 77 * 127 + 127) / 255) as u8;
        assert_eq!(pixel(&frame, 15, 3), Color::gray(mixed), "kind {kind}");
    }
}

#[test]
fn clear_parts_show_what_is_below() {
    let storage = MemoryStorage::new();
    let image = flat(&storage, 2, 0);
    let mut frame = frame(16, 16);
    let area = Rect::new(0, 0, 16, 16);
    ui::draw_image_over(&mut frame.canvas(), &storage, &image, area, 255);
    assert_eq!(pixel(&frame, 0, 0), PAPER);
    assert_eq!(pixel(&frame, 5, 9), PAPER);
    assert_eq!(pixel(&frame, 10, 9), Color::BLACK);
    assert_eq!(pixel(&frame, 15, 15), Color::BLACK);
    // Made larger, the edge is blended, not stepped.
    let edge = pixel(&frame, 7, 9).luma();
    assert!(edge < PAPER.luma() && edge > 0, "{edge}");
}

/// A small picture made larger is interpolated: black to white goes
/// through grays, evenly, rather than in two blocks.
#[test]
fn small_pictures_are_made_larger_smoothly() {
    let storage = MemoryStorage::new();
    let mut writer = GrayWriter::new(&storage, "two.gray");
    writer.start(2, 1).unwrap();
    writer.row(&[0, 255]).unwrap();
    writer.finish().unwrap();
    let image = ImageRef {
        path: "two.gray".into(),
        width: 2,
        height: 1,
    };
    let mut frame =
        paperpaw_graphics::Framebuffer::new(8, 1, paperpaw_sdk::PixelFormat::Gray8).unwrap();
    ui::draw_image(&mut frame.canvas(), &storage, &image, Rect::new(0, 0, 8, 1));
    let lumas: Vec<u8> = (0..8).map(|x| pixel(&frame, x, 0).luma()).collect();
    assert_eq!((lumas[0], lumas[7]), (0, 255), "{lumas:?}");
    assert!(lumas.windows(2).all(|w| w[0] <= w[1]), "{lumas:?}");
    assert!(
        lumas.iter().filter(|&&l| l > 0 && l < 255).count() >= 4,
        "{lumas:?}"
    );
}

#[test]
fn transparency_is_kept_from_a_png() {
    // 2 × 1, gray and alpha: black opaque, white clear.
    let storage = MemoryStorage::new();
    let mut png = Vec::new();
    png.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    let chunk = |out: &mut Vec<u8>, kind: &[u8], data: &[u8]| {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let crc = crc32(&out[start..]);
        out.extend_from_slice(&crc.to_be_bytes());
    };
    chunk(&mut png, b"IHDR", &[0, 0, 0, 2, 0, 0, 0, 1, 8, 4, 0, 0, 0]);
    // A stored deflate block of the one filtered row.
    let raw = [0u8, 0, 255, 255, 0];
    let mut zlib = vec![0x78, 0x01, 1, 5, 0, 0xfa, 0xff];
    zlib.extend_from_slice(&raw);
    let (mut a, mut b) = (1u32, 0u32);
    for &v in &raw {
        a = (a + u32::from(v)) % 65521;
        b = (b + a) % 65521;
    }
    zlib.extend_from_slice(&((b << 16) | a).to_be_bytes());
    chunk(&mut png, b"IDAT", &zlib);
    chunk(&mut png, b"IEND", &[]);
    storage.write("a.png", &png).unwrap();
    let image =
        image::picture_with_alpha(&storage, "a.png", "a.ppga", image::Fit::side(10)).unwrap();
    assert_eq!((image.width, image.height), (2, 1));
    let mut frame = frame(2, 1);
    ui::draw_image_over(
        &mut frame.canvas(),
        &storage,
        &image,
        Rect::new(0, 0, 2, 1),
        255,
    );
    assert_eq!(pixel(&frame, 0, 0), Color::BLACK);
    assert_eq!(pixel(&frame, 1, 0), PAPER);
    assert!(image::picture_with_alpha(&storage, "gone.png", "b", image::Fit::side(10)).is_err());
    storage.write("text.png", b"not a picture").unwrap();
    let why = image::picture_with_alpha(&storage, "text.png", "b", image::Fit::side(10));
    assert!(why.unwrap_err().to_string().contains("format"));
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(48)))]

    /// Whatever its kind and opacity, a picture is drawn in its area only,
    /// and in a clipped canvas in the clip only.
    #[test]
    fn pictures_over_others_stay_in_their_area(
        kind in 0u8..3,
        opacity in any::<u8>(),
        area in (0u16..60, 0u16..60, 1u16..120, 1u16..120),
        clip in (0u16..120, 0u16..120, 0u16..120, 0u16..120),
    ) {
        let storage = MemoryStorage::new();
        let image = flat(&storage, kind, 0);
        let area = Rect::new(area.0, area.1, area.2, area.3);
        let clip = Rect::new(clip.0, clip.1, clip.2, clip.3);
        let mut frame = frame(160, 160);
        ui::draw_image_over(&mut frame.canvas().clipped(clip), &storage, &image, area, opacity);
        let both = area.intersection(&clip).unwrap_or_default();
        prop_assert_eq!(drawn_outside(&mut frame, both), None);
    }
}

/// Each axis on its own: made taller but narrower, the columns are
/// averaged (not sampled between), the rows interpolated.
#[test]
fn an_axis_made_smaller_is_averaged_while_the_other_grows() {
    let storage = MemoryStorage::new();
    let mut writer = GrayWriter::new(&storage, "stripes.gray");
    writer.start(8, 1).unwrap();
    writer.row(&[0, 255, 0, 255, 0, 255, 0, 255]).unwrap();
    writer.finish().unwrap();
    let image = ImageRef {
        path: "stripes.gray".into(),
        width: 8,
        height: 1,
    };
    let mut frame =
        paperpaw_graphics::Framebuffer::new(4, 4, paperpaw_sdk::PixelFormat::Gray8).unwrap();
    ui::draw_image(&mut frame.canvas(), &storage, &image, Rect::new(0, 0, 4, 4));
    for y in 0..4 {
        for x in 0..4 {
            let luma = pixel(&frame, x, y).luma();
            assert!(luma.abs_diff(127) <= 1, "{x},{y}: {luma}");
        }
    }
}

/// A wide colour source stretched taller took more than the 32 KiB the native
/// draw may take, and plugins' `draw_image_over` must still draw it (only the
/// host's `try_draw_picture` is limited).
#[test]
fn a_wide_colour_picture_stretched_taller_is_still_drawn_for_plugins() {
    let storage = MemoryStorage::new();
    let (width, height) = (4096u32, 2u32);
    let mut writer = ColorWriter::new(&storage, "wide.rgb");
    writer.start(width, height).unwrap();
    for _ in 0..height {
        writer.row_rgb(&vec![0u8; width as usize * 3]).unwrap();
    }
    writer.finish().unwrap();
    let image = ImageRef {
        path: "wide.rgb".into(),
        width,
        height,
    };
    let mut frame = frame(40, 40);
    // See-through, so the blended (gray and opacity) drawer takes it.
    ui::draw_image_over(
        &mut frame.canvas(),
        &storage,
        &image,
        Rect::new(0, 0, 40, 40),
        200,
    );
    assert!(
        pixel(&frame, 20, 20).luma() < PAPER.luma(),
        "drawn, not dropped"
    );
    // The native draw refuses the same picture before drawing anything.
    let mut native = self::frame(40, 40);
    let picture = ui::Picture::new("wide.rgb", ui::PictureKind::Color, width, height, 32 * 1024);
    assert_eq!(
        ui::try_draw_picture(
            &mut native.canvas(),
            &storage,
            &picture,
            Rect::new(0, 0, 40, 40),
            200
        ),
        Err(image::ImageError::OutOfMemory)
    );
    assert_eq!(pixel(&native, 20, 20), PAPER);
    // Where the device has the memory (a larger cap) the native draw draws it.
    let roomy = ui::Picture::new("wide.rgb", ui::PictureKind::Color, width, height, 1 << 20);
    let mut native = self::frame(40, 40);
    ui::try_draw_picture(
        &mut native.canvas(),
        &storage,
        &roomy,
        Rect::new(0, 0, 40, 40),
        200,
    )
    .unwrap();
    assert!(pixel(&native, 20, 20).luma() < PAPER.luma());
}
