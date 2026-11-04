//! The colours of widgets (#206): papers and inks by the widget's
//! settings and those of all widgets, the paper `draw_frame` paints, and
//! the plate a grid lays under its items.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod support;

use paperpaw_sdk::builtin::settings::{WIDGET_INK, WIDGET_PAPER};
use paperpaw_sdk::decorations::Tone;
use paperpaw_sdk::event::Event;
use paperpaw_sdk::ui::choice::Picked;
use paperpaw_sdk::widgets::grid::{Cells, Geometry, GridView, Item, What};
use paperpaw_sdk::widgets::{
    self, ColourSpec, Drawn, Frame, INK_AUTO, PAPER_DARK_GRAY, PAPER_INVERTED, PAPER_LIGHT_GRAY,
    PAPER_NONE, PAPER_NORMAL, Plate, WidgetSpec,
};
use paperpaw_sdk::{Canvas, Color, Env, Id, Rect, SettingValue};

use support::{Device, Harness, PAPER, drawn_in, drawn_outside, frame};

const CLOCK: Id = Id::new_static("t.clock");

fn nothing(_: &mut Canvas<'_>, _: &Env<'_>, _: Rect) {}

fn short(_: &Env<'_>, _: u16) -> u16 {
    60
}

/// A widget `t.clock` with its frame, paper and ink settings, and those
/// of all widgets.
fn harness() -> Harness {
    let mut h = Harness::new(Device::BUTTONS);
    h.widgets.add(WidgetSpec::drawn(
        CLOCK,
        "t.clock",
        Drawn::new(short, nothing),
    ));
    let group = Id::new_static("t.group");
    for spec in [
        widgets::frame_setting(&CLOCK, Frame::None),
        widgets::paper_setting(&CLOCK),
        widgets::ink_setting(&CLOCK),
        widgets::all_paper_spec(group.clone()),
        widgets::all_ink_spec(group),
    ] {
        h.settings.define(spec).unwrap();
    }
    h
}

fn set(h: &mut Harness, id: &str, value: &str) {
    h.settings
        .set(id, SettingValue::Text(value.to_string().into()))
        .unwrap();
}

/// Paper, ink and faint ink of the clock.
fn colours(h: &Harness) -> (Color, Color, Color) {
    let look = widgets::widget_look(&h.env(), "t.clock", Rect::new(0, 0, 200, 100));
    (look.paper, look.ink, look.faint)
}

#[test]
fn a_colour_is_a_paper_a_tone_or_a_code() {
    use ColourSpec::{Auto, Is, None};
    assert_eq!(widgets::colour_spec(PAPER_NORMAL), Auto);
    assert_eq!(widgets::colour_spec(INK_AUTO), Auto);
    assert_eq!(widgets::colour_spec(PAPER_INVERTED), Is(Color::BLACK));
    assert_eq!(
        widgets::colour_spec(PAPER_LIGHT_GRAY),
        Is(Tone::LightGray.color())
    );
    assert_eq!(
        widgets::colour_spec(PAPER_DARK_GRAY),
        Is(Tone::DarkGray.color())
    );
    assert_eq!(widgets::colour_spec(PAPER_NONE), None);
    assert_eq!(widgets::colour_spec("core.tone.none"), None);
    assert_eq!(
        widgets::colour_spec("core.tone.blue"),
        Is(Tone::Blue.color())
    );
    assert_eq!(
        widgets::colour_spec("#3b6fd4"),
        Is(Color::rgb(0x3b, 0x6f, 0xd4))
    );
    assert_eq!(widgets::colour_spec("#fff"), Is(Color::WHITE));
    // Only a code says so; nothing else is a colour.
    for value in ["", "3b6fd4", "#3b6fd", "#xyzxyz", "core.tone.", "blue", "#"] {
        assert_eq!(widgets::colour_spec(value), Auto, "{value:?}");
    }
}

/// Without colours chosen, a widget looks as before: black on white,
/// white on a dark card.
#[test]
fn without_colours_chosen_the_frame_makes_them() {
    let mut h = harness();
    let (paper, ink, _) = colours(&h);
    assert_eq!((paper, ink), (Color::WHITE, Color::BLACK));
    set(&mut h, "t.clock.frame", Frame::Dark.value());
    let (paper, ink, _) = colours(&h);
    assert_eq!((paper, ink), (Color::BLACK, Color::WHITE));
    // So does "normal", chosen.
    set(&mut h, "t.clock.paper", PAPER_NORMAL);
    set(&mut h, "t.clock.ink", INK_AUTO);
    assert_eq!(colours(&h).0, Color::BLACK);
}

/// The automatic ink reads on the paper; one chosen is taken as it is.
#[test]
fn the_ink_reads_on_the_paper_unless_one_is_chosen() {
    let mut h = harness();
    for (paper, ink) in [
        (PAPER_INVERTED, Color::WHITE),
        (PAPER_DARK_GRAY, Color::WHITE),
        (PAPER_LIGHT_GRAY, Color::BLACK),
        ("core.tone.yellow", Color::BLACK),
        ("#1b2a44", Color::WHITE),
        // The page shows, taken to be light.
        (PAPER_NONE, Color::BLACK),
    ] {
        set(&mut h, "t.clock.paper", paper);
        let (_, drawn, faint) = colours(&h);
        assert_eq!(drawn, ink, "{paper}");
        assert_ne!(
            faint,
            if ink == Color::WHITE {
                Color::BLACK
            } else {
                Color::WHITE
            }
        );
    }
    set(&mut h, "t.clock.paper", PAPER_LIGHT_GRAY);
    set(&mut h, "t.clock.ink", Tone::White.id());
    assert_eq!(
        colours(&h),
        (Tone::LightGray.color(), Color::WHITE, Color::WHITE)
    );
    set(&mut h, "t.clock.ink", "#a03020");
    assert_eq!(colours(&h).1, Color::rgb(0xa0, 0x30, 0x20));
}

