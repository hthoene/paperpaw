//! The drawing helpers every screen is made of, in the UI font PaperPaw
//! ships: rows, switches, numbers, buttons, headers, pagers, notes, empty
//! screens and the row of hints draw inside the rectangle they are given
//! (long texts shortened, never run into what is next to them), keep to
//! the same margins, and a header's or pager's arrows are hit where they
//! are drawn, on touch screens only.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod support;

use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::semantics::{self, Role};
use paperpaw_sdk::ui::{self, Header, Pager, Paging, UiStyle};
use paperpaw_sdk::widgets;
use paperpaw_sdk::{Color, Hint, Point, Rect};
use proptest::prelude::*;

use support::{Device, Harness, describe, drawn_in, drawn_outside, frame, middle};

/// A way of drawing, to try each in turn.
type Draw<'a> = &'a dyn Fn(&mut paperpaw_sdk::Canvas<'_>);

/// Texts of all kinds: none, short, long, other scripts, marks.
fn text_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just(String::new()),
        "[a-zA-Z ]{1,12}",
        "[a-zA-Z äöüß]{20,90}",
        "\\PC{0,40}",
    ]
}

#[test]
fn the_ui_font_and_icons_are_there() {
    let h = Harness::new(Device::TOUCH);
    let env = h.env();
    let style = UiStyle::new(&env);
    assert!(style.font(&env).is_some());
    assert!(ui::has_icons(&env));
    assert!(ui::icon_width(&env, "settings", style.text()) > 0.0);
    assert_eq!(ui::icon_width(&env, "no-such-icon", style.text()), 0.0);
}

// ---- Shortening -------------------------------------------------------

/// A text shortened counts its pairs as they are set: the UI font kerns
/// `f\` apart, so a text of such pairs takes more than its advances.
#[test]
fn shortening_counts_the_pairs_kerned_apart() {
    let h = Harness::new(Device::TOUCH);
    let env = h.env();
    let style = UiStyle::new(&env);
    let font = style.font(&env).unwrap();
    let size = style.text();
    assert!(font.kern(size, 'f', '\\') > 1.0, "the pair the test needs");
    let text = "f\\".repeat(40);
    for width in [60.0, 120.0, 200.0, 333.0] {
        let shown = ui::fit(&font, size, &text, width);
        assert!(font.measure(size, &shown) <= width, "{width}: {shown}");
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256)))]

    /// A text shortened to a width fits it (with a pixel of slack), and
    /// is the text itself or its beginning and "…".
    #[test]
    fn shortened_text_fits_and_begins_as_the_text(text in text_strategy(), width in 10.0f32..400.0) {
        let h = Harness::new(Device::TOUCH);
        let env = h.env();
        let style = UiStyle::new(&env);
        let font = style.font(&env).unwrap();
        let shown = ui::fit(&font, style.text(), &text, width);
        prop_assert!(font.measure(style.text(), &shown) <= width.max(font.measure(style.text(), "…")) + 1.0);
        if shown != text {
            let start = shown.strip_suffix('…').expect("ends with …");
            prop_assert!(text.starts_with(start));
        }
    }

    /// A title and a value side by side take no more than the room, the
    /// value never pushes the title below its share.
    #[test]
    fn a_title_and_a_value_share_the_width(
        title in text_strategy(),
        value in text_strategy(),
        width in 60.0f32..480.0,
    ) {
        let h = Harness::new(Device::TOUCH);
        let env = h.env();
        let style = UiStyle::new(&env);
        let font = style.font(&env).unwrap();
        let gap = 16.0;
        let (t, v, value_width) = ui::fit_pair(&font, style.text(), &title, &value, width, gap);
        let tw = font.measure(style.text(), &t);
        prop_assert!((font.measure(style.text(), &v) - value_width).abs() < 0.01);
        // Two pixels of slack: each part has one for rounding.
        let ellipses = 2.0 * font.measure(style.text(), "…");
        prop_assert!(tw + value_width + gap <= width.max(ellipses + gap) + 2.0, "{tw} + {value_width}");
    }
}

// ---- Rows ---------------------------------------------------------------

fn row_strategy() -> impl Strategy<Value = Rect> {
    // At least 400 wide, as rows of lists are on a screen 480 wide: a
    // number's − and + and the room between them take some 300 pixels
    // (narrower, the − goes past the row's start).
    (0u16..80, 0u16..600, 400u16..480, 0u16..3).prop_map(|(x, y, width, kind)| {
        Rect::new(x, y, width.min(480 - x), [60, 72, 110][usize::from(kind)])
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(64)))]

    /// Rows of every kind draw inside their rectangle, focused or not,
    /// whatever their texts (numbers as numbers are: a value far wider
    /// than "000 %" pushes the − to the left, past the row's start).
    #[test]
    fn rows_draw_inside_their_rectangle(
        rect in row_strategy(),
        title in text_strategy(),
        value in text_strategy(),
        note in text_strategy(),
        number in "[0-9]{1,4}( %| px| min)?",
        focused in any::<bool>(),
        on in any::<bool>(),
        touch in any::<bool>(),
    ) {
        let h = Harness::new(if touch { Device::TOUCH } else { Device::BUTTONS });
        let env = h.env();
        let style = UiStyle::new(&env);
        // At least as high as rows are, those with notes as noted rows.
        let row = Rect { height: rect.height.max(style.row()), ..rect };
        let noted = Rect { height: rect.height.max(style.noted_row() as u16), ..rect };
        let draws: [(Rect, Draw<'_>); 6] = [
            (row, &|c| ui::Row::value(&title, &value).focused(focused).draw(&mut ui::Ui::new(c, &env, &style), row)),
            (noted, &|c| ui::Row::value(&title, &value).note(&note).focused(focused).draw(&mut ui::Ui::new(c, &env, &style), noted)),
            (noted, &|c| ui::Row::switch(&title, on).note(&note).focused(focused).draw(&mut ui::Ui::new(c, &env, &style), noted)),
            (row, &|c| ui::Row::number(&title, &number, "").step_focus(focused.then_some(on)).draw(&mut ui::Ui::new(c, &env, &style), row)),
            (row, &|c| ui::draw_stepper(c, &env, &style, row, &title, &number, "", on)),
            (row, &|c| ui::Ui::new(c, &env, &style).heading(row, &title)),
        ];
        for (n, (bound, draw)) in draws.iter().enumerate() {
            let mut frame = frame(480, 800);
            draw(&mut frame.canvas());
            prop_assert_eq!(drawn_outside(&mut frame, *bound), None, "kind {}", n);
        }
    }

    /// A row's focus sits inside the row, as a bar within the margins.
    #[test]
    fn the_focus_bar_stays_in_its_row(rect in row_strategy()) {
        let h = Harness::new(Device::TOUCH);
        let env = h.env();
        let style = UiStyle::new(&env);
        let focus = ui::focus_rect(&style, rect);
        prop_assert!(focus.x >= rect.x && focus.right() <= rect.right());
        prop_assert!(focus.y >= rect.y && focus.bottom() <= rect.bottom());
        prop_assert!(!focus.is_empty());
        let mut frame = frame(480, 800);
        ui::Ui::new(&mut frame.canvas(), &env, &style).focus(rect);
        prop_assert_eq!(drawn_outside(&mut frame, focus), None);
    }
}

