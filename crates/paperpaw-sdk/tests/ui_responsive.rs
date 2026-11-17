//! The helpers that adapt a screen to its size (#88): the readable column,
//! the columns of a cover grid and the columns of a home grid in
//! landscape and a grid laid out anew for another one; the choice and the question in that column. On the X4 in
//! both orientations, on a tablet, and where the interface runs right to
//! left.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod support;

use paperpaw_platform::Rotation;
use paperpaw_sdk::ui::{self, ListView, UiStyle};
use paperpaw_sdk::widgets::grid::{self, Cells, Geometry, Item, MAX_COLUMNS, What};
use paperpaw_sdk::{DeviceInfo, DisplayInfo, Id, InputCaps, MemoryClass, PixelFormat, Point, Rect};

use support::{Device, Harness};

/// The X4 upright, the X4 on its side, a small tablet and a large one, each
/// upright and on its side: width, height, dots per inch.
const X4: (u16, u16, u16) = (480, 800, 220);
const X4_SIDE: (u16, u16, u16) = (800, 480, 220);
const TABLET: (u16, u16, u16) = (1200, 1920, 220);
const TABLET_SIDE: (u16, u16, u16) = (1920, 1200, 220);
const LARGE: (u16, u16, u16) = (1600, 2560, 280);
const LARGE_SIDE: (u16, u16, u16) = (2560, 1600, 280);

/// A device `width` by `height` pixels at `dpi`.
fn harness((width, height, dpi): (u16, u16, u16)) -> Harness {
    let mut h = Harness::new(Device::TOUCH);
    h.device = DeviceInfo::new(
        "test",
        "Test device",
        DisplayInfo::new(width, height, dpi, PixelFormat::Gray8, Rotation::Deg0, None),
        InputCaps::new(true, true, false, &[]),
        MemoryClass::Large,
    );
    h
}

fn whole((width, height, _): (u16, u16, u16)) -> Rect {
    Rect::new(0, 0, width, height)
}

#[test]
fn the_x4_is_never_cut_in_either_orientation() {
    for device in [X4, X4_SIDE] {
        let h = harness(device);
        let env = h.env();
        let style = UiStyle::new(&env);
        assert_eq!(ui::readable(&env, &style, whole(device)), whole(device));
    }
}

/// The column's width for `style`: 110 mm or 36 text heights, whichever is more.
fn column_width(env: &paperpaw_sdk::Env<'_>, style: &UiStyle) -> u16 {
    env.metrics()
        .mm(110)
        .max((style.text() * 36.0) as u32)
        .try_into()
        .unwrap()
}

#[test]
fn a_screen_that_is_not_wider_than_the_column_is_not_cut() {
    // 36 text heights at the default size are wider than a small tablet
    // upright: it has nothing to cut.
    for device in [TABLET, LARGE] {
        let h = harness(device);
        let env = h.env();
        let style = UiStyle::new(&env);
        let column = ui::readable(&env, &style, whole(device));
        assert_eq!(
            column.width,
            whole(device).width.min(column_width(&env, &style))
        );
    }
}

#[test]
fn a_wide_screen_gets_a_column_in_its_middle() {
    for device in [TABLET_SIDE, LARGE_SIDE] {
        let h = harness(device);
        let env = h.env();
        let style = UiStyle::new(&env);
        let area = whole(device);
        let column = ui::readable(&env, &style, area);
        assert_eq!(column.width, column_width(&env, &style), "{device:?}");
        assert!(u32::from(column.width) >= env.metrics().mm(110));
        assert!(column.width < area.width, "{device:?}");
        // In the middle to a pixel, as high as the area.
        assert!(column.x.abs_diff(area.width - column.right() as u16) <= 1);
        assert_eq!((column.y, column.height), (area.y, area.height));
    }
}

#[test]
fn a_column_is_at_least_the_millimetres_where_the_text_is_small() {
    let h = harness(TABLET_SIDE);
    let env = h.env();
    let style = UiStyle::new(&env).with_text(10.0);
    let column = ui::readable(&env, &style, whole(TABLET_SIDE));
    assert_eq!(u32::from(column.width), env.metrics().mm(110));
}

