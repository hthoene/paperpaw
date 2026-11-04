//! Drawing again only the rows of a [`ListView`] that say something else.

#[allow(
    clippy::wildcard_imports,
    reason = "a part of the module, which shares its imports"
)]
use super::*;

/// What a list needs drawn after its rows changed ([`RowsSeen::see_rows`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RowsChange {
    /// No row on the page says something else.
    None,
    /// Only the rows in this area changed: it is all the screen needs.
    Rows(Rect),
    /// The list as a whole: a row got another height, the page or the
    /// number of pages changed, or the rows were not seen before.
    All,
}

impl RowsChange {
    /// Asks for what the change needs, refreshed fast as a focus moved
    /// within a page is: the area of the rows that changed, the whole
    /// screen for [`RowsChange::All`], nothing for [`RowsChange::None`].
    /// Nothing either where another screen covers this one (it is drawn
    /// whole when it shows again, and what it asked for would only make
    /// the refresh of the screen above larger).
    pub fn ask_for_it(self, cx: &mut crate::Cx<'_, '_>) {
        let covered = cx
            .env()
            .top()
            .is_some_and(|top| !top.overlay && Some(top.key) != cx.key());
        if covered {
            return;
        }
        match self {
            RowsChange::None => return,
            RowsChange::Rows(area) => cx.request_redraw_area(area),
            RowsChange::All => cx.request_redraw(),
        }
        cx.request_refresh(crate::RefreshMode::Fast);
    }

    /// Whether nothing needs drawing.
    pub fn draws_nothing(self) -> bool {
        self == RowsChange::None
    }
}

/// What the rows of a list said when they were drawn last, as one key
/// each, to draw again only those that changed: a Wi-Fi scan that finds a
/// network or hears a signal move changes a row or two, and a frame of
/// the whole list takes the device's time (a key pressed meanwhile waits).
///
/// The screen keeps one beside its [`ListView`]. Where something it shows
/// may have changed it makes a key of each row (what the row says, see
/// [`RowsSeen::row_key`]; not its focus, which moves with
/// [`ListChange`]) and asks [`RowsSeen::rows_differ`]; if
/// it does, it measures the rows again if it sizes them by their text and
/// calls [`RowsSeen::see_rows`] with the list as it is now, and asks for the
/// [`RowsChange`] it gets.
#[derive(Debug, Clone, Default)]
pub struct RowsSeen {
    keys: Vec<u64>,
    row: u32,
    page: usize,
    pages: usize,
    seen: bool,
}

impl RowsSeen {
    /// Nothing seen yet: the first [`RowsSeen::see_rows`] says
    /// [`RowsChange::All`].
    pub fn fresh() -> Self {
        Self::default()
    }

