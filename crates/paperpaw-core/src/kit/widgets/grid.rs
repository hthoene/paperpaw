//! Widgets in a grid, like a phone's home screen: an area cut into cells
//! (columns and rows), each item taking a rectangle of them: a widget (any
//! plugin's) or a tool (an action, shown as its icon and name). Cells left
//! empty stay empty, so everything sits where it was put.
//!
//! The layout is data: a text such as
//! `home.clock 0/0 4x1, tool:files.library 0/5` ([`parse`], [`write()`]),
//! each item its id, its column and row, and its size in cells, kept where
//! the plugin likes (a setting, a file). An item without a place (as
//! plugins add them) goes into the first free cells (`place`), as wide as
//! the grid and as high as it needs.
//!
//! `GridView` shows such a grid on a screen of the plugin's own, and edits
//! it in place: drag an item to move it, by its corner to size it, tap it
//! for its options (move or size it with the buttons, its settings, remove
//! it). With buttons alone, the focus goes from item to item.
//!
//! ```ignore
//! struct Board { grid: GridView } // made with `GridView::asking(env, CHOSEN)`
//!
//! impl Board {
//!     fn layout(&mut self, env: &Place<'_>) {
//!         let geometry = Geometry::new(env, area, 4, 0);
//!         let items = grid::parse(env.settings().read(&LAYOUT));
//!         self.grid.layout(env, geometry, &items, &[]);
//!     }
//!
//!     fn respond(&mut self, response: GridResponse, cx: &mut Turn<'_, '_>) -> Handled {
//!         match &response {
//!             GridResponse::Changed(items) => {
//!                 let value = LAYOUT.value(grid::write(items).into());
//!                 cx.command(Ask::SetSetting(LAYOUT.id().clone(), value));
//!             }
//!             GridResponse::Editing(_) => { let env = *cx.env(); self.layout(&env) }
//!             _ => {}
//!         }
//!         response.handled()
//!     }
//! }
//!
//! impl Layer for Board {
//!     fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Place<'_>) { self.grid.draw(canvas, env) }
//!     fn on_action(&mut self, action: &Id, cx: &mut Turn<'_, '_>) -> Handled {
//!         let response = self.grid.on_action(action, cx);
//!         self.respond(response, cx)
//!     }
//!     fn on_timer(&mut self, cx: &mut Turn<'_, '_>) {
//!         let env = *cx.env();
//!         if self.grid.update_widgets(&env, cx) {
//!             cx.request_refresh(RefreshMode::Fast);
//!         }
//!         cx.wake_at(env.now().add_millis(60_000));
//!     }
//!     // on_pointer, on_gesture, on_event, hint, holds_edges as on_action
//! }
//! ```

pub use paperpaw_sdk::widgets::grid::Cells;
pub use paperpaw_sdk::widgets::grid::Geometry;
pub use paperpaw_sdk::widgets::grid::Item;
pub use paperpaw_sdk::widgets::grid::MAX_COLUMNS;
pub use paperpaw_sdk::widgets::grid::MAX_ROWS;
pub use paperpaw_sdk::widgets::grid::What;
pub use paperpaw_sdk::widgets::grid::columns_for;
pub use paperpaw_sdk::widgets::grid::dragged;
pub use paperpaw_sdk::widgets::grid::free_place;
pub use paperpaw_sdk::widgets::grid::parse;
pub use paperpaw_sdk::widgets::grid::write;

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::{Id, Point, Rect};

    fn grid() -> Geometry {
        Geometry::of(Rect::new(0, 0, 400, 300), 4, 3, 10)
    }

    #[test]
    fn layouts_are_read_and_written() {
        let text = "home.clock 0/0 4x1, tool:core.library 1/5 1x1, stats.today";
        let items = parse(text);
        assert_eq!(items.len(), 3);
        assert_eq!(items[1].what, What::Tool(Id::new_static("core.library")));
        assert_eq!(items[0].at, Some(Cells::new(0, 0, 4, 1)));
        assert_eq!(items[2].at, None);
        assert_eq!(write(&items), text);
        // Layouts written without places: places to come.
        assert!(
            parse("home.status, home.date")
                .iter()
                .all(|i| i.at.is_none())
        );
        // What cannot be read is left out.
        assert_eq!(parse("tool:Not An Id, , home.date").len(), 1);
    }

    #[test]
    fn cells_overlap_and_fit() {
        let a = Cells::new(0, 0, 2, 2);
        assert!(a.overlaps(&Cells::new(1, 1, 1, 1)));
        assert!(!a.overlaps(&Cells::new(2, 0, 2, 1)));
        assert!(a.fits(4, 6));
        assert!(!Cells::new(3, 0, 2, 1).fits(4, 6));
        assert!(!Cells::new(250, 0, 10, 1).fits(4, 6));
    }

    #[test]
    fn free_places_are_found_row_by_row() {
        let g = grid();
        let taken = [Cells::new(0, 0, 4, 1), Cells::new(0, 1, 1, 1)];
        assert_eq!(free_place(&g, &taken, 1, 1), Some(Cells::new(1, 1, 1, 1)));
        assert_eq!(free_place(&g, &taken, 4, 1), Some(Cells::new(0, 2, 4, 1)));
        assert_eq!(free_place(&g, &taken, 4, 2), None);
        assert_eq!(g.rect(Cells::new(1, 1, 2, 1)), Rect::new(105, 105, 190, 90));
        assert_eq!(g.cell_at(Point::new(250, 150)), Some((2, 1)));
        assert_eq!(g.cell_at(Point::new(450, 150)), None);
    }

    #[test]
    fn items_are_dragged_and_sized_by_cells() {
        let g = grid();
        let item = Cells::new(1, 0, 2, 1);
        let at = |x, y| Point::new(x, y);
        // Taken in its second cell, moved a cell right and down.
        let moved = dragged(&g, item, at(250, 50), at(350, 150), false);
        assert_eq!(moved, Some(Cells::new(2, 1, 2, 1)));
        // Not left of the grid's edge.
        let left = dragged(&g, item, at(250, 50), at(50, 50), false);
        assert_eq!(left, Some(Cells::new(0, 0, 2, 1)));
        // Its corner pulled to the third row: as high as that.
        let sized = dragged(&g, item, at(290, 90), at(150, 250), true);
        assert_eq!(sized, Some(Cells::new(1, 0, 1, 3)));
        // Off the grid: nowhere.
        assert_eq!(dragged(&g, item, at(250, 50), at(450, 50), false), None);
    }
}
