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

/// The largest size of the reading faces in a lean build: larger ones
/// come from a plugin that extends them (the font library's), drawn by
/// it where text is asked larger.
const LEAN_READING_MAX: f32 = 32.0;

/// The pack of the UI font, which gets [`DISPLAY_SIZES`].
const UI: &str = "paperpaw-sans";

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=fonts");
    println!("cargo::rerun-if-env-changed=PAPERPAW_FONT_SIZES");
    println!("cargo::rerun-if-env-changed=PAPERPAW_FONT_LEAN");
    println!("cargo::rustc-check-cfg=cfg(paperpaw_lean_fonts)");
    // Lean (the firmware): no Literata Bold Italic, which few books use
    // (bold stands in for it), and the reading faces without their
    // largest sizes; the font library has both.
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

    let out = PathBuf::from(
        std::env::var_os("OUT_DIR").unwrap_or_else(|| panic!("OUT_DIR is set by cargo")),
    );
    for &(name, path, weight) in FONTS {
        if lean && name == "literata-bold-italic" {
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
        let reading: Vec<f32> = sizes
            .iter()
            .copied()
            .filter(|size| !lean || *size <= LEAN_READING_MAX)
            .collect();
        let pack = if name == UI {
            with_display(&font, &sizes, chars)
        } else {
            paperpaw_text::build_pack(&font, &reading, chars)
        };
        write(&out.join(format!("{name}.ppfp")), &pack);
    }
    // Icons sit in the private use area and above it, Tabler's newer ones
    // (`tag`) past U+FFFF.
    let path = "fonts/PaperPawIcons.ttf";
    let data = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let font = paperpaw_text::OutlineFont::new(data, &[]).unwrap_or_else(|e| panic!("{path}: {e}"));
    let chars = ('\u{e000}'..=char::MAX).filter(|ch| font.has_char(*ch));
    let pack = paperpaw_text::build_pack(&font, &sizes, chars);
    write(&out.join("icons.ppfp"), &pack);
}

/// The pack of the UI font: `chars` at `sizes`, and [`DISPLAY_CHARS`] at
/// [`DISPLAY_SIZES`] too, with the font's kerning of `chars`.
fn with_display(
    font: &paperpaw_text::OutlineFont,
    sizes: &[f32],
    chars: impl Iterator<Item = char> + Clone,
) -> Vec<u8> {
    let mut builder = paperpaw_text::PackBuilder::new();
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
        let glyphs = chars
            .into_iter()
            .filter(|&ch| font.has_char(ch))
            .map(|ch| (ch, font.rasterize(ch, size)))
            .collect();
        builder.add_strike(size, font.line_metrics(size), glyphs);
    }
    builder.build()
}

fn write(path: &Path, data: &[u8]) {
    std::fs::write(path, data).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
}
