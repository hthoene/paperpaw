//! PNG decoding against images built here: every colour type and depth,
//! every filter, image data split anyhow, transparency over white; and
//! damaged, truncated or unsupported files failing cleanly. Also what
//! [`PngWriter`] writes reads back pixel for pixel.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use std::cell::RefCell;

use common::{
    Checked, Dribble, PNG_KINDS, PNG_SIGNATURE, Png, config, decoded, png_channels, png_chunk, run,
};
use paperpaw_image::{Fit, Format, ImageError, PngWriter, decode, dimensions};
use paperpaw_platform::{DirEntry, Storage, StorageError};
use proptest::prelude::*;

fn png_result(data: &[u8], fit: Fit, color: bool) -> (Result<(), ImageError>, Checked) {
    run(Format::Png, data, fit, color)
}

/// Each pixel within `tolerance` of the expected.
fn assert_close(got: &[u8], expected: &[u8], tolerance: u8, what: &str) {
    assert_eq!(got.len(), expected.len(), "{what}: length");
    for (i, (&a, &b)) in got.iter().zip(expected).enumerate() {
        assert!(
            a.abs_diff(b) <= tolerance,
            "{what}: pixel {i} is {a}, expected {b}"
        );
    }
}

#[test]
fn every_colour_type_and_depth_decodes() {
    for (color, depth) in PNG_KINDS {
        for (w, h) in [(1, 1), (7, 3), (17, 5), (33, 2)] {
            let png = Png::new(w, h, color, depth, u64::from(w * 31 + h));
            let image = decoded(&png.encode(), Fit::side(1000), false);
            assert_eq!((image.width, image.height), (w, h));
            assert_close(
                &image.pixels,
                &png.expected_gray(),
                1,
                &format!("type {color} depth {depth} {w}x{h}"),
            );
        }
    }
}

#[test]
fn black_white_and_transparent_are_exact() {
    // The extremes of each kind: no rounding about them.
    for (color, depth) in PNG_KINDS {
        let max = (1u32 << depth) - 1;
        let channels = png_channels(color);
        let mut png = Png::new(3, 1, color, depth, 0);
        let pixel = |v: u16, alpha: u16| -> Vec<u16> {
            match color {
                0 | 3 => vec![v],
                4 => vec![v, alpha],
                2 => vec![v; 3],
                _ => vec![v, v, v, alpha],
            }
        };
        let top = max as u16;
        let mut row = Vec::new();
        if color == 3 && depth == 1 {
            // Two entries only: the white one transparent.
            png.palette = vec![[0, 0, 0], [255, 255, 255]];
            png.trns = Some(vec![255, 0]);
            row.extend([0, 1, 1]);
        } else if color == 3 {
            png.palette = vec![[0, 0, 0], [255, 255, 255], [0, 0, 0]];
            png.trns = Some(vec![255, 255, 0]);
            row.extend([0, 1, 2]);
        } else {
            row.extend(pixel(0, top));
            row.extend(pixel(top, top));
            // Black, fully transparent where there is alpha.
            row.extend(pixel(0, if channels.is_multiple_of(2) { 0 } else { top }));
        }
        png.samples = vec![row];
        let image = decoded(&png.encode(), Fit::side(10), false);
        let transparent = if color >= 3 { 255 } else { 0 };
        assert_eq!(
            image.pixels,
            [0, 255, transparent],
            "type {color} depth {depth}"
        );
    }
}

#[test]
fn every_filter_gives_the_same_pixels() {
    for (color, depth) in PNG_KINDS {
        let mut png = Png::new(19, 9, color, depth, 77);
        let plain = decoded(&png.encode(), Fit::side(100), false);
        for filters in [vec![1], vec![2], vec![3], vec![4], vec![4, 3, 2, 1, 0]] {
            png.filters = filters.clone();
            let filtered = decoded(&png.encode(), Fit::side(100), false);
            assert_eq!(
                plain.pixels, filtered.pixels,
                "type {color} depth {depth}, filters {filters:?}"
            );
        }
    }
}

