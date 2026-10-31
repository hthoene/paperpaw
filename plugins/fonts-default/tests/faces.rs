//! The default fonts as the plugin registers them: every role bound to a
//! face of the right family, weight and slant; the text faces (and with
//! the `packs` feature their packs) hold the Latin set packs are made of;
//! bold faces are bolder; and the icon font holds every icon first-party
//! plugins ask for.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::fs;
use std::path::Path;

use paperpaw_plugin_fonts_default::{
    DefaultFonts, ICONS, LITERATA, LITERATA_BOLD, LITERATA_BOLD_ITALIC, LITERATA_ITALIC, PLUGIN,
    SOURCE_SANS, UI_FAMILY,
};
#[cfg(any(feature = "outline", feature = "packs"))]
use paperpaw_sdk::Font;
use paperpaw_sdk::builtin::fonts;
use paperpaw_sdk::{Contribution, FontSpec, Id, Plugin, Registrar, icons};
use paperpaw_text::charset::{chars, named};

fn registered() -> Vec<Contribution> {
    let mut registrar = Registrar::new(PLUGIN);
    DefaultFonts.register(&mut registrar).unwrap();
    registrar.into_contributions()
}

fn fonts_of(contributions: &[Contribution]) -> Vec<&FontSpec> {
    contributions
        .iter()
        .filter_map(|c| match c {
            Contribution::Font(spec) => Some(spec.as_ref()),
            _ => None,
        })
        .collect()
}

fn font<'a>(contributions: &'a [Contribution], id: &Id) -> &'a FontSpec {
    fonts_of(contributions)
        .into_iter()
        .find(|spec| spec.id == *id)
        .unwrap_or_else(|| panic!("{id} is registered"))
}

#[test]
fn every_role_is_bound_to_a_face_of_the_right_style() {
    let contributions = registered();
    let mut bindings: Vec<(Id, Id)> = contributions
        .iter()
        .filter_map(|c| match c {
            Contribution::FontBinding { role, font, .. } => Some((role.clone(), font.clone())),
            _ => None,
        })
        .collect();
    bindings.sort_by(|a, b| (a.0.as_str(), a.1.as_str()).cmp(&(b.0.as_str(), b.1.as_str())));
    let mut expected = vec![
        (fonts::READING, LITERATA),
        (fonts::READING_BOLD, LITERATA_BOLD),
        (fonts::READING_ITALIC, LITERATA_ITALIC),
        (fonts::READING_BOLD_ITALIC, LITERATA_BOLD_ITALIC),
        (fonts::UI, SOURCE_SANS),
        (fonts::ICONS, ICONS),
    ];
    expected.sort_by(|a, b| (a.0.as_str(), a.1.as_str()).cmp(&(b.0.as_str(), b.1.as_str())));
    assert_eq!(bindings, expected);

    for (id, family, bold, italic) in [
        (&LITERATA, Some("Literata"), false, false),
        (&LITERATA_BOLD, Some("Literata"), true, false),
        (&LITERATA_ITALIC, Some("Literata"), false, true),
        (&LITERATA_BOLD_ITALIC, Some("Literata"), true, true),
        (&SOURCE_SANS, Some(UI_FAMILY), false, false),
        // Never offered for text.
        (&ICONS, None, false, false),
    ] {
        let spec = font(&contributions, id);
        assert_eq!(spec.family.as_deref(), family, "{id}");
        assert_eq!((spec.bold, spec.italic), (bold, italic), "{id}");
    }
    assert_eq!(fonts_of(&contributions).len(), 6);
}

const TEXT_FACES: [&Id; 5] = [
    &LITERATA,
    &LITERATA_BOLD,
    &LITERATA_ITALIC,
    &LITERATA_BOLD_ITALIC,
    &SOURCE_SANS,
];

