//! The `Downscaler` every decoder (and the SDK's thumbnails) makes rows
//! smaller with: the size it gives, every row it owes and no more, each
//! pixel the mean of those it covers, gray or RGB.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::{Checked, config};
use paperpaw_image::{Downscaler, ImageError};
use proptest::prelude::*;

/// Rows of `width` × `height`, `channels` a pixel, through a downscaler
/// to `size`.
fn scale(
    width: u32,
    height: u32,
    size: (u32, u32),
    channels: usize,
    pixel: impl Fn(u32, u32, usize) -> u8,
) -> Checked {
    let mut scaler = Downscaler::new(width, height, size, channels).unwrap();
    let mut sink = if channels == 3 {
        Checked::colour()
    } else {
        Checked::gray()
    };
    paperpaw_image::ImageSink::start(&mut sink, scaler.out_width(), scaler.out_height()).unwrap();
    let mut row = vec![0u8; width as usize * channels];
    for y in 0..height {
        for x in 0..width {
            for c in 0..channels {
                row[x as usize * channels + c] = pixel(x, y, c);
            }
        }
        scaler.row(&row, &mut sink).unwrap();
    }
    scaler.finish(&mut sink).unwrap();
    sink
}

#[test]
fn never_larger_than_the_rows_never_empty() {
    let scaler = Downscaler::new(10, 20, (100, 100), 1).unwrap();
    assert_eq!((scaler.out_width(), scaler.out_height()), (10, 20));
    let scaler = Downscaler::new(10, 20, (0, 0), 1).unwrap();
    assert_eq!((scaler.out_width(), scaler.out_height()), (1, 1));
    // Rows wider than a u16 counts are refused, narrower ones taken.
    assert_eq!(
        Downscaler::new(70_000, 1, (70_000, 1), 1).unwrap_err(),
        ImageError::TooLarge
    );
    assert!(Downscaler::new(70_000, 1, (500, 1), 3).is_ok());
}

#[test]
fn rows_of_the_same_size_pass_unchanged() {
    let image = scale(5, 3, (5, 3), 1, |x, y, _| (x * 40 + y) as u8);
    assert_eq!(
        image.pixels,
        [
            0, 40, 80, 120, 160, 1, 41, 81, 121, 161, 2, 42, 82, 122, 162
        ]
    );
}

#[test]
fn halves_are_means_of_pairs() {
    let image = scale(4, 2, (2, 1), 1, |x, y, _| {
        [[0, 100, 200, 250], [50, 50, 0, 2]][y as usize][x as usize]
    });
    // (0 + 100 + 50 + 50) / 4 and (200 + 250 + 0 + 2) / 4.
    assert_eq!(image.pixels, [50, 113]);
    // Colour, channel by channel.
    let image = scale(2, 1, (1, 1), 3, |x, _, c| {
        [[10, 20, 30], [30, 40, 50]][x as usize][c]
    });
    assert_eq!(image.pixels, [20, 30, 40]);
}

#[test]
fn rows_after_the_last_are_ignored() {
    let mut scaler = Downscaler::new(2, 2, (1, 1), 1).unwrap();
    let mut sink = Checked::gray();
    paperpaw_image::ImageSink::start(&mut sink, 1, 1).unwrap();
    for _ in 0..5 {
        scaler.row(&[200, 200], &mut sink).unwrap();
    }
    scaler.finish(&mut sink).unwrap();
    assert_eq!(sink.pixels, [200]);
}

proptest! {
    #![proptest_config(config(256))]

    /// Every row owed, of the width given, each pixel within what it
    /// covers; flat rows stay flat.
    #[test]
    fn every_row_owed_and_means_in_range(
        width in 1u32..120,
        height in 1u32..120,
        size in (0u32..130, 0u32..130),
        channels in prop_oneof![Just(1usize), Just(3)],
        seed in any::<u64>(),
        flat in any::<u8>(),
    ) {
        let image = scale(width, height, size, channels, |x, y, c| {
            (seed.wrapping_mul(u64::from(x * 7 + y * 13 + c as u32 + 1)) >> 7) as u8
        });
        let expected = (size.0.clamp(1, width), size.1.clamp(1, height));
        prop_assert_eq!((image.width, image.height), expected);
        prop_assert!(image.complete());
        let image = scale(width, height, size, channels, |_, _, _| flat);
        prop_assert!(image.pixels.iter().all(|&p| p == flat));
    }

    /// The mean of the whole stays the mean (to a level, with rounding
    /// down) where the size divides evenly.
    #[test]
    fn even_divisions_keep_the_mean(
        out in (1u32..12, 1u32..12),
        factor in (1u32..6, 1u32..6),
        seed in any::<u64>(),
    ) {
        let (w, h) = (out.0 * factor.0, out.1 * factor.1);
        let pixel = |x: u32, y: u32, _| (seed.wrapping_mul(u64::from(x * 31 + y * 17 + 1)) >> 13) as u8;
        let image = scale(w, h, out, 1, pixel);
        let mut sum = 0u64;
        for y in 0..h {
            for x in 0..w {
                sum += u64::from(pixel(x, y, 0));
            }
        }
        let mean_in = sum as f64 / f64::from(w * h);
        let mean_out = image.pixels.iter().map(|&p| f64::from(p)).sum::<f64>() / image.pixels.len() as f64;
        prop_assert!((mean_in - mean_out).abs() < 1.0, "{} against {}", mean_in, mean_out);
    }
}

/// Sums are 32 bits and column counts 16: a pixel may cover as much as
/// they hold, and comes out right.
#[test]
fn a_pixel_may_cover_as_much_as_its_sums_hold() {
    // 4104 × 4104 × 255 is just below 2^32.
    let image = scale(4104, 4104, (1, 1), 1, |_, _, _| 255);
    assert_eq!(image.pixels, [255]);
    let image = scale(65_535, 1, (1, 1), 1, |_, _, _| 255);
    assert_eq!(image.pixels, [255]);
}

/// A pixel covering more than its sums or counts hold is refused at the
/// start, never a wrong gray or an overflow (5000 × 5000 white pixels
/// added up to 6.4e9; 140000 columns stopped the count at 65535). Large
/// pictures reach this from the decoders with a fit of a few pixels.
#[test]
fn a_pixel_covering_more_than_its_sums_hold_is_refused() {
    for (width, height) in [(5000, 5000), (4105, 4105), (140_000, 1), (65_536, 1)] {
        assert_eq!(
            Downscaler::new(width, height, (1, 1), 1).err(),
            Some(ImageError::TooLarge),
            "{width} x {height}"
        );
    }
}