#[test]
fn the_switch_sits_at_the_rows_end_and_the_title_ends_before_it() {
    let h = Harness::new(Device::TOUCH);
    let env = h.env();
    let style = UiStyle::new(&env);
    let row = Rect::new(0, 100, 480, style.row());
    let switch = ui::switch_rect(&style, row);
    // Flush with the margin, where other rows end their values.
    assert_eq!(switch.right() + u32::from(style.margin()), row.right());
    assert_eq!(
        switch.y - row.y,
        (row.bottom() - switch.bottom()) as u16,
        "centred"
    );
    let long = "A title much longer than the row could ever hold beside a switch";
    let (described, mut frame) = describe(480, 800, |c| {
        ui::Row::switch(long, true)
            .note("")
            .focused(false)
            .draw(&mut ui::Ui::new(c, &env, &style), row);
    });
    assert_eq!(drawn_outside(&mut frame, row), None);
    // From a gap before the switch on, nothing but the switch: the title
    // was shortened before it.
    let mut alone = support::frame(480, 800);
    ui::Ui::new(&mut alone.canvas(), &env, &style).switch(row, true, false);
    let from = switch.x - style.gap();
    let (view, alone) = (frame.view(), alone.view());
    for y in row.y..row.bottom() as u16 {
        for x in from..row.right() as u16 {
            assert_eq!(view.pixel(x, y), alone.pixel(x, y), "({x}, {y})");
        }
    }
    let parts = semantics::parts(described);
    assert!(
        parts
            .iter()
            .any(|p| p.role == Role::Switch && p.on == Some(true))
    );
}

/// Numbers above each other with the same room line their − and +
/// buttons up, whatever their titles and values.
#[test]
fn steppers_of_rows_above_each_other_line_up() {
    let h = Harness::new(Device::TOUCH);
    let env = h.env();
    let style = UiStyle::new(&env);
    let room = "1000 px";
    let mut columns = Vec::new();
    for (n, value) in ["1", "250 %", "1000 px", ""].iter().enumerate() {
        let row = Rect::new(0, 100 + n as u16 * 70, 480, 70);
        let (minus, plus) = ui::stepper(&env, &style, row, room).unwrap();
        columns.push((minus.x, plus.x));
        assert_eq!(plus.right() + u32::from(style.margin()), row.right());
        assert!(minus.right() < u32::from(plus.x));
        let mut frame = frame(480, 800);
        ui::Row::number("Size", value, room)
            .step_focus(Some(true))
            .draw(&mut ui::Ui::new(&mut frame.canvas(), &env, &style), row);
        assert_eq!(drawn_outside(&mut frame, row), None, "{value}");
    }
    assert!(columns.windows(2).all(|w| w[0] == w[1]), "{columns:?}");
}

#[test]
fn buttons_draw_inside_their_frame_and_tell_what_they_are() {
    let h = Harness::new(Device::TOUCH);
    let env = h.env();
    let style = UiStyle::new(&env);
    for label in [
        "OK",
        "+",
        "m",
        "A label far too long for a small button",
        "",
    ] {
        for (on, focused) in [(false, false), (true, false), (false, true), (true, true)] {
            let rect = Rect::new(100, 100, 90, 50);
            let (described, mut frame) = describe(480, 300, |c| {
                ui::Ui::new(c, &env, &style).button(rect, label, on, focused);
            });
            // The focus frame goes round the button.
            let ring = if focused {
                (ui::button_border(&style) * 3) as u16
            } else {
                0
            };
            assert_eq!(
                drawn_outside(&mut frame, rect.grow(ring)),
                None,
                "{label:?} on {on} focused {focused}"
            );
            let button = &described[0];
            assert_eq!(
                (button.role, button.label.as_str(), button.focused),
                (Role::Button, label, focused)
            );
            assert_eq!(button.on, on.then_some(true));
        }
    }
}

/// A `ui::Ui` drawn in a part of the canvas draws nothing outside it, and
/// tells what it draws.
#[test]
fn the_ui_context_keeps_to_its_part() {
    let h = Harness::new(Device::TOUCH);
    let env = h.env();
    let style = UiStyle::new(&env);
    let rect = Rect::new(40, 40, 120, 50);
    let part = Rect::new(0, 0, 100, 300);
    let (told, mut clipped) = describe(480, 300, |c| {
        let mut ui = ui::Ui::new(c, &env, &style);
        ui.clipped(part).button(rect, "OK", true, false);
    });
    assert_eq!(drawn_outside(&mut clipped, part), None);
    assert_eq!(told[0].role, Role::Button);
}

/// A label on one line draws as `draw_label` does, keeps to its area
/// when too long, and sits where it is aligned.
#[test]
fn a_label_on_one_line_keeps_to_its_area_and_alignment() {
    let h = Harness::new(Device::TOUCH);
    let env = h.env();
    let style = UiStyle::new(&env);
    let font = style.font(&env).unwrap();
    let area = Rect::new(40, 40, 200, 40);
    let size = style.text();
    let text = "Words";
    // As the parts' function draws it.
    let (told, old) = describe(480, 200, |c| {
        ui::draw_label(c, &font, size, f32::from(area.x), area, text, Color::BLACK);
    });
    let (said, new) = describe(480, 200, |c| {
        ui::Label::new(text)
            .px(size)
            .color(Color::BLACK)
            .draw(&mut ui::Ui::new(c, &env, &style), area);
    });
    assert_eq!(told, said);
    assert!(old.view().data == new.view().data);
    // From between pixels, in a font given, as `draw_label` too.
    for x in [41.37, 100.5, 240.0, 300.0] {
        let (told, old) = describe(480, 200, |c| {
            ui::draw_label(c, &font, size, x, area, text, Color::BLACK);
        });
        let (said, new) = describe(480, 200, |c| {
            ui::Label::new(text)
                .font(font)
                .px(size)
                .color(Color::BLACK)
                .draw_from(&mut ui::Ui::new(c, &env, &style), x, area);
        });
        assert_eq!(told, said);
        assert!(old.view().data == new.view().data, "{x}");
    }
    // Too long: shortened within its area.
    let long = "A label far too long for the room it is given here";
    let (_, mut frame) = describe(480, 200, |c| {
        let end = ui::Label::new(long).draw(&mut ui::Ui::new(c, &env, &style), area);
        assert!(end <= f32::from(area.x + area.width) + 1.0, "{end}");
    });
    assert_eq!(drawn_outside(&mut frame, area), None);
    // Centred and at the right.
    let width = ui::Label::new(text).width(&env, &style);
    for (align, start) in [
        (
            widgets::Align::Center,
            f32::from(area.x) + (f32::from(area.width) - width) / 2.0,
        ),
        (
            widgets::Align::Right,
            f32::from(area.x + area.width) - width,
        ),
    ] {
        let (_, frame) = describe(480, 200, |c| {
            ui::Label::new(text)
                .align(align)
                .draw(&mut ui::Ui::new(c, &env, &style), area);
        });
        let left = Rect::new(
            area.x,
            area.y,
            (start as u16).saturating_sub(area.x + 2),
            area.height,
        );
        assert!(!drawn_in(&frame, left), "{align:?}");
    }
    // Smaller down to its least size rather than shortened.
    let wide = ui::Label::new(long).width(&env, &style);
    let (_, frame) = describe(480, 200, |c| {
        let end = ui::Label::new(long)
            .min(1.0)
            .draw(&mut ui::Ui::new(c, &env, &style), area);
        assert!(end <= f32::from(area.x + area.width) + 1.0);
    });
    assert!(wide > f32::from(area.width) && drawn_in(&frame, area));
}

