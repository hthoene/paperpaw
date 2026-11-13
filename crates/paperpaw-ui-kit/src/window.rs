//! A paged list over a [`RowSource`]: only the rows of the page shown are
//! ever built, so a long list (a word list of thousands) costs one page of
//! memory. Rows are all one height (title, and one line of note below when
//! the list has notes), so the page size is known without building any row.
use crate::{
    Messages, Page, Paint, Style, header,
    items::{ItemStep, RowItem},
    paint, text,
};
use alloc::vec::Vec;
use paperpaw_sdk::native::canvas::PartRole;
use paperpaw_sdk::{
    Point, Rect,
    native::{
        Error, Host,
        callbacks::{DrawFrame, DrawPurpose, Input, InputGestureKind},
        commands::{CommandSender, RedrawMode},
        fonts::{Font, FontRole},
        snapshot,
    },
};

/// Where the rows of a [`WindowList`] come from.
pub trait RowSource {
    /// The rows in all.
    fn len(&self) -> usize;

    /// Whether there is no row.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Pushes the rows `first..first + max` (fewer at the end) onto `out`,
    /// which is empty. Only these are held while the page is drawn.
    ///
    /// # Errors
    /// Any error, which stops the draw and leaves the last frame's geometry.
    fn fill(
        &mut self,
        host: &Host<'_>,
        first: usize,
        max: usize,
        out: &mut Vec<RowItem>,
    ) -> Result<(), Error>;
}

/// Which rows a page shows: the arithmetic of a draw, apart from drawing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Window {
    /// The focus, taken into range.
    focus: usize,
    /// The page shown.
    page: usize,
    /// Pages in all, at least 1.
    pages: usize,
    /// The first row of the page.
    first: usize,
}

impl Window {
    fn of(focus: usize, len: usize, per_page: usize) -> Self {
        let per_page = per_page.max(1);
        let pages = len.div_ceil(per_page).max(1);
        let focus = focus.min(len.saturating_sub(1));
        let page = (focus / per_page).min(pages - 1);
        Self {
            focus,
            page,
            pages,
            first: page * per_page,
        }
    }
}

/// Rows of `high` pixels that `room` holds, at least one.
fn per_page(room: u16, high: u16) -> usize {
    (usize::from(room) / usize::from(high.max(1))).max(1)
}

/// A paged list of rows read from a [`RowSource`] page by page. Every row
/// can be chosen; a row's `zone`, `stepper` and `disabled` are not used.
/// Only scalar geometry survives a draw.
///
/// A chosen row is an index counted when the input was decided: the caller
/// must check it against a fresh [`RowSource::len`] before acting on it, for
/// the source may have shrunk since the frame. The drawing has no snapshot
/// test until the host exists; the arithmetic and the input are tested.
#[derive(Debug, Default)]
pub struct WindowList {
    focus: usize,
    /// Rows a page held in the last frame; 0 before the first.
    per_page: usize,
    hits: Vec<(Rect, usize)>,
    header: Page,
    /// Whether rows have a line for a note.
    notes: bool,
}

impl WindowList {
    /// A list focused on row 0. `notes` gives every row a line for a note.
    #[must_use]
    pub fn new(notes: bool) -> Self {
        Self {
            notes,
            ..Self::default()
        }
    }

    /// The focused row.
    #[must_use]
    pub const fn focus(&self) -> usize {
        self.focus
    }

    /// Focuses row `index` (taken in range at the next input or draw).
    pub fn set_focus(&mut self, index: usize) {
        self.focus = index;
    }

    /// Rows a page held in the last frame (0 before the first).
    #[must_use]
    pub const fn per_page(&self) -> usize {
        self.per_page
    }

    /// Where row `index` was drawn in the last frame; `None` off the page.
    #[must_use]
    pub fn rect_of(&self, index: usize) -> Option<Rect> {
        self.hits.iter().find(|h| h.1 == index).map(|h| h.0)
    }

