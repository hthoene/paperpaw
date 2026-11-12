//! Where the parts of the picture home dashboard go and how its pages are
//! turned, without the host: the bar's strip, the grid and the footer (the same
//! arithmetic as the interface-6 screen, in the same order), the footer's
//! buttons and dots, the page that shows, and the page a swipe or a page button
//! goes to.
use paperpaw_sdk::{
    Rect,
    widgets::grid::{self, Geometry},
};

/// What the layout needs of the device, the style and the settings, as numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Metrics {
    /// The screen, as it is now.
    pub width: u16,
    pub height: u16,
    /// The style's margin, and a row's height (the footer's).
    pub margin: u16,
    pub row: u16,
    /// The gap between widgets (`widgets::gap`), and the style's gap.
    pub gap: u16,
    pub style_gap: u16,
    /// The bar's strip: a line of small text with room round it.
    pub bar: u16,
}

/// Whether the screen is on its side.
#[must_use]
pub const fn landscape(m: &Metrics) -> bool {
    m.width > m.height
}

/// The parts of the screen.
#[derive(Debug, Clone, Copy)]
pub struct Frame {
    /// The bar's strip (the whole width of the screen), where the bar has one.
    /// It is kept free whether the bar shows or not, so that the page does not
    /// move when it comes.
    pub strip: Option<Rect>,
    pub geometry: Geometry,
    pub footer: Rect,
}

/// Where the parts of the screen go: the bar's strip (at the bottom or the top
/// of the grid, where there is one), the grid (on its side with the columns that
/// keep the cells as wide as they are upright, and the rows its height gives),
/// the footer. `columns` are the upright ones, `rows` the setting (0: as many as
/// fit).
#[must_use]
pub fn frame(m: &Metrics, strip: Option<bool>, columns: u8, rows: i64) -> Frame {
    let (width, height) = (m.width, m.height);
    let wide = landscape(m);
    let footer = Rect::new(0, height.saturating_sub(m.row), width, m.row);
    let mut top = m.gap / 2;
    let mut bottom = footer.y;
    let strip = strip.map(|at_bottom| {
        if at_bottom {
            bottom = bottom.saturating_sub(m.bar);
            Rect::new(0, bottom, width, m.bar)
        } else {
            let r = Rect::new(0, top, width, m.bar);
            top = top.saturating_add(m.bar);
            r
        }
    });
    // Half a gap wider than the margins on either side, as the grid leaves half
    // a gap round its cells: they line up with the margins. A margin narrower
    // than that is as wide as that, inside the screen.
    let outer = m.margin.max(m.gap / 2);
    let area = Rect::new(
        outer - m.gap / 2,
        top,
        width
            .saturating_sub(outer.saturating_mul(2))
            .saturating_add(m.gap),
        bottom.saturating_sub(top),
    );
    let (columns, rows) = if wide {
        (grid::columns_for(area, columns), 0)
    } else {
        (columns, rows.clamp(0, i64::from(grid::MAX_ROWS)) as u8)
    };
    Frame {
        strip,
        geometry: geometry(area, columns, rows, m.gap),
        footer,
    }
}

/// The grid in `area`: [`Geometry::new`]'s arithmetic without an environment.
#[must_use]
pub fn geometry(area: Rect, columns: u8, rows: u8, gap: u16) -> Geometry {
    let columns = columns.clamp(2, grid::MAX_COLUMNS);
    let cell_w = area.width / u16::from(columns);
    let rows = match rows {
        0 => {
            let target = (u32::from(cell_w) * 85 / 100).max(1);
            ((u32::from(area.height) + target / 2) / target).clamp(3, u32::from(grid::MAX_ROWS))
                as u8
        }
        n => n.clamp(2, grid::MAX_ROWS),
    };
    Geometry::of(area, columns, rows, gap)
}

/// The footer's buttons.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Footer {
    /// The pencil (*Done* while editing), at the right.
    pub pencil: Rect,
    /// *Add* (while editing), at the left.
    pub plus: Rect,
}

/// What the footer shows besides the dots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wanted {
    pub editing: bool,
    /// The width of the word on the *Done* button.
    pub done_word: u16,
}

/// The footer's buttons in `footer`: add and done are buttons a finger finds
/// while editing, at least two rows wide (a tick alone was too small to hit: a
/// missed tap fell on the page and turned it). The picture home screen has no
/// footer actions.
#[must_use]
pub fn footer(m: &Metrics, footer: Rect, want: &Wanted) -> Footer {
    let side = footer.height;
    let (plus, done) = if want.editing {
        (
            side.saturating_mul(2),
            want.done_word
                .saturating_add(m.margin.saturating_mul(2))
                .max(side.saturating_mul(2)),
        )
    } else {
        (side, side)
    };
    Footer {
        pencil: Rect::new(
            m.width.saturating_sub(m.margin.saturating_add(done)),
            footer.y,
            done,
            side,
        ),
        plus: Rect::new(m.margin, footer.y, plus, side),
    }
}