#[test]
fn an_area_that_does_not_start_at_the_edge_keeps_its_place() {
    let h = harness(TABLET_SIDE);
    let env = h.env();
    let style = UiStyle::new(&env);
    let area = Rect::new(100, 64, 1700, 1000);
    let column = ui::readable(&env, &style, area);
    assert_eq!((column.y, column.height), (64, 1000));
    assert!(column.x > 100 && column.right() < area.right());
    assert_eq!(column.x - 100, area.right() as u16 - column.right() as u16);
}

#[test]
fn larger_text_makes_the_column_wider_but_not_wider_than_the_area() {
    let h = harness(LARGE_SIDE);
    let env = h.env();
    let style = UiStyle::new(&env);
    let area = whole(LARGE_SIDE);
    let normal = ui::readable(&env, &style, area).width;
    let large = ui::readable(&env, &style.with_text(style.text() * 1.5), area).width;
    assert!(large > normal);
    let huge = ui::readable(&env, &style.with_text(style.text() * 100.0), area);
    assert_eq!(huge, area);
    // A zero-width area is not a column.
    let nothing = ui::readable(&env, &style, Rect::new(5, 6, 0, 0));
    assert_eq!(nothing, Rect::new(5, 6, 0, 0));
}

#[test]
fn the_column_is_the_same_where_the_interface_runs_right_to_left() {
    let h = harness(TABLET_SIDE);
    let env = h.env();
    let ltr = UiStyle::new(&env);
    let rtl = ltr.with_rtl(true);
    let area = whole(TABLET_SIDE);
    let column = ui::readable(&env, &rtl, area);
    assert_eq!(column, ui::readable(&env, &ltr, area));
    // Rows of the list in it are the mirror image of the left-to-right
    // ones, inside the column and not the screen.
    let list = ListView::readable(&env, &rtl, area, 80).columns(3);
    assert_eq!(list.area(), column);
    let first = list.row_rect(0);
    let mirrored = rtl.flip(list.area(), first);
    assert_eq!(
        mirrored.x - column.x,
        column.right() as u16 - first.right() as u16
    );
    assert_eq!(mirrored.width, first.width);
}

#[test]
fn a_list_in_the_column_pages_and_hits_as_any_list() {
    let h = harness(TABLET_SIDE);
    let env = h.env();
    let style = UiStyle::new(&env);
    let area = Rect::new(0, 100, 1920, 800);
    let list = ListView::readable(&env, &style, area, 100);
    assert_eq!(list.area(), ui::readable(&env, &style, area));
    assert_eq!(list.rows(), 8);
    assert_eq!(list.pages(20), 3);
    let inside = list.row_rect(2);
    let point = Point::new(i32::from(inside.x) + 3, i32::from(inside.y) + 3);
    assert_eq!(list.hit(point, 20), Some(2));
    // Outside the column is not a row: nothing there to tap.
    assert_eq!(list.hit(Point::new(5, 150), 20), None);
    assert_eq!(list.hit(Point::new(1915, 150), 20), None);
    // On a narrow screen it is the plain list.
    let narrow = harness(X4);
    let env = narrow.env();
    let area = Rect::new(0, 64, 480, 700);
    assert_eq!(
        ListView::readable(&env, &UiStyle::new(&env), area, 90),
        ListView::new(area, 90)
    );
}

#[test]
fn a_cover_grid_gets_the_columns_that_fit() {
    // 22 mm is 190 pixels at 220 dpi.
    for (device, columns) in [
        (X4, 2),
        (X4_SIDE, 4),
        (TABLET, 6),
        (TABLET_SIDE, 10),
        (LARGE, 6),
        (LARGE_SIDE, 10),
    ] {
        let h = harness(device);
        let env = h.env();
        let fit = ListView::fit_columns(&env, u32::from(whole(device).width), 22);
        assert_eq!(fit, columns, "{device:?}");
        // Every column is at least as wide as asked, and one more would not be.
        let width = u32::from(whole(device).width);
        let cell = env.metrics().mm(22);
        assert!(width / fit >= cell && width / (fit + 1) < cell);
    }
}

