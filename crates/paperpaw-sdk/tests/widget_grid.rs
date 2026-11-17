//! The home screen's grid of widgets and tools ([`grid`], [`GridView`]):
//! layouts read from text however broken, written and read back alike;
//! items placed, dragged, sized and moved by touch and by buttons never
//! overlap, never leave the grid and are never lost; the layout survives
//! being kept and read again; every item is reached by buttons as by
//! touch.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod support;

use paperpaw_sdk::builtin::{actions, params};
use paperpaw_sdk::event::{Event, EventData};
use paperpaw_sdk::ui::choice::{OPTION, Picked};
use paperpaw_sdk::widgets::grid::{
    self, Cells, Geometry, GridResponse, GridView, Item, MAX_COLUMNS, MAX_ROWS, What,
};
use paperpaw_sdk::widgets::{Drawn, Widget, WidgetSpec, WidgetTarget};
use paperpaw_sdk::{
    Canvas, Command, Commands, Cx, Env, GestureKind, Id, Output, Params, Point, PointerPhase, Rect,
};
use proptest::prelude::*;

use support::{Device, Harness, drawn_outside, frame, middle, pointer, tap};

const CHOSEN: Event<Picked> = Event::new("t.chosen");
/// What a tap on the book widget runs.
const OPEN: Id = Id::new_static("t.open");

fn short(_: &Env<'_>, _: u16) -> u16 {
    60
}

fn tall(_: &Env<'_>, _: u16) -> u16 {
    250
}

fn nothing(_: &mut Canvas<'_>, _: &Env<'_>, _: Rect) {}

/// A widget that opens something when tapped anywhere on it.
struct Book;

impl Widget for Book {
    fn height(&self, _: &Env<'_>, _: u16) -> u16 {
        90
    }

    fn draw(&self, canvas: &mut Canvas<'_>, _: &Env<'_>, area: Rect) {
        canvas.fill(area, paperpaw_sdk::Color::BLACK);
    }

    fn targets(&self, _: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        vec![WidgetTarget::new(area, Command::run(OPEN))]
    }
}

/// A device with three widgets: a short one, a tall one and a book.
fn harness(device: Device) -> Harness {
    let mut h = Harness::new(device);
    h.widgets.add(WidgetSpec::drawn(
        Id::new_static("t.clock"),
        "t.clock",
        Drawn::new(short, nothing),
    ));
    h.widgets.add(WidgetSpec::drawn(
        Id::new_static("t.tall"),
        "t.tall",
        Drawn::new(tall, nothing),
    ));
    h.widgets
        .add(WidgetSpec::new(Id::new_static("t.book"), "t.book", |_| {
            Box::new(Book)
        }));
    h
}

/// Four columns of 120 and six rows of 100 below a header.
fn geometry() -> Geometry {
    Geometry::of(Rect::new(0, 100, 480, 600), 4, 6, 8)
}

fn view(env: &Env<'_>, items: &[Item]) -> GridView {
    let mut view = GridView::asking(env, CHOSEN);
    view.layout(env, geometry(), items, &[]);
    view
}

fn tool(action: &'static str) -> What {
    What::Tool(Id::new_static(action))
}

fn widget(id: &str) -> What {
    What::Widget(id.to_string())
}

/// The items shown, each with its cells: those of the view's items that
/// have room where they are (the view keeps the others, not shown).
/// Placed again, the items shown stay where they are; if two shown items
/// overlapped, one of them would no longer be shown.
fn placed(env: &Env<'_>, view: &GridView) -> Vec<(What, Cells)> {
    let items = view.items();
    let shown: Vec<(What, Cells)> = grid::place(env, view.geometry(), &items)
        .into_iter()
        .map(|(item, cells)| (item.what, cells))
        .collect();
    // The view lists those it shows first, in their cells.
    let first: Vec<(What, Cells)> = items
        .iter()
        .take(shown.len())
        .map(|i| (i.what.clone(), i.at.unwrap()))
        .collect();
    assert_eq!(first, shown, "the items shown are where the view has them");
    shown
}

/// No two items shown overlap, and each lies in the grid.
fn assert_tidy(env: &Env<'_>, view: &GridView) -> Result<(), TestCaseError> {
    let g = view.geometry();
    let shown = placed(env, view);
    for (n, (what, c)) in shown.iter().enumerate() {
        prop_assert!(
            c.fits(g.columns, g.rows),
            "{what:?} at {c:?} leaves the grid"
        );
        for (other, d) in &shown[n + 1..] {
            prop_assert!(!c.overlaps(d), "{what:?} at {c:?} on {other:?} at {d:?}");
        }
    }
    Ok(())
}

/// The whats of `items`, sorted, to compare which items there are.
fn whats(items: &[Item]) -> Vec<String> {
    let mut whats: Vec<String> = items
        .iter()
        .map(|i| grid::write(&[Item::new(i.what.clone())]))
        .collect();
    whats.sort();
    whats
}

/// Runs `f` with a `Cx` and returns what it answered and asked for.
fn with_cx<R>(env: &Env<'_>, f: impl FnOnce(&mut Cx<'_, '_>) -> R) -> (R, Output) {
    let mut output = Output::default();
    let result = {
        let mut cx = Cx::new(env, &mut output);
        f(&mut cx)
    };
    (result, output)
}

/// Chooses `option` for item `index`, as the choice screen answers.
fn choose(view: &mut GridView, env: &Env<'_>, index: usize, option: &str) -> GridResponse {
    let picked = Picked::new(index.to_string(), format!("core.grid.{option}"));
    let event = Id::parse(CHOSEN.as_str()).unwrap();
    with_cx(env, |cx| view.on_event(&event, &picked.to_params(), cx)).0
}

fn press(view: &mut GridView, env: &Env<'_>, action: &Id) -> GridResponse {
    with_cx(env, |cx| view.on_action(action, cx)).0
}

