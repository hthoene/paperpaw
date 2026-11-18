use super::*;
use crate::dashboard::grid as native_grid;
use crate::fail_alloc::sweep;
use paperpaw_sdk::{Point, Rect, widgets::CellSpan};

fn grid() -> Geometry {
    Geometry::of(Rect::new(0, 0, 400, 300), 4, 3, 0)
}

fn items(text: &str) -> Vec<Item> {
    native_grid::parse(text).unwrap()
}

#[test]
fn the_buttons_name_a_direction_and_the_page_buttons_are_up_and_down() {
    assert_eq!(Dir::of("core.left"), Some(Dir::Left));
    assert_eq!(Dir::of("core.previous"), Some(Dir::Left));
    assert_eq!(Dir::of("core.next"), Some(Dir::Right));
    assert_eq!(Dir::of("core.prev-page"), Some(Dir::Up));
    assert_eq!(Dir::of("core.next-page"), Some(Dir::Down));
    assert_eq!(Dir::of("core.confirm"), None);
}

#[test]
fn moving_goes_a_cell_and_sizing_a_cell_wider_or_taller_but_never_below_one() {
    let at = Cells::new(1, 1, 2, 2);
    assert_eq!(
        stepped(Mode::Move, at, Dir::Left, 1),
        Cells::new(0, 1, 2, 2)
    );
    assert_eq!(
        stepped(Mode::Move, at, Dir::Down, 1),
        Cells::new(1, 2, 2, 2)
    );
    assert_eq!(
        stepped(Mode::Resize, at, Dir::Right, 1),
        Cells::new(1, 1, 3, 2)
    );
    assert_eq!(
        stepped(Mode::Resize, at, Dir::Up, 1),
        Cells::new(1, 1, 2, 1)
    );
    // Not below one cell, nor lower than the widget measured.
    let one = Cells::new(0, 0, 1, 1);
    assert_eq!(stepped(Mode::Resize, one, Dir::Left, 1), one);
    assert_eq!(stepped(Mode::Resize, one, Dir::Up, 1), one);
    let tall = Cells::new(0, 0, 1, 2);
    assert_eq!(stepped(Mode::Resize, tall, Dir::Up, 2), tall);
    // At the first column or row a move stays (the grid's edge refuses it).
    assert_eq!(stepped(Mode::Move, one, Dir::Left, 1), one);
}

#[test]
fn an_item_goes_where_there_is_room() {
    let mut list = items("a.b 0/0 2x1, c.d 2/0 1x1");
    let g = grid();
    assert_eq!(put(&mut list, 0, Cells::new(0, 1, 2, 1), &g), Put::Done);
    assert_eq!(list[0].at, Some(Cells::new(0, 1, 2, 1)));
}

#[test]
fn an_item_does_not_go_past_the_grid_or_onto_another() {
    let mut list = items("a.b 0/0 2x1, c.d 2/0 1x1");
    let g = grid();
    // Wider into the other: said.
    assert_eq!(put(&mut list, 0, Cells::new(0, 0, 3, 1), &g), Put::InTheWay);
    // Past the edges: nothing said (as on interface 6).
    assert_eq!(put(&mut list, 1, Cells::new(4, 0, 1, 1), &g), Put::Same);
    assert_eq!(put(&mut list, 1, Cells::new(2, 3, 1, 1), &g), Put::Same);
    // The same cells: nothing. An item that is not there: nothing.
    assert_eq!(put(&mut list, 0, Cells::new(0, 0, 2, 1), &g), Put::Same);
    assert_eq!(put(&mut list, 9, Cells::new(0, 0, 1, 1), &g), Put::Same);
    assert_eq!(list[0].at, Some(Cells::new(0, 0, 2, 1)));
}

