//! GIF decoding against images built here: pictures on their screen
//! (inside, partly outside or wholly outside it), interlaced or not,
//! transparency, every LZW code size and a table that fills up; and
//! damaged, truncated or unusual files failing cleanly.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::{Checked, Gif, config, decoded, run};
use paperpaw_image::{Fit, Format, ImageError, decode, dimensions};
use proptest::prelude::*;

fn gif_result(data: &[u8], fit: Fit, color: bool) -> (Result<(), ImageError>, Checked) {
    run(Format::Gif, data, fit, color)
}

#[test]
fn pictures_show_at_their_place_on_the_screen() {
    // Inside, at an edge, partly outside, wholly outside.
    for (left, top) in [(0, 0), (3, 2), (10, 0), (0, 7), (15, 9), (40, 40), (100, 0)] {
        for interlaced in [false, true] {
            let mut gif = Gif::new(6, 5, 4, u64::from(left * 7 + top));
            gif.width = 20;
            gif.height = 12;
            gif.left = left;
            gif.top = top;
            gif.interlaced = interlaced;
            let image = decoded(&gif.encode(), Fit::side(100), false);
            assert_eq!((image.width, image.height), (20, 12));
            assert_eq!(
                image.pixels,
                gif.expected_gray(),
                "at ({left}, {top}), interlaced {interlaced}"
            );
        }
    }
}

#[test]
fn transparent_pixels_are_white() {
    let mut gif = Gif::new(8, 8, 4, 1);
    gif.global = Some(vec![[0, 0, 0], [10, 10, 10], [200, 0, 0], [0, 0, 90]]);
    for index in 0..4 {
        gif.transparent = Some(index);
        let image = decoded(&gif.encode(), Fit::side(100), false);
        assert_eq!(image.pixels, gif.expected_gray());
        assert!(
            gif.indices
                .iter()
                .zip(&image.pixels)
                .all(|(&i, &p)| i != index || p == 255)
        );
        let colour = decoded(&gif.encode(), Fit::side(100), true);
        for (&i, p) in gif.indices.iter().zip(colour.pixels.chunks(3)) {
            if i == index {
                assert_eq!(p, [255, 255, 255]);
            }
        }
    }
    // A transparent index past the table.
    gif.transparent = Some(200);
    gif.global = Some(vec![[0, 0, 0], [9, 9, 9], [0, 0, 0], [9, 9, 9]]);
    assert_eq!(
        decoded(&gif.encode(), Fit::side(100), false).pixels,
        gif.expected_gray()
    );
}

#[test]
fn a_local_table_wins_over_the_global_one() {
    let mut gif = Gif::new(5, 4, 2, 3);
    gif.local = Some(vec![[255, 255, 255], [0, 0, 0]]);
    let image = decoded(&gif.encode(), Fit::side(10), false);
    assert_eq!(image.pixels, gif.expected_gray());
    // No table at all: an error.
    gif.local = None;
    gif.global = None;
    assert_eq!(
        gif_result(&gif.encode(), Fit::side(10), false).0,
        Err(ImageError::Invalid("GIF without colours"))
    );
}

#[test]
fn every_code_size_decodes() {
    // From 1 (two colours, below what GIF writes) to 8; the decoder's
    // table growing to 12 bits, cleared now and then or never.
    for min in 1..=8u8 {
        for clear_every in [Some(3), Some(5000), None] {
            let colors = 1usize << min.max(1);
            let mut gif = Gif::new(70, 70, colors, u64::from(min));
            gif.min_code_size = min;
            gif.clear_every = clear_every;
            let image = decoded(&gif.encode(), Fit::side(100), false);
            assert_eq!(
                image.pixels,
                gif.expected_gray(),
                "code size {min}, clear every {clear_every:?}"
            );
        }
    }
}

#[test]
fn code_sizes_gif_does_not_have_are_invalid() {
    for min in [0u8, 12, 13, 255] {
        let mut gif = Gif::new(4, 4, 2, 0);
        gif.min_code_size = min;
        let data = gif.with_data(&[0, 0, 0]);
        assert_eq!(
            gif_result(&data, Fit::side(10), false).0,
            Err(ImageError::Invalid("LZW code size")),
            "{min}"
        );
    }
}