/// A wrapped label takes the lines its height says, and where its area
/// holds fewer its last line says there is more.
#[test]
fn a_wrapped_label_takes_its_lines_and_cuts_the_last() {
    let h = Harness::new(Device::TOUCH);
    let env = h.env();
    let style = UiStyle::new(&env);
    let text = "Many words that need a few lines in a narrow area to be read whole";
    let label = ui::Label::new(text).wrap();
    let width = 160;
    let height = label.height(&env, &style, width);
    let one = ui::Label::new(text).height(&env, &style, width);
    assert!(height > 2 * one - 2, "{height} {one}");
    assert_eq!(
        label.lines(2).height(&env, &style, width),
        2 * (one - 1) + 1
    );
    // All of it in the height it says.
    let area = Rect::new(10, 10, width, height);
    let (said, mut frame) = describe(480, 400, |c| {
        label.draw(&mut ui::Ui::new(c, &env, &style), area);
    });
    assert_eq!(said[0].label, text);
    assert_eq!(drawn_outside(&mut frame, area), None);
    // Two lines' room: the second ends in "…".
    let short = Rect::new(10, 10, width, 2 * one);
    let (said, mut frame) = describe(480, 400, |c| {
        label.draw(&mut ui::Ui::new(c, &env, &style), short);
    });
    assert!(said[0].label.ends_with('…'), "{:?}", said[0].label);
    assert_eq!(drawn_outside(&mut frame, short), None);
}

/// A row as a value keeps within the height it says, and tells what a
/// point of it is on.
#[test]
fn a_row_value_keeps_to_its_height_and_hits_its_parts() {
    let h = Harness::new(Device::TOUCH);
    let env = h.env();
    let style = UiStyle::new(&env);
    let width = 440;
    let note = "A note of a few words below the title, long enough for two lines";
    for focused in [false, true] {
        for note in ["", note] {
            for row in [
                ui::Row::value("Title", "Value"),
                ui::Row::switch("Title", true),
                ui::Row::number("Size", "12", "000"),
            ] {
                let row = row.note(note).focused(focused);
                let rect = Rect::new(20, 20, width, row.height(&env, &style, width));
                let (_, mut frame) = describe(480, 400, |c| {
                    row.draw(&mut ui::Ui::new(c, &env, &style), rect);
                });
                assert_eq!(drawn_outside(&mut frame, rect), None, "{row:?}");
            }
        }
    }
    // What a point is on.
    let rect = Rect::new(
        20,
        20,
        width,
        ui::Row::switch("Wi-Fi", true).height(&env, &style, width),
    );
    let switch = ui::switch_rect(&style, rect);
    let on = ui::Row::switch("Wi-Fi", true);
    assert_eq!(
        on.hit(&env, &style, rect, middle(switch)),
        Some(ui::RowPart::Switch)
    );
    assert_eq!(
        on.hit(&env, &style, rect, Point::new(30, 30)),
        Some(ui::RowPart::Row)
    );
    assert_eq!(on.hit(&env, &style, rect, Point::new(5, 5)), None);
    let (minus, plus) = ui::value_stepper(&env, &style, rect, "000", "12").unwrap();
    let number = ui::Row::number("Size", "12", "000");
    assert_eq!(
        number.hit(&env, &style, rect, middle(minus)),
        Some(ui::RowPart::Minus)
    );
    assert_eq!(
        number.hit(&env, &style, rect, middle(plus)),
        Some(ui::RowPart::Plus)
    );
}

/// A section's heading too long for its row ends, shortened, at the
/// right margin as the rows' values do: its last letter's ink stays in
/// the row (it reached a pixel past it, 413 in a row 413 wide).
#[test]
fn a_long_heading_ends_at_the_right_margin() {
    for device in [Device::BUTTONS, Device::TOUCH] {
        let h = Harness::new(device);
        let env = h.env();
        let style = UiStyle::new(&env);
        let row = Rect::new(0, 0, 413, style.row());
        let mut frame = frame(480, 800);
        ui::Ui::new(&mut frame.canvas(), &env, &style).heading(row, "ßPö vßnäAAä üßääüWüßä Sßäük");
        assert_eq!(drawn_outside(&mut frame, row), None);
        let view = frame.view();
        let last = (0..480u16)
            .rev()
            .find(|&x| (row.y..row.y + row.height).any(|y| view.pixel(x, y) != support::PAPER))
            .unwrap();
        assert!(last < 413 - style.margin() + 2, "ends at {last}");
    }
}

