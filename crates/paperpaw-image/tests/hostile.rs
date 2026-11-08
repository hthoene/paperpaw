//! Damaged and crafted images, built here byte by byte: none may panic,
//! ask for memory beyond its size or take long to decode.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::time::{Duration, Instant};

use paperpaw_image::{Downscaler, Fit, Format, ImageError, ImageSink, decode};

/// Long enough for any of these in a debug build, far too short for the
/// work the crafted files used to ask for (minutes).
const QUICK: Duration = Duration::from_secs(10);

#[derive(Default)]
struct Collect {
    width: u32,
    height: u32,
    channels: usize,
    color: bool,
    pixels: Vec<u8>,
}

impl ImageSink for Collect {
    fn start(&mut self, width: u32, height: u32) -> Result<(), ImageError> {
        self.width = width;
        self.height = height;
        self.channels = if self.color { 3 } else { 1 };
        Ok(())
    }

    fn row(&mut self, gray: &[u8]) -> Result<(), ImageError> {
        assert_eq!(gray.len(), self.width as usize);
        // Gray images come in gray to a sink that wants colour too.
        let channels = if self.color { 3 } else { 1 };
        for &g in gray {
            self.pixels.extend(std::iter::repeat_n(g, channels));
        }
        Ok(())
    }

    fn wants_color(&self) -> bool {
        self.color
    }

    fn row_rgb(&mut self, rgb: &[u8]) -> Result<(), ImageError> {
        assert_eq!(rgb.len(), self.width as usize * 3);
        self.pixels.extend_from_slice(rgb);
        Ok(())
    }
}

/// `data` decoded for `fit`, in colour if `color`, and how long it took.
fn run(data: &[u8], fit: Fit, color: bool) -> (Result<(), ImageError>, Collect, Duration) {
    let format = Format::detect(data).expect("known format");
    let mut source = data;
    let mut out = Collect {
        color,
        ..Collect::default()
    };
    let started = Instant::now();
    let result = decode(format, &mut source, fit, &mut out);
    let took = started.elapsed();
    if result.is_ok() {
        let size = out.width as usize * out.height as usize * out.channels;
        assert_eq!(out.pixels.len(), size);
    }
    (result, out, took)
}

#[test]
fn a_format_of_a_later_build_is_refused_without_a_size() {
    let mut source: &[u8] = b"RIFF\0\0\0\0WEBPVP8 ";
    let result = decode(
        Format::Other(1),
        &mut source,
        Fit::side(8),
        &mut Collect::default(),
    );
    assert_eq!(result, Err(ImageError::Unsupported("format")));
    assert_eq!(Format::detect(b"RIFF\0\0\0\0WEBPVP8 "), None);
}

// GIF

/// LZW codes of 3 bits (2-bit indices), least significant bit first, in
/// sub-blocks.
fn lzw_blocks(codes: &[u16]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let (mut acc, mut count) = (0u32, 0);
    for &code in codes {
        acc |= u32::from(code) << count;
        count += 3;
        while count >= 8 {
            bytes.push(acc as u8);
            acc >>= 8;
            count -= 8;
        }
    }
    if count > 0 {
        bytes.push(acc as u8);
    }
    let mut out = Vec::new();
    for block in bytes.chunks(255) {
        out.push(block.len() as u8);
        out.extend_from_slice(block);
    }
    out.push(0);
    out
}

/// `indices` (0 to 3) as LZW codes that stay 3 bits long: a clear code
/// before every second index keeps the table from growing.
fn lzw_literals(indices: &[u8]) -> Vec<u16> {
    const CLEAR: u16 = 4;
    const END: u16 = 5;
    let mut codes = Vec::new();
    for pair in indices.chunks(2) {
        codes.push(CLEAR);
        codes.extend(pair.iter().map(|&i| u16::from(i)));
    }
    codes.push(END);
    codes
}