#[test]
fn larger_code_sizes_than_needed_decode() {
    // Code size 11 for a four-colour picture: valid LZW, wide codes.
    let mut gif = Gif::new(9, 9, 4, 8);
    gif.min_code_size = 11;
    gif.clear_every = Some(100);
    let image = decoded(&gif.encode(), Fit::side(10), false);
    assert_eq!(image.pixels, gif.expected_gray());
}

#[test]
fn codes_not_in_the_table_are_invalid() {
    let gif = Gif::new(4, 4, 4, 0);
    // Code size 2: clear 4, end 5, first free code 6, codes of 3 bits.
    // After a clear, a code that is no index.
    let pack = |codes: &[(u32, u32)]| -> Vec<u8> {
        let (mut acc, mut count, mut out) = (0u64, 0u32, Vec::new());
        for &(code, size) in codes {
            acc |= u64::from(code) << count;
            count += size;
            while count >= 8 {
                out.push(acc as u8);
                acc >>= 8;
                count -= 8;
            }
        }
        out.push(acc as u8);
        out
    };
    let first_not_an_index = pack(&[(4, 3), (6, 3)]);
    assert_eq!(
        gif_result(&gif.with_data(&first_not_an_index), Fit::side(10), false).0,
        Err(ImageError::Invalid("LZW code"))
    );
    // A code beyond the next free one.
    let too_far = pack(&[(4, 3), (1, 3), (7, 3)]);
    assert_eq!(
        gif_result(&gif.with_data(&too_far), Fit::side(10), false).0,
        Err(ImageError::Invalid("LZW code"))
    );
    // The next free code itself (the previous string and its first
    // index again): valid.
    let next = pack(&[(4, 3), (1, 3), (6, 3), (5, 3)]);
    let (result, sink) = gif_result(&gif.with_data(&next), Fit::side(10), false);
    assert_eq!(result, Ok(()));
    // Index 1, then 1 1: three pixels of colour 1, the rest white.
    let one = sink.pixels[0];
    assert_eq!(&sink.pixels[..4], [one, one, one, 255]);
}

#[test]
fn pictures_cut_short_are_white_below() {
    // The end code early: the rest of the picture white, as the screen.
    let gif = Gif::new(10, 6, 4, 5);
    let mut short = gif.clone();
    short.indices.truncate(25);
    short.frame_height = 6;
    // The builder writes the indices it has, then the end code.
    let data = short.with_data(&short.lzw());
    for interlaced in [false, true] {
        let mut data = data.clone();
        if interlaced {
            let mut g = short.clone();
            g.interlaced = true;
            data = g.with_data(&g.lzw());
        }
        let image = decoded(&data, Fit::side(100), false);
        let full = gif.expected_gray();
        if !interlaced {
            // Two whole rows; the third, unfinished, white.
            assert_eq!(&image.pixels[..20], &full[..20]);
            assert!(image.pixels[20..].iter().all(|&p| p == 255));
        }
        assert_eq!(image.height, 6);
    }
}

#[test]
fn the_trailer_and_unknown_extensions_do_not_matter() {
    let mut gif = Gif::new(7, 7, 8, 2);
    let with = decoded(&gif.encode(), Fit::side(10), false);
    gif.trailer = false;
    let without = decoded(&gif.encode(), Fit::side(10), false);
    assert_eq!(with.pixels, without.pixels);
    // A comment and a plain text extension before the picture.
    let data = gif.encode();
    let at = 13 + 3 * 8;
    let mut more = data[..at].to_vec();
    more.extend_from_slice(&[0x21, 0xfe, 3, b'h', b'e', b'y', 0]);
    more.extend_from_slice(&[0x21, 0x01, 12]);
    more.extend_from_slice(&[0; 12]);
    more.extend_from_slice(&[2, b'h', b'i', 0]);
    // A graphic control extension shorter than it should be.
    more.extend_from_slice(&[0x21, 0xf9, 2, 0, 0, 0]);
    more.extend_from_slice(&data[at..]);
    assert_eq!(decoded(&more, Fit::side(10), false).pixels, with.pixels);
}

