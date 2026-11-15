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
            let glyphs = font.rasterize_chars(chars.into_iter(), size, Hinting::Light);
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