#[test]
fn colour_rows_keep_their_colour() {
    let mut png = Png::new(2, 1, 2, 8, 0);
    png.samples = vec![vec![255, 0, 0, 0, 0, 255]];
    let image = decoded(&png.encode(), Fit::side(10), true);
    assert_eq!(image.pixels, [255, 0, 0, 0, 0, 255]);
    // Half transparent red over white: pink.
    let mut png = Png::new(1, 1, 6, 8, 0);
    png.samples = vec![vec![255, 0, 0, 128]];
    let image = decoded(&png.encode(), Fit::side(10), true);
    assert_eq!(image.pixels[0], 255);
    assert!(image.pixels[1].abs_diff(127) <= 1 && image.pixels[1] == image.pixels[2]);
    // Gray images in colour: gray.
    let png = Png::new(4, 4, 0, 4, 3);
    let gray = decoded(&png.encode(), Fit::side(10), false);
    let colour = decoded(&png.encode(), Fit::side(10), true);
    let expanded: Vec<u8> = gray.pixels.iter().flat_map(|&g| [g, g, g]).collect();
    assert_eq!(colour.pixels, expanded);
}

#[test]
fn image_data_split_anyhow_decodes_the_same() {
    let mut png = Png::new(23, 11, 6, 16, 5);
    let whole = decoded(&png.encode(), Fit::side(100), false);
    for idat in [1, 2, 7, 100] {
        png.idat = idat;
        let split = decoded(&png.encode(), Fit::side(100), false);
        assert_eq!(whole.pixels, split.pixels, "IDATs of {idat}");
    }
    // Empty IDAT chunks among them.
    let zlib = miniz_oxide::deflate::compress_to_vec_zlib(&png.raw(), 9);
    let mut data = PNG_SIGNATURE.to_vec();
    png_chunk(&mut data, b"IHDR", &png.ihdr());
    png_chunk(&mut data, b"IDAT", &[]);
    for part in zlib.chunks(50) {
        png_chunk(&mut data, b"IDAT", part);
        png_chunk(&mut data, b"IDAT", &[]);
    }
    png_chunk(&mut data, b"IEND", &[]);
    assert_eq!(decoded(&data, Fit::side(100), false).pixels, whole.pixels);
    // Bytes handed over a few at a time.
    let data = png.encode();
    for step in [1, 3, 1000] {
        let mut source = Dribble { data: &data, step };
        let mut sink = Checked::gray();
        decode(Format::Png, &mut source, Fit::side(100), &mut sink).unwrap();
        assert_eq!(sink.pixels, whole.pixels);
    }
}

#[test]
fn stored_and_compressed_data_decode_the_same() {
    // Deflate without compression (stored blocks) and with it.
    let mut png = Png::new(40, 30, 2, 8, 9);
    // Gradients, which compress.
    png.samples = (0..30u16)
        .map(|y| (0..120u16).map(|i| (i * 2 + y) % 256).collect())
        .collect();
    let stored = png.with_zlib(&miniz_oxide::deflate::compress_to_vec_zlib(&png.raw(), 0));
    let best = png.with_zlib(&miniz_oxide::deflate::compress_to_vec_zlib(&png.raw(), 10));
    assert_ne!(stored.len(), best.len());
    assert_eq!(
        decoded(&stored, Fit::side(100), false).pixels,
        decoded(&best, Fit::side(100), false).pixels
    );
}

#[test]
fn palette_indices_beyond_the_palette_do_not_panic() {
    // Indices past the PLTE's entries (and past tRNS's): some colour,
    // the pixels within the palette as they are.
    let mut png = Png::new(4, 1, 3, 8, 0);
    png.palette = vec![[255, 255, 255], [0, 0, 0]];
    png.trns = Some(vec![255]);
    png.samples = vec![vec![0, 1, 200, 255]];
    for color in [false, true] {
        let image = decoded(&png.encode(), Fit::side(10), color);
        let channels = if color { 3 } else { 1 };
        assert!(image.pixels[..channels].iter().all(|&v| v == 255));
        assert!(image.pixels[channels..2 * channels].iter().all(|&v| v == 0));
    }
    // A palette image without a palette at all.
    let mut png = Png::new(4, 2, 3, 2, 1);
    png.palette.clear();
    let _ = png_result(&png.encode(), Fit::side(10), true);
}