#[test]
fn unknown_blocks_and_missing_pictures_fail_cleanly() {
    let gif = Gif::new(7, 7, 8, 2);
    let data = gif.encode();
    let at = 13 + 3 * 8;
    let mut odd = data[..at].to_vec();
    odd.push(0x99);
    assert_eq!(
        gif_result(&odd, Fit::side(10), false).0,
        Err(ImageError::Invalid("GIF block"))
    );
    let mut empty = data[..at].to_vec();
    empty.push(0x3b);
    assert_eq!(
        gif_result(&empty, Fit::side(10), false).0,
        Err(ImageError::Invalid("GIF without a picture"))
    );
    // No size.
    for (w, h) in [(0, 5), (5, 0)] {
        let mut zero = gif.clone();
        zero.width = w;
        zero.height = h;
        assert_eq!(
            gif_result(&zero.encode(), Fit::side(10), false).0,
            Err(ImageError::Invalid("GIF size"))
        );
        assert_eq!(dimensions(&zero.encode()), None);
    }
    // Neither 87a nor 89a.
    let mut other = data.clone();
    other[4] = b'8';
    assert_eq!(
        gif_result(&other, Fit::side(10), false).0,
        Err(ImageError::Invalid("GIF signature"))
    );
}

#[test]
fn screens_over_64_megapixels_are_refused() {
    let mut gif = Gif::new(2, 2, 2, 0);
    gif.width = 65535;
    gif.height = 65535;
    let (result, sink) = gif_result(&gif.encode(), Fit::screen(480, 800), false);
    assert_eq!(result, Err(ImageError::TooLarge));
    assert!(!sink.started);
    // 64 megapixels exactly: taken, the screen white around the picture.
    gif.width = 8192;
    gif.height = 8192;
    let image = decoded(&gif.encode(), Fit::side(64), false);
    assert_eq!((image.width, image.height), (64, 64));
    assert!(image.pixels[1..].iter().all(|&p| p == 255));
}

#[test]
fn truncated_at_every_length_never_panics() {
    for interlaced in [false, true] {
        let mut gif = Gif::new(13, 9, 16, 3);
        gif.interlaced = interlaced;
        gif.transparent = Some(2);
        gif.left = 2;
        let data = gif.encode();
        let mut whole = 0;
        for cut in 0..data.len() {
            if gif_result(&data[..cut], Fit::side(7), interlaced).0.is_ok() {
                whole += 1;
            }
        }
        // Without its trailer or the end of the last sub-block, it is
        // whole; shorter, an error.
        assert!(whole <= 2, "{whole}");
    }
    for data in [
        &include_bytes!("data/transparent.gif")[..],
        &include_bytes!("data/alice-cover.gif")[..2000],
    ] {
        for cut in 0..data.len() {
            let _ = gif_result(&data[..cut], Fit::side(16), true);
        }
    }
}

#[test]
fn a_sink_that_stops_stops_decoding() {
    for interlaced in [false, true] {
        for color in [false, true] {
            let mut gif = Gif::new(9, 9, 4, 3);
            gif.interlaced = interlaced;
            let data = gif.encode();
            let mut sink = if color {
                Checked::colour()
            } else {
                Checked::gray()
            };
            sink.stop_at = Some(2);
            let mut source = &data[..];
            let result = decode(Format::Gif, &mut source, Fit::side(10), &mut sink);
            assert_eq!(result, Err(ImageError::Stopped));
            assert_eq!(sink.rows, 2);
        }
    }
}

