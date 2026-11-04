//! The plate under a widget in a grid keeps to the widget's shape (#206,
//! #600, #601): what lies outside its rounded outline, or beside its
//! shadow, is the page's, in every corner, frame, paper and shadow the
//! reader can choose, also for a second placement of the same widget.
//!
//! The widget here draws nothing itself, so that every pixel of the cell
//! is the grid's plate or the page's: the corner pixels say, to the pixel,
//! which. (`widget_shape` compares with a widget that covers its plate.)

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod support;

use paperpaw_sdk::builtin::settings::{CORNERS, UI_CORNERS};
use paperpaw_sdk::decorations::Tone;
use paperpaw_sdk::event::Event;
use paperpaw_sdk::ui::choice::Picked;
use paperpaw_sdk::widgets::grid::{Cells, Geometry, GridView, Item, What};
use paperpaw_sdk::widgets::{
    self, Drawn, Frame, PAPER_DARK_GRAY, PAPER_INVERTED, PAPER_LIGHT_GRAY, PAPER_NORMAL, Plate,
    SHADOW_HARD, SHADOW_NONE, WidgetSpec,
};
use paperpaw_sdk::{Canvas, Color, Env, Id, Point, Rect, SettingSpec, SettingValue};

use support::{Device, Harness, PAPER, frame};

const CLOCK: Id = Id::new_static("t.clock");
/// The widget's place: the only cell of the grid.
const CELL: Rect = Rect::new(40, 40, 200, 120);
const LEFT: i32 = 40;
const TOP: i32 = 40;
const RIGHT: i32 = 240;
const BOTTOM: i32 = 160;

fn short(_: &Env<'_>, _: u16) -> u16 {
    60
}

fn nothing(_: &mut Canvas<'_>, _: &Env<'_>, _: Rect) {}