    /// A key of what a row says: the same parts give the same key (FNV-1a
    /// over them, set apart).
    pub fn row_key<'a>(parts: impl IntoIterator<Item = &'a str>) -> u64 {
        parts.into_iter().fold(0xcbf2_9ce4_8422_2325, |hash, part| {
            part.bytes()
                .chain(core::iter::once(b'\t'))
                .fold(hash, |h, b| {
                    (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
                })
        })
    }

    /// Whether the rows say something else than when last seen
    /// ([`RowsSeen::see_rows`]), or were not seen yet.
    pub fn rows_differ(&self, keys: &[u64]) -> bool {
        !self.seen || self.keys != keys
    }

    /// Takes `keys` (one per row of the list, in order) as seen and says
    /// what the list needs drawn. `list` is as it will be drawn: its row
    /// height set for the new rows.
    pub fn see_rows(&mut self, list: &ListView, keys: &[u64]) -> RowsChange {
        let pages = list.pages(keys.len());
        let whole =
            !self.seen || self.row != list.row() || self.page != list.page() || self.pages != pages;
        let change = if whole {
            RowsChange::All
        } else {
            let first = list.page() * list.rows();
            let mut area: Option<Rect> = None;
            for slot in 0..list.rows() {
                let index = first + slot;
                let said_other = self.keys.get(index) != keys.get(index);
                if said_other {
                    let rect = list.row_rect(slot);
                    area = Some(area.map_or(rect, |a| a.union(&rect)));
                }
            }
            area.map_or(RowsChange::None, RowsChange::Rows)
        };
        self.keys.clear();
        self.keys.extend_from_slice(keys);
        self.row = list.row();
        self.page = list.page();
        self.pages = pages;
        self.seen = true;
        change
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn three_row_list() -> ListView {
        // Three rows of 100 on a page.
        ListView::new(Rect::new(0, 100, 200, 300), 100)
    }

    fn numbered(n: u64) -> Vec<u64> {
        (0..n).collect()
    }

    #[test]
    fn the_first_update_draws_all_and_the_same_rows_again_nothing() {
        let (list, mut seen) = (three_row_list(), RowsSeen::fresh());
        assert!(seen.rows_differ(&numbered(3)));
        assert_eq!(seen.see_rows(&list, &numbered(3)), RowsChange::All);
        assert!(!seen.rows_differ(&numbered(3)));
        assert_eq!(seen.see_rows(&list, &numbered(3)), RowsChange::None);
        assert!(seen.see_rows(&list, &numbered(3)).draws_nothing());
    }

    #[test]
    fn a_row_that_changed_is_drawn_alone_and_neighbours_make_one_area() {
        let (list, mut seen) = (three_row_list(), RowsSeen::fresh());
        seen.see_rows(&list, &numbered(3));
        let mut now = numbered(3);
        now[1] = 99;
        assert_eq!(
            seen.see_rows(&list, &now),
            RowsChange::Rows(list.row_rect(1))
        );
        // Seen as it is: only the next change counts.
        let (mut two, both) = (now.clone(), list.row_rect(0).union(&list.row_rect(1)));
        two[0] = 97;
        two[1] = 96;
        assert_eq!(seen.see_rows(&list, &two), RowsChange::Rows(both));
    }

    #[test]
    fn a_row_gone_or_new_on_the_page_is_drawn_not_the_rest() {
        let (list, mut seen) = (three_row_list(), RowsSeen::fresh());
        seen.see_rows(&list, &numbered(2));
        // A third row comes: its place is drawn.
        assert_eq!(
            seen.see_rows(&list, &numbered(3)),
            RowsChange::Rows(list.row_rect(2))
        );
        // It goes again: the place is cleared.
        assert_eq!(
            seen.see_rows(&list, &numbered(2)),
            RowsChange::Rows(list.row_rect(2))
        );
    }

    #[test]
    fn another_row_height_or_number_of_pages_draws_all() {
        let mut list = three_row_list();
        let mut seen = RowsSeen::fresh();
        seen.see_rows(&list, &numbered(3));
        list.set_row(120);
        assert_eq!(seen.see_rows(&list, &numbered(3)), RowsChange::All);
        list.set_row(100);
        seen.see_rows(&list, &numbered(3));
        // A fourth row: a second page (the header says so).
        assert_eq!(seen.see_rows(&list, &numbered(4)), RowsChange::All);
    }

    #[test]
    fn rows_on_other_pages_draw_nothing_and_another_page_draws_all() {
        let mut list = three_row_list();
        let mut seen = RowsSeen::fresh();
        seen.see_rows(&list, &numbered(6));
        let mut now = numbered(6);
        now[4] = 99;
        assert_eq!(seen.see_rows(&list, &now), RowsChange::None);
        list.set_focus(4);
        assert_eq!(seen.see_rows(&list, &now), RowsChange::All);
        now[4] = 98;
        assert_eq!(
            seen.see_rows(&list, &now),
            RowsChange::Rows(list.row_rect(1))
        );
    }

    #[test]
    fn a_focus_that_moved_is_not_a_row_that_changed() {
        let mut list = three_row_list();
        let mut seen = RowsSeen::fresh();
        seen.see_rows(&list, &numbered(3));
        list.set_focus(2);
        assert_eq!(seen.see_rows(&list, &numbered(3)), RowsChange::None);
    }

    #[test]
    fn keys_depend_on_every_part_and_where_it_ends() {
        let key = RowsSeen::row_key;
        assert_eq!(key(["Home", "ok"]), key(["Home", "ok"]));
        assert_ne!(key(["Home", "ok"]), key(["Home", "ok2"]));
        assert_ne!(key(["Ho", "meok"]), key(["Home", "ok"]));
    }
}