#[test]
fn transparency_chunks_of_other_sizes_are_read_as_far_as_they_go() {
    let mut png = Png::new(4, 1, 3, 8, 0);
    png.palette = vec![[0, 0, 0]; 4];
    png.samples = vec![vec![0, 1, 2, 3]];
    // Shorter than the palette: the rest opaque.
    png.trns = Some(vec![0, 0]);
    assert_eq!(
        decoded(&png.encode(), Fit::side(10), false).pixels,
        [255, 255, 0, 0]
    );
    // Longer than 256 entries: the rest skipped, the chunks after read.
    png.trns = Some(vec![0; 300]);
    assert_eq!(
        decoded(&png.encode(), Fit::side(10), false).pixels,
        [255; 4]
    );
}

#[test]
fn palettes_longer_than_256_entries_are_cut() {
    let mut png = Png::new(2, 1, 3, 8, 0);
    png.palette = vec![[0, 0, 0]; 300];
    png.palette[0] = [255, 255, 255];
    png.samples = vec![vec![0, 1]];
    assert_eq!(
        decoded(&png.encode(), Fit::side(10), false).pixels,
        [255, 0]
    );
}

#[test]
fn interlaced_png_is_unsupported() {
    let mut png = Png::new(8, 8, 0, 8, 0);
    png.interlace = 1;
    let (result, sink) = png_result(&png.encode(), Fit::side(10), false);
    assert_eq!(result, Err(ImageError::Unsupported("interlaced PNG")));
    assert!(!sink.started);
}

#[test]
fn invalid_headers_fail_cleanly() {
    // Colour types and depths PNG does not have.
    for (color, depth) in [
        (0, 3),
        (0, 32),
        (2, 4),
        (3, 16),
        (4, 1),
        (5, 8),
        (6, 4),
        (7, 8),
        (0, 0),
    ] {
        let mut png = Png::new(2, 2, 0, 8, 0);
        png.color = color;
        png.depth = depth;
        let (result, _) = png_result(&png.with_zlib(&[0x78, 0x01]), Fit::side(10), false);
        assert_eq!(
            result,
            Err(ImageError::Invalid("bad PNG header")),
            "{color} {depth}"
        );
    }
    // No width or height.
    for (w, h) in [(0, 1), (1, 0), (0, 0)] {
        let mut png = Png::new(1, 1, 0, 8, 0);
        png.width = w;
        png.height = h;
        assert_eq!(
            png_result(&png.with_zlib(&[0x78, 0x01]), Fit::side(10), false).0,
            Err(ImageError::Invalid("bad PNG header"))
        );
    }
    // Larger than 65536 either way.
    for (w, h) in [(65537, 1), (1, 65537), (u32::MAX, u32::MAX)] {
        let mut png = Png::new(1, 1, 0, 8, 0);
        png.width = w;
        png.height = h;
        assert_eq!(
            png_result(&png.with_zlib(&[0x78, 0x01]), Fit::side(10), false).0,
            Err(ImageError::TooLarge)
        );
    }
    // Not a PNG at all.
    assert_eq!(
        png_result(b"\x89PNG\r\n\x1a\x0b", Fit::side(10), false).0,
        Err(ImageError::Invalid("not a PNG"))
    );
}