    /// What `input` means for a list of `len` rows, without doing anything.
    /// Page actions move by the page of the last frame.
    #[must_use]
    pub fn decide(&self, len: usize, input: &Input<'_>) -> ItemStep {
        let last = len.saturating_sub(1);
        let focus = self.focus.min(last);
        let page = self.per_page.max(1);
        match *input {
            Input::Action { action, .. } => {
                let target = match action {
                    "core.confirm" if len > 0 => return ItemStep::Chosen(focus),
                    "core.down" | "core.next" | "core.right" => (focus + 1).min(last),
                    "core.up" | "core.previous" | "core.left" => focus.saturating_sub(1),
                    "core.next-page" => ((focus / page + 1) * page).min(last),
                    "core.prev-page" => (focus / page).saturating_sub(1) * page,
                    _ => return ItemStep::Ignored,
                };
                ItemStep::Focus(target)
            }
            Input::Gesture {
                position,
                kind: InputGestureKind::Tap,
                ..
            } => self.tap(position),
            Input::Gesture {
                position,
                kind: InputGestureKind::LongPress,
                ..
            } => match self.tap(position) {
                ItemStep::Chosen(item) => ItemStep::Held(item),
                _ => ItemStep::Ignored,
            },
            _ => ItemStep::Ignored,
        }
    }

    fn tap(&self, position: Point) -> ItemStep {
        if let Some(action) = self.header.action(position) {
            return ItemStep::Header(action);
        }
        match self.hits.iter().find(|(rect, _)| rect.contains(position)) {
            Some((_, item)) => ItemStep::Chosen(*item),
            None => ItemStep::Ignored,
        }
    }