/// A GIF of a `screen` with a four-colour table (black, dark gray, light
/// gray, white) and one picture: at `place` (left, top), of `size`,
/// interlaced or not, with `codes` as its data.
fn gif(
    screen: (u16, u16),
    place: (u16, u16),
    size: (u16, u16),
    interlaced: bool,
    codes: &[u16],
) -> Vec<u8> {
    let mut data = b"GIF89a".to_vec();
    data.extend_from_slice(&screen.0.to_le_bytes());
    data.extend_from_slice(&screen.1.to_le_bytes());
    // A global table of four colours.
    data.extend_from_slice(&[0x81, 0, 0]);
    data.extend_from_slice(&[0, 0, 0, 85, 85, 85, 170, 170, 170, 255, 255, 255]);
    data.push(0x2c);
    for value in [place.0, place.1, size.0, size.1] {
        data.extend_from_slice(&value.to_le_bytes());
    }
    data.push(if interlaced { 0x40 } else { 0 });
    // Indices of 2 bits.
    data.push(2);
    data.extend_from_slice(&lzw_blocks(codes));
    data.push(0x3b);
    data
}

#[test]
fn gif_picture_without_width_or_height_fails_cleanly() {
    for (size, interlaced) in [
        ((0, 1), true),
        ((0, 1), false),
        ((1, 0), true),
        ((0, 0), true),
    ] {
        let data = gif((1, 1), (0, 0), size, interlaced, &lzw_literals(&[0, 0]));
        let (result, _, _) = run(&data, Fit::side(64), false);
        assert_eq!(result, Err(ImageError::Invalid("GIF picture size")));
    }
}

#[test]
fn gif_picture_larger_than_the_limit_is_refused_whatever_the_screen() {
    // 65535 × 2000 pixels on a screen of one: refused before its rows are
    // gathered (131 MB), interlaced or not.
    for interlaced in [true, false] {
        let data = gif(
            (1, 1),
            (0, 0),
            (65535, 2000),
            interlaced,
            &lzw_literals(&[0]),
        );
        let (result, _, _) = run(&data, Fit::side(64), false);
        assert_eq!(result, Err(ImageError::TooLarge));
    }
}

#[test]
fn gif_decoding_stops_at_the_bottom_of_the_screen() {
    // A screen of 2 × 1 and a picture of 2 × 2: its first row shows, its
    // second (a broken code here) is not read.
    let codes = [4, 0, 1, 4, 6, 5];
    let data = gif((2, 1), (0, 0), (2, 2), false, &codes);
    let (result, image, _) = run(&data, Fit::side(64), false);
    result.unwrap();
    assert_eq!((image.width, image.height), (2, 1));
    assert_eq!(image.pixels, [0, 85]);
}

#[test]
fn interlaced_gif_partly_off_the_screen_matches_the_same_in_order() {
    // A picture of 4 × 5 at (1, 1) on a screen of 3 × 4: its right and
    // bottom parts are off the screen.
    let (w, h) = (4u16, 5u16);
    let indices: Vec<u8> = (0..w * h).map(|i| ((i * 7 + i / 3) % 4) as u8).collect();
    let rows: Vec<&[u8]> = indices.chunks(usize::from(w)).collect();
    let stored: Vec<u8> = [0, 4, 2, 1, 3]
        .iter()
        .flat_map(|&r| rows[r].iter().copied())
        .collect();
    let in_order = gif((3, 4), (1, 1), (w, h), false, &lzw_literals(&indices));
    let interlaced = gif((3, 4), (1, 1), (w, h), true, &lzw_literals(&stored));
    for color in [false, true] {
        let (result, expected, _) = run(&in_order, Fit::side(64), color);
        result.unwrap();
        let (result, image, _) = run(&interlaced, Fit::side(64), color);
        result.unwrap();
        assert_eq!((image.width, image.height), (3, 4));
        assert_eq!(image.pixels, expected.pixels);
    }
    // The screen's top row and left column are white.
    let (_, image, _) = run(&in_order, Fit::side(64), false);
    assert_eq!(&image.pixels[..3], [255, 255, 255]);
    assert_eq!(image.pixels[3], 255);
    // Row 0 of the picture, as far as the screen reaches.
    let gray = [0u8, 85, 170, 255];
    assert_eq!(
        &image.pixels[4..6],
        [gray[usize::from(rows[0][0])], gray[usize::from(rows[0][1])]]
    );
}