#[test]
fn chunks_out_of_order_or_missing_fail_cleanly() {
    let png = Png::new(5, 5, 2, 8, 0);
    let zlib = miniz_oxide::deflate::compress_to_vec_zlib(&png.raw(), 6);
    let mut idat = Vec::new();
    png_chunk(&mut idat, b"IDAT", &zlib);
    let mut ihdr = Vec::new();
    png_chunk(&mut ihdr, b"IHDR", &png.ihdr());
    let mut iend = Vec::new();
    png_chunk(&mut iend, b"IEND", &[]);
    let file = |parts: &[&[u8]]| -> Vec<u8> {
        let mut out = PNG_SIGNATURE.to_vec();
        for part in parts {
            out.extend_from_slice(part);
        }
        out
    };
    // Image data before the header.
    assert_eq!(
        png_result(&file(&[&idat, &ihdr, &iend]), Fit::side(10), false).0,
        Err(ImageError::Invalid("IDAT before IHDR"))
    );
    // No image data.
    assert_eq!(
        png_result(&file(&[&ihdr, &iend]), Fit::side(10), false).0,
        Err(ImageError::Invalid("no image data"))
    );
    // No end: the data runs out.
    assert_eq!(
        png_result(&file(&[&ihdr, &idat]), Fit::side(10), false).0,
        Err(ImageError::Invalid("unexpected end"))
    );
    // A second header after the data, and chunks after the end: ignored.
    let a = decoded(&file(&[&ihdr, &idat, &iend]), Fit::side(10), false);
    let b = decoded(
        &file(&[&ihdr, &idat, &ihdr, &iend, b"garbage"]),
        Fit::side(10),
        false,
    );
    assert_eq!(a.pixels, b.pixels);
}

#[test]
fn image_data_of_fewer_rows_is_truncated() {
    let png = Png::new(6, 8, 0, 8, 2);
    let raw = png.raw();
    // Five rows of eight, each 1 + 6 bytes.
    let short = png.with_zlib(&miniz_oxide::deflate::compress_to_vec_zlib(
        &raw[..5 * 7 + 3],
        6,
    ));
    let (result, sink) = png_result(&short, Fit::side(10), false);
    assert_eq!(result, Err(ImageError::Invalid("truncated PNG data")));
    assert_eq!(sink.rows, 5);
    // More rows than the height: the rest ignored.
    let mut long = raw.clone();
    long.extend_from_slice(&raw);
    let long = png.with_zlib(&miniz_oxide::deflate::compress_to_vec_zlib(&long, 6));
    assert_eq!(
        decoded(&long, Fit::side(10), false).pixels,
        decoded(&png.encode(), Fit::side(10), false).pixels
    );
}

#[test]
fn unknown_filter_types_are_invalid() {
    let png = Png::new(4, 3, 0, 8, 2);
    for filter in [5u8, 9, 255] {
        let mut raw = png.raw();
        raw[5] = filter;
        let data = png.with_zlib(&miniz_oxide::deflate::compress_to_vec_zlib(&raw, 6));
        assert_eq!(
            png_result(&data, Fit::side(10), false).0,
            Err(ImageError::Invalid("bad PNG filter"))
        );
    }
}

#[test]
fn damaged_deflate_data_fails_cleanly() {
    let png = Png::new(30, 20, 6, 8, 4);
    let zlib = miniz_oxide::deflate::compress_to_vec_zlib(&png.raw(), 9);
    let mut failed = 0;
    for at in 2..zlib.len() - 4 {
        for flip in [0x01, 0x10, 0xff] {
            let mut damaged = zlib.clone();
            damaged[at] ^= flip;
            let (result, _) = png_result(&png.with_zlib(&damaged), Fit::side(16), false);
            failed += usize::from(result.is_err());
        }
    }
    // Damage shows (the checksum is not checked, so not all of it).
    assert!(failed > 0);
    // Not deflate at all.
    let (result, _) = png_result(
        &png.with_zlib(&[0x78, 0x01, 0xff, 0xff, 0xff]),
        Fit::side(16),
        false,
    );
    assert_eq!(result, Err(ImageError::Invalid("corrupt PNG data")));
}

#[test]
fn chunk_crcs_are_not_what_decoding_depends_on() {
    // Damaged CRCs do not change the pixels.
    let png = Png::new(9, 9, 2, 8, 6);
    let data = png.encode();
    let whole = decoded(&data, Fit::side(10), false);
    let mut damaged = data.clone();
    let mut at = 8;
    while at + 12 <= damaged.len() {
        let len = u32::from_be_bytes(damaged[at..at + 4].try_into().unwrap()) as usize;
        damaged[at + 8 + len] ^= 0xff;
        at += 12 + len;
    }
    let (result, sink) = png_result(&damaged, Fit::side(10), false);
    if result.is_ok() {
        assert_eq!(sink.pixels, whole.pixels);
    }
}

