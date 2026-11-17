//! Status lines: the items listed in a setting, read however the list is
//! written; a line drawn inside its area whatever its items say, short
//! items (the page, the time) kept whole beside a long chapter title;
//! status items and whole lines found by their ids.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

extern crate std;
#[allow(unused_imports)]
use std::{
    borrow::ToOwned,
    boxed::Box,
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};

use super::support;

use std::borrow::Cow;

use crate::kit::status::StatusFrame;
use crate::kit::status::StatusItemSpec;
use crate::kit::status::StatusLineSpec;
use crate::kit::status::StatusValue;
use crate::kit::status::{StatusLine, item_ids};
use paperpaw_sdk::{Color, Id, Params, Rect};
use proptest::prelude::*;

use support::{Device, Harness, PAPER, drawn_outside, frame};

const PAGE: &str = "t.page";
const CHAPTER: &str = "t.chapter";
const LEVEL: &str = "t.battery";
const ICON: &str = "t.speaking";
const NOTHING: &str = "t.nothing";

/// Items whose values come from the context: the chapter's title, the
/// page, a battery at a level, an icon, and one that shows nothing.
fn harness() -> Harness {
    let mut h = Harness::new(Device::TOUCH);
    /// The text the context has under `key`.
    fn text(context: &Params, key: &str) -> Option<StatusValue> {
        context.get(key).map(|v| StatusValue::Text(v.to_string()))
    }
    h.status_items.add(StatusItemSpec::new(
        Id::new_static(PAGE),
        "t.page.title",
        |_, context| text(context, "page"),
    ));
    h.status_items.add(StatusItemSpec::new(
        Id::new_static(CHAPTER),
        "t.chapter.title",
        |_, context| text(context, "chapter"),
    ));
    h.status_items.add(StatusItemSpec::new(
        Id::new_static(LEVEL),
        "t.battery.title",
        |_, context| {
            let percent = context.get("level")?.parse().ok()?;
            Some(StatusValue::Level {
                percent,
                charging: context.get("charging").is_some(),
            })
        },
    ));
    h.status_items.add(StatusItemSpec::new(
        Id::new_static(ICON),
        "t.speaking.title",
        |_, _| Some(StatusValue::Icon(Cow::Borrowed("volume"))),
    ));
    h.status_items.add(StatusItemSpec::new(
        Id::new_static(NOTHING),
        "t.nothing.title",
        |_, _| None,
    ));
    h
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256)))]

    /// However a list is written (spaces, empty parts), its ids come out
    /// trimmed, none empty, in their order.
    #[test]
    fn listed_ids_come_out_trimmed_and_never_empty(list in "[a-z., \t]{0,60}") {
        let ids: Vec<&str> = item_ids(&list).collect();
        for id in &ids {
            prop_assert!(!id.is_empty());
            prop_assert_eq!(id.trim(), *id);
            prop_assert!(!id.contains(','));
        }
        let expected: Vec<&str> = list.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
        prop_assert_eq!(ids, expected);
    }
}

fn list_strategy() -> impl Strategy<Value = String> {
    proptest::collection::vec(
        proptest::sample::select(vec![PAGE, CHAPTER, LEVEL, ICON, NOTHING, "t.unknown", ""]),
        0..4,
    )
    .prop_map(|ids| ids.join(", "))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(96)))]

    /// A status line draws inside its area, whatever its items show, at
    /// any size the user sets for status lines.
    #[test]
    fn a_status_line_draws_inside_its_area(
        lists in (list_strategy(), list_strategy(), list_strategy()),
        chapter in "\\PC{0,80}",
        page in "[0-9 /%]{0,12}",
        level in 0u16..300,
        charging in any::<bool>(),
        size in 10.0f32..28.0,
        width in 300u16..470,
        percent in any::<bool>(),
        top in any::<bool>(),
    ) {
        let h = harness();
        let env = h.env();
        let font = crate::kit::ui::UiStyle::new(&env).font(&env).unwrap();
        let height = (size * 1.6) as u16;
        let area = Rect::new(475 - width, if top { 0 } else { 800 - height }, width, height);
        let line = StatusLine::new(area, [&lists.0, &lists.1, &lists.2], size, Color::BLACK)
            .percent(percent);
        let mut context = Params::new()
            .with("chapter", chapter)
            .with("page", page)
            .with("level", level.to_string());
        if charging {
            context = context.with("charging", "yes");
        }
        let mut frame = frame(480, 800);
        line.draw(&mut frame.canvas(), &env, &font, &context);
        // A pixel or two to the side for letters reaching past their
        // start (accents of Greek capitals do).
        let bound = Rect::new(area.x - 2, area.y, area.width + 4, area.height);
        prop_assert_eq!(drawn_outside(&mut frame, bound), None);
    }
}

