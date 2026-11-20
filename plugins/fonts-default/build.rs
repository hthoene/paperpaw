//! Generates bitmap font packs when the `packs` feature is enabled or the
//! `outline` feature is disabled.

use std::path::{Path, PathBuf};

/// Pixel sizes for packs; covers UI and reading text on ~220 dpi screens.
const DEFAULT_SIZES: &str = "20,24,28,32,36,40,48";

/// Large sizes of the UI font for what is shown large alone, a clock's
/// time on the home screen: only [`DISPLAY_CHARS`], the rest at those
/// sizes coming from the nearest size that has them. Packs of every
/// character at such sizes would take megabytes.
/// About 14 % apart, so a clock fills its place closely: some 150 KB
/// of flash, no memory while running (packs are read in place).
const DISPLAY_SIZES: &[f32] = &[56.0, 64.0, 74.0, 84.0, 96.0, 110.0, 126.0, 144.0, 164.0];
/// The largest size text is drawn at: the sizes above are a clock's.
const LARGEST_TEXT: f32 = 48.0;
/// The digits and the colon, and the dashes of a clock not yet set.
const DISPLAY_CHARS: &str = "0123456789:-";

/// (pack name, font file, weight)
const FONTS: &[(&str, &str, f32)] = &[
    ("literata", "fonts/Literata.ttf", 400.0),
    ("literata-bold", "fonts/Literata.ttf", 700.0),
    ("literata-italic", "fonts/Literata-Italic.ttf", 400.0),
    ("literata-bold-italic", "fonts/Literata-Italic.ttf", 700.0),
    // The UI font's weight is `UI_WEIGHT` in the plugin.
    // Source Sans 3 changed (one weight, Latin only, rasterised): a
    // Modified Version under its licence, which reserves the name
    // "Source", so the pack is PaperPaw Sans.
    ("paperpaw-sans", "fonts/SourceSans3.ttf", 430.0),
];

/// The packs a lean build does not carry: Literata. There the UI font
/// sets book text too, and the data plugin `literata-styles` supplies
/// Literata from the card for whoever chooses it.
const LEAN_LEAVES_OUT: &[&str] = &[
    "literata",
    "literata-bold",
    "literata-italic",
    "literata-bold-italic",
];

/// The pack of the UI font, which gets [`DISPLAY_SIZES`].
const UI: &str = "paperpaw-sans";

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=fonts");
    println!("cargo::rerun-if-env-changed=PAPERPAW_FONT_SIZES");
    println!("cargo::rerun-if-env-changed=PAPERPAW_FONT_HINT");
    println!("cargo::rerun-if-env-changed=PAPERPAW_FONT_LEAN");
    println!("cargo::rustc-check-cfg=cfg(paperpaw_lean_fonts)");
    // Lean (the firmware): no Literata bold, italic or bold italic (the
    // plugin `literata-styles` brings them from the card, 0.5 MB of
    // flash saved; regular text stands in until then), and the regular
    // face without its largest sizes; the font library has both.
    let lean = std::env::var_os("PAPERPAW_FONT_LEAN").is_some_and(|v| v == "1");
    if lean {
        println!("cargo::rustc-cfg=paperpaw_lean_fonts");
    }
    if std::env::var_os("CARGO_FEATURE_OUTLINE").is_some()
        && std::env::var_os("CARGO_FEATURE_PACKS").is_none()
    {
        return;
    }

    let sizes_text = std::env::var("PAPERPAW_FONT_SIZES").unwrap_or(DEFAULT_SIZES.into());
    let sizes: Vec<f32> = sizes_text
        .split(',')
        .map(|s| {
            s.trim()
                .parse()
                .unwrap_or_else(|_| panic!("invalid font size `{s}`"))
        })
        .collect();

    // Letters fitted to the pixel grid, so that a black-and-white frame
    // draws them even and crisp; `none` (or another mode) to compare.
    let hinting = match std::env::var("PAPERPAW_FONT_HINT") {
        Ok(name) => paperpaw_text::Hinting::parse(&name).unwrap_or_else(|| {
            panic!("PAPERPAW_FONT_HINT: `{name}` is none, light, normal or mono")
        }),
        Err(_) => paperpaw_text::Hinting::Light,
    };

    let out = PathBuf::from(
        std::env::var_os("OUT_DIR").unwrap_or_else(|| panic!("OUT_DIR is set by cargo")),
    );
    for &(name, path, weight) in FONTS {
        if lean && LEAN_LEAVES_OUT.contains(&name) {
            continue;
        }
        let data = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let font = paperpaw_text::OutlineFont::new(data, &[("wght", weight)])
            .unwrap_or_else(|e| panic!("{path}: {e}"));
        // Latin, and the ligatures where the font has them.
        let charset = paperpaw_text::charset::chars;
        let ligatures = charset(paperpaw_text::charset::named("ligatures").unwrap_or(&[]))
            .filter(|ch| font.has_char(*ch));
        let chars = charset(paperpaw_text::charset::named("latin").unwrap_or(&[])).chain(ligatures);
        let pack = if name == UI {
            let pack = with_display(&font, &sizes, chars.clone(), hinting, true);
            // Packed: well under what the plain one takes (and, each glyph read
            // back as it was made, a pack that is not is not written).
            let plain = with_display(&font, &sizes, chars, hinting, false);
            assert!(pack.len() * 10 < plain.len() * 7, "{name}: not packed");
            pack
        } else {
            paperpaw_text::build_pack_hinted(&font, &sizes, chars, hinting)
        };
        write(&out.join(format!("{name}.ppfp")), &deflated(&pack));
    }
    // Icons sit in the private use area and above it, Tabler's newer ones
    // (`tag`) past U+FFFF.
    let path = "fonts/PaperPawIcons.ttf";
    let data = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let font = paperpaw_text::OutlineFont::new(data, &[]).unwrap_or_else(|e| panic!("{path}: {e}"));
    let chars = ('\u{e000}'..=char::MAX).filter(|ch| font.has_char(*ch));
    let pack = paperpaw_text::build_pack_compact(&font, &sizes, chars.clone());
    let plain = paperpaw_text::build_pack(&font, &sizes, chars);
    assert!(pack.len() * 10 < plain.len() * 7, "icons: not packed");
    write(&out.join("icons.ppfp"), &pack);
}