/// The characters of the packs' Latin set each text face lacks.
fn missing_latin(contributions: &[Contribution], id: &Id) -> String {
    let face = &font(contributions, id).font;
    // Drawn as itself or as a stand-in of nearly the same look.
    chars(named("latin").unwrap())
        .filter(|&c| !face.has_char(face.shown(c)))
        .collect()
}

/// Packs are made of the `latin` set: every character of it is drawn
/// by every text face, as itself or, where the face lacks it, as a
/// stand-in of nearly the same look (`charset::stand_in`): Literata has
/// no non-breaking hyphen (U+2011, common in EPUBs), long s (U+017F, in
/// older German texts), replacement character or some rare punctuation,
/// which would show as `?` on the X4.
#[test]
fn text_faces_hold_all_of_the_latin_set() {
    let contributions = registered();
    for id in TEXT_FACES {
        let missing = missing_latin(&contributions, id);
        assert!(missing.is_empty(), "{id} lacks {missing:?}");
    }
}

/// Ink of `ch` at 32 px: the sum of its pixels' coverage.
#[cfg(feature = "outline")]
fn ink(face: &Font, ch: char) -> u64 {
    match face {
        Font::Outline(outline) => {
            let glyph = outline.rasterize(ch, 32.0);
            let glyph = glyph.as_ref();
            (0..glyph.height)
                .flat_map(|y| (0..glyph.width).map(move |x| (x, y)))
                .map(|(x, y)| u64::from(glyph.coverage(x, y)))
                .sum()
        }
        _ => panic!("scalable fonts with the outline feature"),
    }
}

#[cfg(feature = "outline")]
#[test]
fn bold_faces_are_bolder_and_the_ui_a_little_above_regular() {
    let contributions = registered();
    let ink_of = |id: &Id| ink(&font(&contributions, id).font, 'H');
    for (regular, bold) in [
        (&LITERATA, &LITERATA_BOLD),
        (&LITERATA_ITALIC, &LITERATA_BOLD_ITALIC),
    ] {
        let (light, heavy) = (ink_of(regular), ink_of(bold));
        assert!(
            heavy as f64 > light as f64 * 1.2,
            "{bold} ({heavy}) is not bolder than {regular} ({light})"
        );
    }
    // The UI weight (430) inks more than the font at 400.
    let data = include_bytes!("../fonts/SourceSans3.ttf");
    let regular = Font::from_data(&data[..], &[("wght", 400.0)]).unwrap();
    assert!(ink_of(&SOURCE_SANS) > ink(&regular, 'H'));
}

/// Icon names in the code of first-party plugins (`plugins/*/src`):
/// the last string of each `add_icon(` and `icons::char(` call, with the
/// file it is in.
fn icon_names_in_plugins() -> Vec<(String, String)> {
    let plugins = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut dirs: Vec<_> = fs::read_dir(plugins)
        .unwrap()
        .flatten()
        .map(|entry| entry.path().join("src"))
        .collect();
    let mut found = Vec::new();
    while let Some(dir) = dirs.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
                continue;
            }
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            let text = fs::read_to_string(&path).unwrap();
            for marker in ["add_icon(", "icons::char("] {
                for (at, _) in text.match_indices(marker) {
                    let rest = &text[at..];
                    let Some(end) = rest.find(')').filter(|&end| end < 200) else {
                        continue;
                    };
                    let call = &rest[..end];
                    if call.matches('"').count() >= 2
                        && let Some(name) = call.rsplit('"').nth(1)
                    {
                        found.push((name.to_string(), path.display().to_string()));
                    }
                }
            }
            // Icons in `static` lists of items and icons (`add_icons`): the
            // quoted name on each row.
            for (at, _) in text.match_indices("[(Id, &str); ") {
                let rest = &text[at..];
                let table = &rest[..rest.find("];").unwrap_or(rest.len())];
                let rows = table.split_once("= [").map_or("", |(_, rows)| rows);
                for row in rows.split(')') {
                    if row.matches('"').count() >= 2
                        && let Some(name) = row.rsplit('"').nth(1)
                    {
                        found.push((name.to_string(), path.display().to_string()));
                    }
                }
            }
        }
    }
    found
}

