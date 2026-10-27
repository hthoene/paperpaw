//! Paged, focusable lists ([`ListView`]) and moving a focus.

#[allow(
    clippy::wildcard_imports,
    reason = "a part of the module, which shares its imports"
)]
use super::*;

/// What changed after moving a list's focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ListChange {
    None,
    /// The focus moved within the visible page: from the item at `left`
    /// to the one at `reached` (the same where a focus only came to
    /// show). Made with [`ListChange::focus`].
    #[non_exhaustive]
    Focus {
        left: Rect,
        reached: Rect,
    },
    /// The visible page changed.
    Page,
}

impl ListChange {
    /// Asks for what the change needs, as every list does: for a focus
    /// moved within the page the item it left and the one it came to
    /// drawn again, refreshed fast (a step in a menu on the X4: a fraction
    /// of a whole frame); for a new page the screen, a little more
    /// thoroughly; nothing for no change. A screen whose focus changes
    /// more than its items, such as a line about the focused one, asks
    /// for that too (`Cx::request_redraw_area`).
    pub fn request(self, cx: &mut crate::Cx<'_, '_>) {
        match self {
            ListChange::None => {}
            ListChange::Focus { left, reached } => {
                cx.request_redraw_area(left);
                cx.request_redraw_area(reached);
                cx.request_refresh(crate::RefreshMode::Fast);
            }
            ListChange::Page => {
                cx.request_redraw();
                cx.request_refresh(crate::RefreshMode::Partial);
            }
        }
    }

    /// Whether the focus moved within the page.
    pub fn is_focus(self) -> bool {
        matches!(self, ListChange::Focus { .. })
    }

    /// A focus moved within the page from the item at `left` to the one
    /// at `reached`: for lists of a plugin's own.
    pub fn focus(left: Rect, reached: Rect) -> Self {
        ListChange::Focus { left, reached }
    }
}

/// How far a navigation action moves a focus: one back for up, left and
/// previous, one on for down, right and next, `page` for the next page
/// and back for the previous one; `None` for any other action. Screens
/// with a [`ListView`] use [`ListView::navigate`].
pub fn list_step(action: &crate::Id, page: isize) -> Option<isize> {
    use actions::{DOWN, LEFT, NEXT, NEXT_PAGE, PREV_PAGE, PREVIOUS, RIGHT, UP};
    match action {
        a if [UP, LEFT, PREVIOUS].contains(a) => Some(-1),
        a if [DOWN, RIGHT, NEXT].contains(a) => Some(1),
        a if *a == NEXT_PAGE => Some(page),
        a if *a == PREV_PAGE => Some(-page),
        _ => None,
    }
}

/// A paged, focusable list: the basic building block of menus, the
/// library and settings. Works with buttons (focus) and touch (hit tests).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListView {
    /// Where the rows are drawn.
    area: Rect,
    /// The height of a row, at least 1.
    row: u32,
    focus: usize,
    /// Whether the focused row is highlighted; off while the focus is on
    /// another element of the screen.
    active: bool,
    /// Whether the focus shows at all: on touch screens only once a
    /// button moved it (see [`ListView::focus_on_keys`]), as in menus.
    shown: bool,
    /// Items side by side in a row (a grid, e.g. of covers); 1 for a list.
    columns: u32,
    /// The item focused before the focus last moved.
    before: usize,
}

impl ListView {
    pub fn new(area: Rect, row: u32) -> Self {
        Self {
            area,
            row: row.max(1),
            focus: 0,
            active: true,
            shown: true,
            columns: 1,
            before: 0,
        }
    }

    /// Where the rows are drawn: a copy, so a change to it changes
    /// nothing; [`set_area`](Self::set_area) moves the rows.
    pub fn area(&self) -> Rect {
        self.area
    }

    /// Draws the rows in `area` from now on (e.g. after the screen was
    /// laid out anew).
    pub fn set_area(&mut self, area: Rect) {
        self.area = area;
    }

    /// The height of a row.
    pub fn row(&self) -> u32 {
        self.row
    }

    /// Makes the rows `row` high from now on (e.g. as high as their notes
    /// need, which changed: [`UiStyle::noted_row_for`]), the focus kept.
    pub fn set_row(&mut self, row: u32) {
        self.row = row.max(1);
    }

