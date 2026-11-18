//! The interface's text is inked as `core.ui-text-curve` says, a font
//! handle draws with the ink it was given, and a curve of the setting for
//! one script leaves other languages as they are.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

extern crate std;
#[allow(
    unused_imports,
    reason = "the names of std the tests had as SDK integration tests, not all used"
)]
use std::{
    borrow::ToOwned,
    boxed::Box,
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};

use super::support;

use crate::kit::ui::UiStyle;
use paperpaw_sdk::Color;
use paperpaw_sdk::text::{TextInk, ToneCurve};

use support::{Device, Harness, frame};

#[test]
fn a_handle_draws_with_its_ink_and_draw_toned_keeps_its_own() {
    let h = Harness::new(Device::TOUCH);
    let env = h.env();
    let font = env
        .fonts()
        .role("core.ui")
        .or_else(|| UiStyle::new(&env).font(&env))
        .unwrap();
    assert_eq!(font.text_ink().tone(), ToneCurve::SMOOTH);
    let heavy = font.inked(TextInk::new(ToneCurve::contrast(2)));
    assert_eq!(heavy.text_ink().tone(), ToneCurve::contrast(2));
    let sum = |font: &paperpaw_sdk::FontHandle<'_>, tone: Option<ToneCurve>| {
        let mut f = frame(120, 30);
        let mut canvas = f.canvas();
        match tone {
            None => font.draw(&mut canvas, 14.0, 2.0, 20, "Hamburg", Color::BLACK),
            Some(t) => font.draw_toned(&mut canvas, 14.0, 2.0, 20, "Hamburg", Color::BLACK, 0, t),
        };
        let view = f.view();
        (0..30u16)
            .flat_map(|y| (0..120u16).map(move |x| (x, y)))
            .map(|(x, y)| u32::from(255 - view.pixel(x, y).luma()))
            .sum::<u32>()
    };
    assert!(sum(&heavy, None) > sum(&font, None));
    // An explicit tone wins over the handle's.
    assert_eq!(
        sum(&heavy, Some(ToneCurve::SMOOTH)),
        sum(&font, Some(ToneCurve::SMOOTH))
    );
    assert_eq!(sum(&font, None), sum(&font, Some(ToneCurve::SMOOTH)));
}

/// `arab=` reaches languages by their usual script (`ar`, `fa`, `ur`), not
/// only tags that name it (`fa-Arab`), in the interface and in a book.
#[test]
fn an_entry_for_a_script_reaches_the_languages_written_in_it() {
    use paperpaw_sdk::Lang;
    let curve = |tag: &str| {
        ToneCurve::parse_for("smooth; arab=contrast:2", Lang::parse(tag).likely_script())
    };
    let stock = ToneCurve::parse_for("smooth", None);
    for tag in ["ar", "fa", "ur", "ps", "fa-Arab"] {
        assert_eq!(
            curve(tag),
            ToneCurve::parse_for("contrast:2", None),
            "{tag}"
        );
    }
    for tag in ["en", "de", "he", "ko", ""] {
        assert_eq!(curve(tag), stock, "{tag}");
    }
}
