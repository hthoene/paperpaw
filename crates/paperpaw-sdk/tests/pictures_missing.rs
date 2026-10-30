//! A picture of a page that was not decoded (the heap had no room for it)
//! is drawn as a frame with a note in its place, not as nothing; one that
//! was decoded is drawn as always.

// The reading functions come with the `unstable` feature.
#![cfg(feature = "unstable")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod support;

use paperpaw_image::{GrayWriter, ImageSink};
use paperpaw_sdk::document::ImageRef;
use paperpaw_sdk::layout::PageImage;
use paperpaw_sdk::reading::{PageType, StatusRoom, draw_image};
use paperpaw_sdk::{Rect, ui};

use support::{Device, Harness, drawn_in, frame};

fn page_image(path: &str) -> PageImage {
    let image = ImageRef {
        path: path.into(),
        width: 8,
        height: 8,
    };
    PageImage::new(20.0, 30.0, 160.0, 120.0, image)
}

#[test]
fn a_picture_not_decoded_is_drawn_as_a_frame() {
    let h = Harness::new(Device::BUTTONS);
    let env = h.env();
    let font = ui::UiStyle::new(&env).font(&env).unwrap();
    let page_type = PageType::new(&env, &font, "en", false, StatusRoom::NONE);
    let mut frame = frame(480, 800);
    let image = page_image("gone.gray");
    let (x, y) = (
        (page_type.x + image.x) as u16,
        (page_type.y + image.y) as u16,
    );
    draw_image(&mut frame.canvas(), &env, &page_type, &image);
    let area = Rect::new(x, y, 160, 120);
    assert!(
        drawn_in(&frame, area),
        "nothing in the place of the picture"
    );
    // Its edge is inked, and inside it a note or a cross, within the frame.
    let view = frame.view();
    assert_ne!(view.pixel(x, y + 60), support::PAPER);
}

#[test]
fn a_picture_decoded_is_drawn_as_its_pixels() {
    let h = Harness::new(Device::BUTTONS);
    let env = h.env();
    let font = ui::UiStyle::new(&env).font(&env).unwrap();
    let page_type = PageType::new(&env, &font, "en", false, StatusRoom::NONE);
    let mut writer = GrayWriter::new(&h.storage, "here.gray");
    writer.start(8, 8).unwrap();
    for _ in 0..8 {
        writer.row(&[0; 8]).unwrap();
    }
    writer.finish().unwrap();
    let mut frame = frame(480, 800);
    let image = page_image("here.gray");
    let (x, y) = (
        (page_type.x + image.x) as u16,
        (page_type.y + image.y) as u16,
    );
    draw_image(&mut frame.canvas(), &env, &page_type, &image);
    // Black all over, with no note: a pixel in the middle is black.
    assert_eq!(
        frame.view().pixel(x + 80, y + 60),
        paperpaw_sdk::Color::BLACK
    );
}
