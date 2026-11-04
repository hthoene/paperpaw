//! The shape of widgets (#206): corners, frame line, shadow, spacing and
//! gap, by the widget's settings and those of all widgets; with none
//! chosen a widget is drawn as before.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod support;

use paperpaw_graphics::Framebuffer;
use paperpaw_sdk::builtin::settings::{
    CORNERS, WIDGET_CORNERS, WIDGET_FRAME_LINE, WIDGET_GAP, WIDGET_PADDING, WIDGET_SHADOW,
};
use paperpaw_sdk::decorations::Tone;
use paperpaw_sdk::ui::UiStyle;
use paperpaw_sdk::widgets::{
    self, Drawn, FRAMES, Frame, GAP_LARGE, GAP_SMALL, LINE_THICK, LINE_THIN, PAPER_DARK_GRAY,
    PAPER_INVERTED, SHADOW_HARD, Shadow, WidgetSpec,
};
use paperpaw_sdk::{Canvas, Color, Env, Id, PixelFormat, Point, Rect, SettingValue};

use support::{Device, Harness, PAPER, drawn_outside, frame};

const CLOCK: Id = Id::new_static("t.clock");
const AREA: Rect = Rect::new(40, 40, 200, 120);

fn short(_: &Env<'_>, _: u16) -> u16 {
    60
}

/// A widget `t.clock` with its frame and padding settings; with
/// `shape`, also its shape and colour settings and those of all widgets.
fn harness(shape: bool) -> Harness {
    let mut h = Harness::new(Device::BUTTONS);
    h.widgets.add(WidgetSpec::drawn(
        CLOCK,
        "t.clock",
        Drawn::new(short, clock_frame),
    ));
    let group = Id::new_static("t.group");
    let mut specs = vec![
        widgets::frame_setting(&CLOCK, Frame::None),
        widgets::padding_setting(&CLOCK, widgets::Padding::Auto),
    ];
    if shape {
        specs.extend([
            widgets::corners_setting(&CLOCK),
            widgets::frame_line_setting(&CLOCK),
            widgets::shadow_setting(&CLOCK),
            widgets::paper_setting(&CLOCK),
            widgets::all_corners_spec(group.clone()),
            widgets::all_frame_line_spec(group.clone()),
            widgets::all_shadow_spec(group.clone()),
            widgets::all_padding_spec(group.clone()),
            widgets::all_gap_spec(group.clone()),
            widgets::all_paper_spec(group),
        ]);
    }
    for spec in specs {
        h.settings.define(spec).unwrap();
    }
    h
}

fn set(h: &mut Harness, id: &str, value: &str) {
    h.settings
        .set(id, SettingValue::Text(value.to_string().into()))
        .unwrap();
}

/// The pixels `draw_frame` leaves, and the look it returns.
fn drawn(h: &Harness) -> (Vec<Color>, widgets::WidgetLook) {
    let mut drawn = frame(480, 800);
    let look = widgets::draw_frame(&mut drawn.canvas(), &h.env(), "t.clock", AREA);
    let canvas = drawn.canvas();
    let pixels = (0..220)
        .flat_map(|y| (0..300).map(move |x| Point::new(x, y)))
        .map(|p| canvas.pixel(p).unwrap())
        .collect();
    (pixels, look)
}

fn pixel(h: &Harness, x: i32, y: i32) -> Color {
    let mut drawn = frame(480, 800);
    widgets::draw_frame(&mut drawn.canvas(), &h.env(), "t.clock", AREA);
    drawn.canvas().pixel(Point::new(x, y)).unwrap()
}