    /// The item with the focus.
    pub fn focus(&self) -> usize {
        self.focus
    }

    /// Puts the focus on item `index` (see [`ListView::set_len`] to keep
    /// it among the items there are); what that changed on the screen.
    pub fn set_focus(&mut self, index: usize) -> ListChange {
        if index == self.focus {
            return ListChange::None;
        }
        let page = self.page();
        self.before = self.focus;
        self.focus = index;
        if self.page() == page {
            self.focus_change()
        } else {
            ListChange::Page
        }
    }

    /// Whether the focused row is highlighted; off while the focus is on
    /// another element of the screen.
    pub fn active(&self) -> bool {
        self.active
    }

    pub fn set_active(&mut self, active: bool) {
        self.active = active;
    }

    /// Whether the focus shows at all: on touch screens only once a
    /// button moved it (see [`ListView::focus_on_keys`]).
    pub fn shown(&self) -> bool {
        self.shown
    }

    pub fn set_shown(&mut self, shown: bool) {
        self.shown = shown;
    }

    /// The focus as it last moved within the page: from the item focused
    /// before to the one focused now (both the same where it only came to
    /// show, as after [`confirm`](Self::confirm)).
    pub fn focus_change(&self) -> ListChange {
        let rows = self.rows();
        ListChange::focus(
            self.row_rect(self.before % rows),
            self.row_rect(self.focus % rows),
        )
    }

    /// A grid of `columns` items a row instead of a list (rows are then
    /// `row` high and items as wide as a column).
    pub fn columns(mut self, columns: u32) -> Self {
        self.columns = columns.clamp(1, u32::from(u16::MAX));
        self
    }

    /// The height of a row, at least 1 (`row` is a field anyone can set).
    fn row_height(&self) -> u32 {
        self.row.max(1)
    }

    /// The width of a column, at least 1.
    fn column_width(&self) -> u16 {
        (self.area.width / self.columns as u16).max(1)
    }

    /// Items side by side in a row.
    pub fn column_count(&self) -> usize {
        self.columns as usize
    }

    /// Items per page (rows times columns).
    pub fn rows(&self) -> usize {
        ((u32::from(self.area.height) / self.row_height()).max(1) * self.columns) as usize
    }

    /// The page containing the focus (0-based).
    pub fn page(&self) -> usize {
        self.focus / self.rows()
    }

    /// The page and the number of pages, e.g. for [`Header::draw`]; always
    /// `Some` (a list of one page shows no page number there).
    pub fn paging(&self, len: usize) -> Option<super::Paging> {
        Some(super::Paging::new(self.page(), self.pages(len)))
    }

    pub fn pages(&self, len: usize) -> usize {
        len.div_ceil(self.rows()).max(1)
    }

    /// On touch screens, hides the focus until a button moves it
    /// ([`focus_shown`](super::focus_shown)): taps need no focus, and a
    /// black row nobody chose looks like a mistake.
    pub fn focus_on_keys(mut self, env: &Env<'_>) -> Self {
        self.shown = super::focus_shown(env);
        self
    }

    /// The item a confirming button chooses, or `None` while the focus is
    /// hidden (on touch screens before a button moved it): then this first
    /// press shows the focus instead, so that a button never chooses a row
    /// nobody saw (draw it again: [`focus_change`](Self::focus_change)).
    pub fn confirm(&mut self) -> Option<usize> {
        if self.shown {
            Some(self.focus)
        } else {
            self.shown = true;
            self.before = self.focus;
            None
        }
    }

    /// Keeps the focus among `len` items after the list changed (the last
    /// one if it was past the end); the page follows the focus.
    pub fn set_len(&mut self, len: usize) {
        let last = len.saturating_sub(1);
        if self.focus > last {
            self.before = self.focus;
            self.focus = last;
        }
    }

    /// Moves the focus by a navigation action among `len` items (see
    /// [`list_step`]; a page is a page of the list); `None` if `action`
    /// is another.
    pub fn navigate(&mut self, action: &crate::Id, len: usize) -> Option<ListChange> {
        let delta = list_step(action, self.rows() as isize)?;
        Some(self.move_focus(delta, len))
    }

