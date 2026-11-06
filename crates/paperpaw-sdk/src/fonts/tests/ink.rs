//! `FontHandle::ink_drawn` says where `draw` puts ink, pixel for pixel.

use paperpaw_graphics::{Framebuffer, InkBounds};
use paperpaw_platform::{Color, PixelFormat};

use alloc::rc::Rc;
use alloc::string::String;

use super::inked_fonts;
use crate::fonts::{Font, FontHandle, FontSpec, Fonts};
use crate::{Id, MemoryStorage, Storage};

/// Literata's `a` and space, and Nanum Myeongjo's syllables as a pack
/// extending it, drawn from outlines.
fn hangul_fonts() -> (Fonts, Rc<dyn Storage>) {
    let outline =
        |data: &'static [u8]| paperpaw_text::OutlineFont::new(data.to_vec(), &[]).unwrap();
    let latin = paperpaw_text::build_pack(
        &outline(include_bytes!(
            "../../../../../plugins/fonts-default/fonts/Literata.ttf"
        )),
        &[10.0],
        "a ".chars(),
    );
    let syllables = paperpaw_text::build_pack(
        &outline(include_bytes!(
            "../../../../../tools/fonts/NanumMyeongjo-Regular.ttf"
        )),
        &[10.0],
        "가한국".chars(),
    );
    let storage: Rc<dyn Storage> =
        Rc::new(MemoryStorage::new().with("plugins/k/fonts/latin-hangul.ppfp", syllables));
    let mut fonts = Fonts::new(64 * 1024);
    let font = Font::from_data(latin, &[]).unwrap();
    fonts
        .add(FontSpec::new(Id::new_static("a.latin"), font))
        .unwrap();
    fonts
        .add_extension(
            Id::new_static("k.latin-hangul"),
            String::from("plugins/k/fonts/latin-hangul.ppfp"),
            Id::new_static("a.latin"),
        )
        .unwrap();
    assert!(fonts.load(&storage).is_empty());
    (fonts, storage)
}

/// Where drawing `text` on a white frame left pixels at least half black,
/// counted from the pen at `(X, BASELINE)`.
fn ink_on_frame(font: &FontHandle<'_>, size: f32, text: &str) -> Option<[i32; 4]> {
    const X: i32 = 200;
    const BASELINE: i32 = 60;
    const WIDTH: u16 = 600;
    let mut frame = Framebuffer::new(WIDTH, 100, PixelFormat::Gray8).unwrap();
    {
        let mut canvas = frame.canvas();
        canvas.clear(Color::WHITE);
        font.draw(&mut canvas, size, X as f32, BASELINE, text, Color::BLACK);
    }
    let view = frame.view();
    let mut ink: Option<[i32; 4]> = None;
    for (at, gray) in view.data.iter().enumerate() {
        if *gray < 128 {
            let (x, y) = (
                (at % usize::from(WIDTH)) as i32 - X,
                (at / usize::from(WIDTH)) as i32 - BASELINE,
            );
            ink = Some(ink.map_or([x, y, x + 1, y + 1], |[l, t, r, b]| {
                [l.min(x), t.min(y), r.max(x + 1), b.max(y + 1)]
            }));
        }
    }
    ink
}

fn sides(ink: InkBounds) -> [i32; 4] {
    [ink.left, ink.top, ink.right, ink.bottom]
}

fn same_as_drawn(font: &FontHandle<'_>, size: f32, text: &str) {
    let probed = font.ink_drawn(size, text).map(sides);
    assert!(probed.is_some(), "{text:?} has ink");
    assert_eq!(probed, ink_on_frame(font, size, text), "{text:?}");
}

#[test]
fn latin_text_inks_where_it_is_drawn() {
    let (fonts, _storage) = inked_fonts();
    let font = fonts.get("a.latin").unwrap();
    for text in ["a", "(a):", "a. a"] {
        same_as_drawn(&font, 20.0, text);
    }
}

#[test]
fn right_to_left_and_joined_text_inks_where_it_is_drawn() {
    let (fonts, _storage) = inked_fonts();
    let font = fonts.get("a.latin").unwrap();
    // Arabic joined, Hebrew, and both ways with a mirrored bracket.
    for text in [
        "\u{628}\u{644}\u{62F}",
        "\u{5E9}\u{5DC}\u{5D5}\u{5DD}",
        "a (\u{628}\u{644}\u{62F}) a",
    ] {
        same_as_drawn(&font, 20.0, text);
    }
}

#[test]
fn hangul_in_jamo_inks_as_its_syllables() {
    let (fonts, _storage) = hangul_fonts();
    let font = fonts.get("a.latin").unwrap();
    let nfd = "a \u{1112}\u{1161}\u{11AB}\u{1100}\u{116E}\u{11A8}";
    same_as_drawn(&font, 10.0, nfd);
    assert_eq!(font.ink_drawn(10.0, nfd), font.ink_drawn(10.0, "a 한국"));
}

#[test]
fn text_without_ink_has_none_also_while_describing() {
    let (fonts, _storage) = inked_fonts();
    let font = fonts.get("a.latin").unwrap();
    assert_eq!(font.ink_drawn(20.0, "  "), None);
    assert_eq!(font.ink_drawn(20.0, ""), None);
    let plain = font.ink_drawn(20.0, "a");
    fonts.set_describing(true);
    assert_eq!(font.ink_drawn(20.0, "a"), plain);
    assert!(
        fonts.drawing.describing.get(),
        "describing is left as it was"
    );
    fonts.set_describing(false);
}