#[test]
fn a_grid_is_at_least_one_column_even_where_nothing_fits() {
    let h = harness(X4);
    let env = h.env();
    assert_eq!(ListView::fit_columns(&env, 100, 22), 1);
    assert_eq!(ListView::fit_columns(&env, 0, 22), 1);
    // No size asked for: as many as there are pixels, not a crash.
    assert!(ListView::fit_columns(&env, 480, 0) <= u32::from(u16::MAX));
    assert!(ListView::fit_columns(&env, 480, 0) >= 1);
}

#[test]
fn a_kept_list_changes_its_columns_and_its_area_and_keeps_its_item() {
    let h = harness(X4);
    let env = h.env();
    let mut list = ListView::new(Rect::new(0, 64, 480, 700), 200).columns(2);
    list.set_focus(5);
    assert_eq!((list.column_count(), list.rows(), list.page()), (2, 6, 0));
    // On its side: wider, lower, more columns.
    list.set_area(Rect::new(0, 64, 800, 400));
    list.set_columns(ListView::fit_columns(
        &env,
        u32::from(list.area().width),
        22,
    ));
    assert_eq!(list.column_count(), 4);
    assert_eq!(list.focus(), 5);
    assert_eq!((list.rows(), list.page()), (8, 0));
    // Never zero, never more than a width can hold.
    list.set_columns(0);
    assert_eq!(list.column_count(), 1);
    list.set_columns(u32::MAX);
    assert_eq!(list.column_count(), usize::from(u16::MAX));
}

#[test]
fn a_home_grid_gets_more_columns_on_its_side() {
    // Upright, or square: as many as were made for.
    assert_eq!(grid::columns_for(whole(X4), 4), 4);
    assert_eq!(grid::columns_for(Rect::new(0, 0, 600, 600), 4), 4);
    // The X4 on its side: 4 times 800/480 is 6.7, 7, at most 6.
    assert_eq!(grid::columns_for(whole(X4_SIDE), 4), 6);
    assert_eq!(grid::columns_for(whole(X4_SIDE), 3), 5);
    assert_eq!(grid::columns_for(whole(X4_SIDE), 2), 3);
    assert_eq!(grid::columns_for(whole(TABLET_SIDE), 4), 6);
    // A little wider than high: no fewer than were made for.
    assert_eq!(grid::columns_for(Rect::new(0, 0, 520, 480), 4), 4);
    // Nonsense in, something usable out.
    assert_eq!(grid::columns_for(Rect::new(0, 0, 100, 0), 0), MAX_COLUMNS);
    assert_eq!(grid::columns_for(Rect::default(), 9), MAX_COLUMNS);
    assert_eq!(grid::columns_for(whole(X4), 0), 1);
    for columns in 0..=255u8 {
        for area in [whole(X4), whole(X4_SIDE), Rect::new(0, 0, u16::MAX, 1)] {
            let n = grid::columns_for(area, columns);
            assert!((1..=MAX_COLUMNS).contains(&n));
        }
    }
}

fn tool(name: &str, at: Option<Cells>) -> Item {
    let what = What::Tool(Id::parse(name).unwrap());
    at.map_or_else(
        || Item::new(what.clone()),
        |c| Item::placed(what.clone(), c),
    )
}

fn widget(name: &str, at: Cells) -> Item {
    Item::placed(What::Widget(name.to_string()), at)
}

/// What an item is and where it was put, for comparing.
fn cells(items: &[Item]) -> Vec<(String, Option<Cells>)> {
    items
        .iter()
        .map(|i| {
            let name = match &i.what {
                What::Tool(id) => id.as_str().to_string(),
                What::Widget(id) => id.clone(),
                other => panic!("{other:?}"),
            };
            (name, i.at)
        })
        .collect()
}