/// With the shape settings there but none chosen, every frame is drawn
/// as it is without them, to the pixel, in the same place.
#[test]
fn with_no_shape_chosen_widgets_are_drawn_as_before() {
    for value in FRAMES {
        let mut before = harness(false);
        let mut after = harness(true);
        set(&mut before, "t.clock.frame", value);
        set(&mut after, "t.clock.frame", value);
        let (old, old_look) = drawn(&before);
        let (new, new_look) = drawn(&after);
        assert_eq!(old_look, new_look, "{value}");
        assert!(old == new, "{value}: drawn otherwise");
        assert_eq!(
            widgets::frame_height(&before.env(), "t.clock"),
            widgets::frame_height(&after.env(), "t.clock"),
            "{value}"
        );
    }
    let h = harness(true);
    assert_eq!(widgets::corners(&h.env(), "t.clock"), None);
    assert_eq!(widgets::frame_line(&h.env(), "t.clock"), None);
    assert_eq!(widgets::shadow(&h.env(), "t.clock"), Shadow::None);
    let style = UiStyle::new(&h.env());
    assert_eq!(widgets::gap(&h.env()), style.gap());
}

/// A widget's own value wins where the reader chose it; else that of all
/// widgets, unless that is automatic.
#[test]
fn its_own_shape_wins_over_that_of_all_widgets() {
    let mut h = harness(true);
    set(&mut h, WIDGET_CORNERS.static_str(), CORNERS[2]);
    let round = widgets::corners(&h.env(), "t.clock").unwrap();
    assert!(round > 0);
    set(&mut h, "t.clock.corners", CORNERS[0]);
    assert_eq!(widgets::corners(&h.env(), "t.clock"), Some(0), "its own");
    set(&mut h, "t.clock.corners", widgets::CORNERS_AUTO);
    assert_eq!(
        widgets::corners(&h.env(), "t.clock"),
        None,
        "its own, automatic"
    );

    set(&mut h, WIDGET_FRAME_LINE.static_str(), LINE_THICK);
    let thick = widgets::frame_line(&h.env(), "t.clock").unwrap();
    set(&mut h, "t.clock.frame-line", LINE_THIN);
    assert!(widgets::frame_line(&h.env(), "t.clock").unwrap() < thick);

    set(&mut h, WIDGET_SHADOW.static_str(), SHADOW_HARD);
    assert_eq!(widgets::shadow(&h.env(), "t.clock"), Shadow::Hard);
    set(&mut h, "t.clock.drop-shadow", widgets::SHADOW_NONE);
    assert_eq!(
        widgets::shadow(&h.env(), "t.clock"),
        Shadow::None,
        "its own"
    );

    set(&mut h, WIDGET_PADDING.static_str(), "core.padding.large");
    assert_eq!(
        widgets::padding(&h.env(), "t.clock"),
        widgets::Padding::Large
    );
    set(&mut h, "t.clock.padding", "core.padding.none");
    assert_eq!(
        widgets::padding(&h.env(), "t.clock"),
        widgets::Padding::None
    );
}

/// Square corners fill a card's corner; round ones leave it.
#[test]
fn corners_shape_the_card() {
    let mut h = harness(true);
    set(&mut h, "t.clock.frame", Frame::Dark.value());
    set(&mut h, "t.clock.corners", CORNERS[0]);
    assert_eq!(pixel(&h, 40, 40), Color::BLACK, "square");
    set(&mut h, "t.clock.corners", CORNERS[3]);
    assert_eq!(pixel(&h, 40, 40), PAPER, "soft");
}

/// A thick line is thicker, and the content keeps clear of it.
#[test]
fn a_thick_frame_line_is_thick_and_kept_clear_of() {
    let mut h = harness(true);
    set(&mut h, "t.clock.frame", Frame::Line.value());
    set(&mut h, "t.clock.corners", CORNERS[0]);
    let thin_area = drawn(&h).1.area;
    set(&mut h, "t.clock.frame-line", LINE_THICK);
    let thick = widgets::frame_line(&h.env(), "t.clock").unwrap();
    assert!(thick >= 4, "{thick}");
    let (_, look) = drawn(&h);
    assert_eq!(pixel(&h, 140, 40 + i32::from(thick) - 1), Color::BLACK);
    assert!(look.area.y >= 40 + thick, "{look:?}");
    assert!(look.area.height < thin_area.height);
}

