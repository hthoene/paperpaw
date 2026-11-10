//! How many rows a widget takes in the grid: its natural and its least height
//! (`ui.widget_measure`) at the grid's width, in cells. The same arithmetic as
//! the interface-6 `Geometry::size_of`; the host call is the plugin's.
//!
//! A widget that declared its cells (`widget.spec.cells`, with its least size and
//! whether it stays square) is not measured: `native_source::declared` has its
//! size, as interface 6 does.
use paperpaw_sdk::{
    native::{Error, Host, ui::Ui},
    widgets::grid::Geometry,
};

use crate::native_made::Made;

/// Where the widgets are made (the place the plugin declares).
pub const PLACE: &str = "home.pages";

/// The width a widget has across the whole grid, in pixels.
#[must_use]
pub fn grid_width(geometry: &Geometry) -> u32 {
    (u32::from(geometry.columns) * u32::from(geometry.cell_w))
        .saturating_sub(u32::from(geometry.gap))
}

/// The rows a height of `height` pixels takes (at least one, at most the grid's).
#[must_use]
pub fn rows_for(geometry: &Geometry, height: u32) -> u8 {
    let rows = (height.saturating_add(u32::from(geometry.gap)))
        .div_ceil(u32::from(geometry.cell_h.max(1)))
        .clamp(1, u32::from(geometry.rows.max(1)));
    u8::try_from(rows).unwrap_or(u8::MAX)
}

/// A widget's rows and its fewest rows, from what it measured.
#[must_use]
pub fn rows_of(geometry: &Geometry, height: u32, least: u32) -> (u8, u8) {
    let rows = rows_for(geometry, height);
    (rows, rows_for(geometry, least.min(height)).min(rows))
}

/// Makes the widget `id` at `place`, measures it and lets it go; `None` where
/// it is gone or not for this screen.
///
/// # Errors
/// Allocation and the host's refusals.
pub fn measure_widget(
    host: &Host<'_>,
    ui: &Ui,
    id: &str,
    place: &str,
    geometry: &Geometry,
) -> Result<Option<(u8, u8)>, Error> {
    let made = match Made::make(host, ui, id, place) {
        Ok(made) => made,
        Err(Error::NotFound | Error::Denied) => return Ok(None),
        Err(error) => return Err(error),
    };
    let measured = made.measure(host, ui, geometry);
    made.release(host, ui);
    measured.map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::Rect;

    fn grid() -> Geometry {
        // 4 columns of 100 px, 3 rows of 85 px, a gap of 10 px.
        Geometry::of(Rect::new(0, 0, 400, 255), 4, 3, 0)
    }

    #[test]
    fn a_widget_is_as_wide_as_the_grid_less_a_gap() {
        let g = grid();
        assert_eq!(
            grid_width(&g),
            u32::from(g.columns) * u32::from(g.cell_w) - u32::from(g.gap)
        );
    }

    #[test]
    fn heights_are_rows_rounded_up_within_the_grid() {
        let g = grid();
        let row = u32::from(g.cell_h);
        let gap = u32::from(g.gap);
        // Nothing shown: a row.
        assert_eq!(rows_for(&g, 0), 1);
        // Just a row, plus a pixel: two.
        assert_eq!(rows_for(&g, row - gap), 1);
        assert_eq!(rows_for(&g, row - gap + 1), 2);
        // More than the grid has: the grid's rows.
        assert_eq!(rows_for(&g, 100_000), g.rows);
    }

    #[test]
    fn the_fewest_rows_are_never_more_than_the_rows() {
        let g = grid();
        let row = u32::from(g.cell_h);
        assert_eq!(
            rows_of(&g, 2 * row, row),
            (rows_for(&g, 2 * row), rows_for(&g, row))
        );
        // A least above the height is cut to it.
        let (rows, least) = rows_of(&g, row, 5 * row);
        assert!(least <= rows);
    }
}