/// A long chapter title on the left gives way: the page on the right is
/// drawn as it is without it.
#[test]
fn the_page_stays_whole_beside_a_long_chapter_title() {
    let h = harness();
    let env = h.env();
    let font = crate::kit::ui::UiStyle::new(&env).font(&env).unwrap();
    let area = Rect::new(0, 760, 480, 40);
    let context = Params::new()
        .with(
            "chapter",
            "Chapter Twelve, in which a very long title goes on and on and on",
        )
        .with("page", "123 / 456");
    let draw = |left: &str| {
        let mut frame = frame(480, 800);
        let line = StatusLine::new(area, [left, "", PAGE], 22.0, Color::BLACK);
        line.draw(&mut frame.canvas(), &env, &font, &context);
        frame
    };
    let alone = draw("");
    let beside = draw(CHAPTER);
    // The right half: the page, the same pixels either way.
    let (a, b) = (alone.view(), beside.view());
    let width = font.measure(22.0, "123 / 456").ceil() as u16;
    for y in area.y..area.bottom() as u16 {
        for x in (480 - width)..480 {
            assert_eq!(a.pixel(x, y), b.pixel(x, y), "({x}, {y})");
        }
    }
    assert!(
        support::drawn_in(&beside, Rect::new(0, 760, 100, 40)),
        "the chapter shows"
    );
}

#[test]
fn an_empty_line_and_items_that_show_nothing_draw_nothing() {
    let h = harness();
    let env = h.env();
    let font = crate::kit::ui::UiStyle::new(&env).font(&env).unwrap();
    let area = Rect::new(0, 0, 480, 40);
    let empty = StatusLine::new(area, [" , ", "", ","], 20.0, Color::BLACK).percent(false);
    assert!(empty.is_empty());
    let nothing = StatusLine::new(area, [NOTHING, "t.unknown", NOTHING], 20.0, Color::BLACK);
    assert!(!nothing.is_empty());
    for line in [empty, nothing] {
        let mut frame = frame(480, 100);
        line.draw(&mut frame.canvas(), &env, &font, &Params::new());
        assert_eq!(frame.take_dirty(), None);
    }
}

/// Twice the vertical middle of what is drawn in `area` (twice, so that a
/// middle between two pixels is said exactly), counting the pixels
/// nearer the ink (black) than the paper ([`PAPER`]), as a panel of black
/// and white shows them.
fn twice_ink_middle(frame: &paperpaw_graphics::Framebuffer, area: Rect) -> Option<i32> {
    let view = frame.view();
    let ink = PAPER.r / 2;
    let inked = |y: u16| (area.x..area.x + area.width).any(|x| view.pixel(x, y).r < ink);
    let rows: Vec<u16> = (area.y..area.y + area.height)
        .filter(|y| inked(*y))
        .collect();
    Some(i32::from(*rows.first()?) + i32::from(*rows.last()?) + 1)
}

/// Every item of a status line stands on one line: the middle of its ink
/// (the time's and the page's figures, the battery, its percentage, an
/// icon, a bar) is the middle of the figures, which is the middle of the
/// band, top and bottom alike, at every status size; to the pixel, or
/// half a pixel off where an odd height leaves no other way.
#[test]
fn every_item_stands_on_the_line_s_middle() {
    let mut h = harness();
    h.status_items.add(StatusItemSpec::new(
        Id::new_static("t.progress"),
        "t.progress.title",
        |_, _| Some(StatusValue::Bar { permille: 400 }),
    ));
    let env = h.env();
    let font = crate::kit::ui::UiStyle::new(&env).font(&env).unwrap();
    // Figures alone (a slash or a percent sign reaches past them).
    let context = Params::new()
        .with("page", "0123456789")
        .with("level", "80")
        .with("chapter", "10:40");
    // The status sizes of 5 to 16 pt on the X4s (220 dpi) and the Boox
    // (300 dpi), in bands as high as the reader gives them.
    let sizes = [13.0f32, 15.3, 18.3, 21.4, 25.0, 29.2, 33.3, 44.0];
    for size in sizes {
        let height = (size * 2.1 + 0.5) as u16;
        let area = Rect::new(10, 20, 460, height);
        let mut middles = Vec::new();
        for item in [PAGE, CHAPTER, LEVEL, ICON, "t.progress"] {
            let line = StatusLine::new(area, [item, "", ""], size, Color::BLACK);
            let mut frame = frame(480, 200);
            line.draw(&mut frame.canvas(), &env, &font, &context);
            let middle = |part: Rect| {
                twice_ink_middle(&frame, part)
                    .unwrap_or_else(|| panic!("{item} drew nothing at {size}"))
            };
            if item == LEVEL {
                // The battery (its width, from the left) and its percentage
                // beside it, apart.
                let battery = (size * 1.3) as u16;
                middles.push((
                    "the battery",
                    middle(Rect::new(area.x, area.y, battery, area.height)),
                ));
                let rest = Rect::new(
                    area.x + battery + 1,
                    area.y,
                    area.width - battery - 1,
                    area.height,
                );
                middles.push(("its percentage", middle(rest)));
            } else {
                middles.push((item, middle(area)));
            }
        }
        // The figures' middle: in the band's, or half a pixel above it
        // where an odd pixel is left.
        let band = 2 * i32::from(area.y) + i32::from(area.height);
        let (top, bottom) = font.ink(size, "0123456789").unwrap();
        let room = i32::from(area.height) - (bottom - top);
        let figures = 2 * (i32::from(area.y) + room.div_euclid(2) - top) + top + bottom;
        assert!(
            (0..=1).contains(&(band - figures)),
            "at {size} px the figures stand at {} px, the band's middle is {} px",
            figures as f32 / 2.0,
            band as f32 / 2.0
        );
        for (item, middle) in &middles {
            // To the pixel, or half a pixel where heights differ odd and
            // even; a pixel for texts of other glyphs (a colon, the percent
            // sign, which is taller than the figures).
            let most = if [CHAPTER, "its percentage"].contains(item) {
                2
            } else {
                1
            };
            assert!(
                (middle - figures).abs() <= most,
                "at {size} px {item} stands at {} px, the figures at {} px: {middles:?}",
                *middle as f32 / 2.0,
                figures as f32 / 2.0
            );
        }
    }
}