/// Icons first-party plugins ask for that the font has no drawing of
/// (see `the_icon_font_holds_every_icon_plugins_ask_for`).
const KNOWN_MISSING_ICONS: [&str; 2] = ["notebook", "cloud-upload"];

fn missing_icons(contributions: &[Contribution]) -> Vec<(String, String)> {
    let icon_font = &font(contributions, &ICONS).font;
    let names = icon_names_in_plugins();
    assert!(names.len() > 20, "found the plugins' icons: {names:?}");
    names
        .into_iter()
        .filter(|(name, _)| !icons::char(name).is_some_and(|c| icon_font.has_char(c)))
        .collect()
}

#[test]
fn the_icon_font_holds_the_icons_plugins_ask_for() {
    let missing: Vec<_> = missing_icons(&registered())
        .into_iter()
        .filter(|(name, _)| !KNOWN_MISSING_ICONS.contains(&name.as_str()))
        .collect();
    assert!(missing.is_empty(), "icons without a drawing: {missing:#?}");
}

/// The Joplin plugin gives its entries the icons `notebook` and
/// `cloud-upload`, which `tools/icons/subset.py` does not put into the
/// icon font: its menu entries show no icon.
#[test]
fn the_icon_font_holds_every_icon_plugins_ask_for() {
    let missing = missing_icons(&registered());
    assert!(missing.is_empty(), "icons without a drawing: {missing:#?}");
}

/// The bitmap packs (feature `packs`): every text face at every default
/// size, with the same characters as the scalable fonts.
#[cfg(feature = "packs")]
#[test]
fn packs_hold_the_latin_set_at_every_size() {
    use paperpaw_plugin_fonts_default::DefaultFontPacks;

    let mut registrar = Registrar::new(PLUGIN);
    DefaultFontPacks.register(&mut registrar).unwrap();
    let contributions = registrar.into_contributions();
    for id in TEXT_FACES {
        let face = &font(&contributions, id).font;
        let Font::Bitmap(pack) = face else {
            panic!("{id} is a pack");
        };
        // The sizes text is drawn at; the UI font's large ones are for a
        // clock's digits alone.
        let text_sizes: Vec<f32> = pack
            .sizes()
            .filter(|&size| pack.size_for(size, "A", false) == size)
            .collect();
        if std::env::var_os("PAPERPAW_FONT_SIZES").is_none() {
            assert_eq!(
                text_sizes,
                [20.0, 24.0, 28.0, 32.0, 36.0, 40.0, 48.0],
                "{id}"
            );
        }
        if *id == paperpaw_sdk::builtin::fonts::UI {
            let large: Vec<f32> = pack.sizes().filter(|s| !text_sizes.contains(s)).collect();
            assert!(large.iter().any(|&s| s > 150.0), "{large:?}");
            for size in large {
                assert_eq!(pack.size_for(size, "12:30", true), size);
                assert!(pack.size_for(size, "12:30", false) <= 48.0);
                for digit in "0123456789:".chars() {
                    assert_ne!(
                        pack.glyph(digit, size),
                        pack.glyph('?', 48.0),
                        "{digit} at {size}"
                    );
                }
            }
        }
        for size in text_sizes {
            // A character a strike lacks is drawn as its '?' (the
            // replacement character on purpose); others as themselves or
            // their stand-ins.
            let question = pack.glyph('?', size);
            let missing: String = chars(named("latin").unwrap())
                .filter(|&c| face.shown(c) != '?')
                .filter(|&c| pack.glyph(face.shown(c), size) == question)
                .collect();
            assert!(missing.is_empty(), "{id} at {size} lacks {missing:?}");
        }
    }
    let Font::Bitmap(icons) = &font(&contributions, &ICONS).font else {
        panic!("the icons are a pack");
    };
    assert!(icons.has_char(icons::char("home").unwrap()));
}