    /// Does what [`Self::decide`] says: moves the focus (with a redraw) or
    /// runs the header's action. A chosen row is only reported.
    ///
    /// # Errors
    /// Propagates command queueing errors; the focus is unchanged then.
    pub fn input(
        &mut self,
        host: &Host<'_>,
        len: usize,
        input: &Input<'_>,
    ) -> Result<ItemStep, Error> {
        let step = self.decide(len, input);
        match step {
            ItemStep::Focus(to) => {
                if to != self.focus {
                    let mut scratch = [0; 64];
                    CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)?;
                    self.focus = to;
                }
            }
            ItemStep::Chosen(item) | ItemStep::Held(item) | ItemStep::Step(item, _) => {
                self.focus = item;
            }
            ItemStep::Header(action) => {
                let mut scratch = [0; 128];
                CommandSender::new(host, &mut scratch).run_action(action)?;
            }
            ItemStep::Ignored => {}
        }
        Ok(step)
    }

    /// Draws the page the focus is on under `title`, asking `source` for the
    /// rows of that page only.
    ///
    /// # Errors
    /// Propagates allocation, font, catalog, source and drawing errors; the
    /// retained geometry stays that of the last completed frame.
    pub fn draw(
        &mut self,
        host: &Host<'_>,
        frame: &mut DrawFrame<'_>,
        title: &str,
        source: &mut impl RowSource,
        shown: bool,
    ) -> Result<(), Error> {
        if !matches!(frame.purpose, DrawPurpose::Screen) {
            return Ok(());
        }
        let display = snapshot::capture_snapshot(host)?;
        let font = Font::acquire_role(host, FontRole::Ui)?;
        let mut style = Style::read(host, &display, &font)?;
        style.describing = paint::asking(frame);
        let (width, height) = (display.display_width(), display.display_height());
        let top = style.header.saturating_add(style.gap);
        let area = Rect::new(0, top, width, height.saturating_sub(top));
        let high = self.row_height(&font, &style)?;
        let per_page = per_page(area.height, high);
        let len = source.len();
        let Window {
            focus,
            page,
            pages,
            first,
        } = Window::of(self.focus, len, per_page);
        let mut header = Page::default();
        let mut messages = Messages::new()?;
        let title = match messages.copy(host, title) {
            Err(Error::NotFound | Error::Invalid) => text::copy(title)?,
            other => other?,
        };
        header::prepare(
            &mut header,
            host,
            &font,
            &style,
            &mut messages,
            &title,
            width,
            display.has_touch(),
            page,
            pages,
        )?;
        let mut rows = Vec::new();
        rows.try_reserve_exact(per_page.min(len - first.min(len)))
            .map_err(|_| Error::NoMemory)?;
        source.fill(host, first, per_page, &mut rows)?;
        rows.truncate(per_page);
        let mut body = Vec::new();
        paint::push(&mut body, Paint::Fill(area, style.paper))?;
        let mut hits = Vec::new();
        hits.try_reserve_exact(rows.len())
            .map_err(|_| Error::NoMemory)?;
        for (slot, row) in rows.iter().enumerate() {
            let y = top.saturating_add((slot as u16).saturating_mul(high));
            let rect = Rect::new(0, y, width, high);
            let index = first + slot;
            let focused = shown && index == focus;
            draw_window_row(&mut body, &font, &style, rect, row, focused)?;
            hits.push((rect, index));
        }
        paint::replay(frame, &font, header.paint.iter().chain(body.iter()))?;
        font.release()?;
        header.paint = Vec::new();
        self.header = header;
        self.hits = hits;
        self.per_page = per_page;
        self.focus = focus;
        Ok(())
    }

    /// The page the next draw shows of a list of `len` rows: its first row and
    /// how many rows a page holds on this display. A source that reads its rows
    /// from storage reads them with this before the draw (a draw reads nothing:
    /// the X4 draws a frame in bands, each a `draw`).
    ///
    /// # Errors
    /// Snapshot, font and style errors.
    pub fn page_of(&self, host: &Host<'_>, len: usize) -> Result<(usize, usize), Error> {
        let display = snapshot::capture_snapshot(host)?;
        let font = Font::acquire_role(host, FontRole::Ui)?;
        let high = Style::read(host, &display, &font).and_then(|style| {
            let top = style.header.saturating_add(style.gap);
            let room = display.display_height().saturating_sub(top);
            self.row_height(&font, &style).map(|high| (room, high))
        });
        font.release()?;
        let (room, high) = high?;
        let per_page = per_page(room, high);
        Ok((Window::of(self.focus, len, per_page).first, per_page))
    }

    /// The one height of every row.
    fn row_height(&self, font: &Font<'_>, style: &Style) -> Result<u16, Error> {
        if !self.notes {
            return Ok(style.row);
        }
        let line = crate::geometry::line_px(font, style.small)?;
        Ok(style.row.saturating_add(line).saturating_add(style.gap / 2))
    }
}