#[test]
fn removing_leaves_the_others_and_resets_a_placement_only() {
    let list = items("a.b@2 0/0 2x1, c.d 2/0 1x1, tool:core.library 3/0 1x1");
    let (rest, reset) = removed(&list, 0).unwrap();
    assert_eq!(rest.len(), 2);
    assert_eq!(reset.as_deref(), Some("a.b@2"));
    // A widget without a placement and a tool have no values of their own.
    let (rest, reset) = removed(&list, 1).unwrap();
    assert_eq!(rest.len(), 2);
    assert_eq!(reset, None);
    let (_, reset) = removed(&list, 2).unwrap();
    assert_eq!(reset, None);
    // Nothing at that number: nothing removed.
    assert_eq!(removed(&list, 9).unwrap().0.len(), 3);
}

#[test]
fn the_options_are_move_resize_remove_and_cancel_with_the_cancel_word_left_to_the_plugin() {
    let keys: Vec<&str> = CHOICES.iter().map(|(_, key)| *key).collect();
    assert_eq!(
        keys,
        ["core.grid.move", "core.grid.resize", "core.grid.remove", ""]
    );
    let with: Vec<&str> = choices(true).iter().map(|(_, key)| *key).collect();
    assert_eq!(
        with,
        [
            "core.grid.move",
            "core.grid.resize",
            "core.grid.settings",
            "core.grid.remove",
            ""
        ]
    );
    assert_eq!(choices(false).len(), 4);
    assert_eq!(help(Mode::Move), "core.grid.move.help");
    assert_eq!(help(Mode::Resize), "core.grid.resize.help");
}

#[test]
fn every_allocation_may_be_refused() {
    let list = items("a.b@2 0/0 2x1, c.d 2/0 1x1");
    sweep(|| removed(&list, 0).map(drop));
}

#[test]
fn a_finger_moves_an_item_by_as_many_cells_as_it_went() {
    let from = Cells::new(1, 1, 1, 1);
    let at = |x, y| Point::new(x, y);
    let moved = dragged(
        &grid(),
        from,
        (at(150, 150), at(250, 250)),
        false,
        (1, None),
    );
    assert_eq!(moved, Some(Cells::new(2, 2, 1, 1)));
    // Not past the left or the top: the cell where it would go is the first.
    let back = dragged(&grid(), from, (at(150, 150), at(10, 10)), false, (1, None));
    assert_eq!(back, Some(Cells::new(0, 0, 1, 1)));
    // Off the grid: nowhere.
    assert_eq!(
        dragged(
            &grid(),
            from,
            (at(150, 150), at(150, 900)),
            false,
            (1, None)
        ),
        None
    );
}

#[test]
fn a_finger_on_the_corner_sizes_the_item_to_the_cell_under_it() {
    let from = Cells::new(0, 0, 1, 1);
    let to = |x, y, least, span| {
        dragged(
            &grid(),
            from,
            (Point::new(90, 90), Point::new(x, y)),
            true,
            (least, span),
        )
    };
    assert_eq!(to(250, 150, 1, None), Some(Cells::new(0, 0, 3, 2)));
    // Never under one cell: a finger left of and above the item's corner.
    assert_eq!(to(10, 10, 1, None), Some(Cells::new(0, 0, 1, 1)));
    // A widget is no lower than it can do with, nor than the grid.
    assert_eq!(to(150, 50, 2, None), Some(Cells::new(0, 0, 2, 2)));
    assert_eq!(to(150, 50, 9, None), Some(Cells::new(0, 0, 2, 3)));
    // Nor under what it declared.
    let span = CellSpan::new(2, 2).min(2, 2);
    assert_eq!(to(50, 50, 1, Some(span)), Some(Cells::new(0, 0, 2, 2)));
}

#[test]
fn an_item_may_take_cells_inside_the_grid_clear_of_the_others() {
    let items = items("tool:a 0/0 1x1, tool:b 1/0 2x1");
    let g = grid();
    assert!(free(&items, 0, Cells::new(0, 1, 1, 1), &g));
    // Its own cells are free to it.
    assert!(free(&items, 1, Cells::new(1, 0, 2, 1), &g));
    assert!(!free(&items, 0, Cells::new(1, 0, 1, 1), &g));
    assert!(!free(&items, 0, Cells::new(3, 0, 2, 1), &g));
}
