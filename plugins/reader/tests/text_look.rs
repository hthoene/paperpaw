//! How book text is inked, with the real kernel: smooth or black and
//! white (`core.smooth-text`), and how dark (`core.text-contrast`). A page
//! of text without smooth letters asks for no refresh in grays on a panel
//! of black and white; sharp letters have no grays; darker steps ink
//! more; the defaults draw as before. How crisp (`core.text-sharpness`):
//! a step of contrast each way, sharp without grays, normal and unknown
//! values as before, a text curve in its place. And drop capitals: a book's large
//! first letter set over its first lines. And where lines are set: hanging
//! punctuation (`core.hanging-punctuation`) moves marks, never lines, and
//! a list item's lines hang after its marker.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::*;
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::{DeviceInfo, Instant, PixelFormat};

const BOOK: &str = "books/numbered.txt";

/// The buttons device with a panel of black and white alone, as the X4.
const MONO: DeviceInfo = BUTTONS.with_display(BUTTONS.display.with_format(PixelFormat::Mono1));

fn reading(device: DeviceInfo, config: &str) -> Reading {
    let text = numbered_book(120, 12);
    let mut reading = Reading::new(device, Shared::default(), BOOK, text.as_bytes(), config);
    reading.open();
    reading
}

/// Whether turning a page is shown in grays: the refresh it asks for.
fn turns_in_grays(reading: &mut Reading) -> bool {
    reading.now += 100;
    let mut effects = reading
        .kernel
        .run_action(Instant(reading.now), actions::NEXT_PAGE);
    for _ in 0..100 {
        if effects.refresh().is_some() {
            return effects.grays();
        }
        reading.now += 10;
        effects = reading.kernel.tick(Instant(reading.now));
    }
    panic!("the page turned without a refresh");
}

#[test]
fn text_pages_turn_in_grays_only_with_smooth_text() {
    // Off at first on a panel of black and white.
    assert!(!turns_in_grays(&mut reading(MONO, "")));
    let smooth = "[core]\nsmooth-text = true\n";
    assert!(turns_in_grays(&mut reading(MONO, smooth)));
    // As the reader's own setting was called before.
    let before = "[reader]\nsmooth-text = true\n";
    assert!(turns_in_grays(&mut reading(MONO, before)));
}

/// The grays of the page's text lines (not the status line, whose text
/// is the interface's).
fn text_grays(reading: &mut Reading) -> Vec<u8> {
    let image = reading.snapshot();
    let lines = reading.lines(4);
    assert!(lines.len() > 5, "a page of text");
    lines
        .iter()
        .flat_map(|((top, bottom), _)| image[*top..*bottom].iter().flatten().copied())
        .collect()
}

#[test]
fn sharp_letters_have_no_grays() {
    let between = |grays: &[u8]| grays.iter().filter(|g| !matches!(g, 0 | 255)).count();
    // Smooth at first where the panel has grays.
    let smooth = text_grays(&mut reading(BUTTONS, ""));
    assert!(between(&smooth) > 1000, "{}", between(&smooth));
    let sharp = text_grays(&mut reading(BUTTONS, "[core]\nsmooth-text = false\n"));
    assert_eq!(between(&sharp), 0);
    assert!(sharp.contains(&0));
}

#[test]
fn darker_steps_ink_more_and_normal_is_as_before() {
    let ink = |config: &str| -> u64 {
        let grays = text_grays(&mut reading(BUTTONS, config));
        grays.iter().map(|g| u64::from(255 - g)).sum()
    };
    let step = |level: &str| {
        ink(&format!(
            "[core]\ntext-contrast = core.text-contrast.{level}\n"
        ))
    };
    let inks = ["lighter", "normal", "darker", "darkest"].map(step);
    assert!(inks.windows(2).all(|w| w[0] < w[1]), "{inks:?}");
    assert_eq!(inks[1], ink(""));
    // Without grays too: darker steps draw fuller letters.
    let sharp = |level: &str| {
        ink(&format!(
            "[core]\nsmooth-text = false\ntext-contrast = core.text-contrast.{level}\n"
        ))
    };
    assert!(sharp("lighter") < sharp("normal") && sharp("normal") < sharp("darkest"));
}