#[test]
fn truncated_at_every_length_is_an_error() {
    // Up to the end chunk's CRC, which is not read.
    for (color, depth) in PNG_KINDS {
        let data = Png::new(9, 7, color, depth, 1).encode();
        for cut in 0..data.len() - 4 {
            let (result, _) = png_result(&data[..cut], Fit::side(4), color == 6);
            assert!(result.is_err(), "type {color} depth {depth} cut at {cut}");
        }
    }
    for data in [
        &include_bytes!("data/rgba.png")[..],
        include_bytes!("data/palette.png"),
        include_bytes!("data/gray-alpha.png"),
        include_bytes!("data/bilevel.png"),
        include_bytes!("data/colors-palette.png"),
    ] {
        for cut in 0..data.len() - 4 {
            assert!(png_result(&data[..cut], Fit::side(16), true).0.is_err());
        }
    }
}

#[test]
fn chunk_lengths_past_the_end_fail_cleanly() {
    let data = Png::new(9, 7, 3, 4, 1).encode();
    let mut at = 8;
    while at + 12 <= data.len() {
        let len = u32::from_be_bytes(data[at..at + 4].try_into().unwrap()) as usize;
        for bogus in [0u32, 1, 12, 0x7fff_ffff, u32::MAX] {
            let mut damaged = data.clone();
            damaged[at..at + 4].copy_from_slice(&bogus.to_be_bytes());
            let _ = png_result(&damaged, Fit::side(8), true);
        }
        at += 12 + len;
    }
}

#[test]
fn a_sink_that_stops_stops_decoding() {
    let data = Png::new(10, 10, 6, 8, 3).encode();
    for color in [false, true] {
        let mut sink = if color {
            Checked::colour()
        } else {
            Checked::gray()
        };
        sink.stop_at = Some(4);
        let mut source = &data[..];
        let result = decode(Format::Png, &mut source, Fit::side(10), &mut sink);
        assert_eq!(result, Err(ImageError::Stopped));
        assert_eq!((sink.rows, sink.after_stop), (4, 1));
    }
}

#[test]
fn the_widest_png_fits_the_screen() {
    // 65536 pixels wide: made as small as the screen asks.
    let mut png = Png::new(1, 1, 0, 8, 0);
    png.width = 65536;
    png.height = 3;
    png.samples = vec![vec![0; 65536]; 3];
    let image = decoded(&png.encode(), Fit::screen(480, 800), false);
    assert_eq!((image.width, image.height), (800, 1));
    // Decoded whole, it would be wider than a row may be.
    let (result, _) = png_result(&png.encode(), Fit::side(100_000), false);
    assert_eq!(result, Err(ImageError::TooLarge));
}

fn kind() -> impl Strategy<Value = (u8, u8)> {
    (0..PNG_KINDS.len()).prop_map(|i| PNG_KINDS[i])
}

