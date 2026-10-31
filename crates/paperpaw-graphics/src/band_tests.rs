use alloc::vec;
use paperpaw_platform::{PixelFormat, Rect, Rotation};

use super::*;

fn view(format: PixelFormat, rows: &mut [u8]) -> BandView<'_> {
    BandView::new(rows, Rect::new(0, 100, 16, 2), format, FramePass::GRAY)
}

#[test]
fn grays_round_trip_in_every_packing() {
    for (format, levels) in [
        (PixelFormat::Mono1, vec![0u8, 255]),
        (PixelFormat::Gray2, vec![0, 85, 170, 255]),
        (PixelFormat::Gray4, (0..16).map(|v| v * 17).collect()),
        (PixelFormat::Gray8, vec![0, 1, 77, 200, 255]),
        (PixelFormat::Rgb888, vec![0, 90, 255]),
    ] {
        let mut bytes = vec![0u8; format.stride(16) * 2];
        let mut band = view(format, &mut bytes);
        for (n, level) in levels.iter().enumerate() {
            let (x, y) = ((n * 3 % 16) as u16, 100 + (n % 2) as u16);
            band.put_gray(x, y, *level);
            assert_eq!(band.gray(x, y), *level, "{format:?} {level}");
        }
    }
}

#[test]
fn a_write_changes_one_pixel_only() {
    let mut bytes = [0xffu8; 8];
    let mut band = view(PixelFormat::Gray2, &mut bytes);
    band.put_gray(5, 101, 0);
    let was = band.gray(4, 101);
    assert_eq!((band.gray(5, 101), was, band.gray(6, 101)), (0, 255, 255));
}

#[test]
fn quantising_goes_to_the_nearest_level() {
    let mut bytes = [0u8; 8];
    let mut band = view(PixelFormat::Gray2, &mut bytes);
    band.put_gray(0, 100, 40);
    band.put_gray(1, 100, 50);
    assert_eq!((band.gray(0, 100), band.gray(1, 100)), (0, 85));
    let mut bytes = [0u8; 4];
    let mut band = view(PixelFormat::Mono1, &mut bytes);
    band.put_gray(0, 100, 127);
    band.put_gray(1, 100, 128);
    assert_eq!((band.gray(0, 100), band.gray(1, 100)), (0, 255));
}

#[test]
fn out_of_range_is_ignored_and_reads_zero() {
    let mut bytes = [0xffu8; 8];
    let mut band = view(PixelFormat::Gray2, &mut bytes);
    band.put_gray(16, 100, 0);
    band.put_gray(0, 99, 0);
    band.put_gray(0, 102, 0);
    band.put_gray(u16::MAX, u16::MAX, 0);
    assert_eq!(band.gray(16, 100), 0);
    assert_eq!(band.gray(0, 102), 0);
    assert_eq!(band.gray(0, 99), 0);
    assert!(band.row(99).is_none() && band.row(102).is_none() && band.row_mut(99).is_none());
    assert!(bytes.iter().all(|b| *b == 0xff));
}

#[test]
fn rows_above_are_read_only_and_final() {
    let (mut rows, above) = ([0u8; 8], [0b01_01_01_01u8; 8]);
    let mut band = view(PixelFormat::Gray2, &mut rows).with_above(&above);
    assert_eq!(band.gray(3, 99), 85);
    assert_eq!(band.gray(3, 98), 85);
    assert_eq!(band.gray(3, 97), 0);
    band.put_gray(3, 99, 0);
    assert_eq!(band.gray(3, 99), 85);
    assert!(band.row_mut(99).is_none());
}

#[test]
fn a_format_it_does_not_know_reads_zero_and_ignores_writes() {
    // `row` still gives the bytes.
    let mut bytes = [0x5au8; 4];
    let mut band = BandView::new(
        &mut bytes,
        Rect::new(0, 0, 2, 1),
        PixelFormat::Rgb565,
        FramePass::MONO,
    );
    band.put_gray(0, 0, 255);
    assert_eq!(band.gray(0, 0), 255);
    assert_eq!(band.row(0).map(<[u8]>::len), Some(4));
}

#[test]
fn map_gray_visits_every_pixel_of_the_band_once() {
    let mut bytes = [0u8; 8];
    let mut band = view(PixelFormat::Gray2, &mut bytes);
    let mut count = 0;
    band.map_gray(|g| {
        count += 1;
        255 - g
    });
    assert_eq!(count, 32);
    assert_eq!(bytes, [0xff; 8]);
}

#[test]
fn short_rows_never_panic() {
    let mut bytes = [0u8; 3];
    let mut band = view(PixelFormat::Gray8, &mut bytes);
    band.map_gray(|g| g);
    band.put_gray(15, 101, 9);
    assert_eq!(band.gray(15, 101), 0);
}

#[test]
fn the_carry_regions_rotation_and_first_flag_are_what_was_given() {
    let (mut rows, mut carry) = ([0u8; 8], [7u8; 4]);
    let regions = [Region::new(RegionKind::IMAGE, Rect::new(0, 100, 8, 2))];
    let mut band = view(PixelFormat::Gray2, &mut rows)
        .with_carry(&mut carry, true)
        .with_regions(&regions)
        .with_screen(Rotation::Deg270, true);
    assert!(band.is_first() && band.inverted());
    assert_eq!(band.rotation(), Rotation::Deg270);
    assert_eq!(band.regions()[0].kind(), RegionKind::IMAGE);
    assert_eq!(band.carry(), [7; 4]);
    band.carry_mut()[0] = 1;
    assert_eq!(band.carry()[0], 1);
    // Plain: nothing is known.
    let mut rows = [0u8; 8];
    let plain = view(PixelFormat::Gray2, &mut rows);
    assert!(!plain.is_first() && plain.regions().is_empty() && plain.carry().is_empty());
    assert_eq!(plain.rotation(), Rotation::Deg0);
}

#[test]
fn passes_are_a_set() {
    let both = FramePass::MONO | FramePass::GRAY;
    assert!(both.contains(FramePass::GRAY) && both.contains(FramePass::MONO));
    assert!(!FramePass::MONO.contains(both) && !both.contains(FramePass::COLOUR));
    assert!(!both.contains(FramePass::from_value(0)));
}

/// Both coordinates are frame coordinates, also where a band does not
/// start at x 0.
#[test]
fn columns_are_frame_coordinates_like_rows() {
    let mut rows = [255u8; 32];
    let mut band = BandView::new(
        &mut rows,
        Rect::new(40, 100, 16, 2),
        PixelFormat::Gray8,
        FramePass::GRAY,
    );
    band.put_gray(40, 100, 7);
    band.put_gray(55, 101, 9);
    // Outside the band's columns: ignored, reads zero.
    band.put_gray(0, 100, 1);
    band.put_gray(56, 100, 1);
    assert_eq!((band.gray(40, 100), band.gray(55, 101)), (7, 9));
    assert_eq!(
        (band.gray(0, 100), band.gray(39, 100), band.gray(56, 100)),
        (0, 0, 0)
    );
    assert_eq!((rows[0], rows[31]), (7, 9));
    assert_eq!(rows.iter().filter(|g| **g != 255).count(), 2);
}