#[test]
fn a_grid_laid_out_anew_fills_from_the_top_left_in_the_order_given() {
    let h = harness(X4_SIDE);
    let env = h.env();
    let items = [
        tool("t.a", Some(Cells::new(0, 0, 1, 1))),
        tool("t.b", Some(Cells::new(3, 0, 1, 1))),
        tool("t.c", Some(Cells::new(0, 1, 1, 1))),
        tool("t.z", None),
    ];
    let wide = Geometry::of(whole(X4_SIDE), 6, 3, 8);
    let (placed, left) = grid::refit(&env, &wide, 4, u8::MAX, &items);
    assert!(left.is_empty());
    let at = |col, row| Some(Cells::new(col, row, 1, 1));
    let expect: Vec<_> = [("t.a", 0), ("t.b", 1), ("t.c", 2), ("t.z", 3)]
        .iter()
        .map(|(n, i)| ((*n).to_string(), at(*i, 0)))
        .collect();
    assert_eq!(cells(&placed), expect);
}

#[test]
fn a_widget_keeps_its_share_of_the_width_and_a_tool_its_size() {
    let h = harness(X4_SIDE);
    let env = h.env();
    // On 4 columns: one as wide as the grid, one of half, a tool.
    let items = [
        widget("t.full", Cells::new(0, 0, 4, 1)),
        widget("t.half", Cells::new(0, 1, 2, 1)),
        tool("t.tool", Some(Cells::new(2, 1, 1, 1))),
    ];
    let wide = Geometry::of(whole(X4_SIDE), 6, 4, 8);
    let (placed, left) = grid::refit(&env, &wide, 4, u8::MAX, &items);
    assert!(left.is_empty());
    assert_eq!(
        placed[0].at,
        Some(Cells::new(0, 0, 6, 1)),
        "as wide as the grid"
    );
    assert_eq!(placed[1].at, Some(Cells::new(0, 1, 3, 1)), "half of it");
    assert_eq!(
        placed[2].at,
        Some(Cells::new(3, 1, 1, 1)),
        "a tool is a cell"
    );
    // Fewer columns: shares shrink the same way, never below a column.
    let narrow = Geometry::of(whole(X4), 2, 4, 8);
    let (placed, _) = grid::refit(&env, &narrow, 4, u8::MAX, &items);
    assert_eq!(placed[0].at, Some(Cells::new(0, 0, 2, 1)));
    assert_eq!(placed[1].at, Some(Cells::new(0, 1, 1, 1)));
}

#[test]
fn no_widget_is_wider_than_the_most_it_may_take_and_tools_follow_beside_it() {
    let h = harness(X4_SIDE);
    let env = h.env();
    let items = [
        widget("t.full", Cells::new(0, 0, 4, 1)),
        widget("t.half", Cells::new(0, 1, 2, 1)),
        tool("t.a", Some(Cells::new(2, 1, 1, 1))),
        tool("t.b", None),
    ];
    let wide = Geometry::of(whole(X4_SIDE), 6, 3, 8);
    let (placed, left) = grid::refit(&env, &wide, 4, 3, &items);
    assert!(left.is_empty());
    assert_eq!(
        placed[0].at,
        Some(Cells::new(0, 0, 3, 1)),
        "as wide as the most"
    );
    // The room beside the first widget is used by what follows: the
    // second, of half the grid, and under them the tools.
    assert_eq!(placed[1].at, Some(Cells::new(3, 0, 3, 1)), "half is 3 of 6");
    assert_eq!(placed[2].at, Some(Cells::new(0, 1, 1, 1)));
    assert_eq!(placed[3].at, Some(Cells::new(1, 1, 1, 1)));
    // A most of nothing (a plugin's slip) is still a column.
    let (placed, _) = grid::refit(&env, &wide, 4, 0, &items[..1]);
    assert_eq!(placed[0].at, Some(Cells::new(0, 0, 1, 1)));
}