/// One dot of the page indicator: its top left corner and size, and whether it
/// stands for the page that shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dot {
    pub x: i32,
    pub y: i32,
    pub size: u32,
    pub current: bool,
}

/// The dots of `pages` pages in `footer`, the page `page` filled; none for one
/// page.
pub fn dots(footer: Rect, style_gap: u16, pages: usize, page: usize) -> impl Iterator<Item = Dot> {
    let dot = (u32::from(style_gap) * 2 / 3).max(6);
    let spacing = dot * 2;
    let count = if pages > 1 { pages } else { 0 };
    let total = dot * count as u32 + (spacing - dot) * (count as u32).saturating_sub(1);
    let x0 = i32::from(footer.x) + (i32::from(footer.width) - total as i32) / 2;
    let y = i32::from(footer.y) + (i32::from(footer.height) - dot as i32) / 2;
    (0..count).map(move |at| Dot {
        x: x0 + (at as u32 * spacing) as i32,
        y,
        size: dot,
        current: at == page,
    })
}

/// The pill the dots sit on over a picture (interface 6: paper round the dots,
/// a dot's size beyond them at the sides and half that above and below) and
/// its corner's radius; none where no dots show (one page).
#[must_use]
pub fn pill(footer: Rect, style_gap: u16, pages: usize) -> Option<(Rect, u32)> {
    let mut all = dots(footer, style_gap, pages, 0);
    let first = all.next()?;
    let last = all.last().unwrap_or(first);
    let pad = i64::from(first.size);
    let left = i64::from(first.x) - pad;
    let top = i64::from(first.y) - pad / 2;
    let width = i64::from(last.x) + i64::from(last.size) + pad - left;
    let height = i64::from(first.size) + pad;
    let clamp = |v: i64| u16::try_from(v.max(0)).unwrap_or(u16::MAX);
    let radius = u32::try_from(height / 2).unwrap_or(0);
    Some((
        Rect::new(clamp(left), clamp(top), clamp(width), clamp(height)),
        radius,
    ))
}

/// The page that shows and how many there are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Nav {
    pub page: usize,
    pub pages: usize,
}

impl Nav {
    /// `pages` pages (at least one), the first showing.
    #[must_use]
    pub const fn new(pages: usize) -> Self {
        Self {
            page: 0,
            pages: if pages == 0 { 1 } else { pages },
        }
    }

    /// The page kept where the pages became more or fewer.
    pub fn resize(&mut self, pages: usize) {
        self.pages = pages.max(1);
        self.page = self.page.min(self.pages - 1);
    }

    /// The page `forward` or back, if there is one.
    #[must_use]
    pub const fn turned(&self, forward: bool) -> Option<usize> {
        if forward {
            if self.page + 1 < self.pages {
                Some(self.page + 1)
            } else {
                None
            }
        } else {
            self.page.checked_sub(1)
        }
    }