/// A finger down at `from`, moved to `to` and lifted there.
fn drag(view: &mut GridView, env: &Env<'_>, from: Point, to: Point) -> GridResponse {
    let mut last = GridResponse::Ignored;
    for (phase, at) in [
        (PointerPhase::Down, from),
        (PointerPhase::Move, to),
        (PointerPhase::Up, to),
    ] {
        last = with_cx(env, |cx| view.on_pointer(&pointer(phase, at.x, at.y), cx)).0;
    }
    last
}

/// The corner that sizes an item in `area` (rows are 60 here: at least
/// 30 pixels of it).
fn corner(area: Rect) -> Point {
    Point::new(area.right() as i32 - 3, area.bottom() as i32 - 3)
}

// ---- Layouts as text ---------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256)))]

    /// Any text at all is read without a panic; what is read is written
    /// and read again alike.
    #[test]
    fn any_layout_text_is_read_and_written_back_alike(text in ".{0,200}") {
        let items = grid::parse(&text);
        let written = grid::write(&items);
        prop_assert_eq!(grid::parse(&written), items);
    }

    /// Layouts of valid items are written and read back unchanged.
    #[test]
    fn layouts_round_trip(
        items in proptest::collection::vec(
            (
                any::<bool>(),
                "[a-z]{1,8}\\.[a-z][a-z0-9-]{0,10}(@[1-9])?",
                proptest::option::of((any::<u8>(), any::<u8>(), any::<u8>(), any::<u8>())),
            ),
            0..12,
        ),
    ) {
        let items: Vec<Item> = items
            .into_iter()
            .filter_map(|(is_tool, id, at)| {
                let what = if is_tool {
                    What::Tool(Id::parse(id.split('@').next()?).ok()?)
                } else {
                    What::Widget(id)
                };
                Some(match at {
                    Some((col, row, w, h)) => Item::placed(what, Cells::new(col, row, w, h)),
                    None => Item::new(what),
                })
            })
            .collect();
        prop_assert_eq!(grid::parse(&grid::write(&items)), items);
    }
}

#[test]
fn broken_places_keep_their_items_without_a_place() {
    let items = grid::parse(
        "t.clock 0/0, t.tall 1/x 2x2, t.book 300/0 1x1, tool:t.a 0/0 1x, tool:t.b -1/0 1x1, t.c 0/0 0x0",
    );
    let at: Vec<Option<Cells>> = items.iter().map(|i| i.at).collect();
    assert_eq!(items.len(), 6, "no item is lost");
    assert_eq!(&at[..5], &[None; 5]);
    // Zero cells is read; it fits no grid, so it is placed anew.
    assert_eq!(at[5], Some(Cells::new(0, 0, 0, 0)));
    // Empty parts, ids that are no ids: left out.
    assert!(grid::parse(" , ,,").is_empty());
    assert!(grid::parse("tool:Bad").is_empty());
}

// ---- Geometry and placing ---------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256)))]

    /// The cells of a grid lie in its area, apart by the gap, and the
    /// cell under each cell's middle is that cell.
    #[test]
    fn cells_lie_in_the_area_and_are_found_where_they_are(
        x in 0u16..100,
        y in 0u16..200,
        width in 100u16..800,
        height in 100u16..900,
        columns in 1u8..=MAX_COLUMNS,
        rows in 1u8..=MAX_ROWS,
        gap in 0u16..20,
    ) {
        let g = Geometry::of(Rect::new(x, y, width, height), columns, rows, gap);
        for row in 0..g.rows {
            for col in 0..g.columns {
                let c = Cells::new(col, row, 1, 1);
                let r = g.rect(c);
                prop_assert!(r.x >= g.area.x && r.right() <= g.area.right());
                prop_assert!(r.y >= g.area.y && r.bottom() <= g.area.bottom());
                if !r.is_empty() {
                    let m = middle(r);
                    prop_assert_eq!(g.cell_at(m), Some((col, row)));
                }
            }
        }
        prop_assert_eq!(g.cell_at(Point::new(i32::from(x) - 1, i32::from(y))), None);
        prop_assert_eq!(g.cell_at(Point::new(i32::from(x), g.area.bottom() as i32 + 1)), None);
    }

    /// However the layout text says, the items shown never overlap or
    /// leave the grid, and those without room are kept.
    #[test]
    fn placing_any_layout_keeps_items_apart_and_in_the_grid(
        items in proptest::collection::vec(
            (0usize..5, proptest::option::of((0u8..8, 0u8..8, 0u8..5, 0u8..5))),
            0..16,
        ),
    ) {
        let h = harness(Device::TOUCH);
        let env = h.env();
        // Each item once (placements of the widgets, tools of their own):
        // twins are another test's.
        let items: Vec<Item> = items
            .into_iter()
            .enumerate()
            .map(|(n, (kind, at))| {
                let n = n + 1;
                let what = match kind {
                    0 => widget(&format!("t.clock@{n}")),
                    1 => widget(&format!("t.tall@{n}")),
                    2 => widget(&format!("t.book@{n}")),
                    3 => What::Tool(Id::parse(&format!("t.tool{n}")).unwrap()),
                    _ => widget(&format!("t.unknown@{n}")),
                };
                match at {
                    Some((col, row, w, h)) => Item::placed(what, Cells::new(col, row, w, h)),
                    None => Item::new(what),
                }
            })
            .collect();
        let view = view(&env, &items);
        assert_tidy(&env, &view)?;
        prop_assert_eq!(whats(&view.items()), whats(&items), "every item is kept");
    }
}

#[test]
fn widgets_without_a_place_fill_the_width_and_tools_one_cell() {
    let h = harness(Device::TOUCH);
    let env = h.env();
    let g = geometry();
    let items = [
        Item::new(widget("t.clock")),
        Item::new(tool("t.a")),
        Item::new(widget("t.tall")),
    ];
    let placed = grid::place(&env, &g, &items);
    let cells: Vec<Cells> = placed.iter().map(|(_, c)| *c).collect();
    // 60 high (and the gap): a row; 250: three.
    assert_eq!(
        cells,
        [
            Cells::new(0, 0, 4, 1),
            Cells::new(0, 1, 1, 1),
            Cells::new(0, 2, 4, 3)
        ]
    );
    // Added where there is room, the others keeping their places.
    let added = grid::add(&env, &g, &items, tool("t.b")).unwrap();
    assert_eq!(added.last().unwrap().at, Some(Cells::new(1, 1, 1, 1)));
    // No room left for another tall one: none added.
    let full: Vec<Item> = (0..6)
        .map(|row| Item::placed(widget("t.clock"), Cells::new(0, row, 4, 1)))
        .collect();
    assert_eq!(grid::add(&env, &g, &full, widget("t.tall")), None);
}