fn harness() -> Harness {
    let mut h = Harness::new(Device::BUTTONS);
    h.widgets.add(WidgetSpec::drawn(
        CLOCK,
        "t.clock",
        Drawn::new(short, nothing),
    ));
    h.settings
        .define(SettingSpec::choice(
            UI_CORNERS.static_str(),
            "t.ui-corners",
            CORNERS,
            CORNERS[0],
            paperpaw_sdk::builtin::groups::OTHER,
        ))
        .unwrap();
    for spec in [
        widgets::frame_setting(&CLOCK, Frame::None),
        widgets::corners_setting(&CLOCK),
        widgets::shadow_setting(&CLOCK),
        widgets::paper_setting(&CLOCK),
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

/// The cell's page after the grid drew `widget` on `plate`.
fn drawn(h: &Harness, widget: &str, plate: Plate) -> paperpaw_graphics::Framebuffer {
    let env = h.env();
    let geometry = Geometry::of(CELL, 1, 1, 0);
    let cells = Cells::new(0, 0, 1, 1);
    let mut view = GridView::asking(&env, Event::<Picked>::new("t.chosen"));
    view.layout(
        &env,
        geometry,
        &[Item::placed(What::Widget(widget.into()), cells)],
        &[],
    );
    let mut page = frame(480, 800);
    view.draw_on(&mut page.canvas(), &env, plate);
    page
}

fn at(page: &mut paperpaw_graphics::Framebuffer, x: i32, y: i32) -> Color {
    page.canvas().pixel(Point::new(x, y)).unwrap()
}

/// The four extreme corner pixels of `area`.
fn corners_of(area: Rect) -> [(i32, i32); 4] {
    let (l, t) = (i32::from(area.x), i32::from(area.y));
    let (r, b) = (area.right() as i32 - 1, area.bottom() as i32 - 1);
    [(l, t), (r, t), (l, b), (r, b)]
}

/// The plates the reader can choose (the paper of widgets, and a dark
/// page's colour): white, light and dark gray, and black.
fn plates() -> [(&'static str, Plate, Color); 4] {
    [
        ("paper", Plate::Paper, Color::WHITE),
        (
            "light gray",
            Plate::Colour(Tone::LightGray.color()),
            Tone::LightGray.color(),
        ),
        (
            "dark gray",
            Plate::Colour(Tone::DarkGray.color()),
            Tone::DarkGray.color(),
        ),
        ("black", Plate::Colour(Color::BLACK), Color::BLACK),
    ]
}

/// A frameless widget's plate has the corners chosen: square leaves its
/// corner pixels the plate's, every rounded one the page's; the middle of
/// every edge is the plate's whichever corners.
#[test]
fn a_frameless_plate_is_as_round_as_chosen_on_every_paper() {
    let mut h = harness();
    for corners in CORNERS {
        set(&mut h, "t.clock.corners", corners);
        let square = *corners == CORNERS[0];
        for (name, plate, colour) in plates() {
            let mut page = drawn(&h, "t.clock", plate);
            for (x, y) in corners_of(CELL) {
                let want = if square { colour } else { PAPER };
                assert_eq!(
                    at(&mut page, x, y),
                    want,
                    "{corners} on {name} at ({x}, {y})"
                );
            }
            for (x, y) in [
                (LEFT + 100, TOP),
                (LEFT + 100, BOTTOM - 1),
                (LEFT, TOP + 60),
                (RIGHT - 1, TOP + 60),
                (LEFT + 100, TOP + 60),
            ] {
                assert_eq!(
                    at(&mut page, x, y),
                    colour,
                    "{corners} on {name} at ({x}, {y})"
                );
            }
        }
    }
}

/// Nothing is drawn outside the cell, and with no plate nothing in it.
#[test]
fn the_plate_stays_in_its_cell_and_none_leaves_the_page() {
    let mut h = harness();
    for corners in CORNERS {
        set(&mut h, "t.clock.corners", corners);
        for (name, plate, _) in plates() {
            let mut page = drawn(&h, "t.clock", plate);
            for (x, y) in [
                (LEFT - 1, TOP + 60),
                (RIGHT, TOP + 60),
                (LEFT + 100, TOP - 1),
                (LEFT + 100, BOTTOM),
                (LEFT - 1, TOP - 1),
                (RIGHT, BOTTOM),
            ] {
                assert_eq!(
                    at(&mut page, x, y),
                    PAPER,
                    "{corners} on {name} at ({x}, {y})"
                );
            }
        }
        let mut page = drawn(&h, "t.clock", Plate::None);
        for (x, y) in corners_of(CELL).into_iter().chain([(LEFT + 100, TOP + 60)]) {
            assert_eq!(
                at(&mut page, x, y),
                PAPER,
                "no plate, {corners} at ({x}, {y})"
            );
        }
    }
}

/// A card or a dark card has the corners chosen for the widget, else the
/// interface's, on whatever paper the widget has and the grid lays.
#[test]
fn a_card_plate_is_as_round_as_the_interface_on_every_paper() {
    let mut h = harness();
    for frame_kind in [Frame::Card, Frame::Dark] {
        set(&mut h, "t.clock.frame", frame_kind.value());
        for ui in CORNERS {
            set(&mut h, UI_CORNERS.static_str(), ui);
            for corners in [widgets::CORNERS_AUTO, CORNERS[0], CORNERS[2]] {
                set(&mut h, "t.clock.corners", corners);
                // Chosen corners are the card's; automatic follow the interface's.
                let chosen = if corners == widgets::CORNERS_AUTO {
                    ui
                } else {
                    corners
                };
                let square = chosen == CORNERS[0];
                for paper in [
                    PAPER_NORMAL,
                    PAPER_INVERTED,
                    PAPER_LIGHT_GRAY,
                    PAPER_DARK_GRAY,
                ] {
                    set(&mut h, "t.clock.paper", paper);
                    for (name, plate, colour) in plates() {
                        let mut page = drawn(&h, "t.clock", plate);
                        for (x, y) in corners_of(CELL) {
                            let want = if square { colour } else { PAPER };
                            assert_eq!(
                                at(&mut page, x, y),
                                want,
                                "{frame_kind:?}, interface {ui}, {corners}, {paper} on {name} at ({x}, {y})"
                            );
                        }
                        assert_eq!(
                            at(&mut page, LEFT + 100, TOP + 60),
                            colour,
                            "{frame_kind:?}, interface {ui}, {corners}, {paper} on {name}"
                        );
                    }
                }
            }
        }
    }
}

/// A hard shadow takes room right and below: the plate is smaller by it,
/// and that strip, the shadow's, is the page's (the widget draws it, the
/// grid's plate does not); the plate's corners are round in both (the
/// interface's corners round here).
#[test]
fn the_plate_leaves_the_room_of_a_shadow_to_the_page() {
    let mut h = harness();
    set(&mut h, "t.clock.frame", Frame::Card.value());
    set(&mut h, UI_CORNERS.static_str(), CORNERS[2]);
    for shadow in [SHADOW_NONE, SHADOW_HARD] {
        set(&mut h, "t.clock.drop-shadow", shadow);
        for (name, plate, colour) in plates() {
            let mut page = drawn(&h, "t.clock", plate);
            let shadowed = shadow == SHADOW_HARD;
            // Where the plate ends, right and below: found along the
            // middle row and column.
            let right = (LEFT..RIGHT)
                .rev()
                .find(|&x| at(&mut page, x, TOP + 60) == colour)
                .unwrap();
            let below = (TOP..BOTTOM)
                .rev()
                .find(|&y| at(&mut page, LEFT + 100, y) == colour)
                .unwrap();
            if shadowed {
                assert!(right < RIGHT - 4, "{name}: plate ends at {right}");
                assert!(below < BOTTOM - 4, "{name}: plate ends at {below}");
            } else {
                assert_eq!(right, RIGHT - 1, "{name}");
                assert_eq!(below, BOTTOM - 1, "{name}");
            }
            // The plate's top left corner is round, in both; at the cell's
            // bottom right corner the page shows (round, or the shadow's).
            assert_eq!(at(&mut page, LEFT, TOP), PAPER, "{shadow} on {name}");
            assert_eq!(
                at(&mut page, RIGHT - 1, BOTTOM - 1),
                PAPER,
                "{shadow} on {name}"
            );
        }
    }
}

/// A second placement of a widget (`t.clock@2`) has the plate of its own
/// choices, else of the widget's: square where the second is square and
/// the first round, and round the other way round.
#[test]
fn a_second_placement_has_a_plate_of_its_own_corners() {
    let mut h = harness();
    for (base, second) in [(CORNERS[0], CORNERS[2]), (CORNERS[2], CORNERS[0])] {
        set(&mut h, "t.clock.corners", base);
        set(&mut h, "t.clock@2.corners", second);
        for (name, plate, colour) in plates() {
            for (widget, corners) in [("t.clock", base), ("t.clock@2", second)] {
                let mut page = drawn(&h, widget, plate);
                let want = if corners == CORNERS[0] { colour } else { PAPER };
                for (x, y) in corners_of(CELL) {
                    assert_eq!(
                        at(&mut page, x, y),
                        want,
                        "{widget} ({corners}) on {name} at ({x}, {y})"
                    );
                }
            }
        }
    }
    // With nothing chosen for the second, it follows the widget.
    set(&mut h, "t.clock.corners", CORNERS[2]);
    h.settings.reset("t.clock@2.corners").unwrap();
    let mut page = drawn(&h, "t.clock@2", Plate::Paper);
    assert_eq!(at(&mut page, LEFT, TOP), PAPER);
}