/// A progress bar's fill keeps inside its rounded track at every value,
/// from a sliver to full, on the densities of the X4, the X4 Pro and the
/// Boox, with square, slightly rounded and pill corners: no pixel of it
/// lies where the track's own shape does not.
#[test]
fn a_progress_bar_fills_inside_its_track() {
    for dpi in [220, 254, 300] {
        let mut h = Harness::new(Device::TOUCH);
        h.device.display.dpi = dpi;
        let env = h.env();
        let base = UiStyle::new(&env);
        for (corner, pill) in [(0, false), (env.metrics().tenth_mm(8), false), (0, true)] {
            let style = base.with_corner(corner as u16).with_pill(pill);
            for (width, height) in [(300u16, 24u16), (120, 12), (400, 40), (60, 9)] {
                let area = Rect::new(20, 30, width, height);
                // The track's shape: what a filled track would cover.
                let mut shape = frame(480, 200);
                let radius = style.radius(u32::from(height));
                shape
                    .canvas()
                    .fill_round(area, radius, paperpaw_sdk::Color::BLACK);
                let shape = shape.view();
                for permille in (0..=1000).step_by(7).chain([1, 2, 999, 1000]) {
                    let mut bar = frame(480, 200);
                    ui::Ui::new(&mut bar.canvas(), &env, &style).progress(
                        area,
                        permille,
                        paperpaw_sdk::Color::BLACK,
                    );
                    let bar = bar.view();
                    for y in 0..200 {
                        for x in 0..480 {
                            if bar.pixel(x, y) != support::PAPER {
                                assert_ne!(
                                    shape.pixel(x, y),
                                    support::PAPER,
                                    "{dpi} dpi, corner {corner}, pill {pill}, {width}x{height},                                      {permille}: ({x}, {y}) outside the track"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

/// List icons are centred by their ink in their column, whatever room
/// an icon's drawing leaves at its sides: icons of one list line up (they
/// were 7 to 9 px apart where drawn from their advance).
#[test]
fn row_icons_are_centred_by_their_ink() {
    for device in [Device::BUTTONS, Device::TOUCH] {
        let h = Harness::new(device);
        let env = h.env();
        let style = UiStyle::new(&env);
        let size = style.text() * ui::ICON_SCALE;
        let column = f32::from(style.margin()) + size / 2.0;
        let room = ui::icon_room(&env, &style, true);
        let mut seen = 0;
        for icon in [
            "search",
            "books",
            "settings",
            "hand-click",
            "paw",
            "devices",
            "adjustments",
        ] {
            if ui::icon_ink(&env, icon, size).is_none() {
                continue;
            }
            seen += 1;
            let row = Rect::new(0, 100, 480, style.row());
            let mut frame = frame(480, 800);
            ui::Ui::new(&mut frame.canvas(), &env, &style).row_icon(
                row,
                room,
                Some(icon),
                paperpaw_sdk::Color::BLACK,
            );
            let (first, last) = ink_columns(&frame, row.y..row.y + row.height);
            let middle = (f32::from(first) + f32::from(last) + 1.0) / 2.0;
            assert!(
                (middle - column).abs() <= 1.0,
                "{icon}: ink {first}..{last}, column middle {column}"
            );
        }
        assert!(seen >= 3, "icons to compare");
    }
}

// ---- Header and pager --------------------------------------------------

/// Points across the whole header, every 4 pixels.
fn across(area: Rect) -> impl Iterator<Item = Point> {
    let y = middle(area).y;
    (i32::from(area.x)..area.right() as i32)
        .step_by(4)
        .map(move |x| Point::new(x, y))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(64)))]

    /// On a touch screen, the back arrow and the page arrows are hit
    /// where they are: back at the left, the previous page left of the
    /// page number's middle, the next page right of it; the title
    /// between them is no arrow. Turning stops nowhere: every page but
    /// the first can go back, every one but the last on.
    #[test]
    fn header_arrows_are_hit_where_they_are(
        page in 0usize..20,
        pages in 0usize..20,
        back in any::<bool>(),
        title in text_strategy(),
    ) {
        let h = Harness::new(Device::TOUCH);
        let env = h.env();
        let style = UiStyle::new(&env);
        let header = Header::new(&env, &style, back);
        let paging = (pages > 0).then_some(Paging::new(page % pages.max(1), pages));
        let a = header.area();
        let actions_across: Vec<Option<paperpaw_sdk::Id>> =
            across(a).map(|p| header.tap(&env, &style, p, paging)).collect();
        // From the left: back (if any), the title, then previous and next.
        let mut seen: Vec<Option<paperpaw_sdk::Id>> = Vec::new();
        for action in &actions_across {
            if seen.last() != Some(action) {
                seen.push(action.clone());
            }
        }
        let mut expected = Vec::new();
        if back {
            expected.push(Some(actions::BACK));
        }
        expected.push(None);
        if pages > 1 {
            expected.push(Some(actions::PREV_PAGE));
            expected.push(Some(actions::NEXT_PAGE));
        }
        prop_assert_eq!(seen, expected);
        // The title is where no arrow is, from the back arrow or the
        // margin on.
        let left = if back { a.height } else { style.margin() };
        for (p, action) in across(a).zip(&actions_across) {
            let title = action.is_none() && p.x >= i32::from(left);
            prop_assert_eq!(header.on_title(&env, &style, p, paging), title, "{:?}", p);
        }
        // Outside: nothing.
        prop_assert_eq!(header.tap(&env, &style, Point::new(5, a.bottom() as i32 + 1), paging), None);
        // Drawn inside the header, the title shortened before the arrows.
        let (described, mut frame) = describe(480, 800, |c| {
            header.draw(&mut paperpaw_sdk::ui::Ui::new(c, &env, &style), &title, Some("by title"), paging);
        });
        prop_assert_eq!(drawn_outside(&mut frame, a), None);
        prop_assert_eq!(described[0].role, Role::Heading);
    }

    /// With buttons alone, the header has no arrows: a tap anywhere is
    /// no arrow (there are none to tap), the page number shows all the
    /// same.
    #[test]
    fn without_touch_the_header_has_no_arrows(
        page in 0usize..20,
        pages in 2usize..20,
        back in any::<bool>(),
    ) {
        let h = Harness::new(Device::BUTTONS);
        let env = h.env();
        let style = UiStyle::new(&env);
        let header = Header::new(&env, &style, back);
        let paging = Some(Paging::new(page % pages, pages));
        for p in across(header.area()) {
            prop_assert_eq!(header.tap(&env, &style, p, paging), None);
        }
        let (described, mut frame) = describe(480, 800, |c| {
            header.draw(&mut paperpaw_sdk::ui::Ui::new(c, &env, &style), "Library", None, paging);
        });
        prop_assert_eq!(drawn_outside(&mut frame, header.area()), None);
        let label = format!("{} / {}", page % pages + 1, pages);
        prop_assert!(described.iter().any(|d| d.label == label), "the page number");
    }
}

/// The columns of ink in `rows` of `frame`: the first and the last.
fn ink_columns(frame: &paperpaw_graphics::Framebuffer, rows: std::ops::Range<u16>) -> (u16, u16) {
    let view = frame.view();
    let width = frame.width();
    // Against what is left of it at the edge: the header's own paper.
    let inked = |x: u16| rows.clone().any(|y| view.pixel(x, y) != view.pixel(0, y));
    let first = (0..width).find(|&x| inked(x)).expect("ink");
    let last = (0..width).rev().find(|&x| inked(x)).expect("ink");
    (first, last)
}

/// The header's arrows keep to the margins the content keeps, on every
/// density (the X4 to the Boox): the back arrow's ink begins where list
/// icons begin, the next page's arrow ends at the right margin, and the
/// title begins where labels after icons do.
#[test]
fn the_header_keeps_to_the_content_margins() {
    for dpi in [150, 220, 254, 300] {
        let mut h = Harness::new(Device::TOUCH);
        h.device.display.dpi = dpi;
        let env = h.env();
        let style = UiStyle::new(&env);
        let header = Header::new(&env, &style, true);
        let area = header.area();
        let middle = area.y + area.height / 4..area.y + area.height * 3 / 4;
        let mut arrows = frame(480, 800);
        header.draw(
            &mut paperpaw_sdk::ui::Ui::new(&mut arrows.canvas(), &env, &style),
            "",
            None,
            Some(Paging::new(1, 3)),
        );
        let (first, last) = ink_columns(&arrows, middle.clone());
        let margin = style.margin();
        assert_eq!(first, margin, "back arrow at {dpi} dpi");
        assert_eq!(last, 480 - margin - 1, "next arrow at {dpi} dpi");
        // A list icon, as rows draw it.
        let row = Rect::new(0, 100, 480, style.row());
        let mut icon = frame(480, 800);
        let room = ui::icon_room(&env, &style, true);
        let label = ui::Ui::new(&mut icon.canvas(), &env, &style).row_icon(
            row,
            room,
            Some("books"),
            paperpaw_sdk::Color::BLACK,
        );
        let (icon_first, _) = ink_columns(&icon, row.y..row.y + row.height);
        // An icon's box begins at the margin, its drawing a little after
        // (the icon's own side bearing), never before.
        let bearing = (style.text() * ui::ICON_SCALE / 4.0) as u16;
        assert!(
            (first..=first + bearing).contains(&icon_first),
            "icon {icon_first}, arrow {first} at {dpi} dpi"
        );
        // The title where the label after the icon begins.
        let (described, _) = describe(480, 800, |c| {
            header.draw(
                &mut paperpaw_sdk::ui::Ui::new(c, &env, &style),
                "Library",
                None,
                None,
            );
        });
        let title = described
            .iter()
            .find(|d| d.label == "Library" && d.role == Role::Text)
            .map(|d| d.area.x)
            .expect("the title");
        assert_eq!(title, label.x + margin, "title at {dpi} dpi");
    }
}

/// The first and last rows with ink in `columns`, against the paper at
/// the left edge.
fn ink_rows(frame: &paperpaw_graphics::Framebuffer, columns: std::ops::Range<u16>) -> (u16, u16) {
    let view = frame.view();
    let inked = |y: u16| {
        columns
            .clone()
            .any(|x| view.pixel(x, y) != view.pixel(0, y))
    };
    let first = (0..frame.height()).find(|&y| inked(y)).expect("ink");
    let last = (0..frame.height()).rev().find(|&y| inked(y)).expect("ink");
    (first, last)
}

/// Every title stands on one line, its capitals in the middle of the bar
/// (as the arrows are): one with a descender as one without, on every
/// density.
#[test]
fn header_titles_are_centred_by_their_capitals() {
    for dpi in [150, 220, 300] {
        let mut h = Harness::new(Device::BUTTONS);
        h.device.display.dpi = dpi;
        let env = h.env();
        let style = UiStyle::new(&env);
        let header = Header::new(&env, &style, false);
        let height = header.area().height;
        let title = |text: &str| {
            let mut shot = frame(480, 800);
            header.draw(
                &mut paperpaw_sdk::ui::Ui::new(&mut shot.canvas(), &env, &style),
                text,
                None,
                None,
            );
            ink_rows(&shot, 0..240)
        };
        let (caps, base) = title("HELL");
        let (top, _) = title("Hey");
        assert_eq!(top, caps, "the same capitals' top at {dpi} dpi");
        // A line below the bar may take a pixel or two of it, the edges
        // of the letters (gray) one more.
        let (above, below) = (caps, height - 1 - base);
        assert!(
            above.abs_diff(below) <= style.line() + 2,
            "capitals {caps}..{base} in {height} at {dpi} dpi"
        );
    }
}

#[test]
fn a_focused_header_note_is_told_as_a_focused_button() {
    let h = Harness::new(Device::BUTTONS);
    let env = h.env();
    let style = UiStyle::new(&env);
    let header = Header::new(&env, &style, true).note_focused(true);
    let (described, _) = describe(480, 800, |c| {
        header.draw(
            &mut paperpaw_sdk::ui::Ui::new(c, &env, &style),
            "Library",
            Some("by author"),
            None,
        );
    });
    let note = described.iter().find(|d| d.role == Role::Button).unwrap();
    assert_eq!(note.label, "by author");
    assert!(note.focused);
}

/// A note that only tells something ("3 hits") is a text; one that
/// does something is a button as high as the bar, a fingertip by touch.
#[test]
fn a_header_note_is_a_button_only_where_it_does_something() {
    let h = Harness::new(Device::TOUCH);
    let env = h.env();
    let style = UiStyle::new(&env);
    let mut header = Header::new(&env, &style, true);
    let note = |header: &Header| {
        let (described, _) = describe(480, 800, |c| {
            header.draw(
                &mut paperpaw_sdk::ui::Ui::new(c, &env, &style),
                "Search",
                Some("3 hits"),
                None,
            );
        });
        described
    };
    // Said with the title, no button.
    let told = note(&header);
    assert!(
        told.iter()
            .any(|d| d.role == Role::Heading && d.label == "Search, 3 hits")
    );
    assert!(
        !told
            .iter()
            .any(|d| d.role == Role::Button && d.label == "3 hits")
    );
    header = header.note_button(true);
    let button = note(&header)
        .into_iter()
        .find(|d| d.role == Role::Button && d.label == "3 hits")
        .unwrap();
    assert!(
        u32::from(button.area.height) >= u32::from(style.finger()),
        "{button:?}"
    );
}

#[test]
fn the_pager_turns_by_halves_on_touch_screens_only() {
    for touch in [true, false] {
        let h = Harness::new(if touch {
            Device::TOUCH
        } else {
            Device::BUTTONS
        });
        let env = h.env();
        let style = UiStyle::new(&env);
        let area = Rect::new(20, 700, 400, Pager::height(&style));
        let pager = Pager::new(&env, area);
        let left = Point::new(21, middle(area).y);
        let right = Point::new(419, middle(area).y);
        let outside = Point::new(10, middle(area).y);
        if touch {
            assert_eq!(pager.tap(left), Some(actions::PREV_PAGE));
            assert_eq!(pager.tap(right), Some(actions::NEXT_PAGE));
        } else {
            assert_eq!(pager.tap(left), None);
            assert_eq!(pager.tap(right), None);
        }
        assert_eq!(pager.tap(outside), None);
        for (page, pages) in [(0, 1), (0, 3), (2, 3), (9, 3)] {
            let page = Paging::new(page, pages);
            let mut frame = frame(480, 800);
            pager.draw(
                &mut paperpaw_sdk::ui::Ui::new(&mut frame.canvas(), &env, &style),
                page,
            );
            assert_eq!(drawn_outside(&mut frame, area), None, "{page:?}");
        }
    }
}

// ---- Messages, notes, empty screens, hints ------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(64)))]

    /// Messages, notes at the bottom and empty screens draw inside their
    /// area at any size from a line of text up, however long the text.
    /// (In less, a message shows its first line all the same.)
    #[test]
    fn texts_in_areas_draw_inside_them(
        x in 0u16..100,
        y in 0u16..400,
        width in 100u16..380,
        height in 60u16..400,
        title in text_strategy(),
        text in text_strategy(),
    ) {
        let h = Harness::new(Device::TOUCH);
        let env = h.env();
        let style = UiStyle::new(&env);
        let area = Rect::new(x, y, width, height);
        let draws: [Draw<'_>; 3] = [
            &|c| ui::Ui::new(c, &env, &style).message(area, &text),
            &|c| ui::Ui::new(c, &env, &style).note_line(area, &text),
            &|c| ui::Ui::new(c, &env, &style).empty(area, "books", &title, &text),
        ];
        for (n, draw) in draws.iter().enumerate() {
            let mut frame = frame(480, 800);
            draw(&mut frame.canvas());
            prop_assert_eq!(drawn_outside(&mut frame, area), None, "kind {}", n);
        }
    }

    /// The row of hints draws each button's hint in its share of the
    /// width, inside the row, the pressed one inverted.
    #[test]
    fn the_row_of_hints_draws_inside_it(
        hints in proptest::collection::vec(
            prop_oneof![
                Just(Hint::Default),
                Just(Hint::Hidden),
                Just(Hint::Icon("arrow-left")),
                "[a-zA-Z ]{0,30}".prop_map(Hint::Text),
            ],
            0..6,
        ),
        pressed in proptest::option::of(0usize..6),
    ) {
        let h = Harness::new(Device::BUTTONS);
        let env = h.env();
        let height = ui::button_hints_height(&env);
        let area = Rect::new(0, 800 - height, 480, height);
        let mut frame = frame(480, 800);
        ui::draw_button_hints(&mut frame.canvas(), &env, area, &hints, pressed);
        prop_assert_eq!(drawn_outside(&mut frame, area), None);
    }
}

/// The ink of hint row `area` in `frame` (what is not white below its
/// line): its columns from left to right.
fn hint_ink(frame: &paperpaw_graphics::Framebuffer, area: Rect) -> Vec<u16> {
    let view = frame.view();
    (area.x..area.right() as u16)
        .filter(|&x| (area.y + 4..area.bottom() as u16).any(|y| view.pixel(x, y) != Color::WHITE))
        .collect()
}

/// Two hints too long for their shares side by side keep half a gap or
/// more between their words.
#[test]
fn long_hints_side_by_side_stay_apart() {
    let h = Harness::new(Device::BUTTONS);
    let env = h.env();
    let gap = UiStyle::new(&env).gap();
    let height = ui::button_hints_height(&env);
    let area = Rect::new(0, 800 - height, 480, height);
    let long = |c: char| Hint::Text(std::iter::repeat_n(c, 30).collect());
    let hints = [long('W'), long('M'), long('W'), long('M')];
    let mut frame = frame(480, 800);
    ui::draw_button_hints(&mut frame.canvas(), &env, area, &hints, None);
    let ink = hint_ink(&frame, area);
    for boundary in [120, 240, 360] {
        let near: Vec<_> = ink
            .iter()
            .filter(|&&x| x + gap / 2 > boundary && x < boundary + gap / 2)
            .collect();
        assert!(near.is_empty(), "ink at {near:?} near {boundary}");
    }
}

/// A pressed hint whose words reach past its share has its box round all
/// of them: white words never on the white row.
#[test]
fn a_pressed_hint_keeps_its_words_in_its_box() {
    let h = Harness::new(Device::BUTTONS);
    let env = h.env();
    let height = ui::button_hints_height(&env);
    let area = Rect::new(0, 800 - height, 480, height);
    let hints = [
        Hint::Text("Back".into()),
        Hint::Text("Without a router at all".into()),
        Hint::Hidden,
        Hint::Icon("arrow-left"),
    ];
    let mut plain = frame(480, 800);
    ui::draw_button_hints(&mut plain.canvas(), &env, area, &hints, None);
    // The words of the second hint, where they are drawn.
    let words: Vec<u16> = hint_ink(&plain, area)
        .into_iter()
        .filter(|&x| (125..355).contains(&x))
        .collect();
    let (first, last) = (words[0], words[words.len() - 1]);
    assert!(
        last > 240,
        "the words reach past their share: {first}..{last}"
    );
    let mut pressed = frame(480, 800);
    ui::draw_button_hints(&mut pressed.canvas(), &env, area, &hints, Some(1));
    let view = pressed.view();
    let middle = area.y + area.height / 2 + 1;
    for x in [first - 1, last + 1] {
        assert_eq!(view.pixel(x, middle), Color::BLACK, "box at {x}");
    }
}

#[test]
fn an_empty_screen_shows_its_icon_when_there_is_room() {
    let h = Harness::new(Device::TOUCH);
    let env = h.env();
    let style = UiStyle::new(&env);
    let area = Rect::new(0, 100, 480, 600);
    let (described, frame) = describe(480, 800, |c| {
        ui::Ui::new(c, &env, &style).empty(
            area,
            "books",
            "No books yet",
            "Copy books into the folder books",
        );
    });
    assert!(described.iter().any(|d| d.label == "No books yet"));
    // The icon, gray, above the title in the upper part.
    let top = Rect::new(0, 100, 480, 200);
    assert!(drawn_in(&frame, top));
}

/// The page number between a header's arrows is told apart from them, so
/// that a long list can ask for a page there; without pages there is none.
#[test]
fn the_page_number_is_hit_where_it_is_drawn() {
    let h = Harness::new(Device::TOUCH);
    let env = h.env();
    let style = UiStyle::new(&env);
    let header = Header::new(&env, &style, true);
    let page = Some(Paging::new(1, 30));
    let (described, _) = describe(480, 800, |c| {
        header.draw(
            &mut paperpaw_sdk::ui::Ui::new(c, &env, &style),
            "Reading",
            None,
            page,
        );
    });
    let number = described
        .iter()
        .find(|d| d.label.contains("30"))
        .expect("the page number");
    assert!(header.on_page_number(&env, &style, middle(number.area), page));
    // Not the title, nor without pages.
    let title = described.iter().find(|d| d.label == "Reading").unwrap();
    assert!(!header.on_page_number(&env, &style, middle(title.area), page));
    assert!(!header.on_page_number(&env, &style, middle(number.area), None));
}

/// Rows with a note of one line are lower than those of two, as high as
/// a title and that line.
#[test]
fn noted_rows_grow_with_their_lines() {
    let h = Harness::new(Device::BUTTONS);
    let style = UiStyle::new(&h.env());
    assert_eq!(style.noted_row_of(ui::NOTE_LINES), style.noted_row());
    assert!(style.noted_row_of(1) < style.noted_row());
    assert!(style.noted_row_of(1) > u32::from(style.row()) * 3 / 4);
}

/// A QR code takes whole pixels a module, square, inside its area, with
/// white around it; one that does not fit is not drawn.
#[test]
fn a_qr_code_is_drawn_whole_or_not_at_all() {
    let qr = paperpaw_sdk::qr::QrCode::new(b"https://codeberg.org/someone/clock").unwrap();
    let area = Rect::new(40, 100, 300, 260);
    let mut drawn = frame(480, 800);
    let at = ui::draw_qr(&mut drawn.canvas(), &qr, area).unwrap();
    assert_eq!(at.width, at.height);
    assert_eq!(at.width % (qr.size() as u16 + 8), 0);
    assert!(at.x >= area.x && at.right() <= area.right());
    assert!(at.y >= area.y && at.bottom() <= area.bottom());
    assert!(drawn_in(&drawn, at));
    assert_eq!(drawn_outside(&mut drawn, at), None);
    let mut small = frame(480, 800);
    assert_eq!(
        ui::draw_qr(&mut small.canvas(), &qr, Rect::new(0, 0, 20, 20)),
        None
    );
    assert!(!drawn_in(&small, Rect::new(0, 0, 480, 800)));
}

/// A look one plugin gives the parts: the SDK places them and tells
/// screen readers what they are, the look paints them.
#[test]
fn a_look_paints_the_parts_and_keeps_what_they_say() {
    use paperpaw_sdk::Color;
    use paperpaw_sdk::ui::look::{ButtonPart, FocusPart, Look, PaintCx, Painters, SwitchPart};
    const SWITCH: Color = Color::gray(100);
    const FOCUS: Color = Color::gray(60);
    const BUTTON: Color = Color::gray(140);
    fn switch(cx: &mut PaintCx<'_, '_>, part: &SwitchPart) -> bool {
        cx.canvas.fill(part.rect, SWITCH);
        true
    }
    fn focus(cx: &mut PaintCx<'_, '_>, part: &FocusPart) -> bool {
        cx.canvas.fill(part.rect, FOCUS);
        true
    }
    fn button(cx: &mut PaintCx<'_, '_>, part: &ButtonPart) -> Option<Color> {
        cx.canvas.fill(part.rect, BUTTON);
        Some(Color::WHITE)
    }
    static SQUARE: Painters = Painters::new().switch(switch).focus(focus).button(button);

    let h = Harness::new(Device::TOUCH);
    let env = h.env();
    let row = Rect::new(0, 100, 480, 80);
    let at = |style: &UiStyle| {
        let (described, frame) = describe(480, 300, |canvas| {
            ui::Ui::new(canvas, &env, style).switch(row, true, false);
            ui::Ui::new(canvas, &env, style).focus(Rect::new(0, 200, 480, 60));
            ui::Ui::new(canvas, &env, style).button(Rect::new(20, 20, 100, 50), "OK", false, false);
        });
        let view = frame.view();
        let switch = ui::switch_rect(style, row);
        let focus = ui::focus_rect(style, Rect::new(0, 200, 480, 60));
        let shown = (
            view.pixel(switch.x + 1, switch.y + switch.height / 2),
            view.pixel(focus.x + 1, focus.y + focus.height / 2),
            view.pixel(22, 22),
        );
        let said: Vec<(Role, Option<bool>)> = described.iter().map(|d| (d.role, d.on)).collect();
        (shown, said)
    };
    let plain = UiStyle::new(&env);
    let looked = plain.with_look(Look::of(&SQUARE));
    let (before, said_before) = at(&plain);
    let (after, said_after) = at(&looked);
    assert_eq!(after, (SWITCH, FOCUS, BUTTON));
    assert_ne!(before.0, SWITCH);
    assert_ne!(before.1, FOCUS);
    assert_ne!(before.2, BUTTON);
    // What they are is told the same with any look.
    assert_eq!(said_after, said_before);
    assert!(said_after.contains(&(Role::Switch, Some(true))));
}

/// A theme's grays: the palette names them by what they are for, and
/// the parts draw in them.
#[test]
fn the_palette_sets_the_grays_the_parts_draw_in() {
    use paperpaw_sdk::Color;
    use paperpaw_sdk::ui::{Ink, Palette, TextRole};
    let palette = Palette::parse("muted=60 line=100 bad nope=3 faint=x note=300");
    assert_eq!(palette.color(Ink::MUTED), Color::gray(60));
    assert_eq!(palette.color(Ink::LINE), Color::gray(100));
    // What it does not set, or sets wrongly, keeps its default.
    for ink in [Ink::FAINT, Ink::NOTE, Ink::RULE, Ink::TEXT, Ink::PAPER] {
        assert_eq!(palette.color(ink), Palette::DEFAULT.color(ink), "{ink:?}");
    }
    assert_eq!(Palette::parse(""), Palette::DEFAULT);

    let h = Harness::new(Device::TOUCH);
    let env = h.env();
    let style = UiStyle::new(&env)
        .with_separators(ui::Separators::Line)
        .with_palette(palette);
    let row = Rect::new(0, 0, 480, 80);
    let mut lined = frame(480, 100);
    ui::Ui::new(&mut lined.canvas(), &env, &style).separator(row, 0);
    let y = row.height - style.line();
    assert_eq!(lined.view().pixel(240, y), Color::gray(100));
    assert_eq!(style.ink(Ink::MUTED), Color::gray(60));
    // Strong contrast draws gray text black, not the lines.
    let style = style.with_strong(true);
    assert_eq!(style.ink(Ink::MUTED), Color::BLACK);
    assert_eq!(style.ink(Ink::LINE), Color::gray(100));
    // Text of one kind is as large everywhere.
    assert_eq!(style.size(TextRole::BODY), style.text());
    assert_eq!(style.size(TextRole::HEADLINE), style.title() * 1.6);
}

/// Roles are numbers of one open type: the new ones are sized as the
/// roles they stand for and one nobody named yet as body text.
#[test]
fn text_roles_are_open() {
    use paperpaw_sdk::ui::{TextRole, UiStyle};
    let h = Harness::new(Device::TOUCH);
    let env = h.env();
    let style = UiStyle::new(&env);
    assert_eq!(style.size(TextRole::HINT), style.size(TextRole::SMALL));
    assert_eq!(style.size(TextRole::BUTTON), style.size(TextRole::BODY));
    assert_eq!(style.size(TextRole::ICON), style.size(TextRole::BODY));
    let later = TextRole::from_value(200);
    assert_eq!(later.value(), 200);
    assert_eq!(style.size(later), style.size(TextRole::BODY));
    // The old order of the enum is the numbers.
    assert_eq!(
        [
            TextRole::CAPTION,
            TextRole::SMALL,
            TextRole::BODY,
            TextRole::NUMERAL
        ]
        .map(TextRole::value),
        [0, 1, 2, 7]
    );
    assert_eq!(format!("{:?}", TextRole::BODY), "TextRole::BODY");
    assert_eq!(format!("{:?}", later), "TextRole(200)");
    // A role matches as a constant.
    assert!(matches!(TextRole::from_value(9), TextRole::BUTTON));
}

/// Semantic icons: what they say has the drawing a theme names for it,
/// else its default, else the arrow drawn here.
#[test]
fn a_theme_gives_the_arrows_and_marks_icons_of_its_own() {
    use paperpaw_sdk::{Color, Id, icons};
    let mut h = Harness::new(Device::TOUCH);
    {
        let env = h.env();
        // Holding has its hand by default; the arrows are drawn.
        assert!(ui::has_icon(&env, icons::HOLD));
        assert!(!ui::has_icon(&env, icons::NEXT));
    }
    let arrow = |h: &Harness| {
        let env = h.env();
        let style = UiStyle::new(&env);
        let mut drawn = frame(200, 200);
        ui::draw_arrow(
            &mut drawn.canvas(),
            &env,
            &style,
            (100, 100),
            icons::NEXT,
            false,
            Color::BLACK,
        );
        let view = drawn.view();
        (0..200u16)
            .flat_map(|y| (0..200u16).map(move |x| (x, y)))
            .filter(|&(x, y)| view.pixel(x, y) != support::PAPER)
            .collect::<Vec<_>>()
    };
    let chevron = arrow(&h);
    assert!(!chevron.is_empty());
    h.menus.set_icon(
        Id::new_static(icons::NEXT),
        std::borrow::Cow::Borrowed("search"),
    );
    assert!(ui::has_icon(&h.env(), icons::NEXT));
    let icon = arrow(&h);
    assert!(!icon.is_empty());
    assert_ne!(icon, chevron);
}

/// A setting's title is never cut for its control: where it does not fit
/// beside `−` value `+` (or a switch), it gets a line of its own with the
/// control below it at the right, the row a line higher; a short one
/// stays beside it. By touch (buttons a fingertip wide) and with buttons.
#[test]
fn setting_titles_are_never_cut_for_their_control() {
    use paperpaw_sdk::SettingSpec;
    use paperpaw_sdk::builtin::groups;
    const NUMBER: &str = "Offline below (%)";
    const SWITCH: &str = "Pictures in night mode too";
    let specs = [
        SettingSpec::number("t.long", NUMBER, 0, 100, 20, groups::OTHER),
        SettingSpec::switch("t.switch", SWITCH, false, groups::OTHER),
    ];
    let short = SettingSpec::number("t.short", "Size", 0, 100, 20, groups::OTHER);
    for device in [Device::TOUCH, Device::BUTTONS] {
        let h = Harness::new(device);
        let env = h.env();
        let style = UiStyle::new(&env);
        for noted in [false, true] {
            let row = if noted {
                style.noted_row()
            } else {
                u32::from(style.row())
            };
            for spec in &specs {
                let side = ui::setting_row(&style, spec, row, noted);
                let height = ui::setting_height(&env, &style, spec, 480, "", row, noted);
                assert!(height > side, "{} noted {noted}", spec.id);
                let rect = Rect::new(0, 100, 480, height as u16);
                let lines = ui::row_lines(&env, &style, spec, 480, "", row, noted).at(rect);
                assert!(lines.stacked(), "{} noted {noted}", spec.id);
                assert_eq!(u32::from(lines.height), height);
                assert!(lines.title.y >= rect.y && lines.title.bottom() <= lines.control.y.into());
                assert!(lines.control.bottom() <= rect.bottom());
                let title = spec.title.as_ref();
                let (described, _) = describe(480, 800, |c| match spec.kind {
                    paperpaw_sdk::SettingKind::Int { .. } => {
                        ui::draw_number_lines(c, &env, &style, lines, title, "20", "", None);
                    }
                    _ => {
                        ui::draw_switch_lines(c, &env, &style, rect, lines, false, title, "", true)
                    }
                });
                assert!(
                    described.iter().any(|d| d.label == title),
                    "{} noted {noted}: {described:?}",
                    spec.id
                );
            }
            let side = ui::setting_row(&style, &short, row, noted);
            assert_eq!(
                ui::setting_height(&env, &style, &short, 480, "", row, noted),
                side
            );
            assert!(!ui::row_lines(&env, &style, &short, 480, "", row, noted).stacked());
        }
    }
}

/// A row is laid out for every value it may show: a choice of two that
/// switches in place stacks (or not) whichever of them is set, so the
/// row never grows past its place nor leaves a line empty.
#[test]
fn a_rows_lines_hold_for_every_value_it_may_show() {
    use paperpaw_sdk::builtin::groups;
    use paperpaw_sdk::{Choices, SettingSpec, SettingValue};
    const OPTIONS: [&str; 2] = ["Off", "Only while the device is charging"];
    let mut h = Harness::new(Device::TOUCH);
    let spec = SettingSpec::choice("t.when", "Sync", &OPTIONS, "Off", groups::OTHER);
    assert!(matches!(
        spec.kind,
        paperpaw_sdk::SettingKind::Choice(Choices::Fixed(_))
    ));
    h.settings.define(spec.clone()).unwrap();
    let mut seen = Vec::new();
    for value in OPTIONS {
        h.settings
            .set("t.when", SettingValue::Text(value.into()))
            .unwrap();
        let env = h.env();
        let style = UiStyle::new(&env);
        seen.push(ui::row_lines(
            &env,
            &style,
            &spec,
            480,
            "",
            u32::from(style.row()),
            false,
        ));
    }
    assert_eq!(seen[0], seen[1]);
    assert!(
        seen[0].stacked(),
        "the long option does not fit beside the title"
    );
    // On and off shown as a value (a switch with an editor of its own).
    static EDITED: paperpaw_sdk::SettingExtra =
        paperpaw_sdk::SettingExtra::new().editor(paperpaw_sdk::Id::new_static("t.editor"));
    let spec = SettingSpec::switch("t.on", "Sync", false, groups::OTHER).with_extra(&EDITED);
    h.settings.define(spec.clone()).unwrap();
    // At every width, where one of them fits beside the title and the
    // other not as well.
    for width in (100..=480).step_by(2) {
        let mut seen = Vec::new();
        for on in [false, true] {
            h.settings.set("t.on", SettingValue::Bool(on)).unwrap();
            let env = h.env();
            let style = UiStyle::new(&env);
            seen.push(ui::row_lines(
                &env,
                &style,
                &spec,
                width,
                "",
                u32::from(style.row()),
                false,
            ));
        }
        assert_eq!(seen[0], seen[1], "{width} px");
    }
}

/// The hit areas of a number's − and + are where they are drawn, also
/// with a value wider than the room in bytes but not as drawn (the mark
/// of a scope, "•", takes three bytes).
#[test]
fn a_numbers_steps_are_hit_where_they_are_drawn() {
    let h = Harness::new(Device::TOUCH);
    let env = h.env();
    let style = UiStyle::new(&env);
    let rect = Rect::new(0, 100, 480, style.touch_row(u32::from(style.row())) as u16);
    for (room, value) in [("000 %", "• 20 %"), ("100 %", "• 9"), ("", "12345 px")] {
        let (described, _) = describe(480, 800, |c| {
            ui::Row::number("Size", value, room)
                .step_focus(None)
                .draw(&mut ui::Ui::new(c, &env, &style), rect);
        });
        let drawn: Vec<Rect> = described
            .iter()
            .filter(|d| d.role == Role::Button)
            .map(|d| d.area)
            .collect();
        let (minus, plus) = ui::value_stepper(&env, &style, rect, room, value).unwrap();
        assert_eq!(drawn, [minus, plus], "{room:?} {value:?}");
    }
}

/// A switch without a note on a page of noted rows is measured where it
/// is drawn (the whole row, a larger switch): its title is not cut.
#[test]
fn a_switch_without_a_note_is_measured_as_drawn() {
    use paperpaw_sdk::SettingSpec;
    use paperpaw_sdk::builtin::groups;
    let h = Harness::new(Device::TOUCH);
    let env = h.env();
    let style = UiStyle::new(&env);
    let row = style.noted_row();
    // Titles from short to long: whichever the row keeps side by side,
    // drawn there it is whole.
    for title in [
        "Night",
        "Night mode",
        "Pictures in night",
        "Pictures in night mode",
    ] {
        let spec = SettingSpec::switch("t.switch", title, false, groups::OTHER);
        assert!(spec.description.is_none());
        let lines = ui::row_lines(&env, &style, &spec, 480, "", row, true);
        let rect = Rect::new(0, 100, 480, lines.height);
        let lines = lines.at(rect);
        let (described, _) = describe(480, 800, |c| {
            ui::draw_switch_lines(c, &env, &style, rect, lines, false, title, "", true);
        });
        assert!(
            described.iter().any(|d| d.label == title),
            "{title}: {described:?}"
        );
    }
}

/// An empty screen in an area too small for its margins keeps its ink
/// inside: a letter reaching past its start (an accent) too.
#[test]
fn an_empty_screen_in_a_small_area_draws_inside_it() {
    let h = Harness::new(Device::TOUCH);
    let env = h.env();
    let style = UiStyle::new(&env);
    let area = Rect::new(1, 0, 149, 60);
    let mut frame = frame(480, 800);
    ui::Ui::new(&mut frame.canvas(), &env, &style).empty(area, "books", "Ὕ\u{dd6}#🀰a\"", "");
    assert_eq!(drawn_outside(&mut frame, area), None);
}