/// Two copies of an item without a place (a gap, which a layout may hold
/// more than once) where there is room for one: the other is to be kept,
/// not shown (`GridView::layout`), but it is taken for the one placed
/// and dropped, so it is gone once the layout is kept.
#[test]
fn an_item_without_room_is_kept_even_when_its_twin_was_placed() {
    let h = harness(Device::TOUCH);
    let env = h.env();
    let mut items: Vec<Item> = (0..5)
        .map(|row| Item::placed(widget("t.clock"), Cells::new(0, row, 4, 1)))
        .collect();
    // One row left, for one of the two.
    items.push(Item::new(widget(paperpaw_sdk::widgets::SPACE)));
    items.push(Item::new(widget(paperpaw_sdk::widgets::SPACE)));
    let view = view(&env, &items);
    assert_eq!(placed(&env, &view).len(), 6);
    assert_eq!(view.items().len(), 7, "the one without room is kept");
}

// ---- Editing: touch and buttons ---------------------------------------

#[derive(Debug, Clone)]
enum Op {
    /// Drag item `n` (of those shown) by its body by cells.
    Drag(usize, i8, i8),
    /// Drag item `n`'s corner by cells.
    Size(usize, i8, i8),
    /// Move item `n` with the buttons.
    Move(usize, Vec<u8>),
    /// Size item `n` with the buttons.
    Resize(usize, Vec<u8>),
    Remove(usize),
}

fn op_strategy() -> impl Strategy<Value = Op> {
    let presses = proptest::collection::vec(0u8..4, 0..6);
    prop_oneof![
        (0usize..10, -4i8..4, -6i8..6).prop_map(|(n, x, y)| Op::Drag(n, x, y)),
        (0usize..10, -4i8..4, -6i8..6).prop_map(|(n, x, y)| Op::Size(n, x, y)),
        (0usize..10, presses.clone()).prop_map(|(n, p)| Op::Move(n, p)),
        (0usize..10, presses).prop_map(|(n, p)| Op::Resize(n, p)),
        (0usize..10).prop_map(Op::Remove),
    ]
}

/// The buttons that move an item: on the X4 the page buttons stand for
/// up and down.
const ARROWS: [Id; 4] = [
    actions::LEFT,
    actions::RIGHT,
    actions::PREV_PAGE,
    actions::DOWN,
];

fn shift(p: Point, g: &Geometry, dx: i8, dy: i8) -> Point {
    Point::new(
        p.x + i32::from(dx) * i32::from(g.cell_w),
        p.y + i32::from(dy) * i32::from(g.cell_h),
    )
}

fn start_layout() -> Vec<Item> {
    vec![
        Item::placed(widget("t.clock"), Cells::new(0, 0, 4, 1)),
        Item::placed(widget("t.book"), Cells::new(0, 1, 2, 2)),
        Item::placed(tool("t.a"), Cells::new(2, 1, 1, 1)),
        Item::placed(tool("t.b"), Cells::new(3, 1, 1, 1)),
        Item::placed(widget("t.tall"), Cells::new(0, 3, 4, 2)),
        Item::new(tool("t.c")),
    ]
}

/// [`start_layout`] without the item placed anew (rows 2 and 5 free
/// right of the book).
fn fixed_layout() -> Vec<Item> {
    let mut items = start_layout();
    items.pop();
    items
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(96)))]

    /// Any sequence of drags, sizes, moves and removals: the items shown
    /// never overlap or leave the grid, none is lost or doubled but those
    /// removed, each change the screen is told of is what the grid holds,
    /// and kept as text and read again, the layout comes back the same.
    #[test]
    fn edits_by_touch_and_buttons_keep_the_grid_tidy(
        ops in proptest::collection::vec(op_strategy(), 1..12),
    ) {
        let h = harness(Device::TOUCH);
        let env = h.env();
        let mut view = view(&env, &start_layout());
        view.set_editing(true);
        view.layout(&env, geometry(), &start_layout(), &[]);
        let mut expected = whats(&view.items());
        let g = *view.geometry();
        for op in ops {
            let shown = placed(&env, &view);
            if shown.is_empty() {
                break;
            }
            let resize = matches!(op, Op::Resize(..));
            let response = match op {
                Op::Drag(n, dx, dy) => {
                    let area = g.rect(shown[n % shown.len()].1);
                    let from = Point::new(i32::from(area.x) + 5, i32::from(area.y) + 5);
                    drag(&mut view, &env, from, shift(from, &g, dx, dy))
                }
                Op::Size(n, dx, dy) => {
                    let area = g.rect(shown[n % shown.len()].1);
                    let from = corner(area);
                    drag(&mut view, &env, from, shift(from, &g, dx, dy))
                }
                Op::Move(n, presses) | Op::Resize(n, presses) => {
                    let n = n % shown.len();
                    choose(&mut view, &env, n, if resize { "resize" } else { "move" });
                    for p in presses {
                        let response = press(&mut view, &env, &ARROWS[usize::from(p)]);
                        prop_assert_eq!(response, GridResponse::Handled);
                        assert_tidy(&env, &view)?;
                    }
                    press(&mut view, &env, &actions::CONFIRM)
                }
                Op::Remove(n) => {
                    let n = n % shown.len();
                    let gone = whats(&[Item::new(shown[n].0.clone())]).remove(0);
                    let at = expected.iter().position(|w| *w == gone).unwrap();
                    expected.remove(at);
                    choose(&mut view, &env, n, "remove")
                }
            };
            assert_tidy(&env, &view)?;
            prop_assert_eq!(&whats(&view.items()), &expected, "no item lost or doubled");
            if let GridResponse::Changed(items) = response {
                prop_assert_eq!(items, view.items());
            }
        }
        // Kept and read again.
        let text = grid::write(&view.items());
        let again = self::view(&env, &grid::parse(&text));
        prop_assert_eq!(placed(&env, &again), placed(&env, &view));
        prop_assert_eq!(whats(&again.items()), whats(&view.items()));
    }
}

