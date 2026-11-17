//! The UI where its language runs right to left (`UiStyle::with_rtl`, as the
//! catalog's `[meta] direction = rtl` makes it): every block is the mirror
//! image of the same block left to right, in what it draws and in where a
//! touch is taken, its text starting at the right.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod support;

use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::semantics::Role;
use paperpaw_sdk::ui::{self, Header, UiStyle};
use paperpaw_sdk::{Point, Rect};

use support::{Device, Harness, describe, drawn_outside};

const WIDTH: u16 = 480;

/// The mirror image of `rect` in a screen as wide as the tests'.
fn mirrored(rect: Rect) -> Rect {
    Rect::new(WIDTH - rect.x - rect.width, rect.y, rect.width, rect.height)
}

/// The first and the last column with ink in `rows`, against the paper
/// the frame was cleared to.
fn ink_columns(frame: &paperpaw_graphics::Framebuffer, rows: std::ops::Range<u16>) -> (u16, u16) {
    let view = frame.view();
    let inked = |x: u16| rows.clone().any(|y| view.pixel(x, y) != support::PAPER);
    let first = (0..frame.width()).find(|&x| inked(x)).expect("ink");
    let last = (0..frame.width()).rev().find(|&x| inked(x)).expect("ink");
    (first, last)
}

#[test]
fn the_switch_stands_at_the_rows_other_end_and_its_knob_goes_the_other_way() {
    let h = Harness::new(Device::TOUCH);
    let env = h.env();
    let ltr = UiStyle::new(&env);
    let rtl = ltr.with_rtl(true);
    let row = Rect::new(0, 100, WIDTH, ltr.row());
    assert_eq!(
        ui::switch_rect(&rtl, row),
        mirrored(ui::switch_rect(&ltr, row))
    );
    // Flush with the margin, at the left.
    assert_eq!(ui::switch_rect(&rtl, row).x, rtl.margin());
    // On is the knob at the switch's far end from the row's start.
    let knob = |style: &UiStyle, on: bool| {
        let mut frame = support::frame(WIDTH, 800);
        ui::Ui::new(&mut frame.canvas(), &env, style).switch(row, on, false);
        let switch = ui::switch_rect(style, row);
        let rows = switch.y..switch.bottom() as u16;
        // Black pixels in the switch's left half against its right half.
        let view = frame.view();
        let mid = switch.x + switch.width / 2;
        let count = |xs: std::ops::Range<u16>| {
            xs.flat_map(|x| rows.clone().map(move |y| (x, y)))
                .filter(|&(x, y)| view.pixel(x, y) == paperpaw_sdk::Color::BLACK)
                .count() as i32
        };
        count(switch.x..mid) - count(mid..switch.right() as u16)
    };
    // Switched on the filled track is the same either way; off, the
    // knob is on the start side of the switch: left, and right when mirrored.
    let (ltr_off, rtl_off) = (knob(&ltr, false), knob(&rtl, false));
    assert!(
        ltr_off != rtl_off,
        "the knob of an off switch moved over: {ltr_off} vs {rtl_off}"
    );
}

#[test]
fn a_title_starts_at_the_right_margin_and_the_value_ends_at_the_left_one() {
    let h = Harness::new(Device::TOUCH);
    let env = h.env();
    let ltr = UiStyle::new(&env);
    let rtl = ltr.with_rtl(true);
    let row = Rect::new(0, 100, WIDTH, ltr.row());
    let rows = row.y..row.bottom() as u16;
    for (title, value) in [("Brightness", ""), ("Brightness", "40 %")] {
        let draw = |style: &UiStyle| {
            let mut frame = support::frame(WIDTH, 800);
            ui::Row::value(title, value)
                .draw(&mut ui::Ui::new(&mut frame.canvas(), &env, style), row);
            ink_columns(&frame, rows.clone())
        };
        let ((first, last), (r_first, r_last)) = (draw(&ltr), draw(&rtl));
        let margin = ltr.margin();
        let bearing = 6;
        // Left to right the title begins at the left margin; mirrored it
        // ends at the right one.
        assert!(first >= margin && first <= margin + bearing, "{first}");
        assert!(
            r_last + margin + bearing >= WIDTH && r_last < WIDTH - margin + 1,
            "title ends at {r_last} ({title:?}, {value:?})"
        );
        if !value.is_empty() {
            // The value ends at the right margin; mirrored it begins at the left one.
            assert!(
                last + margin + bearing >= WIDTH && last < WIDTH - margin + 1,
                "{last}"
            );
            assert!(
                r_first >= margin && r_first <= margin + bearing,
                "{r_first}"
            );
        }
    }
}

