//! The pack format (version 1) as documented: `testdata/pack.ppfp.v1` was
//! written by hand from the format description, and the builder must
//! still make exactly these bytes, so packs made by older and newer
//! versions of `paperpaw-fontgen` read the same.

use alloc::vec;
use alloc::vec::Vec;

use super::*;

const PACK_V1: &[u8] = include_bytes!("../../testdata/pack.ppfp.v1");

fn glyph(advance: f32, left: i16, top: i16, width: u16, height: u16, data: &[u8]) -> Glyph {
    Glyph {
        format: crate::glyph::COVERAGE4,
        left,
        top,
        width,
        height,
        advance,
        data: data.to_vec(),
    }
}

#[test]
fn the_builder_writes_version_1_as_documented() {
    let mut builder = PackBuilder::new();
    // Glyphs in any order and twice: sorted and deduplicated.
    builder.add_strike(
        12.0,
        LineMetrics {
            ascent: 9.5,
            descent: 2.5,
            line_height: 14.25,
        },
        vec![
            ('é', glyph(7.0, 0, -10, 1, 1, &[0xf0])),
            ('A', glyph(7.5, 0, -9, 3, 2, &[0xf8, 0x30, 0x0f, 0xf0])),
            ('?', glyph(6.0, 1, -9, 2, 1, &[0x5a])),
        ],
    );
    builder.add_strike(
        24.5,
        LineMetrics {
            ascent: 19.0,
            descent: 5.0,
            line_height: 28.5,
        },
        vec![
            ('?', glyph(12.0, 2, -18, 2, 2, &[0x9c, 0x30])),
            (
                'A',
                glyph(15.0, 0, -18, 4, 3, &[0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc]),
            ),
            ('é', glyph(14.0, 1, -20, 2, 1, &[0xff])),
        ],
    );
    let built: Vec<u8> = builder.build();
    assert_eq!(built, PACK_V1);
}

#[test]
fn version_1_packs_are_read() {
    let font = BitmapFont::new(PACK_V1).unwrap();
    assert_eq!(font.sizes().collect::<Vec<_>>(), [12.0, 24.5]);
    let metrics = font.line_metrics(13.0);
    assert_eq!(
        (metrics.ascent, metrics.descent, metrics.line_height),
        (9.5, 2.5, 14.25)
    );
    assert_eq!(font.line_metrics(30.0).line_height, 28.5);

    let a = font.glyph('A', 12.0);
    assert_eq!((a.left, a.top, a.width, a.height), (0, -9, 3, 2));
    assert_eq!(a.advance, 7.5);
    // Two pixels a byte, high nibble first, rows padded.
    let pixels: Vec<u8> = (0..2)
        .flat_map(|y| (0..3).map(move |x| (x, y)))
        .map(|(x, y)| a.coverage(x, y))
        .collect();
    assert_eq!(pixels, [255, 136, 51, 0, 255, 255]);

    let big = font.glyph('é', 24.0);
    assert_eq!((big.left, big.top, big.width, big.height), (1, -20, 2, 1));
    assert_eq!(big.advance, 14.0);
    // Missing characters: '?' (the pack has no U+FFFD).
    let missing = font.glyph('Ж', 12.0);
    assert_eq!((missing.width, missing.advance), (2, 6.0));
    assert!(font.has_char('é') && !font.has_char('Ж'));
}