#[test]
fn a_grid_without_columns_or_rows_is_one_of_one_and_nothing_panics() {
    let h = harness(X4_SIDE);
    let env = h.env();
    let items = [
        widget("t.w", Cells::new(0, 0, 4, 3)),
        tool("t.a", Some(Cells::new(1, 1, 1, 1))),
        tool("t.b", None),
    ];
    for geometry in [Geometry::default(), Geometry::of(whole(X4_SIDE), 0, 0, 8)] {
        let (placed, left) = grid::refit(&env, &geometry, 4, 2, &items);
        // Whatever finds room is inside the grid, the rest comes back as it
        // was, in order.
        assert_eq!(placed.len() + left.len(), items.len());
        for item in &placed {
            let c = item.at.expect("placed");
            assert!(c.col + c.w <= geometry.columns.max(1) && c.row + c.h <= geometry.rows.max(1));
        }
        let back: Vec<_> = left.iter().map(|i| i.what.clone()).collect();
        let sent: Vec<_> = items
            .iter()
            .map(|i| i.what.clone())
            .filter(|w| back.contains(w))
            .collect();
        assert_eq!(back, sent);
    }
}

#[test]
fn what_has_no_room_comes_back_and_nothing_overlaps_or_leaves_the_grid() {
    let h = harness(X4_SIDE);
    let env = h.env();
    // Twelve tools in 4 by 3, and a tool wider than any grid.
    let mut items: Vec<Item> = (0..12)
        .map(|i| tool(&format!("t.{i:02}"), Some(Cells::new(i % 4, i / 4, 1, 1))))
        .collect();
    items.push(tool("t.wide", Some(Cells::new(0, 5, 9, 1))));
    let small = Geometry::of(whole(X4_SIDE), 3, 3, 8);
    let (placed, left) = grid::refit(&env, &small, 4, u8::MAX, &items);
    // Nine fit; the other four come back as they were.
    assert_eq!(placed.len(), 9);
    assert_eq!(left.len(), 4);
    assert_eq!(left[3].at, Some(Cells::new(0, 5, 9, 1)));
    let taken: Vec<Cells> = placed.iter().map(|i| i.at.unwrap()).collect();
    for (i, a) in taken.iter().enumerate() {
        assert!(a.fits(small.columns, small.rows));
        assert!(taken[i + 1..].iter().all(|b| !a.overlaps(b)));
    }
    assert_eq!(placed.len() + left.len(), items.len());
}

#[test]
fn a_span_wider_than_the_new_grid_is_clamped_to_it() {
    let h = harness(X4);
    let env = h.env();
    let items = [tool("t.a", Some(Cells::new(0, 0, 6, 2)))];
    let narrow = Geometry::of(whole(X4), 2, 4, 8);
    let (placed, left) = grid::refit(&env, &narrow, 4, u8::MAX, &items);
    assert!(left.is_empty());
    assert_eq!(placed[0].at, Some(Cells::new(0, 0, 2, 2)));
    let tiny = Geometry::of(Rect::new(0, 0, 100, 100), 1, 1, 0);
    let (placed, _) = grid::refit(&env, &tiny, 4, u8::MAX, &items);
    assert_eq!(placed[0].at, Some(Cells::new(0, 0, 1, 1)));
}

#[test]
fn laying_out_anew_does_not_change_the_grid_it_came_from_and_an_empty_one_is_nothing() {
    let h = harness(X4_SIDE);
    let env = h.env();
    let items: Vec<Item> = (0..9)
        .map(|i| tool(&format!("t.{i}"), Some(Cells::new(i % 4, i / 4, 1, 1))))
        .collect();
    let wide = Geometry::of(whole(X4_SIDE), 6, 3, 8);
    let (once, _) = grid::refit(&env, &wide, 4, u8::MAX, &items);
    // Not stored: the portrait layout is as it was, and laid out again
    // from the same it gives the same.
    assert_eq!(items[4].at, Some(Cells::new(0, 1, 1, 1)));
    assert_eq!(
        cells(&once),
        cells(&grid::refit(&env, &wide, 4, u8::MAX, &items).0)
    );
    let (placed, left) = grid::refit(&env, &Geometry::default(), 4, u8::MAX, &[]);
    assert!(placed.is_empty() && left.is_empty());
    // A grid made from none (0 columns) is not a division by zero.
    let (placed, _) = grid::refit(&env, &wide, 0, u8::MAX, &items);
    assert_eq!(placed.len(), 9);
}