    /// Goes to the page `forward` or back; whether there was one.
    pub fn turn(&mut self, forward: bool) -> bool {
        match self.turned(forward) {
            Some(page) => {
                self.page = page;
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    /// A 480 x 800 screen, 3 mm margins about 24 px, rows 60 px.
    const M: Metrics = Metrics {
        width: 480,
        height: 800,
        margin: 24,
        row: 60,
        gap: 16,
        style_gap: 16,
        bar: 32,
    };

    #[test]
    fn the_bar_strip_is_kept_above_or_below_the_grid_and_the_footer_below_both() {
        let top = frame(&M, Some(false), 4, 0);
        assert_eq!(top.footer, Rect::new(0, 740, 480, 60));
        assert_eq!(top.strip, Some(Rect::new(0, 8, 480, 32)));
        // The grid: from under the strip to the footer, half a gap wider than
        // the margins either side.
        assert_eq!(top.geometry.area, Rect::new(16, 40, 448, 700));
        assert_eq!(top.geometry.columns, 4);
        assert_eq!(top.geometry.cell_w, 112);
        let bottom = frame(&M, Some(true), 4, 0);
        assert_eq!(bottom.strip, Some(Rect::new(0, 708, 480, 32)));
        assert_eq!(bottom.geometry.area, Rect::new(16, 8, 448, 700));
        let none = frame(&M, None, 4, 0);
        assert_eq!(none.strip, None);
        assert_eq!(none.geometry.area, Rect::new(16, 8, 448, 732));
    }

    #[test]
    fn rows_are_the_setting_or_as_many_as_make_square_cells() {
        // 0: cells about as high as wide (85 % of 112 = 95, in 732 px).
        let auto = frame(&M, None, 4, 0);
        assert_eq!(auto.geometry.rows, 8);
        let set = frame(&M, None, 4, 5);
        assert_eq!(set.geometry.rows, 5);
        // Out of range is held to the grid's.
        assert_eq!(frame(&M, None, 4, 99).geometry.rows, grid::MAX_ROWS);
        assert_eq!(frame(&M, None, 4, 1).geometry.rows, 2);
    }

    #[test]
    fn on_its_side_the_grid_has_the_columns_that_keep_cells_as_wide() {
        let side = Metrics {
            width: 800,
            height: 480,
            ..M
        };
        assert!(landscape(&side) && !landscape(&M));
        let f = frame(&side, None, 4, 7);
        // The area is wider than high: more columns, rows as they fit.
        assert!(f.geometry.columns > 4);
        assert_eq!(f.footer, Rect::new(0, 420, 800, 60));
        // The rows setting is for the upright grid only.
        assert_ne!(f.geometry.rows, 7);
    }

    #[test]
    fn a_margin_narrower_than_the_gap_is_held_to_it() {
        let tight = Metrics { margin: 4, ..M };
        let f = frame(&tight, None, 4, 0);
        assert_eq!(f.geometry.area.x, 0);
        assert_eq!(f.geometry.area.width, 480 - 2 * 8 + 16);
    }

    #[test]
    fn a_strip_taller_than_the_room_leaves_a_grid_of_no_height_not_a_wrap() {
        let tall = Metrics { bar: u16::MAX, ..M };
        let f = frame(&tall, Some(true), 4, 0);
        assert_eq!(f.geometry.area.height, 0);
        let f = frame(&tall, Some(false), 4, 0);
        assert_eq!(f.geometry.area.height, 0);
    }

    fn footer_of(want: Wanted) -> Footer {
        footer(&M, Rect::new(0, 740, 480, 60), &want)
    }

    #[test]
    fn the_pencil_is_at_the_right_and_add_at_the_left() {
        let f = footer_of(Wanted {
            editing: false,
            done_word: 0,
        });
        assert_eq!(f.pencil, Rect::new(396, 740, 60, 60));
        assert_eq!(f.plus, Rect::new(24, 740, 60, 60));
    }

    #[test]
    fn editing_has_add_and_a_wide_done() {
        let f = footer_of(Wanted {
            editing: true,
            done_word: 30,
        });
        assert_eq!(f.plus, Rect::new(24, 740, 120, 60));
        // Its word with room round it, and at least two rows wide.
        assert_eq!(f.pencil.width, 120);
        let long = footer_of(Wanted {
            editing: true,
            done_word: 200,
        });
        assert_eq!(long.pencil.width, 248);
        assert_eq!(long.pencil.x, 480 - 24 - 248);
    }

    #[test]
    fn a_dot_a_page_the_current_one_filled_and_none_for_one_page() {
        let footer = Rect::new(0, 740, 480, 60);
        assert_eq!(dots(footer, 16, 1, 0).count(), 0);
        let d: Vec<Dot> = dots(footer, 16, 3, 1).collect();
        assert_eq!(d.len(), 3);
        assert_eq!(d.iter().filter(|d| d.current).count(), 1);
        assert!(d[1].current);
        // Size from the style's gap (two thirds, at least 6), a dot's width
        // between dots, centred.
        assert_eq!(d[0].size, 10);
        assert_eq!(d[1].x - d[0].x, 20);
        let total = 3 * 10 + 10 * 2;
        assert_eq!(d[0].x, (480 - total) / 2);
        assert_eq!(d[0].y, 740 + (60 - 10) / 2);
        // Small gaps still give a dot of 6.
        assert_eq!(dots(footer, 3, 2, 0).next().unwrap().size, 6);
    }

    #[test]
    fn over_a_picture_the_dots_sit_on_a_pill_as_on_interface_6() {
        let footer = Rect::new(0, 740, 480, 60);
        assert_eq!(pill(footer, 16, 1), None);
        // Three dots of 10, 50 wide in all from x 215: a dot more at each side,
        // half a dot above and below.
        let (rect, radius) = pill(footer, 16, 3).unwrap();
        assert_eq!(rect, Rect::new(205, 760, 70, 20));
        assert_eq!(radius, 10);
        // Every dot is inside it.
        for dot in dots(footer, 16, 3, 0) {
            assert!(dot.x >= i32::from(rect.x) && dot.y >= i32::from(rect.y));
            assert!(dot.x + dot.size as i32 <= i32::from(rect.x) + i32::from(rect.width));
            assert!(dot.y + dot.size as i32 <= i32::from(rect.y) + i32::from(rect.height));
        }
    }

    #[test]
    fn pages_turn_within_their_number() {
        let mut nav = Nav::new(3);
        assert!(!nav.turn(false));
        assert!(nav.turn(true));
        assert!(nav.turn(true));
        assert!(!nav.turn(true));
        assert_eq!(nav.page, 2);
        assert_eq!(nav.turned(false), Some(1));
        nav.resize(2);
        assert_eq!(nav.page, 1);
        nav.resize(0);
        assert_eq!((nav.page, nav.pages), (0, 1));
        assert_eq!(Nav::new(0).pages, 1);
    }
}
