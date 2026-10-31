//! A look's text painter and its list of extra painters: the SDK asks
//! the text painter about the labels it draws, a painter changes how a
//! label looks and not how wide it is, and a look's extras are found by
//! kind, the first of a kind winning.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod support;

use std::cell::Cell;

use paperpaw_sdk::ui::look::{ExtraPaint, ExtraPart, Look, PaintCx, Painters, PartKind, TextPart};
use paperpaw_sdk::ui::{self, Label, TextRole, UiStyle};
use paperpaw_sdk::{Color, Rect};

use support::{Device, Harness, frame};

/// A paper-colour frame drawn on by `draw`.
fn on_paper(
    width: u16,
    height: u16,
    draw: impl FnOnce(&mut paperpaw_sdk::Canvas<'_>),
) -> paperpaw_graphics::Framebuffer {
    let mut f = frame(width, height);
    draw(&mut f.canvas());
    f
}

thread_local! {
    static SEEN: Cell<(u32, u8, u32)> = const { Cell::new((0, 255, 0)) };
}

/// Draws the text one pixel lower in gray, as a shadow, and the text.
fn shadow(cx: &mut PaintCx<'_, '_>, part: &TextPart<'_>) -> bool {
    SEEN.with(|s| {
        let (n, _, w) = s.get();
        s.set((n + 1, part.role.value(), w + part.text.len() as u32));
    });
    let shade = Color::gray(150);
    part.font.draw(
        cx.canvas,
        part.size,
        part.x,
        part.baseline + 2,
        part.text,
        shade,
    );
    part.font.draw(
        cx.canvas,
        part.size,
        part.x,
        part.baseline,
        part.text,
        part.color,
    );
    true
}

fn declines(_: &mut PaintCx<'_, '_>, _: &TextPart<'_>) -> bool {
    false
}

static SHADOW: Painters = Painters::new().text(shadow);
static DECLINES: Painters = Painters::new().text(declines);

fn ink_rows(style: &UiStyle, env: &paperpaw_sdk::Env<'_>) -> (u32, f32) {
    let area = Rect::new(10, 10, 300, 40);
    let mut end = 0.0;
    let frame = on_paper(320, 60, |canvas| {
        end = Label::new("Settings")
            .role(TextRole::TITLE)
            .draw(&mut ui::Ui::new(canvas, env, style), area);
    });
    let view = frame.view();
    let mut dark = 0;
    for y in 0..60 {
        for x in 0..320 {
            if view.pixel(x, y) != support::PAPER {
                dark += 1;
            }
        }
    }
    (dark, end)
}

#[test]
fn the_text_painter_is_asked_about_labels_and_keeps_their_width() {
    let h = Harness::new(Device::TOUCH);
    let env = h.env();
    let plain = UiStyle::new(&env);
    let looked = plain.with_look(Look::of(&SHADOW));
    let (plain_dark, plain_end) = ink_rows(&plain, &env);
    SEEN.with(|s| s.set((0, 255, 0)));
    let (looked_dark, looked_end) = ink_rows(&looked, &env);
    let (asked, role, chars) = SEEN.with(Cell::get);
    assert_eq!((asked, role, chars), (1, TextRole::TITLE.value(), 8));
    // The shadow adds ink; where the label ends is the stock end.
    assert!(looked_dark > plain_dark, "{looked_dark} {plain_dark}");
    // (Measured, not drawn: within a pixel of what drawing says.)
    assert!(
        (looked_end - plain_end).abs() < 1.0,
        "{looked_end} {plain_end}"
    );
}

#[test]
fn a_painter_that_declines_leaves_the_label_as_usual() {
    let h = Harness::new(Device::TOUCH);
    let env = h.env();
    let plain = UiStyle::new(&env);
    let declining = plain.with_look(Look::of(&DECLINES));
    assert_eq!(ink_rows(&plain, &env), ink_rows(&declining, &env));
}

fn bar(cx: &mut PaintCx<'_, '_>, part: &ExtraPart<'_>) -> bool {
    cx.canvas.fill(part.rect, Color::gray(40));
    true
}
fn other_bar(cx: &mut PaintCx<'_, '_>, part: &ExtraPart<'_>) -> bool {
    cx.canvas.fill(part.rect, Color::gray(200));
    true
}

static EXTRAS: [ExtraPaint; 3] = [
    ExtraPaint::new(PartKind::SCROLLBAR, bar),
    ExtraPaint::new(PartKind::SCROLLBAR, other_bar),
    ExtraPaint::new(PartKind::TAB, other_bar),
];
static WITH_EXTRAS: Painters = Painters::new().text(shadow).extras(&EXTRAS).flags(0);

#[test]
fn extras_are_found_by_kind_and_the_first_of_a_kind_wins() {
    let h = Harness::new(Device::TOUCH);
    let env = h.env();
    let style = UiStyle::new(&env).with_look(Look::of(&WITH_EXTRAS));
    let rect = Rect::new(2, 2, 10, 10);
    let drawn = |kind: PartKind| {
        let mut painted = false;
        let frame = on_paper(20, 20, |canvas| {
            painted = style
                .look()
                .extra(canvas, &style, &ExtraPart::new(kind, rect));
        });
        (painted, frame.view().pixel(5, 5))
    };
    assert_eq!(drawn(PartKind::SCROLLBAR), (true, Color::gray(40)));
    assert_eq!(drawn(PartKind::TAB), (true, Color::gray(200)));
    // A kind the look has no painter for is left to the SDK.
    assert!(!drawn(PartKind::BADGE).0);
    assert!(!drawn(PartKind::from_value(900)).0);
    assert_eq!(WITH_EXTRAS.flag_bits(), 0);
    // No look at all: nothing painted.
    let plain = UiStyle::new(&env);
    let mut painted = true;
    let _ = on_paper(20, 20, |canvas| {
        painted = plain
            .look()
            .extra(canvas, &plain, &ExtraPart::new(PartKind::TAB, rect));
    });
    assert!(!painted);
}

#[test]
fn the_new_fields_are_zero_and_kinds_have_numbers() {
    assert_eq!(
        [
            PartKind::SCROLLBAR,
            PartKind::TAB,
            PartKind::BADGE,
            PartKind::TOAST,
            PartKind::DIALOG
        ]
        .map(PartKind::value),
        [1, 2, 3, 4, 5]
    );
    let part = ExtraPart::new(PartKind::TAB, Rect::new(0, 0, 1, 1))
        .with_state(1)
        .with_values([1, 2, 3, 4])
        .with_label("x");
    assert!(format!("{part:?}").contains("spare: [0, 0]"));
}

/// Text drawn outside `Label` (a button, a header, `draw_label_in`) reaches
/// the text painter with its role; `draw_label` is the stock look.
#[test]
fn text_of_buttons_headers_and_draw_label_in_reaches_the_painter() {
    let h = Harness::new(Device::TOUCH);
    let env = h.env();
    let style = UiStyle::new(&env).with_look(Look::of(&SHADOW));
    let area = Rect::new(10, 10, 300, 40);
    let asked = |draw: &dyn Fn(&mut ui::Ui<'_>)| {
        SEEN.with(|s| s.set((0, 255, 0)));
        let _ = on_paper(320, 60, |canvas| {
            draw(&mut ui::Ui::new(canvas, &env, &style))
        });
        SEEN.with(Cell::get)
    };
    let font = style.font(&env).unwrap();
    let (n, role, _) = asked(&|ui| ui.button(area, "Open", false, false));
    assert_eq!((n, role), (1, TextRole::BUTTON.value()));
    let (n, _, _) = asked(&|ui| ui.header(area, "Library", None));
    assert!(n >= 1, "{n}");
    let (n, role, chars) = asked(&|ui| {
        ui::draw_label_in(
            ui.canvas(),
            &font,
            &style,
            TextRole::HINT,
            style.small(),
            12.0,
            area,
            "Hint",
            Color::BLACK,
        );
    });
    assert_eq!((n, role, chars), (1, TextRole::HINT.value(), 4));
    let (n, _, _) = asked(&|ui| {
        ui::draw_label(
            ui.canvas(),
            &font,
            style.small(),
            12.0,
            area,
            "Hint",
            Color::BLACK,
        );
    });
    assert_eq!(n, 0);
}