/// A solid shadow falls right and below a card, inside the widget's
/// place, which it takes room from; a line without paper casts none.
#[test]
fn a_shadow_falls_below_a_card_inside_the_place() {
    let mut h = harness(true);
    set(&mut h, "t.clock.frame", Frame::Card.value());
    set(&mut h, "t.clock.corners", CORNERS[0]);
    let height = widgets::frame_height(&h.env(), "t.clock");
    set(&mut h, "t.clock.drop-shadow", SHADOW_HARD);
    let (_, look) = drawn(&h);
    let mut frame = frame(480, 800);
    widgets::draw_frame(&mut frame.canvas(), &h.env(), "t.clock", AREA);
    assert_eq!(drawn_outside(&mut frame, AREA), None);
    // The corner at the bottom right is the shadow's.
    assert_eq!(pixel(&h, 239, 159), Color::BLACK);
    assert_eq!(pixel(&h, 40, 159), PAPER, "the place's corner below left");
    assert!(widgets::frame_height(&h.env(), "t.clock") > height);
    assert!(look.area.right() < AREA.right());
    // A line alone casts none: it is drawn as without a shadow.
    set(&mut h, "t.clock.frame", Frame::Line.value());
    let with = drawn(&h);
    set(&mut h, "t.clock.drop-shadow", widgets::SHADOW_NONE);
    let without = drawn(&h);
    assert_eq!(with.1, without.1);
    assert!(with.0 == without.0, "a line casts no shadow");
    set(&mut h, "t.clock.drop-shadow", SHADOW_HARD);
    // A paper of its own does: a black one a dark gray shadow.
    set(&mut h, "t.clock.paper", PAPER_INVERTED);
    assert_eq!(pixel(&h, 239, 159), Tone::DarkGray.color());
}

/// Below a black plate, a dark frame's or inverted paper's, the shadow is
/// dark gray, which stands out from it (owner decision 106); a dark gray
/// plate keeps a black one; a panel of two grays dithers it.
#[test]
fn a_shadow_below_a_black_plate_is_dark_gray() {
    let mut h = harness(true);
    set(&mut h, "t.clock.corners", CORNERS[0]);
    set(&mut h, "t.clock.drop-shadow", SHADOW_HARD);
    let dark_gray = Tone::DarkGray.color();
    set(&mut h, "t.clock.frame", Frame::Dark.value());
    assert_eq!(pixel(&h, 239, 159), dark_gray, "below a dark frame");
    set(&mut h, "t.clock.frame", Frame::Card.value());
    assert_eq!(pixel(&h, 239, 159), Color::BLACK, "below a card");
    set(&mut h, "t.clock.paper", PAPER_DARK_GRAY);
    assert_eq!(pixel(&h, 239, 159), Color::BLACK, "below dark gray");
    set(&mut h, "t.clock.paper", PAPER_INVERTED);
    assert_eq!(pixel(&h, 239, 159), dark_gray, "below inverted paper");
    // Black and white only: the dark gray dithered, as grays are.
    let mut mono = Framebuffer::new(480, 800, PixelFormat::Mono1).unwrap();
    mono.canvas().clear(Color::WHITE);
    widgets::draw_frame(&mut mono.canvas(), &h.env(), "t.clock", AREA);
    let canvas = mono.canvas();
    let corner: Vec<Color> = (156..160)
        .flat_map(|y| (236..240).map(move |x| Point::new(x, y)))
        .map(|p| canvas.pixel(p).unwrap())
        .collect();
    assert!(corner.contains(&Color::BLACK) && corner.contains(&Color::WHITE));
}

/// Places too small for round corners and a shadow, or at the far end of
/// the coordinates, are drawn in without a panic and nothing outside.
#[test]
fn a_tiny_place_with_round_corners_and_a_shadow_is_safe() {
    let mut h = harness(true);
    set(&mut h, "t.clock.frame", Frame::Card.value());
    set(&mut h, "t.clock.corners", CORNERS[2]);
    set(&mut h, "t.clock.drop-shadow", SHADOW_HARD);
    let far = u16::MAX - 1;
    for area in [
        Rect::new(10, 10, 1, 1),
        Rect::new(10, 10, 0, 0),
        Rect::new(10, 10, 3, 40),
        Rect::new(far, far, 1, 1),
    ] {
        let mut frame = frame(480, 800);
        let look = widgets::draw_frame(&mut frame.canvas(), &h.env(), "t.clock", area);
        assert!(look.area.width <= area.width && look.area.height <= area.height);
        if area.x < 480 {
            assert_eq!(drawn_outside(&mut frame, area), None, "{area:?}");
        }
    }
}

