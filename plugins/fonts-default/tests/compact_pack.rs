//! The surface font and the icons are stored packed in the firmware (Decision
//! 197): every glyph of every strike reads back as it does from the plain
//! pack, pixel for pixel.

use paperpaw_text::{
    BitmapFont, Hinting, OutlineFont, PackBuilder, build_pack, build_pack_compact,
};

const SANS: &[u8] = include_bytes!("../fonts/SourceSans3.ttf");
const ICONS: &[u8] = include_bytes!("../fonts/PaperPawIcons.ttf");
/// As `build.rs`: the text sizes, and the digits at the display sizes.
const SIZES: &[f32] = &[20.0, 24.0, 28.0, 32.0, 36.0];
const DISPLAY_SIZES: &[f32] = &[56.0, 96.0, 164.0];

fn same_pixels(plain: &BitmapFont, packed: &BitmapFont, chars: &[char]) -> usize {
    let mut glyphs = 0;
    let sizes: Vec<f32> = plain.sizes().collect();
    assert_eq!(sizes, packed.sizes().collect::<Vec<_>>());
    for size in sizes {
        for &ch in chars {
            let (a, b) = (plain.glyph(ch, size), packed.glyph(ch, size));
            assert_eq!(
                (a.width, a.height, a.left, a.top, a.advance),
                (b.width, b.height, b.left, b.top, b.advance),
                "{ch:?} at {size}"
            );
            let (la, lb) = (a.levels(), b.levels());
            for y in 0..a.height {
                for x in 0..a.width {
                    let (x, y) = (u32::from(x), u32::from(y));
                    assert_eq!(la(x, y), lb(x, y), "{ch:?} at {size}, {x},{y}");
                }
            }
            glyphs += 1;
        }
    }
    glyphs
}

#[test]
fn the_surface_font_packed_is_the_plain_one_pixel_for_pixel() {
    let font = OutlineFont::new(
        SANS.to_vec(),
        &[("wght", paperpaw_plugin_fonts_default::UI_WEIGHT)],
    )
    .unwrap();
    let chars: Vec<char> =
        paperpaw_text::charset::chars(paperpaw_text::charset::named("latin").unwrap())
            .filter(|&ch| font.has_char(ch))
            .collect();
    let digits: Vec<char> = "0123456789:-".chars().collect();
    let build = |compact: bool| {
        let mut builder = PackBuilder::new();
        if compact {
            builder.compact();
        }
        let strikes = SIZES
            .iter()
            .map(|&s| (s, chars.clone()))
            .chain(DISPLAY_SIZES.iter().map(|&s| (s, digits.clone())));
        for (size, chars) in strikes {
            let glyphs = font.rasterize_chars(chars, size, Hinting::Light);
            builder.add_strike(size, font.line_metrics(size), glyphs);
        }
        BitmapFont::new(builder.build()).unwrap()
    };
    let (plain, packed) = (build(false), build(true));
    let n = same_pixels(&plain, &packed, &chars);
    assert!(n >= SIZES.len() * 300, "{n} glyphs compared");
}

#[test]
fn the_icons_packed_are_the_plain_ones_pixel_for_pixel() {
    let font = OutlineFont::new(ICONS.to_vec(), &[]).unwrap();
    let chars: Vec<char> = ('\u{e000}'..=char::MAX)
        .filter(|&ch| font.has_char(ch))
        .collect();
    let plain = BitmapFont::new(build_pack(&font, SIZES, chars.iter().copied())).unwrap();
    let packed = BitmapFont::new(build_pack_compact(&font, SIZES, chars.iter().copied())).unwrap();
    let n = same_pixels(&plain, &packed, &chars);
    assert!(n >= SIZES.len() * 100, "{n} glyphs compared");
}

/// A measure, not a gate: reading every pixel of a screen of text (about 300
/// glyphs at 24 and 28 px) from the plain pack and from the packed one.
/// `cargo test --release -p paperpaw-plugin-fonts-default --test compact_pack -- --ignored --nocapture`
#[test]
#[ignore = "a measurement"]
fn time_of_reading_a_screen_of_text() {
    use std::time::Instant;
    let font = OutlineFont::new(
        SANS.to_vec(),
        &[("wght", paperpaw_plugin_fonts_default::UI_WEIGHT)],
    )
    .unwrap();
    let text: Vec<char> = "Reading statistics, Library and Settings: the quick brown fox jumps over the lazy dog 0123456789. "
        .chars()
        .cycle()
        .take(300)
        .collect();
    let mut chars: Vec<char> = text.clone();
    chars.sort();
    chars.dedup();
    let scan = |compact: bool| {
        let mut builder = PackBuilder::new();
        if compact {
            builder.compact();
        }
        for size in [24.0, 28.0] {
            let glyphs = font.rasterize_chars(chars.iter().copied(), size, Hinting::Light);
            builder.add_strike(size, font.line_metrics(size), glyphs);
        }
        let pack = BitmapFont::new(builder.build()).unwrap();
        let start = Instant::now();
        let mut sum = 0u64;
        for _ in 0..200 {
            for size in [24.0, 28.0] {
                for &ch in &text {
                    let g = pack.glyph(ch, size);
                    let level = g.levels();
                    for y in 0..u32::from(g.height) {
                        for x in 0..u32::from(g.width) {
                            sum += u64::from(level(x, y));
                        }
                    }
                }
            }
        }
        (start.elapsed(), sum)
    };
    let (plain, a) = scan(false);
    let (packed, b) = scan(true);
    assert_eq!(a, b);
    println!(
        "600 glyphs x 200: plain {plain:?}, packed {packed:?}, per screen of 300 glyphs at two sizes: {:?} vs {:?} ({:+.0} %)",
        plain / 200,
        packed / 200,
        (packed.as_secs_f64() / plain.as_secs_f64() - 1.0) * 100.0
    );
}