// JPEG

/// A segment: its marker, length and `body`.
fn segment(out: &mut Vec<u8>, marker: u8, body: &[u8]) {
    out.extend_from_slice(&[0xff, marker]);
    out.extend_from_slice(&(body.len() as u16 + 2).to_be_bytes());
    out.extend_from_slice(body);
}

/// The start of a JPEG of `width` × `height` with one or three
/// components (`progressive` or baseline), flat quantization, and
/// Huffman tables of a single 1-bit code each: DC difference 0, and for
/// AC `ac_symbol` (0 is the end of the block).
fn jpeg_head(width: u16, height: u16, components: u8, progressive: bool, ac_symbol: u8) -> Vec<u8> {
    let mut data = vec![0xff, 0xd8];
    let mut quant = vec![0u8];
    quant.extend_from_slice(&[1; 64]);
    segment(&mut data, 0xdb, &quant);
    let mut frame = vec![8];
    frame.extend_from_slice(&height.to_be_bytes());
    frame.extend_from_slice(&width.to_be_bytes());
    frame.push(components);
    for id in 1..=components {
        frame.extend_from_slice(&[id, 0x11, 0]);
    }
    segment(&mut data, if progressive { 0xc2 } else { 0xc0 }, &frame);
    let mut counts = [0u8; 16];
    counts[0] = 1;
    for (class, symbol) in [(0x00, 0u8), (0x10, ac_symbol)] {
        let mut table = vec![class];
        table.extend_from_slice(&counts);
        table.push(symbol);
        segment(&mut data, 0xc4, &table);
    }
    data
}

/// A scan header over components `1..=count`, of frequencies `start` to
/// `end`.
fn scan_header(data: &mut Vec<u8>, count: u8, start: u8, end: u8) {
    let mut body = vec![count];
    for id in 1..=count {
        body.extend_from_slice(&[id, 0x00]);
    }
    body.extend_from_slice(&[start, end, 0]);
    segment(data, 0xda, &body);
}

#[test]
fn huge_jpeg_without_data_decodes_quickly() {
    // About 100 bytes declaring 65535 × 65535 (67 million blocks), the
    // end of the image right after the scan's header: gray and colour.
    for components in [1, 3] {
        let mut data = jpeg_head(65535, 65535, components, false, 0);
        scan_header(&mut data, components, 0, 63);
        data.extend_from_slice(&[0xff, 0xd9]);
        for color in [false, true] {
            let (result, image, took) = run(&data, Fit::side(64), color);
            result.unwrap();
            assert!(took < QUICK, "took {took:?}");
            assert_eq!((image.width, image.height), (64, 64));
            // No data: every block the level of a DC of 0, mid-gray.
            assert!(image.pixels.iter().all(|&v| v == 128));
        }
    }
}

#[test]
fn huge_jpeg_of_empty_restart_intervals_decodes_quickly() {
    // Restart markers without data between them, each starting an
    // interval of 65535 blocks.
    let mut data = jpeg_head(65535, 65535, 1, false, 0);
    segment(&mut data, 0xdd, &65535u16.to_be_bytes());
    scan_header(&mut data, 1, 0, 63);
    for i in 0..2000 {
        data.extend_from_slice(&[0xff, 0xd0 + (i % 8) as u8]);
    }
    data.extend_from_slice(&[0xff, 0xd9]);
    let (result, image, took) = run(&data, Fit::side(64), false);
    result.unwrap();
    assert!(took < QUICK, "took {took:?}");
    assert_eq!((image.width, image.height), (64, 64));
}