#[test]
fn the_gap_between_widgets() {
    let mut h = harness(true);
    let normal = widgets::gap(&h.env());
    set(&mut h, WIDGET_GAP.static_str(), GAP_SMALL);
    assert_eq!(widgets::gap(&h.env()), normal / 2);
    set(&mut h, WIDGET_GAP.static_str(), GAP_LARGE);
    assert_eq!(widgets::gap(&h.env()), normal * 2);
}

/// A widget look covers the shape of each widget and of all, and the
/// gap.
#[test]
fn a_widget_look_covers_the_shape() {
    let h = harness(true);
    let covered: Vec<String> = paperpaw_sdk::presets::widget_look_settings(&h.settings, &h.widgets)
        .iter()
        .map(|id| id.as_str().to_string())
        .collect();
    for id in [
        "t.clock.corners",
        "t.clock.frame-line",
        "t.clock.drop-shadow",
        WIDGET_CORNERS.static_str(),
        WIDGET_FRAME_LINE.static_str(),
        WIDGET_SHADOW.static_str(),
        WIDGET_PADDING.static_str(),
        WIDGET_GAP.static_str(),
    ] {
        assert!(covered.iter().any(|c| c == id), "{id}: {covered:?}");
    }
}

/// A widget drawing all of itself (its own card, in its own colours)
/// with none of the helpers.
fn own_card(canvas: &mut Canvas<'_>, _: &Env<'_>, area: Rect) {
    canvas.fill(area, Color::gray(120));
    canvas.stroke_round(area, 3, 0, Color::BLACK);
}

/// The shared look is a helper: a widget drawing all of itself is drawn
/// the same whatever the reader set for all widgets, also in a grid.
#[test]
fn a_widget_drawing_itself_is_free_of_the_settings_for_widgets() {
    use paperpaw_sdk::builtin::settings::{WIDGET_FRAME, WIDGET_INK, WIDGET_PAPER};
    use paperpaw_sdk::event::Event;
    use paperpaw_sdk::ui::choice::Picked;
    use paperpaw_sdk::widgets::grid::{Cells, Geometry, GridView, Item, What};
    let draw = |h: &Harness| {
        let env = h.env();
        let geometry = Geometry::of(Rect::new(0, 100, 480, 600), 4, 6, widgets::gap(&env));
        let item = Item::placed(What::Widget("t.own".into()), Cells::new(0, 0, 2, 1));
        let mut view = GridView::asking(&env, Event::<Picked>::new("t.chosen"));
        view.layout(&env, geometry, &[item], &[]);
        let mut drawn = frame(480, 800);
        view.draw(&mut drawn.canvas(), &env);
        let canvas = drawn.canvas();
        (0..800)
            .flat_map(|y| (0..480).map(move |x| Point::new(x, y)))
            .map(|p| canvas.pixel(p).unwrap())
            .collect::<Vec<_>>()
    };
    let mut h = harness(true);
    let own = Id::new_static("t.own");
    h.widgets.add(WidgetSpec::drawn(
        own.clone(),
        "t.own",
        Drawn::new(short, own_card),
    ));
    let group = Id::new_static("t.group");
    for spec in [
        paperpaw_sdk::SettingSpec::choice(
            WIDGET_FRAME.static_str(),
            "t.all",
            widgets::ALL_FRAMES,
            widgets::OWN_FRAME,
            group.clone(),
        ),
        widgets::all_ink_spec(group),
    ] {
        h.settings.define(spec).unwrap();
    }
    let before = draw(&h);
    for (id, value) in [
        (WIDGET_FRAME.static_str(), FRAMES[3]),
        (WIDGET_PAPER.static_str(), PAPER_INVERTED),
        (WIDGET_INK.static_str(), "core.tone.red"),
        (WIDGET_CORNERS.static_str(), CORNERS[3]),
        (WIDGET_FRAME_LINE.static_str(), LINE_THICK),
        (WIDGET_SHADOW.static_str(), SHADOW_HARD),
        (WIDGET_PADDING.static_str(), "core.padding.large"),
    ] {
        set(&mut h, id, value);
    }
    assert!(before == draw(&h), "drawn otherwise");
    // A widget making its own look keeps it.
    let look =
        widgets::WidgetLook::new(AREA, Color::BLACK, Color::BLACK, Color::WHITE, Frame::None);
    assert_eq!(look.within(AREA).ink, Color::BLACK);
}