#[test]
fn rows_finished_inside_one_code_stop_at_the_first_refusal() {
    // A picture one pixel wide: one LZW string covers many rows. Once
    // the sink refused one, the rows the rest of the string finishes
    // must not be offered.
    let mut gif = Gif::new(1, 40, 2, 0);
    gif.indices = vec![0; 40];
    // A real encoder's codes: 0, then 00, 000, ... (strings of the table).
    let codes = [
        (4u32, 3u32),
        (0, 3),
        (6, 3),
        (7, 3),
        (8, 4),
        (9, 4),
        (10, 4),
        (11, 4),
        (5, 4),
    ];
    let (mut acc, mut count, mut lzw) = (0u64, 0u32, Vec::new());
    for (code, size) in codes {
        acc |= u64::from(code) << count;
        count += size;
        while count >= 8 {
            lzw.push(acc as u8);
            acc >>= 8;
            count -= 8;
        }
    }
    lzw.push(acc as u8);
    let data = gif.with_data(&lzw);
    let image = decoded(&data, Fit::side(100), false);
    assert_eq!(image.height, 40);
    // 1 + 2 + 3 + 4 + 5 + 6 + 7 = 28 rows of colour 0, the rest white.
    assert!(image.pixels[28..].iter().all(|&p| p == 255));
    let mut sink = Checked::gray();
    sink.stop_at = Some(10);
    let mut source = &data[..];
    let result = decode(Format::Gif, &mut source, Fit::side(100), &mut sink);
    assert_eq!(result, Err(ImageError::Stopped));
    assert_eq!(sink.rows, 10);
    assert_eq!(sink.after_stop, 1, "rows offered after the sink stopped");
}

#[test]
fn interlaced_picture_of_no_width_is_an_error_not_a_panic() {
    // Found by `properties::damaged_images_never_panic`: the gathered
    // rows took `w * h` = 0 bytes, and the first index was stored at
    // `gathered[r * w + x]` = `gathered[0]`.
    let mut gif = Gif::new(1, 4, 2, 0);
    gif.interlaced = true;
    let lzw = gif.lzw();
    gif.frame_width = 0;
    let data = gif.with_data(&lzw);
    assert!(common::gif_interlaced_without_width(&data));
    let result = std::panic::catch_unwind(|| gif_result(&data, Fit::side(10), false).0);
    assert!(result.is_ok(), "the decoder panicked");
}

#[test]
fn pictures_of_no_width_or_height_are_refused() {
    // In order or interlaced: an empty picture is a damaged file.
    for interlaced in [false, true] {
        for (w, h) in [(0, 4), (4, 0), (0, 0)] {
            let mut gif = Gif::new(2, 2, 2, 0);
            gif.interlaced = interlaced;
            let lzw = gif.lzw();
            gif.frame_width = w;
            gif.frame_height = h;
            let (result, _) = gif_result(&gif.with_data(&lzw), Fit::side(10), false);
            assert_eq!(
                result,
                Err(ImageError::Invalid("GIF picture size")),
                "{w}x{h}, interlaced {interlaced}"
            );
        }
    }
}

#[test]
fn the_size_read_from_the_first_bytes_is_the_screen() {
    let mut gif = Gif::new(5, 5, 2, 0);
    gif.width = 300;
    gif.height = 17;
    assert_eq!(dimensions(&gif.encode()), Some((300, 17)));
    assert_eq!(dimensions(&gif.encode()[..9]), None);
}

proptest! {
    #![proptest_config(config(128))]

    /// Any picture on any screen: the screen as it should be, at any
    /// size inside the box `Fit` gives; interlaced or not the same.
    #[test]
    fn any_gif_shows_its_picture(
        (w, h) in (1u16..30, 1u16..30),
        (sw, sh) in (1u16..40, 1u16..40),
        (left, top) in (0u16..45, 0u16..45),
        colors in prop_oneof![Just(2usize), Just(4), Just(16), Just(256)],
        seed in any::<u64>(),
        clear_every in prop_oneof![Just(None), (1usize..300).prop_map(Some)],
        transparent in proptest::option::of(any::<u8>()),
        side in 1u32..50,
    ) {
        let mut gif = Gif::new(w, h, colors, seed);
        gif.width = sw;
        gif.height = sh;
        gif.left = left;
        gif.top = top;
        gif.clear_every = clear_every;
        gif.transparent = transparent;
        let expected = gif.expected_gray();
        let image = decoded(&gif.encode(), Fit::side(100), false);
        prop_assert_eq!(&image.pixels, &expected);
        gif.interlaced = true;
        let interlaced = decoded(&gif.encode(), Fit::side(100), false);
        prop_assert_eq!(&interlaced.pixels, &expected);
        let small = decoded(&gif.encode(), Fit::side(side), true);
        prop_assert_eq!((small.width, small.height), Fit::side(side).size(sw.into(), sh.into()));
    }
}