    /// Moves the focus by `delta` rows within `0..len`; one step past
    /// either end goes round to the other (quicker with few buttons),
    /// larger steps (pages) stop at the end.
    pub fn move_focus(&mut self, delta: isize, len: usize) -> ListChange {
        if len == 0 {
            return ListChange::None;
        }
        if !self.shown && delta.abs() == 1 {
            // The first press of a direction shows where the focus is.
            // Turning pages (also by swiping or a tap on the arrows)
            // does not: on touch screens it stays hidden.
            self.shown = true;
            self.before = self.focus;
            return self.focus_change();
        }
        let focus = step_focus(self.focus, delta, len);
        if focus == self.focus {
            return ListChange::None;
        }
        let page = self.page();
        self.before = self.focus;
        self.focus = focus;
        if self.page() == page {
            self.focus_change()
        } else {
            ListChange::Page
        }
    }

    /// The rectangle of visible item `slot` (0 = top left of the page).
    pub fn row_rect(&self, slot: usize) -> Rect {
        let columns = self.columns as usize;
        let width = self.column_width();
        let row = self.row_height();
        Rect::new(
            self.area.x + (slot % columns) as u16 * width,
            self.area.y + ((slot / columns) as u32 * row) as u16,
            width,
            row as u16,
        )
    }

    /// The index of the item at `point`, if any.
    pub fn hit(&self, point: Point, len: usize) -> Option<usize> {
        if !self.area.contains(point) {
            return None;
        }
        let line = ((point.y - i32::from(self.area.y)) as u32 / self.row_height()) as usize;
        let width = u32::from(self.column_width());
        let column = ((point.x - i32::from(self.area.x)) as u32 / width) as usize;
        let slot = line * self.columns as usize + column.min(self.columns as usize - 1);
        let index = self.page() * self.rows() + slot;
        (slot < self.rows() && index < len).then_some(index)
    }

    /// Draws the visible rows: the focused row inverted, the others with a
    /// separator. `draw_row(canvas, index, rect, focused)` draws a row's
    /// content in black, or white when focused.
    pub fn draw(
        &self,
        ui: &mut super::Ui<'_>,
        len: usize,
        mut draw_row: impl FnMut(&mut super::Ui<'_>, usize, Rect, bool),
    ) {
        let style = ui.style();
        let first = self.page() * self.rows();
        for (slot, index) in (first..len).take(self.rows()).enumerate() {
            let rect = self.row_rect(slot);
            if out_of_sight(ui.canvas(), rect) {
                continue;
            }
            let focused = self.active && self.shown && index == self.focus;
            ui.canvas().describe(Role::Row, "", rect).focused(focused);
            if focused {
                draw_focus(ui.canvas(), style, rect);
            } else if self.columns == 1 {
                draw_separator(ui.canvas(), style, rect, 0);
            }
            draw_row(ui, index, rect, focused);
        }
    }
}

/// Whether a row at `rect` lies outside what `canvas` shows, so that
/// drawing it is wasted: a frame is drawn in bands (the X4's are 192 rows,
/// five to a frame) and the screen's `draw` runs once for each, so a row
/// that lies in one or two of them would otherwise be looked up, measured
/// and laid out five times. Not where someone asks what is drawn
/// ([`Canvas::describe`](crate::Canvas::describe)): that is told the whole
/// screen at once.
pub(crate) fn out_of_sight(canvas: &crate::Canvas<'_>, rect: Rect) -> bool {
    !canvas.is_describing() && canvas.clip().intersection(&rect).is_none()
}

/// The focus `delta` steps on from `focus` among `len` things: one step
/// past either end goes round to the other (quicker with few buttons),
/// larger steps (a page, a row of keys) stop at the end. For screens that
/// move a focus of their own, as [`ListView`] does.
pub fn step_focus(focus: usize, delta: isize, len: usize) -> usize {
    let Some(last) = len.checked_sub(1) else {
        return 0;
    };
    let (last, target) = (last as isize, focus as isize + delta);
    match target {
        t if delta.abs() == 1 && t < 0 => last as usize,
        t if delta.abs() == 1 && t > last => 0,
        t => t.clamp(0, last) as usize,
    }
}