#[test]
fn an_item_dragged_onto_another_stays_where_it_was() {
    let h = harness(Device::TOUCH);
    let env = h.env();
    let mut view = view(&env, &start_layout());
    view.set_editing(true);
    let g = geometry();
    let before = placed(&env, &view);
    // Tool a (2/1) onto tool b (3/1).
    let a = g.rect(Cells::new(2, 1, 1, 1));
    let from = Point::new(i32::from(a.x) + 5, i32::from(a.y) + 5);
    let response = drag(&mut view, &env, from, shift(from, &g, 1, 0));
    assert_eq!(response, GridResponse::Handled, "nothing changed");
    assert_eq!(placed(&env, &view), before);
    // Onto free cells (row 5): moved, and the screen is told.
    let response = drag(&mut view, &env, from, shift(from, &g, 0, 4));
    let GridResponse::Changed(items) = response else {
        panic!("{response:?}");
    };
    assert!(items.contains(&Item::placed(tool("t.a"), Cells::new(2, 5, 1, 1))));
}

#[test]
fn an_item_moved_by_buttons_into_another_says_so_and_stays() {
    let h = harness(Device::BUTTONS);
    let env = h.env();
    let mut view = view(&env, &fixed_layout());
    view.set_editing(true);
    view.layout(&env, geometry(), &fixed_layout(), &[]);
    // Tool a is the third item; moved right onto tool b.
    choose(&mut view, &env, 2, "move");
    let (response, output) = with_cx(&env, |cx| view.on_action(&actions::RIGHT, cx));
    assert_eq!(response, GridResponse::Handled);
    assert!(
        output
            .commands
            .iter()
            .any(|c| matches!(c, Command::Notify(_))),
        "the press does not seem lost"
    );
    let at = |view: &GridView, col, row| {
        placed(&env, view).contains(&(tool("t.a"), Cells::new(col, row, 1, 1)))
    };
    assert!(at(&view, 2, 1));
    // Down (the page button, as on the X4): free.
    press(&mut view, &env, &actions::NEXT_PAGE);
    assert!(at(&view, 2, 2));
    // Further down the tall widget is in the way; up the clock.
    let (_, output) = with_cx(&env, |cx| view.on_action(&actions::NEXT_PAGE, cx));
    assert!(at(&view, 2, 2));
    assert!(matches!(output.commands.as_slice(), [Command::Notify(_)]));
    press(&mut view, &env, &actions::UP);
    press(&mut view, &env, &actions::UP);
    assert!(at(&view, 2, 1));
    // Right to the last column, and past the grid's edge: nowhere, and
    // nothing to say (no other item is in the way).
    press(&mut view, &env, &actions::DOWN);
    press(&mut view, &env, &actions::RIGHT);
    assert!(at(&view, 3, 2));
    let (_, output) = with_cx(&env, |cx| view.on_action(&actions::RIGHT, cx));
    assert!(at(&view, 3, 2));
    assert!(output.commands.is_empty());
    // Confirm ends moving and tells the screen.
    let done = press(&mut view, &env, &actions::CONFIRM);
    assert_eq!(done, GridResponse::Changed(view.items()));
    // Now the arrows move the focus again, not the item.
    press(&mut view, &env, &actions::LEFT);
    assert!(at(&view, 3, 2));
}

/// Every item a finger reaches is reached by buttons too: tapped or
/// chosen with confirm, the same options are offered, for the same item.
#[test]
fn every_item_is_reached_by_buttons_as_by_touch_while_editing() {
    let h = harness(Device::BUTTONS);
    let env = h.env();
    let mut view = view(&env, &fixed_layout());
    view.set_editing(true);
    view.layout(&env, geometry(), &fixed_layout(), &[]);
    let shown = placed(&env, &view);
    let opened = |output: &Output| -> Option<String> {
        output.commands.iter().find_map(|c| match c {
            Command::OpenSlot(slot, params) if *slot == paperpaw_sdk::builtin::slots::CHOICE => {
                params.get(params::PATH).map(str::to_string)
            }
            _ => None,
        })
    };
    let g = geometry();
    let mut by_touch = Vec::new();
    for (_, cells) in &shown {
        let m = middle(g.rect(*cells));
        let mut output = Output::default();
        {
            let mut cx = Cx::new(&env, &mut output);
            view.on_pointer(&pointer(PointerPhase::Down, m.x, m.y), &mut cx);
            view.on_pointer(&pointer(PointerPhase::Up, m.x, m.y), &mut cx);
        }
        by_touch.push(opened(&output).expect("a tap offers the item's options"));
    }
    view.reset_focus();
    let mut by_buttons = Vec::new();
    for n in 0..shown.len() {
        if n > 0 {
            press(&mut view, &env, &actions::RIGHT);
        }
        let (_, output) = with_cx(&env, |cx| view.on_action(&actions::CONFIRM, cx));
        by_buttons.push(opened(&output).expect("confirm offers the item's options"));
    }
    assert_eq!(by_buttons, by_touch);
    let all: Vec<String> = (0..shown.len()).map(|n| n.to_string()).collect();
    assert_eq!(by_touch, all);
    // One more step goes round to the first.
    press(&mut view, &env, &actions::RIGHT);
    let (_, output) = with_cx(&env, |cx| view.on_action(&actions::CONFIRM, cx));
    assert_eq!(opened(&output).as_deref(), Some("0"));
}