/// The pack of the UI font: `chars` at `sizes`, and [`DISPLAY_CHARS`] at
/// [`DISPLAY_SIZES`] too, with the font's kerning of `chars`, fitted to
/// the pixel grid as `hinting` says.
fn with_display(
    font: &paperpaw_text::OutlineFont,
    sizes: &[f32],
    chars: impl Iterator<Item = char> + Clone,
    hinting: paperpaw_text::Hinting,
    compact: bool,
) -> Vec<u8> {
    let mut builder = paperpaw_text::PackBuilder::new();
    // The surface font is stored packed (Decision 197): the same pixels in about
    // half the bytes.
    if compact {
        builder.compact();
    }
    builder.kerning(&font.kerning_pairs(&chars.clone().collect::<Vec<char>>()));
    let strikes = sizes
        .iter()
        .map(|&size| (size, chars.clone().collect::<Vec<char>>()))
        .chain(
            DISPLAY_SIZES
                .iter()
                .filter(|size| !sizes.contains(size))
                .map(|&size| (size, DISPLAY_CHARS.chars().collect())),
        );
    for (size, chars) in strikes {
        let chars = chars.into_iter().filter(|&ch| font.has_char(ch));
        let glyphs = font.rasterize_chars(chars, size, hinting);
        builder.add_strike(size, font.line_metrics(size), glyphs);
    }
    builder.build()
}

/// `pack` with the bitmaps of its cold glyphs deflated in blocks of 4 KiB, read
/// back a block at a time by the kernel's glyph cache
/// (`paperpaw_text::deflate_bitmaps`), #428. Cold: the large sizes (a clock's
/// digits, drawn once a minute) and the characters few texts use; the letters
/// of Latin-1, the usual punctuation and the ligatures are what text is drawn
/// with, and stay in place, as quick and as free of memory as they were. The
/// icons stay in place too: a screen draws many of them, and the cache would
/// keep them all.
fn deflated(pack: &[u8]) -> Vec<u8> {
    let block = |bytes: &[u8]| {
        let mut compress = flate2::Compress::new(flate2::Compression::best(), false);
        // Room for what does not compress, too.
        let mut out = Vec::with_capacity(bytes.len() + 1024);
        let status = compress
            .compress_vec(bytes, &mut out, flate2::FlushCompress::Finish)
            .unwrap_or_else(|e| panic!("a block is deflated: {e}"));
        assert_eq!(
            status,
            flate2::Status::StreamEnd,
            "a block is deflated whole"
        );
        out
    };
    let cold = |ch: char, size: f32| {
        size > LARGEST_TEXT
            || !matches!(
                u32::from(ch),
                0x20..=0x7e | 0xa0..=0xff | 0x2010..=0x2027 | 0xfb00..=0xfb06
                    | 0x20ac | 0x2122 | 0x2030 | 0xfffd
            )
    };
    paperpaw_text::deflate_bitmaps(pack, &block, &cold)
        .unwrap_or_else(|| panic!("a pack as the builder lays it out"))
}

fn write(path: &Path, data: &[u8]) {
    std::fs::write(path, data).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
}