#[test]
fn frameless_paper_follows_chosen_corners_but_automatic_stays_square() {
    let mut h = harness(true);
    set(&mut h, "t.clock.paper", PAPER_INVERTED);
    assert_eq!(pixel(&h, 40, 40), Color::BLACK, "automatic is square");
    for corners in [CORNERS[2], CORNERS[3]] {
        set(&mut h, WIDGET_CORNERS.static_str(), corners);
        assert_eq!(pixel(&h, 40, 40), PAPER, "global corners");
        set(&mut h, "t.clock.corners", widgets::CORNERS_AUTO);
        assert_eq!(pixel(&h, 40, 40), Color::BLACK, "own automatic");
        set(&mut h, "t.clock.corners", corners);
        assert_eq!(pixel(&h, 40, 40), PAPER, "own corners");
        assert_eq!(pixel(&h, 140, 100), Color::BLACK, "paper inside");
        // Reset the own choice so the next global value applies.
        h = harness(true);
        set(&mut h, "t.clock.paper", PAPER_INVERTED);
    }
}

fn clock_frame(canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
    widgets::draw_frame(canvas, env, "t.clock", area);
}

#[test]
fn grid_plates_leave_every_pixel_outside_the_widget_shape_untouched() {
    use paperpaw_sdk::event::Event;
    use paperpaw_sdk::ui::choice::Picked;
    use paperpaw_sdk::widgets::grid::{Cells, Geometry, GridView, Item, What};
    use widgets::Plate;
    let mut h = harness(true);
    for frame_kind in [Frame::None, Frame::Card, Frame::Dark] {
        set(&mut h, "t.clock.frame", frame_kind.value());
        set(&mut h, "t.clock.paper", PAPER_INVERTED);
        for corners in [CORNERS[2], CORNERS[3]] {
            set(&mut h, "t.clock.corners", corners);
            for shadow in [widgets::SHADOW_NONE, SHADOW_HARD] {
                set(&mut h, "t.clock.drop-shadow", shadow);
                for format in [PixelFormat::Gray8, PixelFormat::Mono1] {
                    let env = h.env();
                    let geometry = Geometry::of(AREA, 1, 1, 0);
                    let cells = Cells::new(0, 0, 1, 1);
                    let area = geometry.rect(cells);
                    let mut view = GridView::asking(&env, Event::<Picked>::new("t.chosen"));
                    view.layout(
                        &env,
                        geometry,
                        &[Item::placed(What::Widget("t.clock".into()), cells)],
                        &[],
                    );
                    let mut expected = Framebuffer::new(480, 800, format).unwrap();
                    expected.canvas().clear(Color::WHITE);
                    clock_frame(&mut expected.canvas(), &env, area);
                    for plate in [Plate::Paper, Plate::Colour(Color::gray(180))] {
                        let mut actual = Framebuffer::new(480, 800, format).unwrap();
                        actual.canvas().clear(Color::WHITE);
                        view.draw_on(&mut actual.canvas(), &env, plate);
                        let expected = expected.canvas();
                        let actual = actual.canvas();
                        // The opaque widget covers its plate completely. Any extra
                        // pixel is a leaking corner or a fill behind the shadow.
                        for y in 0..200 {
                            for x in 0..260 {
                                let p = Point::new(x, y);
                                assert_eq!(
                                    actual.pixel(p),
                                    expected.pixel(p),
                                    "{frame_kind:?}, {corners}, {shadow}, {format:?}, {plate:?} at {p:?}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}