/// Not editing, what a tap on a tool or a widget's target runs is what
/// confirm runs once the buttons moved the focus there.
#[test]
fn targets_run_the_same_by_tap_and_by_confirm() {
    let h = harness(Device::BUTTONS);
    let env = h.env();
    let mut view = view(&env, &fixed_layout());
    let buttons = [(Rect::new(0, 720, 240, 60), 7u16)];
    view.layout(&env, geometry(), &fixed_layout(), &buttons);
    let g = geometry();
    let ran = |output: &Output| output.commands.first().cloned();
    let mut by_tap = Vec::new();
    for at in [
        g.rect(Cells::new(0, 1, 2, 2)),
        g.rect(Cells::new(2, 1, 1, 1)),
        g.rect(Cells::new(3, 1, 1, 1)),
    ] {
        let (response, output) = with_cx(&env, |cx| {
            let m = middle(at);
            view.on_gesture(tap(m.x, m.y), cx)
        });
        assert_eq!(response, GridResponse::Handled);
        by_tap.push(ran(&output).unwrap());
    }
    assert_eq!(
        by_tap,
        [
            Command::run(OPEN),
            Command::run(Id::new_static("t.a")),
            Command::run(Id::new_static("t.b")),
        ]
    );
    // The screen's own button.
    let (response, _) = with_cx(&env, |cx| view.on_gesture(tap(100, 750), cx));
    assert_eq!(response, GridResponse::Button(7));
    // A tap on a widget without targets, or on nothing: not the grid's.
    let clock = middle(g.rect(Cells::new(0, 0, 4, 1)));
    let (response, _) = with_cx(&env, |cx| view.on_gesture(tap(clock.x, clock.y), cx));
    assert_eq!(response, GridResponse::Ignored);

    // By buttons: book, a, b, then the screen's button.
    view.reset_focus();
    let mut by_confirm = Vec::new();
    for n in 0..3 {
        if n > 0 {
            press(&mut view, &env, &actions::NEXT);
        }
        let (_, output) = with_cx(&env, |cx| view.on_action(&actions::CONFIRM, cx));
        by_confirm.push(ran(&output).unwrap());
    }
    assert_eq!(by_confirm, by_tap);
    press(&mut view, &env, &actions::NEXT);
    assert_eq!(
        press(&mut view, &env, &actions::CONFIRM),
        GridResponse::Button(7)
    );
}

#[test]
fn up_and_down_go_to_the_nearest_target_above_and_below() {
    let h = harness(Device::BUTTONS);
    let env = h.env();
    let items = [
        Item::placed(tool("t.a"), Cells::new(0, 0, 1, 1)),
        Item::placed(tool("t.b"), Cells::new(3, 0, 1, 1)),
        Item::placed(tool("t.c"), Cells::new(0, 2, 1, 1)),
        Item::placed(tool("t.d"), Cells::new(3, 2, 1, 1)),
    ];
    let mut view = view(&env, &items);
    let run = |view: &mut GridView| {
        let (_, output) = with_cx(&env, |cx| view.on_action(&actions::CONFIRM, cx));
        output.commands.first().cloned()
    };
    // b (top right), then down: d below it, not c.
    press(&mut view, &env, &actions::RIGHT);
    assert_eq!(run(&mut view), Some(Command::run(Id::new_static("t.b"))));
    press(&mut view, &env, &actions::DOWN);
    assert_eq!(run(&mut view), Some(Command::run(Id::new_static("t.d"))));
    // Nothing further down: it stays.
    press(&mut view, &env, &actions::DOWN);
    assert_eq!(run(&mut view), Some(Command::run(Id::new_static("t.d"))));
    press(&mut view, &env, &actions::UP);
    assert_eq!(run(&mut view), Some(Command::run(Id::new_static("t.b"))));
}

#[test]
fn a_focus_step_sets_its_refresh_exactly() {
    // Not turned into grays, nor into a full refresh by the kernel because
    // the rectangle round the two targets is large and a picture lies in it.
    let h = harness(Device::BUTTONS);
    let env = h.env();
    let items = [
        Item::placed(tool("t.a"), Cells::new(0, 0, 1, 1)),
        Item::placed(tool("t.b"), Cells::new(3, 2, 1, 1)),
    ];
    let mut view = view(&env, &items);
    let (_, output) = with_cx(&env, |cx| view.on_action(&actions::RIGHT, cx));
    assert_eq!(output.refresh_set, Some(paperpaw_sdk::RefreshMode::Fast));
}

#[test]
fn a_long_press_begins_editing_and_back_ends_it() {
    let h = harness(Device::TOUCH);
    let env = h.env();
    let mut view = view(&env, &start_layout());
    assert!(!view.editing());
    let mut long = tap(10, 150);
    long.kind = GestureKind::LongPress;
    let (response, _) = with_cx(&env, |cx| view.on_gesture(long, cx));
    assert_eq!(response, GridResponse::Editing(true));
    assert!(view.editing() && view.holds_edges());
    // Not editing, the grid leaves the fingers to the screen.
    let (response, _) = with_cx(&env, |cx| view.on_gesture(long, cx));
    assert_eq!(response, GridResponse::Ignored);
    assert_eq!(
        press(&mut view, &env, &actions::BACK),
        GridResponse::Editing(false)
    );
    assert!(!view.editing());
    let (response, _) = with_cx(&env, |cx| {
        view.on_pointer(&pointer(PointerPhase::Down, 10, 150), cx)
    });
    assert_eq!(response, GridResponse::Ignored);
}

#[test]
fn options_for_items_that_are_not_there_do_nothing() {
    let h = harness(Device::TOUCH);
    let env = h.env();
    let mut view = view(&env, &start_layout());
    view.set_editing(true);
    let before = view.items();
    for path in ["99", "-1", "x", ""] {
        let params = Params::new()
            .with(params::PATH, path.to_string())
            .with(OPTION, "remove");
        let event = Id::parse(CHOSEN.as_str()).unwrap();
        let (response, _) = with_cx(&env, |cx| view.on_event(&event, &params, cx));
        assert_eq!(response, GridResponse::Handled, "{path:?}");
    }
    assert_eq!(view.items(), before);
    // Another event: not the grid's.
    let other = Id::new_static("t.other");
    let (response, _) = with_cx(&env, |cx| view.on_event(&other, &Params::new(), cx));
    assert_eq!(response, GridResponse::Ignored);
}

