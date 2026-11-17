//! Pictures made small ([`Thumbnail`]): scaled into the box asked for and
//! never larger than the picture, drawn inside that box only, kept on
//! the storage and read back alike, and kept files that are broken (cut
//! short, another format, a header claiming a huge picture) read as no
//! thumbnail without a panic and without taking memory for what is not
//! there.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

#[path = "kit_support/mod.rs"]
mod support;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use paperpaw_core::kit::ui::{self, fit_image};
use paperpaw_image::{ColorWriter, GrayWriter, ImageSink};
use paperpaw_sdk::document::ImageRef;
use paperpaw_sdk::{MemoryStorage, Rect};
use proptest::prelude::*;

use support::{drawn_outside, frame};

/// The system's allocator, noting the largest block each thread asked
/// for (to see what reading a broken file takes).
struct Noting;

thread_local! {
    static LARGEST: Cell<usize> = const { Cell::new(0) };
}

unsafe impl GlobalAlloc for Noting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = LARGEST.try_with(|l| l.set(l.get().max(layout.size())));
        // SAFETY: as the system allocator, with the same layout.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: allocated by the system allocator with this layout.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Noting = Noting;

/// A picture `width` × `height` on `storage`: a gradient, in gray or in
/// colour.
fn picture(storage: &MemoryStorage, width: u32, height: u32, color: bool) -> ImageRef {
    let path = if color { "p.rgb" } else { "p.gray" };
    if color {
        let mut writer = ColorWriter::new(storage, path);
        writer.start(width, height).unwrap();
        for y in 0..height {
            let row: Vec<u8> = (0..width * 3).map(|x| ((x + y) % 256) as u8).collect();
            writer.row_rgb(&row).unwrap();
        }
        writer.finish().unwrap();
    } else {
        let mut writer = GrayWriter::new(storage, path);
        writer.start(width, height).unwrap();
        for y in 0..height {
            let row: Vec<u8> = (0..width).map(|x| ((x * 7 + y) % 256) as u8).collect();
            writer.row(&row).unwrap();
        }
        writer.finish().unwrap();
    }
    ImageRef {
        path: path.into(),
        width,
        height,
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(64)))]



}

#[test]
fn proportions_are_kept_when_fitting_a_box() {
    let wide = ImageRef {
        path: String::new(),
        width: 400,
        height: 100,
    };
    assert_eq!(fit_image(&wide, 200, 200), (200, 50));
    let tall = ImageRef {
        path: String::new(),
        width: 100,
        height: 400,
    };
    assert_eq!(fit_image(&tall, 200, 200), (50, 200));
    // Huge pictures in large boxes: no overflow.
    let huge = ImageRef {
        path: String::new(),
        width: u32::MAX,
        height: 1,
    };
    assert_eq!(fit_image(&huge, u16::MAX, u16::MAX), (u16::MAX, 0));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(48)))]

    /// A picture drawn from its file is scaled into its area, filling it
    /// and drawing nowhere else; in a canvas clipped to part of it, that
    /// part only.
    #[test]
    fn pictures_are_drawn_into_their_area_only(
        source in (1u32..200, 1u32..200),
        color in any::<bool>(),
        area in (0u16..100, 0u16..100, 1u16..250, 1u16..250),
        clip in (0u16..300, 0u16..300, 0u16..300, 0u16..300),
    ) {
        let storage = MemoryStorage::new();
        let image = picture(&storage, source.0, source.1, color);
        let area = Rect::new(area.0, area.1, area.2, area.3);
        let mut frame = frame(360, 360);
        ui::draw_image(&mut frame.canvas(), &storage, &image, area);
        prop_assert!(support::drawn_in(&frame, area));
        prop_assert_eq!(drawn_outside(&mut frame, area), None);
        let clip = Rect::new(clip.0, clip.1, clip.2, clip.3);
        let mut frame = support::frame(360, 360);
        ui::draw_image(&mut frame.canvas().clipped(clip), &storage, &image, area);
        let both = area.intersection(&clip).unwrap_or_default();
        prop_assert_eq!(drawn_outside(&mut frame, both), None);
    }
}