/// In a line too narrow for its items (a widget of one cell), what does
/// not fit whole is left out rather than drawn past the area: the battery
/// keeps its gauge without the percentage where only that fits.
#[test]
fn a_narrow_line_leaves_out_what_does_not_fit() {
    let h = harness();
    let env = h.env();
    let font = crate::kit::ui::UiStyle::new(&env).font(&env).unwrap();
    let context = Params::new().with("level", "80").with("page", "12 / 340");
    for width in [20u16, 30, 45, 60, 90] {
        let area = Rect::new(200, 100, width, 32);
        let right = format!("{LEVEL}, {ICON}, {PAGE}");
        let line = StatusLine::new(area, ["", "", &right], 20.0, Color::BLACK);
        let mut frame = frame(480, 800);
        line.draw(&mut frame.canvas(), &env, &font, &context);
        assert_eq!(drawn_outside(&mut frame, area), None, "{width} wide");
    }
}

/// A wide line crowded by a long chapter title narrows its gaps; the
/// right part is drawn with those gaps too, so it ends at the line's
/// right edge with the battery and its percentage whole.
#[test]
fn a_crowded_line_keeps_its_right_part_whole_at_the_edge() {
    let h = harness();
    let env = h.env();
    let font = crate::kit::ui::UiStyle::new(&env).font(&env).unwrap();
    let context = Params::new()
        .with(
            "chapter",
            "A chapter with a title much too long for any status line at all",
        )
        .with("level", "80")
        .with("page", "12 / 340");
    let right = format!("{PAGE}, {ICON}, {LEVEL}");
    for width in (300u16..=460).step_by(20) {
        let area = Rect::new(10, 700, width, 32);
        let line = StatusLine::new(area, [CHAPTER, "", &right], 20.0, Color::BLACK);
        let mut frame = frame(480, 800);
        line.draw(&mut frame.canvas(), &env, &font, &context);
        assert_eq!(drawn_outside(&mut frame, area), None, "{width} wide");
        let view = frame.view();
        let edge = area.x + area.width;
        let last = (area.x..edge)
            .rev()
            .find(|&x| (area.y..area.y + area.height).any(|y| view.pixel(x, y) != PAPER))
            .unwrap();
        assert!(
            last + 4 >= edge,
            "{width} wide: ends at {last}, the edge is {edge}"
        );
    }
}

/// A line that answers to an id it had before is listed once, and found
/// by either id.
#[test]
fn a_line_answers_to_its_old_id_and_is_listed_once() {
    let mut h = harness();
    h.status_items.add_line(
        StatusLineSpec::new(Id::new_static("t.line"), "t.line.title", |_, _, _| {})
            .also(Id::new_static("t.old-line")),
    );
    let env = h.env();
    let listed: Vec<&str> = env
        .status_items()
        .lines()
        .map(|(id, _)| id.as_str())
        .filter(|id| id.starts_with("t."))
        .collect();
    assert_eq!(listed, ["t.line"]);
    assert_eq!(env.status_items().title("t.old-line"), Some("t.line.title"));
    let context = Params::new();
    let frame = StatusFrame::new(
        Rect::new(0, 0, 480, 40),
        false,
        20.0,
        Color::BLACK,
        &context,
    );
    let mut canvas = support::frame(480, 100);
    assert!(
        env.status_items()
            .draw_line("t.old-line", &mut canvas.canvas(), &env, &frame)
    );
}