#[test]
fn the_default_page_is_drawn_as_before_on_black_and_white() {
    // Sharp letters from the panel's own threshold: the same pixels as
    // smooth ones drawn in black and white by the frame.
    let page = |config: &str| reading(MONO, config).snapshot();
    let before = page("");
    assert_eq!(before, page("[core]\nsmooth-text = true\n"));
    // The contrast still tells: fuller letters.
    let ink = |image: &[Vec<u8>]| image.iter().flatten().filter(|g| **g < 128).count();
    let darkest = page("[core]\ntext-contrast = core.text-contrast.darkest\n");
    assert!(ink(&darkest) > ink(&before));
}

/// `[core]` lines with `text-sharpness` set to `level` (a level's last
/// part, or any value), and `more` after them.
fn sharpness(level: &str, more: &str) -> String {
    format!("[core]\ntext-sharpness = core.text-sharpness.{level}\n{more}")
}

/// How much ink a page's text lines take: the sum of their darkness.
fn ink_of(device: DeviceInfo, config: &str) -> u64 {
    let grays = text_grays(&mut reading(device, config));
    grays.iter().map(|g| u64::from(255 - g)).sum()
}

#[test]
fn sharpness_normal_and_values_it_does_not_know_draw_as_before() {
    for device in [BUTTONS, MONO] {
        let page = |config: &str| reading(device, config).snapshot();
        let before = page("");
        assert_eq!(before, page(&sharpness("normal", "")));
        for unknown in ["", "sharpest", "Sharp", "core.text-sharpness.sharp.x"] {
            let config = format!("[core]\ntext-sharpness = {unknown}\n");
            assert_eq!(before, page(&config), "{unknown:?}");
        }
        let junk = "[core]\ntext-sharpness = core.text-sharpness.\u{0}\u{ffff}\n";
        assert_eq!(before, page(junk));
    }
}

#[test]
fn soft_is_lighter_and_sharp_is_fuller_without_grays() {
    let between = |grays: &[u8]| grays.iter().filter(|g| !matches!(g, 0 | 255)).count();
    // Where the panel has grays: soft smooth and lighter, sharp without
    // grays even with smooth text on.
    let soft = text_grays(&mut reading(BUTTONS, &sharpness("soft", "")));
    assert!(between(&soft) > 1000, "{}", between(&soft));
    let sharp = text_grays(&mut reading(BUTTONS, &sharpness("sharp", "")));
    assert_eq!(between(&sharp), 0);
    let inks = ["soft", "normal", "sharp"].map(|l| ink_of(BUTTONS, &sharpness(l, "")));
    assert!(inks.windows(2).all(|w| w[0] < w[1]), "{inks:?}");
    // A step of the contrast each way: soft at normal contrast is the
    // lighter step, sharp the darker one without grays.
    let contrast = |level: &str, more: &str| {
        format!("[core]\ntext-contrast = core.text-contrast.{level}\n{more}")
    };
    assert_eq!(inks[0], ink_of(BUTTONS, &contrast("lighter", "")));
    let darker_sharp = contrast("darker", "smooth-text = false\n");
    assert_eq!(inks[2], ink_of(BUTTONS, &darker_sharp));
}

