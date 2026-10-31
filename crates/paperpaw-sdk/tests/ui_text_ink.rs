//! The interface's text is inked as `core.ui-text-curve` says, a font
//! handle draws with the ink it was given, and a curve of the setting for
//! one script leaves other languages as they are.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod support;

use paperpaw_sdk::builtin::settings;
use paperpaw_sdk::text::{TextInk, ToneCurve};
use paperpaw_sdk::ui::{self, Label, UiStyle};
use paperpaw_sdk::{Color, Rect};

use support::{Device, Harness, frame};

/// The sum of how dark a label's pixels are drawn on paper.
fn darkness(h: &Harness) -> u32 {
    let env = h.env();
    let style = UiStyle::new(&env);
    let mut f = frame(200, 40);
    Label::new("Hamburgefons").draw(
        &mut ui::Ui::new(&mut f.canvas(), &env, &style),
        Rect::new(4, 4, 190, 30),
    );
    let view = f.view();
    (0..40u16)
        .flat_map(|y| (0..200u16).map(move |x| (x, y)))
        .map(|(x, y)| u32::from(255 - view.pixel(x, y).luma()) - 178)
        .sum::<u32>()
}

#[test]
fn the_ui_font_is_inked_by_the_setting() {
    let mut plain = Harness::new(Device::TOUCH);
    plain.define_text(settings::UI_TEXT_CURVE.as_str(), "");
    let mut dark = Harness::new(Device::TOUCH);
    dark.define_text(settings::UI_TEXT_CURVE.as_str(), "contrast:2");
    let mut light = Harness::new(Device::TOUCH);
    light.define_text(settings::UI_TEXT_CURVE.as_str(), "contrast:-1");
    let (p, d, l) = (darkness(&plain), darkness(&dark), darkness(&light));
    assert!(d > p && p > l, "{d} {p} {l}");
    // Not set at all, and a value nothing can read: as without.
    assert_eq!(darkness(&Harness::new(Device::TOUCH)), p);
    let mut junk = Harness::new(Device::TOUCH);
    junk.define_text(settings::UI_TEXT_CURVE.as_str(), "shiny");
    assert_eq!(darkness(&junk), p);
}

#[test]
fn an_entry_for_another_script_leaves_the_text_alone() {
    let mut plain = Harness::new(Device::TOUCH);
    plain.define_text(settings::UI_TEXT_CURVE.as_str(), "");
    let mut scripted = Harness::new(Device::TOUCH);
    scripted.define_text(settings::UI_TEXT_CURVE.as_str(), "arab=contrast:2");
    assert_eq!(darkness(&scripted), darkness(&plain));
}

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
