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

extern crate std;
#[allow(
    unused_imports,
    reason = "the names of std the tests had as SDK integration tests, not all used"
)]
use std::{
    borrow::ToOwned,
    boxed::Box,
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};

use crate::kit::ui;
use paperpaw_image::{GrayWriter, ImageSink};
use paperpaw_sdk::document::ImageRef;
use paperpaw_sdk::{Color, MemoryStorage, Rect};
use proptest::prelude::*;

fn pixel(frame: &paperpaw_graphics::Framebuffer, x: u16, y: u16) -> Color {
    frame.view().pixel(x, y)
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

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(48)))]

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
