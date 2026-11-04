//! Where things go on the screen of the library list.

use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::{Env, Rect};

use super::grid;
use super::index::Filter;
use super::rows::Sizes;

/// Where things go on the screen: made anew when a setting changes (the
/// view, the size of the interface).
pub(super) struct Layout {
    pub(super) style: UiStyle,
    pub(super) sizes: Sizes,
    pub(super) header: ui::Header,
    /// The filters' chips (larger screens; none on small ones).
    pub(super) chips: ui::Chips,
    /// The filter's row (small screens).
    pub(super) filter_row: Option<Rect>,
    /// Where the books (or shelves) go.
    pub(super) area: Rect,
    /// The covers' grid, where books show as covers ([`crate::VIEW`]).
    pub(super) grid: Option<grid::Grid>,
    /// Covers in grays ([`crate::COVERS`]).
    pub(super) grays: bool,
}

impl Layout {
    pub(super) fn new(env: &Env<'_>) -> Self {
        let style = UiStyle::new(env);
        let sizes = Sizes::new(env, &style);
        let header = ui::Header::new(env, &style, true);
        let display = env.device().display;
        let gap = style.gap();
        let (chips, filter_row, top) = if sizes.compact {
            let row = Rect::new(0, header.area().height, display.width, sizes.filter_row);
            (
                ui::Chips::default(),
                Some(row),
                row.y + row.height + gap / 2,
            )
        } else {
            // From margin to margin, with room round them for the focus.
            let margin = style.margin();
            let labels = Filter::ALL.map(Filter::label);
            let width = display.width.saturating_sub(2 * margin);
            // As many rows as the words need: none is shortened.
            let band = Rect::new(
                margin,
                header.area().height,
                width,
                ui::Chips::height_for(env, &style, width, &labels) + gap,
            );
            let chips = ui::Chips::new(env, &style, band, labels);
            (chips, None, band.y + band.height)
        };
        let area = Rect::new(0, top, display.width, display.height.saturating_sub(top));
        let grid = (env.settings().read(&crate::VIEW) == crate::VIEW_COVERS)
            .then(|| grid::Grid::new(env, &style, &sizes, area.height));
        Layout {
            style,
            sizes,
            header,
            chips,
            filter_row,
            area,
            grid,
            grays: crate::covers_in_grays(env),
        }
    }
}