proptest! {
    #![proptest_config(config(128))]

    /// Any PNG decodes to its pixels, at any size inside the box `Fit`
    /// gives, and `dimensions` reads its size from its first bytes.
    #[test]
    fn any_png_decodes_to_its_pixels(
        w in 1u32..40,
        h in 1u32..40,
        kind in kind(),
        seed in any::<u64>(),
        filters in proptest::collection::vec(0u8..5, 1..4),
        side in 1u32..60,
    ) {
        let (color, depth) = kind;
        let mut png = Png::new(w, h, color, depth, seed);
        png.filters = filters;
        let data = png.encode();
        prop_assert_eq!(dimensions(&data), Some((w, h)));
        let full = decoded(&data, Fit::side(1000), false);
        let expected = png.expected_gray();
        for (&a, &b) in full.pixels.iter().zip(&expected) {
            prop_assert!(a.abs_diff(b) <= 1, "{} for {}", a, b);
        }
        let small = decoded(&data, Fit::side(side), false);
        prop_assert_eq!((small.width, small.height), Fit::side(side).size(w, h));
        prop_assert!(small.width <= side.max(1) && small.height <= side.max(1));
    }

    /// What PngWriter writes reads back as it was, pixel for pixel.
    #[test]
    fn written_pngs_read_back_exactly(
        w in 1u32..70,
        h in 1u32..30,
        seed in any::<u64>(),
    ) {
        let storage = OneFile::default();
        let mut writer = PngWriter::new(&storage, "a.png", w, h).unwrap();
        let mut rng = common::Lcg(seed);
        let pixels: Vec<u8> = (0..w * h).map(|_| rng.next() as u8).collect();
        for row in pixels.chunks(w as usize) {
            prop_assert!(!writer.is_complete());
            writer.row(&storage, "a.png", row).unwrap();
        }
        prop_assert!(writer.is_complete());
        let data = storage.0.borrow().clone();
        let image = decoded(&data, Fit::side(1000), false);
        prop_assert_eq!((image.width, image.height), (w, h));
        prop_assert_eq!(image.pixels, pixels);
        // Its chunks' CRCs and the zlib checksum are right, as other
        // programs check them.
        let mut at = 8;
        while at < data.len() {
            let len = u32::from_be_bytes(data[at..at + 4].try_into().unwrap()) as usize;
            let crc = u32::from_be_bytes(data[at + 8 + len..at + 12 + len].try_into().unwrap());
            prop_assert_eq!(common::crc32(&data[at + 4..at + 8 + len]), crc);
            at += 12 + len;
        }
        prop_assert_eq!(at, data.len());
    }
}

/// One file in memory.
#[derive(Default)]
struct OneFile(RefCell<Vec<u8>>);

impl Storage for OneFile {
    fn read_at(&self, _: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        let data = self.0.borrow();
        let start = (offset as usize).min(data.len());
        let n = buf.len().min(data.len() - start);
        buf[..n].copy_from_slice(&data[start..start + n]);
        Ok(n)
    }
    fn size(&self, _: &str) -> Result<u64, StorageError> {
        Ok(self.0.borrow().len() as u64)
    }
    fn write(&self, _: &str, data: &[u8]) -> Result<(), StorageError> {
        *self.0.borrow_mut() = data.to_vec();
        Ok(())
    }
    fn append(&self, _: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.borrow_mut().extend_from_slice(data);
        Ok(())
    }
    fn list(&self, _: &str, _: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        Ok(())
    }
    fn remove(&self, _: &str) -> Result<(), StorageError> {
        Ok(())
    }
}

#[test]
fn png_writer_refuses_what_it_cannot_write() {
    let storage = OneFile::default();
    // No size, or rows longer than a stored block holds.
    for (w, h) in [(0, 1), (1, 0), (65535, 1), (100_000, 1)] {
        assert_eq!(
            PngWriter::new(&storage, "a.png", w, h).unwrap_err(),
            ImageError::Unsupported("PNG size"),
            "{w}x{h}"
        );
    }
    // The widest it writes.
    let mut writer = PngWriter::new(&storage, "a.png", 65534, 1).unwrap();
    writer.row(&storage, "a.png", &vec![9; 65534]).unwrap();
    let data = storage.0.borrow().clone();
    let image = decoded(&data, Fit::side(70_000), false);
    assert_eq!(image.width, 65534);
    assert!(image.pixels.iter().all(|&p| p == 9));
    // Short rows, and rows after the last.
    let mut writer = PngWriter::new(&storage, "a.png", 4, 1).unwrap();
    assert_eq!(
        writer.row(&storage, "a.png", &[1, 2, 3]),
        Err(ImageError::Invalid("PNG row"))
    );
    writer.row(&storage, "a.png", &[1, 2, 3, 4, 5]).unwrap();
    assert_eq!(
        writer.row(&storage, "a.png", &[1, 2, 3, 4]),
        Err(ImageError::Invalid("PNG row"))
    );
    assert!(writer.is_complete());
}

#[test]
fn png_writer_refuses_the_largest_width() {
    // A row and its filter byte must fit a stored block; `width + 1`
    // must not overflow on the way (a panic in debug builds, in release
    // a wrap to 0 that took the width).
    let storage = OneFile::default();
    assert_eq!(
        PngWriter::new(&storage, "a.png", u32::MAX, 1).unwrap_err(),
        ImageError::Unsupported("PNG size")
    );
}