#[test]
fn sharpness_on_black_and_white_moves_the_threshold() {
    let page = |config: &str| reading(MONO, config).snapshot();
    let ink = |image: &[Vec<u8>]| image.iter().flatten().filter(|g| **g < 128).count();
    let [soft, normal, sharp] = ["soft", "normal", "sharp"].map(|l| page(&sharpness(l, "")));
    assert!(ink(&soft) < ink(&normal) && ink(&normal) < ink(&sharp));
    // The same pixels as a step of contrast each way.
    let contrast = |level: &str| {
        page(&format!(
            "[core]\ntext-contrast = core.text-contrast.{level}\n"
        ))
    };
    assert_eq!(soft, contrast("lighter"));
    assert_eq!(sharp, contrast("darker"));
    // At either end of the contrast it stays there, never past.
    let darkest = "text-contrast = core.text-contrast.darkest\n";
    assert_eq!(page(&sharpness("sharp", darkest)), contrast("darkest"));
}

#[test]
fn a_text_curve_takes_the_place_of_sharpness() {
    for device in [BUTTONS, MONO] {
        let curve = "text-curve = contrast:-1\n";
        let alone = reading(device, &format!("[core]\n{curve}")).snapshot();
        for level in ["soft", "sharp"] {
            let both = reading(device, &sharpness(level, curve)).snapshot();
            assert_eq!(alone, both, "{level}");
        }
    }
}

/// A book of short words each ending in a comma, so justified lines end
/// in one, with gaps narrow enough to fill the lines.
fn commas(config: &str) -> Reading {
    let mut text = String::new();
    for _ in 0..12 {
        for _ in 0..60 {
            text.push_str("ab, ");
        }
        text.push_str("end.\n\n");
    }
    let mut reading = Reading::new(MONO, Shared::default(), BOOK, text.as_bytes(), config);
    reading.open();
    reading
}

#[test]
fn hanging_punctuation_moves_marks_out_and_keeps_the_lines() {
    let justified = "[core]\njustify = true\n";
    let hanging = "[core]\njustify = true\nhanging-punctuation = true\n";
    // Off by default.
    let off = "[core]\njustify = true\nhanging-punctuation = false\n";
    assert_eq!(commas(justified).snapshot(), commas(off).snapshot());
    let (mut plain, mut hung) = (commas(justified), commas(hanging));
    assert_ne!(plain.snapshot(), hung.snapshot());
    // The same lines in the same rows: the page holds the same text.
    let rows = |lines: &[Line]| lines.iter().map(|(rows, _)| *rows).collect::<Vec<_>>();
    let (plain_lines, hung_lines) = (plain.lines(3), hung.lines(3));
    assert!(plain_lines.len() > 5);
    assert_eq!(rows(&plain_lines), rows(&hung_lines));
    // The commas at the lines' ends past the edge the letters keep.
    let right = |lines: &[Line]| {
        lines
            .iter()
            .filter_map(|(_, words)| words.last())
            .map(|w| w.1)
            .max()
    };
    assert!(right(&hung_lines) > right(&plain_lines));
}

/// A book whose paragraph starts with a drop capital as EPUBs set one
/// (a large letter floated left), or the same letter without its style;
/// in the reader's font (bitmap packs here, as on the X4), or in the
/// book's own, scalable one (Literata, embedded).
fn capital_book(capital: bool, embedded: bool) -> Reading {
    use paperpaw_zip::testing::ZipBuilder;
    const LITERATA: &[u8] = include_bytes!("../../fonts-default/fonts/Literata.ttf");
    let mut css = String::from(".dc { float: left; font-size: 3em }\n");
    if embedded {
        css.push_str("@font-face { font-family: B; src: url(fonts/b.ttf) } p { font-family: B }\n");
    }
    let class = if capital { " class=\"dc\"" } else { "" };
    let words = "lorem ipsum dolor sit amet ".repeat(30);
    let chapter = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml"><head><title>c</title>
<link rel="stylesheet" type="text/css" href="style.css"/></head>
<body><p><span{class}>L</span>orem {words}</p></body></html>"#
    );
    let opf = r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Capital</dc:title><dc:language>en</dc:language></metadata>
  <manifest><item id="c" href="c.xhtml" media-type="application/xhtml+xml"/>
  <item id="s" href="style.css" media-type="text/css"/>
  <item id="f" href="fonts/b.ttf" media-type="font/ttf"/></manifest>
  <spine><itemref idref="c"/></spine>