/// A row with its note cut to one line.
fn draw_window_row(
    paint: &mut Vec<Paint>,
    font: &Font<'_>,
    style: &Style,
    rect: Rect,
    row: &RowItem,
    focused: bool,
) -> Result<(), Error> {
    let band = Rect::new(rect.x, rect.y, rect.width, style.row.min(rect.height));
    crate::list::backdrop(paint, style, rect, focused)?;
    paint::describe(paint, style.describing, PartRole::Row, rect, focused, false)?;
    let ink = crate::list::ink(style, focused);
    crate::list::columns(paint, font, style, band, &row.title, &row.value, ink)?;
    if row.note.is_empty() {
        return Ok(());
    }
    let room = f32::from(rect.width.saturating_sub(2 * style.margin)).max(1.0);
    let note = text::fit(font, style.small, &row.note, room)?;
    let line = crate::geometry::line_px(font, style.small)?;
    let color = if focused && !style.ring {
        style.paper
    } else {
        style.note
    };
    text::label(
        paint,
        font,
        &note,
        style.small,
        f32::from(rect.x) + f32::from(style.margin),
        Rect::new(rect.x, band.y.saturating_add(band.height), rect.width, line),
        color,
        Some(rect),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action(action: &str) -> Input<'_> {
        Input::Action { now_ms: 0, action }
    }

    fn list(per_page: usize, focus: usize) -> WindowList {
        WindowList {
            focus,
            per_page,
            ..WindowList::default()
        }
    }

    #[test]
    fn a_window_follows_the_focus_and_stays_in_range() {
        let w = Window::of(7, 2000, 6);
        assert_eq!((w.focus, w.page, w.pages, w.first), (7, 1, 334, 6));
        // The last page holds the rest.
        let w = Window::of(1999, 2000, 6);
        assert_eq!((w.page, w.first), (333, 1998));
        // A source that shrank: the focus comes back to its last row.
        let w = Window::of(1999, 10, 6);
        assert_eq!((w.focus, w.page, w.pages, w.first), (9, 1, 2, 6));
        // Nothing to show is one empty page.
        let w = Window::of(5, 0, 6);
        assert_eq!((w.focus, w.page, w.pages, w.first), (0, 0, 1, 0));
        // A page of zero rows is taken as one.
        assert_eq!(Window::of(3, 5, 0).page, 3);
        // An exact multiple has no empty page after it.
        assert_eq!(Window::of(11, 12, 6).pages, 2);
    }

    #[test]
    fn the_focus_moves_by_row_and_by_page_within_the_whole_list() {
        let l = list(6, 7);
        assert_eq!(l.decide(2000, &action("core.down")), ItemStep::Focus(8));
        assert_eq!(l.decide(2000, &action("core.up")), ItemStep::Focus(6));
        assert_eq!(
            l.decide(2000, &action("core.next-page")),
            ItemStep::Focus(12)
        );
        assert_eq!(
            l.decide(2000, &action("core.prev-page")),
            ItemStep::Focus(0)
        );
        assert_eq!(
            list(6, 0).decide(2000, &action("core.prev-page")),
            ItemStep::Focus(0)
        );
        assert_eq!(
            list(6, 6).decide(2000, &action("core.prev-page")),
            ItemStep::Focus(0)
        );
        assert_eq!(
            list(6, 9).decide(2000, &action("core.prev-page")),
            ItemStep::Focus(0)
        );
        assert_eq!(
            list(6, 13).decide(2000, &action("core.prev-page")),
            ItemStep::Focus(6)
        );
    }

    #[test]
    fn the_focus_stops_at_the_ends_and_an_empty_list_chooses_nothing() {
        assert_eq!(
            list(6, 1999).decide(2000, &action("core.down")),
            ItemStep::Focus(1999)
        );
        assert_eq!(
            list(6, 1999).decide(2000, &action("core.next-page")),
            ItemStep::Focus(1999)
        );
        assert_eq!(
            list(6, 3000).decide(2000, &action("core.confirm")),
            ItemStep::Chosen(1999)
        );
        assert_eq!(
            list(6, 0).decide(0, &action("core.confirm")),
            ItemStep::Ignored
        );
        assert_eq!(
            list(6, 0).decide(0, &action("core.down")),
            ItemStep::Focus(0)
        );
    }

    #[test]
    fn taps_and_holds_choose_the_row_drawn_there_by_its_index_in_the_whole_list() {
        let mut l = list(2, 41);
        l.hits = Vec::from([
            (Rect::new(0, 40, 100, 40), 40),
            (Rect::new(0, 80, 100, 40), 41),
        ]);
        let gesture = |y, kind| Input::Gesture {
            now_ms: 0,
            position: Point::new(5, y),
            kind,
        };
        assert_eq!(
            l.decide(100, &gesture(45, InputGestureKind::Tap)),
            ItemStep::Chosen(40)
        );
        assert_eq!(
            l.decide(100, &gesture(85, InputGestureKind::LongPress)),
            ItemStep::Held(41)
        );
        assert_eq!(
            l.decide(100, &gesture(5, InputGestureKind::Tap)),
            ItemStep::Ignored
        );
        assert_eq!(l.rect_of(41), Some(Rect::new(0, 80, 100, 40)));
        assert_eq!(l.rect_of(0), None);
    }
}