#[test]
fn progressive_jpeg_of_empty_scans_decodes_quickly() {
    // 8192 × 8192 (a million blocks kept, one coefficient each), and a
    // thousand scans without data, each of which went through them all.
    let mut data = jpeg_head(8192, 8192, 1, true, 0);
    for _ in 0..1000 {
        scan_header(&mut data, 1, 0, 0);
    }
    data.extend_from_slice(&[0xff, 0xd9]);
    let (result, image, took) = run(&data, Fit::side(64), false);
    result.unwrap();
    assert!(took < QUICK, "took {took:?}");
    assert_eq!((image.width, image.height), (64, 64));
}

#[test]
fn progressive_jpeg_of_long_block_runs_decodes_quickly() {
    // 4096 × 4096 kept at a quarter (262144 blocks of 2 × 2 frequencies),
    // and ten thousand scans of 30 bytes each, which end bands of all the
    // blocks in 16 runs of 16384 (no first scan of the DC to give the
    // blocks bits of their own).
    let mut data = jpeg_head(4096, 4096, 1, true, 0xe0);
    for _ in 0..10_000 {
        scan_header(&mut data, 1, 1, 63);
        data.extend_from_slice(&[0; 30]);
    }
    data.extend_from_slice(&[0xff, 0xd9]);
    let (result, image, took) = run(&data, Fit::side(1024), false);
    result.unwrap();
    assert!(took < QUICK, "took {took:?}");
    assert_eq!((image.width, image.height), (1024, 1024));
}

// PNG

/// A chunk, its CRC left zero (not checked).
fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    out.extend_from_slice(&[0; 4]);
}

#[test]
fn png_palette_longer_than_256_colours_is_skipped_exactly() {
    // A palette of 770 bytes (256 colours and two bytes more), then the
    // single pixel of a 1 × 1 image in colour 1.
    let mut data = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(&mut data, b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 3, 0, 0, 0]);
    let mut palette = vec![0u8; 770];
    palette[3..6].copy_from_slice(&[90, 90, 90]);
    chunk(&mut data, b"PLTE", &palette);
    // zlib: one stored block of the row (filter 0, index 1).
    let idat = [
        0x78, 0x01, 0x01, 0x02, 0x00, 0xfd, 0xff, 0x00, 0x01, 0, 0, 0, 0,
    ];
    chunk(&mut data, b"IDAT", &idat);
    chunk(&mut data, b"IEND", &[]);
    let (result, image, _) = run(&data, Fit::side(64), false);
    result.unwrap();
    assert_eq!(image.pixels, [90]);
}

// Downscaling

struct Discard;

impl ImageSink for Discard {
    fn start(&mut self, _: u32, _: u32) -> Result<(), ImageError> {
        Ok(())
    }
    fn row(&mut self, _: &[u8]) -> Result<(), ImageError> {
        Ok(())
    }
}

#[test]
fn downscaling_more_than_its_sums_hold_is_refused() {
    // 4200 × 4200 pixels of white in one: their sum is past 32 bits.
    match Downscaler::new(4200, 4200, (1, 1), 1) {
        Err(e) => assert_eq!(e, ImageError::TooLarge),
        Ok(mut scaler) => {
            let row = vec![255u8; 4200];
            for _ in 0..4200 {
                scaler.row(&row, &mut Discard).unwrap();
            }
            panic!("not refused");
        }
    }
    // As small as that, but within: made.
    assert!(Downscaler::new(4000, 4000, (1, 1), 1).is_ok());
    // A row shorter than the image's: refused, not a panic.
    let mut scaler = Downscaler::new(10, 10, (5, 5), 1).unwrap();
    assert!(scaler.row(&[0; 4], &mut Discard).is_err());
}