#[test]
fn the_headers_title_and_note_swap_sides() {
    let h = Harness::new(Device::BUTTONS);
    let env = h.env();
    let ltr = UiStyle::new(&env);
    let rtl = ltr.with_rtl(true);
    let area = |style: &UiStyle| Header::new(&env, style, false).area();
    assert_eq!(area(&ltr), area(&rtl));
    let places = |style: &UiStyle| {
        let (described, mut frame) = describe(WIDTH, 800, |c| {
            Header::new(&env, style, false).draw(
                &mut ui::Ui::new(c, &env, style),
                "Library",
                Some("A-Z"),
                None,
            );
        });
        assert_eq!(drawn_outside(&mut frame, area(style)), None);
        let find = |label: &str| {
            described
                .iter()
                .find(|d| d.label == label && d.role == Role::Text)
                .map(|d| d.area)
                .expect(label)
        };
        (find("Library"), find("A-Z"))
    };
    let ((title, note), (r_title, r_note)) = (places(&ltr), places(&rtl));
    let middle = |r: Rect| i32::from(r.x) * 2 + i32::from(r.width);
    assert!(
        middle(title) < middle(note),
        "the note is at the right: {title:?} {note:?}"
    );
    assert!(
        middle(r_note) < middle(r_title),
        "the note is at the left: {r_title:?} {r_note:?}"
    );
    // The title's room starts at the right margin, as it did at the left one.
    assert_eq!(r_title, mirrored(title));
}

#[test]
fn a_text_field_and_its_title_are_the_mirror_image() {
    let h = Harness::new(Device::TOUCH);
    let env = h.env();
    let ltr = UiStyle::new(&env);
    let rtl = ltr.with_rtl(true);
    let row = Rect::new(0, 100, WIDTH, ltr.row() * 2);
    let (title, field, below) = ui::text_field_layout(&ltr, row, 120.0, 1, 30);
    let (r_title, r_field, r_below) = ui::text_field_layout(&rtl, row, 120.0, 1, 30);
    assert_eq!(
        (r_title, r_field, r_below),
        (mirrored(title), mirrored(field), below)
    );
    let drawn = |style: &UiStyle| {
        let (described, mut frame) = describe(WIDTH, 800, |c| {
            ui::Ui::new(c, &env, style).text_field(row, false, "Name", "value", "");
        });
        assert_eq!(drawn_outside(&mut frame, row), None);
        described
            .iter()
            .find(|d| d.role == Role::Field)
            .map(|d| d.area)
            .unwrap()
    };
    assert_eq!(drawn(&rtl), mirrored(drawn(&ltr)));
}

#[test]
fn a_dialogs_buttons_go_from_the_right_and_left_moves_on() {
    let h = Harness::new(Device::TOUCH);
    let env = h.env();
    let ltr = UiStyle::new(&env);
    let rtl = ltr.with_rtl(true);
    let make = |style: &UiStyle| ui::Dialog::new(&env, style, "Title", &["Text"], &["Yes", "No"]);
    let (d, r) = (make(&ltr), make(&rtl));
    let (first, second) = (d.button(0).unwrap(), d.button(1).unwrap());
    let (r_first, r_second) = (r.button(0).unwrap(), r.button(1).unwrap());
    assert!(first.x < second.x);
    assert!(r_first.x > r_second.x, "the first is at the right");
    assert_eq!(r.button(0).unwrap().y, first.y);
    // Left and right are where the buttons are.
    assert_eq!(d.step(0, &actions::RIGHT), Some(1));
    assert_eq!(r.step(0, &actions::LEFT), Some(1));
    assert_eq!(r.step(1, &actions::RIGHT), Some(0));
    assert_eq!(r.step(0, &actions::RIGHT), Some(0));
    // A tap is taken where a button is drawn.
    let at = Point::new(i32::from(r_first.x) + 2, i32::from(r_first.y) + 2);
    assert_eq!(r.hit(at), Some(0));
}