#[test]
fn the_grid_draws_inside_its_area_while_edited() {
    let h = harness(Device::TOUCH);
    let env = h.env();
    let mut view = view(&env, &start_layout());
    view.set_editing(true);
    view.set_focus_shown(true);
    view.layout(&env, geometry(), &start_layout(), &[]);
    // A drag under way, onto free cells.
    let g = geometry();
    let a = g.rect(Cells::new(2, 1, 1, 1));
    let from = Point::new(i32::from(a.x) + 5, i32::from(a.y) + 5);
    let to = shift(from, &g, 0, 4);
    with_cx(&env, |cx| {
        view.on_pointer(&pointer(PointerPhase::Down, from.x, from.y), cx);
        view.on_pointer(&pointer(PointerPhase::Move, to.x, to.y), cx);
    });
    let mut frame = frame(480, 800);
    view.draw(&mut frame.canvas(), &env);
    // The focus frame may reach a little past the edge items' cells.
    let reach = (u32::from(paperpaw_sdk::ui::UiStyle::new(&env).line()) * 5) as u16;
    let bound = Rect::new(0, g.area.y - reach, g.area.width, g.area.height + 2 * reach);
    assert_eq!(drawn_outside(&mut frame, bound), None);
}

thread_local! {
    /// What the lively widget was told.
    static TOLD: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn told() -> Vec<String> {
    TOLD.with(|t| t.borrow_mut().drain(..).collect())
}

fn tell(what: String) {
    TOLD.with(|t| t.borrow_mut().push(what));
}

/// A widget that takes swipes, runs something of its own on a long
/// press, draws its own focus and notes where it is shown.
struct Lively;

impl Widget for Lively {
    fn height(&self, _: &Env<'_>, _: u16) -> u16 {
        90
    }

    fn draw(&self, _: &mut Canvas<'_>, _: &Env<'_>, _: Rect) {}

    fn targets(&self, _: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        vec![
            WidgetTarget::new(area, Command::run(OPEN))
                .hold(Command::run(Id::new_static("t.hold"))),
        ]
    }

    fn context(&mut self, _: &Env<'_>, context: &paperpaw_sdk::widgets::WidgetContext<'_>) {
        tell(format!(
            "context {} {}",
            context.placement.unwrap_or("-"),
            context.host.unwrap_or("-")
        ));
    }

    fn on_input(
        &mut self,
        _: &Env<'_>,
        input: paperpaw_sdk::widgets::WidgetInput,
        _: Rect,
        _: &mut Commands,
    ) -> bool {
        use paperpaw_sdk::widgets::WidgetInput;
        match input {
            // Its pages go one way only.
            WidgetInput::Swipe(paperpaw_sdk::Direction::Left) => {
                tell("swipe Left".into());
                true
            }
            WidgetInput::Focus(at) => {
                tell(format!("focus {}", at.is_some()));
                false
            }
            _ => false,
        }
    }

    fn draw_focus(&self, _: &mut Canvas<'_>, _: &Env<'_>, _: Rect) -> bool {
        tell("own focus".into());
        true
    }
}

/// A widget is told where it shows, takes swipes that start on it, runs
/// its own long press, hears the focus come and go, and draws its own.
#[test]
fn widgets_take_swipes_holds_and_the_focus() {
    let mut h = harness(Device::BUTTONS);
    h.widgets.add(WidgetSpec::new(
        Id::new_static("t.lively"),
        "t.lively",
        |_| Box::new(Lively),
    ));
    let env = h.env();
    let items = [
        Item::placed(widget("t.lively@2"), Cells::new(0, 0, 2, 2)),
        Item::placed(tool("t.a"), Cells::new(2, 0, 1, 1)),
    ];
    let mut view = GridView::asking(&env, CHOSEN);
    view.set_host(Id::new_static("t.screen"));
    view.layout(&env, geometry(), &items, &[]);
    assert_eq!(told(), ["context t.lively@2 t.screen"]);
    let area = geometry().rect(Cells::new(0, 0, 2, 2));
    let m = middle(area);
    let gesture = |kind| {
        paperpaw_sdk::Gesture::new(
            kind,
            m,
            Point::new(m.x - 100, m.y),
            paperpaw_platform::PointerTool::Finger,
        )
    };
    let (response, _) = with_cx(&env, |cx| {
        view.on_gesture(
            gesture(paperpaw_sdk::GestureKind::Swipe(
                paperpaw_sdk::Direction::Left,
            )),
            cx,
        )
    });
    assert_eq!(response, GridResponse::Handled);
    assert_eq!(told(), ["swipe Left"]);
    let (response, output) = with_cx(&env, |cx| {
        view.on_gesture(gesture(paperpaw_sdk::GestureKind::LongPress), cx)
    });
    assert_eq!(response, GridResponse::Handled);
    assert_eq!(output.commands, [Command::run(Id::new_static("t.hold"))]);
    assert!(!view.editing());
    // By the buttons: the next page swipes it (it takes that), its long
    // press is Select held; Right moves the focus to the tool, Left back:
    // told each time; drawn its own way.
    press(&mut view, &env, &actions::NEXT_PAGE);
    assert_eq!(told(), ["swipe Left"]);
    let (_, output) = with_cx(&env, |cx| view.on_action(&actions::LONG_CONFIRM, cx));
    assert_eq!(output.commands, [Command::run(Id::new_static("t.hold"))]);
    press(&mut view, &env, &actions::RIGHT);
    assert_eq!(told(), ["focus false"]);
    press(&mut view, &env, &actions::LEFT);
    assert_eq!(told(), ["focus true"]);
    let mut frame = support::frame(480, 800);
    view.draw(&mut frame.canvas(), &env);
    assert_eq!(told(), ["own focus"]);
}

/// Never onto the top row: the screen's veto.
fn not_on_top(_: &Env<'_>, _: &[Item], _: &Item, to: Cells) -> bool {
    to.row > 0
}

/// The screen's own options come after the grid's and are answered to
/// it; the screen's check keeps an item from going where it says no.
#[test]
fn the_screen_adds_options_and_vetoes_drops() {
    let h = harness(Device::BUTTONS);
    let env = h.env();
    let items = [Item::placed(tool("t.a"), Cells::new(0, 1, 1, 1))];
    let mut view = GridView::asking(&env, CHOSEN);
    view.set_options(&["t.duplicate"]);
    view.set_drop_check(not_on_top);
    view.set_editing(true);
    view.layout(&env, geometry(), &items, &[]);
    // Its own option, answered with the item.
    let picked = Picked::new("0".to_string(), "t.duplicate".to_string());
    let event = Id::parse(CHOSEN.as_str()).unwrap();
    let (response, _) = with_cx(&env, |cx| view.on_event(&event, &picked.to_params(), cx));
    assert_eq!(
        response,
        GridResponse::Option {
            item: Item::placed(tool("t.a"), Cells::new(0, 1, 1, 1)),
            option: "t.duplicate".to_string()
        }
    );
    // Moved up by the buttons: vetoed, it stays; down it goes.
    choose(&mut view, &env, 0, "move");
    press(&mut view, &env, &actions::UP);
    assert_eq!(placed(&env, &view)[0].1, Cells::new(0, 1, 1, 1));
    press(&mut view, &env, &actions::DOWN);
    assert_eq!(placed(&env, &view)[0].1, Cells::new(0, 2, 1, 1));
}

/// A target tapped only keeps its long press.
struct Touchy;

impl Widget for Touchy {
    fn height(&self, _: &Env<'_>, _: u16) -> u16 {
        90
    }

    fn draw(&self, _: &mut Canvas<'_>, _: &Env<'_>, _: Rect) {}

    fn targets(&self, _: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        vec![
            WidgetTarget::touch(area, Command::run(OPEN))
                .hold(Command::run(Id::new_static("t.hold"))),
        ]
    }
}

#[test]
fn a_touch_target_keeps_its_long_press() {
    let mut h = harness(Device::TOUCH);
    h.widgets.add(WidgetSpec::new(
        Id::new_static("t.touchy"),
        "t.touchy",
        |_| Box::new(Touchy),
    ));
    let env = h.env();
    let items = [Item::placed(widget("t.touchy"), Cells::new(0, 0, 2, 2))];
    let view = &mut GridView::asking(&env, CHOSEN);
    view.layout(&env, geometry(), &items, &[]);
    let m = middle(geometry().rect(Cells::new(0, 0, 2, 2)));
    let hold = paperpaw_sdk::Gesture::new(
        paperpaw_sdk::GestureKind::LongPress,
        m,
        m,
        paperpaw_platform::PointerTool::Finger,
    );
    let (response, output) = with_cx(&env, |cx| view.on_gesture(hold, cx));
    assert_eq!(response, GridResponse::Handled);
    assert_eq!(output.commands, [Command::run(Id::new_static("t.hold"))]);
    assert!(!view.editing());
}

/// Two pages of two targets side by side, turned by swipes; `endless`
/// takes every swipe (a calendar going on and on).
#[derive(Default)]
struct Paged {
    page: usize,
    endless: bool,
}

impl Widget for Paged {
    fn height(&self, _: &Env<'_>, _: u16) -> u16 {
        90
    }

    fn draw(&self, _: &mut Canvas<'_>, _: &Env<'_>, _: Rect) {}

    fn targets(&self, _: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        let half = area.width / 2;
        (0..2)
            .map(|i| {
                let at = Rect::new(area.x + i * half, area.y, half, area.height);
                let action = Id::parse(&format!("t.p{}-{i}", self.page)).unwrap();
                WidgetTarget::new(at, Command::run(action))
            })
            .collect()
    }

    fn on_input(
        &mut self,
        _: &Env<'_>,
        input: paperpaw_sdk::widgets::WidgetInput,
        _: Rect,
        _: &mut Commands,
    ) -> bool {
        use paperpaw_sdk::widgets::WidgetInput;
        match input {
            WidgetInput::Swipe(_) if self.endless => self.page = (self.page + 1) % 2,
            WidgetInput::Swipe(paperpaw_sdk::Direction::Left) if self.page == 0 => self.page = 1,
            WidgetInput::Swipe(paperpaw_sdk::Direction::Right) if self.page == 1 => self.page = 0,
            _ => return false,
        }
        true
    }
}

/// A grid of a paged widget (`endless` or not) above a tool.
fn paged(h: &mut Harness, endless: bool) {
    let make: paperpaw_sdk::widgets::WidgetFactory = if endless {
        |_| {
            Box::new(Paged {
                page: 0,
                endless: true,
            })
        }
    } else {
        |_| {
            Box::new(Paged {
                page: 0,
                endless: false,
            })
        }
    };
    h.widgets
        .add(WidgetSpec::new(Id::new_static("t.paged"), "t.paged", make));
}

fn paged_items() -> [Item; 2] {
    [
        Item::placed(widget("t.paged"), Cells::new(0, 0, 4, 1)),
        Item::placed(tool("t.a"), Cells::new(0, 1, 1, 1)),
    ]
}

fn confirmed(view: &mut GridView, env: &Env<'_>) -> Option<Command> {
    let (_, output) = with_cx(env, |cx| view.on_action(&actions::CONFIRM, cx));
    output.commands.first().cloned()
}

fn ran(id: &'static str) -> Option<Command> {
    Some(Command::run(Id::new_static(id)))
}

/// With the buttons Left and Right go through a widget's targets and on
/// out of it; the page buttons turn its pages while it has the focus
/// (Select then runs what shows), the screen's where it takes none.
#[test]
fn buttons_go_through_a_widget_and_page_it_with_the_page_buttons() {
    let mut h = harness(Device::BUTTONS);
    paged(&mut h, false);
    let env = h.env();
    let mut view = GridView::asking(&env, CHOSEN);
    view.layout(&env, geometry(), &paged_items(), &[]);
    assert_eq!(confirmed(&mut view, &env), ran("t.p0-0"));
    press(&mut view, &env, &actions::RIGHT);
    assert_eq!(confirmed(&mut view, &env), ran("t.p0-1"), "its next target");
    press(&mut view, &env, &actions::RIGHT);
    assert_eq!(confirmed(&mut view, &env), ran("t.a"), "then out of it");
    press(&mut view, &env, &actions::LEFT);
    assert_eq!(
        press(&mut view, &env, &actions::NEXT_PAGE),
        GridResponse::Handled
    );
    assert_eq!(
        confirmed(&mut view, &env),
        ran("t.p1-0"),
        "its next page, what shows"
    );
    assert_eq!(
        press(&mut view, &env, &actions::NEXT_PAGE),
        GridResponse::Ignored,
        "at its last page the screen's"
    );
    press(&mut view, &env, &actions::PREV_PAGE);
    assert_eq!(confirmed(&mut view, &env), ran("t.p0-0"));
}

/// A widget that takes every swipe never keeps the focus: Left and Right
/// leave it after its last target, on a device with no Up and Down (the
/// X4) and a single page.
#[test]
fn a_widget_paging_without_end_never_keeps_the_focus() {
    let mut h = harness(Device::BUTTONS);
    paged(&mut h, true);
    let env = h.env();
    let mut view = GridView::asking(&env, CHOSEN);
    view.layout(&env, geometry(), &paged_items(), &[]);
    for _ in 0..2 {
        press(&mut view, &env, &actions::RIGHT);
    }
    assert_eq!(confirmed(&mut view, &env), ran("t.a"));
    for _ in 0..3 {
        press(&mut view, &env, &actions::RIGHT);
    }
    assert_eq!(
        confirmed(&mut view, &env),
        ran("t.a"),
        "round and out again"
    );
    press(&mut view, &env, &actions::LEFT);
    press(&mut view, &env, &actions::NEXT_PAGE);
    for _ in 0..2 {
        press(&mut view, &env, &actions::RIGHT);
    }
    assert_eq!(confirmed(&mut view, &env), ran("t.a"), "paged, still out");
}

/// A widget alone on a page (its targets the only ones) leaves the page
/// buttons to the screen, so its other pages stay in reach; Left and
/// Right page it at its first and last target.
#[test]
fn a_widget_alone_leaves_the_page_buttons_to_the_screen() {
    let mut h = harness(Device::BUTTONS);
    paged(&mut h, true);
    let env = h.env();
    let mut view = GridView::asking(&env, CHOSEN);
    let items = [Item::placed(widget("t.paged"), Cells::new(0, 0, 4, 1))];
    view.layout(&env, geometry(), &items, &[]);
    assert_eq!(confirmed(&mut view, &env), ran("t.p0-0"));
    for page in [actions::NEXT_PAGE, actions::PREV_PAGE] {
        assert_eq!(press(&mut view, &env, &page), GridResponse::Ignored);
    }
    press(&mut view, &env, &actions::RIGHT);
    assert_eq!(confirmed(&mut view, &env), ran("t.p0-1"));
    press(&mut view, &env, &actions::RIGHT);
    assert_eq!(
        confirmed(&mut view, &env),
        ran("t.p1-0"),
        "paged at its last"
    );
    press(&mut view, &env, &actions::LEFT);
    assert_eq!(
        confirmed(&mut view, &env),
        ran("t.p0-0"),
        "paged back at its first"
    );
}

/// A swipe on a widget while the focus is elsewhere turns its page and
/// leaves the focus where it was; with the focus on it, the focus goes to
/// its first target and it is told.
#[test]
fn a_swipe_on_a_widget_leaves_the_focus_where_it_was() {
    let mut h = harness(Device::BUTTONS);
    paged(&mut h, false);
    let env = h.env();
    let mut view = GridView::asking(&env, CHOSEN);
    view.layout(&env, geometry(), &paged_items(), &[]);
    for _ in 0..2 {
        press(&mut view, &env, &actions::RIGHT);
    }
    assert_eq!(confirmed(&mut view, &env), ran("t.a"));
    let m = middle(geometry().rect(Cells::new(0, 0, 4, 1)));
    let swipe = paperpaw_sdk::Gesture::new(
        paperpaw_sdk::GestureKind::Swipe(paperpaw_sdk::Direction::Left),
        m,
        Point::new(m.x - 100, m.y),
        paperpaw_platform::PointerTool::Finger,
    );
    let (response, output) = with_cx(&env, |cx| view.on_gesture(swipe, cx));
    assert_eq!(response, GridResponse::Handled);
    assert!(output.commands.is_empty());
    assert_eq!(confirmed(&mut view, &env), ran("t.a"), "the focus stays");
    press(&mut view, &env, &actions::LEFT);
    assert_eq!(confirmed(&mut view, &env), ran("t.p1-1"), "its page turned");
}

/// Dragging an item, by its middle to move it or by its corner to size
/// it: a finger trembling in its cells asks for no refresh (each takes
/// most of a second on some panels), a move into another cell (or size)
/// for one.
#[test]
fn a_drag_refreshes_only_where_the_cells_change() {
    let h = harness(Device::TOUCH);
    let env = h.env();
    let items = [Item::placed(tool("t.a"), Cells::new(0, 0, 1, 1))];
    for resize in [false, true] {
        let mut view = GridView::asking(&env, CHOSEN);
        view.set_editing(true);
        view.layout(&env, geometry(), &items, &[]);
        let area = geometry().rect(Cells::new(0, 0, 1, 1));
        let start = if resize { corner(area) } else { middle(area) };
        let refreshes = |view: &mut GridView, phase, at: Point| {
            let (_, output) = with_cx(&env, |cx| view.on_pointer(&pointer(phase, at.x, at.y), cx));
            usize::from(output.refresh.is_some())
        };
        let mut asked = refreshes(&mut view, PointerPhase::Down, start);
        for n in 1..=10 {
            let trembling = Point::new(start.x - n % 3, start.y - n % 2);
            asked += refreshes(&mut view, PointerPhase::Move, trembling);
        }
        assert_eq!(asked, 0, "resize {resize}");
        let next = Point::new(start.x + 120, start.y);
        assert_eq!(
            refreshes(&mut view, PointerPhase::Move, next),
            1,
            "resize {resize}"
        );
        assert_eq!(
            refreshes(&mut view, PointerPhase::Move, next),
            0,
            "resize {resize}"
        );
    }
}
