use super::*;
use crate::fail_alloc::sweep;
use crate::native_grid;
use paperpaw_sdk::Rect;

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
fn the_options_are_move_resize_remove_and_cancel_with_their_words() {
    let keys: Vec<&str> = CHOICES.iter().map(|(_, key)| *key).collect();
    assert_eq!(
        keys,
        [
            "core.grid.move",
            "core.grid.resize",
            "core.grid.remove",
            "picture-home.edit.cancel"
        ]
    );
    assert_eq!(help(Mode::Move), "core.grid.move.help");
    assert_eq!(help(Mode::Resize), "core.grid.resize.help");
}

#[test]
fn every_allocation_may_be_refused() {
    let list = items("a.b@2 0/0 2x1, c.d 2/0 1x1");
    sweep(|| removed(&list, 0).map(drop));
}