/// The colours of all widgets count where the widget's own are not
/// chosen.
#[test]
fn the_colours_of_all_widgets_unless_one_has_its_own() {
    let mut h = harness();
    set(&mut h, WIDGET_PAPER.static_str(), PAPER_INVERTED);
    assert_eq!(colours(&h).0, Color::BLACK);
    set(&mut h, WIDGET_INK.static_str(), "core.tone.gray");
    assert_eq!(colours(&h).1, Tone::Gray.color());
    set(&mut h, "t.clock.paper", PAPER_LIGHT_GRAY);
    set(&mut h, "t.clock.ink", INK_AUTO);
    assert_eq!(colours(&h).0, Tone::LightGray.color(), "its own, chosen");
    assert_eq!(colours(&h).1, Color::BLACK, "its own automatic, chosen");
}

/// A colour picker's tone or code is kept by the setting, whose own
/// values lack it.
#[test]
fn a_tone_or_a_code_is_kept() {
    let mut h = harness();
    for value in ["core.tone.blue", "#3b6fd4"] {
        set(&mut h, "t.clock.paper", value);
        assert_eq!(h.settings.text("t.clock.paper"), value);
    }
}

/// `draw_frame` paints a chosen paper over the widget's place and
/// nothing outside it; transparent and normal paint nothing without a
/// frame, as before.
#[test]
fn draw_frame_paints_the_paper_chosen() {
    let mut h = harness();
    let area = Rect::new(40, 40, 200, 100);
    let centre = paperpaw_sdk::Point::new(140, 90);
    let draw = |h: &Harness| {
        let mut drawn = frame(480, 800);
        widgets::draw_frame(&mut drawn.canvas(), &h.env(), "t.clock", area);
        drawn
    };
    assert!(!drawn_in(&draw(&h), area), "normal, no frame: nothing");
    set(&mut h, "t.clock.paper", PAPER_INVERTED);
    let mut drawn = draw(&h);
    assert_eq!(drawn.canvas().pixel(centre), Some(Color::BLACK));
    assert_eq!(drawn_outside(&mut drawn, area), None);
    set(&mut h, "t.clock.paper", PAPER_NONE);
    assert!(!drawn_in(&draw(&h), area), "transparent: nothing");
    // A line stays, in the ink.
    set(&mut h, "t.clock.frame", Frame::Line.value());
    set(&mut h, "t.clock.ink", Tone::White.id());
    let mut drawn = draw(&h);
    assert_eq!(
        drawn.canvas().pixel(centre),
        Some(PAPER),
        "inside: the page"
    );
    let edge = paperpaw_sdk::Point::new(140, 40);
    assert_eq!(drawn.canvas().pixel(edge), Some(Color::WHITE));
    // A dark card on a light gray paper is light gray.
    set(&mut h, "t.clock.frame", Frame::Dark.value());
    set(&mut h, "t.clock.paper", PAPER_LIGHT_GRAY);
    let mut drawn = draw(&h);
    assert_ne!(drawn.canvas().pixel(centre), Some(Color::BLACK));
    assert_eq!(drawn_outside(&mut drawn, area), None);
}

fn grid(env: &Env<'_>) -> (GridView, Rect) {
    let geometry = Geometry::of(Rect::new(0, 100, 480, 600), 4, 6, 8);
    let item = Item::placed(What::Widget("t.clock".into()), Cells::new(0, 0, 2, 1));
    let mut view = GridView::asking(env, Event::<Picked>::new("t.chosen"));
    view.layout(env, geometry, &[item], &[]);
    (view, geometry.rect(Cells::new(0, 0, 2, 1)))
}

/// What a grid lays under its items: white as before, nothing, or a
/// colour, whatever the widget's own paper.
#[test]
fn a_grid_lays_its_plate_under_the_items() {
    let mut h = harness();
    let pixel = |h: &Harness, plate: Plate| {
        let env = h.env();
        let (view, area) = grid(&env);
        let mut drawn = frame(480, 800);
        view.draw_on(&mut drawn.canvas(), &env, plate);
        drawn.canvas().pixel(paperpaw_sdk::Point::new(
            i32::from(area.x) + 2,
            i32::from(area.y) + 2,
        ))
    };
    let env = h.env();
    let (view, area) = grid(&env);
    let mut drawn = frame(480, 800);
    view.draw(&mut drawn.canvas(), &env);
    let corner = paperpaw_sdk::Point::new(i32::from(area.x) + 2, i32::from(area.y) + 2);
    assert_eq!(
        drawn.canvas().pixel(corner),
        Some(Color::WHITE),
        "draw: white"
    );
    assert_eq!(pixel(&h, Plate::Paper), Some(Color::WHITE));
    assert_eq!(pixel(&h, Plate::None), Some(PAPER), "nothing: the page");
    assert_eq!(pixel(&h, Plate::Colour(Color::BLACK)), Some(Color::BLACK));
    // A widget's own paper is its own to paint (draw_frame): the plate
    // stays what the screen chose.
    set(&mut h, "t.clock.paper", PAPER_INVERTED);
    assert_eq!(pixel(&h, Plate::Paper), Some(Color::WHITE));
    assert_eq!(pixel(&h, Plate::None), Some(PAPER));
}