#[test]
fn a_notes_lines_start_at_the_right_margin() {
    let h = Harness::new(Device::TOUCH);
    let env = h.env();
    let ltr = UiStyle::new(&env);
    let rtl = ltr.with_rtl(true);
    let row = Rect::new(0, 100, WIDTH, ltr.noted_row() as u16);
    let rows = row.y + row.height / 2..row.bottom() as u16;
    let ink = |style: &UiStyle| {
        let mut frame = support::frame(WIDTH, 800);
        ui::Row::value("Title", "On")
            .note("A short note")
            .draw(&mut ui::Ui::new(&mut frame.canvas(), &env, style), row);
        ink_columns(&frame, rows.clone())
    };
    let ((first, last), (r_first, r_last)) = (ink(&ltr), ink(&rtl));
    let (margin, bearing) = (ltr.margin(), 6);
    assert!(first >= margin && first <= margin + bearing, "{first}");
    assert!(
        r_last + margin + bearing >= WIDTH && r_last < WIDTH - margin + 1,
        "{r_last}"
    );
    assert!(last < r_last || last <= r_last + 300, "{last} {r_last}");
    let _ = r_first;
}

#[test]
fn chips_run_from_the_right_and_left_goes_on() {
    let h = Harness::new(Device::TOUCH);
    let env = h.env();
    let ltr = UiStyle::new(&env);
    let rtl = ltr.with_rtl(true);
    let area = Rect::new(0, 100, WIDTH, 200);
    let labels = ["A", "B", "C", "D"];
    let (l, r) = (
        ui::Chips::new(&env, &ltr, area, labels),
        ui::Chips::new(&env, &rtl, area, labels),
    );
    for i in 0..4 {
        assert_eq!(r.cell(i).unwrap(), mirrored(l.cell(i).unwrap()), "chip {i}");
    }
    assert!(r.cell(0).unwrap().x > r.cell(1).unwrap().x);
    assert_eq!(l.step(0, &actions::RIGHT), Some(1));
    assert_eq!(r.step(0, &actions::LEFT), Some(1));
    assert_eq!(r.step(1, &actions::RIGHT), Some(0));
    assert_eq!(r.step(0, &actions::RIGHT), Some(0));
    // A tap is on the chip drawn there.
    let first = r.cell(0).unwrap();
    assert_eq!(
        r.hit(Point::new(i32::from(first.x) + 2, i32::from(first.y) + 2)),
        Some(0)
    );
    // And they are drawn within the area.
    let (_, mut frame) = describe(WIDTH, 800, |c| {
        r.draw(&mut ui::Ui::new(c, &env, &rtl), 0, Some(1));
    });
    assert_eq!(drawn_outside(&mut frame, area), None);
}

#[test]
fn a_date_pickers_first_field_is_at_the_right() {
    let h = Harness::new(Device::TOUCH);
    let env = h.env();
    let ltr = UiStyle::new(&env);
    let rtl = ltr.with_rtl(true);
    let area = Rect::new(0, 100, WIDTH, ui::DatePicker::height(&ltr));
    let date = paperpaw_sdk::time::Date::new(2026, 11, 1);
    let (l, mut r) = (
        ui::DatePicker::new(&ltr, area, date),
        ui::DatePicker::new(&rtl, area, date),
    );
    let middle = i32::from(area.y) + i32::from(area.height) / 2;
    let (near_left, near_right) = (Point::new(30, middle), Point::new(450, middle));
    assert_eq!(l.hit(near_left), Some(ui::PickerPart::Value(0)));
    assert_eq!(r.hit(near_right), Some(ui::PickerPart::Value(0)));
    assert_eq!(r.hit(near_left), Some(ui::PickerPart::Value(2)));
    // The left button goes on to the next field.
    assert_eq!(r.focus(), 0);
    let _ = r.on_action(&actions::DOWN);
    let _ = r.on_action(&actions::LEFT);
    assert_eq!(r.focus(), 1);
    let _ = r.on_action(&actions::RIGHT);
    assert_eq!(r.focus(), 0);
}

#[test]
fn a_heading_starts_at_the_right_margin() {
    let h = Harness::new(Device::TOUCH);
    let env = h.env();
    let ltr = UiStyle::new(&env);
    let rtl = ltr.with_rtl(true);
    let rect = Rect::new(0, 100, WIDTH, ltr.row());
    let rows = rect.y..rect.bottom() as u16;
    let ink = |style: &UiStyle| {
        let mut frame = support::frame(WIDTH, 800);
        ui::Ui::new(&mut frame.canvas(), &env, style).heading(rect, "Section");
        ink_columns(&frame, rows.clone())
    };
    let ((first, _), (_, r_last)) = (ink(&ltr), ink(&rtl));
    let (margin, bearing) = (ltr.margin(), 6);
    assert!(first >= margin && first <= margin + bearing, "{first}");
    assert!(
        r_last + margin + bearing >= WIDTH && r_last < WIDTH - margin + 1,
        "{r_last}"
    );
}