</package>"#;
    let container = r#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>"#;
    let data = ZipBuilder::new()
        .stored("mimetype", "application/epub+zip")
        .deflated("META-INF/container.xml", container)
        .deflated("OEBPS/content.opf", opf)
        .deflated("OEBPS/style.css", css)
        .stored("OEBPS/fonts/b.ttf", LITERATA)
        .deflated("OEBPS/c.xhtml", chapter)
        .build();
    let mut reading = Reading::new(MONO, Shared::default(), "books/capital.epub", &data, "");
    reading.open();
    reading
}

#[test]
#[ignore = "needs native activation (format removal)"]
fn a_drop_capital_spans_the_first_lines_and_its_word_stays_whole() {
    let (mut plain, mut capped) = (capital_book(false, true), capital_book(true, true));
    let (plain_lines, capped_lines) = (plain.lines(3), capped.lines(3));
    let height = |line: &Line| line.0.1 - line.0.0;
    // The capital's ink joins the rows of the lines beside it into one,
    // as high as more than two of them.
    assert!(
        height(&capped_lines[0]) > 2 * height(&plain_lines[0]),
        "{:?} {:?}",
        capped_lines[0].0,
        plain_lines[0].0
    );
    // It stands at the margin, its first lines set in beside it.
    let margin = plain_lines[1].1[0].0;
    let (capital, beside) = (capped_lines[0].1[0].0, capped_lines[0].1[1].0);
    // (Its ink a little in: a large letter's side bearing.)
    assert!(capital.abs_diff(margin) <= 15, "{capital} {margin}");
    assert!(beside > margin + 20, "{beside} {margin}");
    // A bookmark's text has the word whole.
    capped.run(paperpaw_plugin_reader::actions::TOGGLE_BOOKMARK);
    let kept = paperpaw_sdk::annotations::load(&capped.storage, "books/capital.epub");
    assert_eq!(kept.len(), 1);
    assert!(
        kept[0].text.starts_with("Lorem lorem ipsum"),
        "{:?}",
        kept[0].text
    );
}

#[test]
fn in_a_font_of_fixed_sizes_a_drop_capital_is_left_out() {
    // The packs are no larger than a heading: the page as without it.
    let page = |capital: bool| capital_book(capital, false).snapshot();
    assert_eq!(page(true), page(false));
}

#[test]
#[ignore = "needs native activation (format removal)"]
fn a_list_items_lines_stand_under_its_text_not_its_marker() {
    let words = "lorem ipsum dolor sit amet ".repeat(8);
    let body =
        format!("<ul><li>Bullet {words}</li></ul><ol start=\"7\"><li>Seven {words}</li></ol>");
    let data = epub(&[("c.xhtml", body)], &[]);
    let mut reading = Reading::new(MONO, Shared::default(), "books/list.epub", &data, "");
    reading.open();
    let lines = reading.lines(3);
    let first_word = |line: &Line| line.1[0].0;
    // The bullet's item: its marker, then its text, under which the next
    // line starts (within a little of a letter: the font's widths are
    // estimated) and well after the marker.
    let bullet = lines.iter().position(|l| l.1.len() > 2).unwrap();
    let (marker, text) = (lines[bullet].1[0].0, lines[bullet].1[1].0);
    let next = first_word(&lines[bullet + 1]);
    assert!(next > marker + 5, "{marker} {text} {next}");
    assert!(next.abs_diff(text) <= 4, "{marker} {text} {next}");
    // The same for a number.
    let number = lines.iter().rposition(|l| first_word(l) < next).unwrap();
    let (marker, text) = (lines[number].1[0].0, lines[number].1[1].0);
    let next = first_word(&lines[number + 1]);
    assert!(next > marker + 5, "{marker} {text} {next}");
    assert!(next.abs_diff(text) <= 4, "{marker} {text} {next}");
}
