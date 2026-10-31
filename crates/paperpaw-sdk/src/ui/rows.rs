//! Rows of different heights on pages (see [`Rows`]): entries and the headings of
//! sections, which take less room and are never focused. A section that
//! goes on over a page break has its heading again at the top of the next
//! page.

use alloc::vec::Vec;
use core::ops::Range;

use paperpaw_platform::Color;

use crate::ui::{ListChange, UiStyle};
use crate::{Canvas, Point, Rect};

#[derive(Debug)]
pub struct Rows {
    area: Rect,
    heights: Vec<u16>,
    headings: Vec<bool>,
    /// Entries the focus passes over with the buttons (see
    /// [`Rows::set_passed`]).
    passed: Vec<bool>,
    /// The rows of each page, and the heading repeated at its top.
    pages: Vec<(Option<usize>, Range<usize>)>,
    /// The focused row; never a heading while there are entries.
    focus: usize,
    /// The row focused before the focus last moved.
    before: usize,
}

impl Rows {
    /// Where the rows are drawn.
    pub fn area(&self) -> Rect {
        self.area
    }

    /// The row with the focus.
    pub fn focus(&self) -> usize {
        self.focus
    }

    /// Rows of `heights` in `area`; `headings` marks the headings (rows
    /// it does not reach are entries, marks past the rows are left out).
    pub fn new(area: Rect, heights: Vec<u16>, mut headings: Vec<bool>) -> Self {
        headings.resize(heights.len(), false);
        let mut pages: Vec<(Option<usize>, Range<usize>)> = Vec::new();
        let mut start = 0;
        let mut carried: Option<usize> = None;
        // Wider than the heights: many rows add up past `u16`.
        let mut used = 0u32;
        let room = u32::from(area.height);
        for (i, &height) in heights.iter().enumerate() {
            let height = u32::from(height);
            if i > start && used + height > room {
                // A heading never ends a page: it goes with its entries.
                let end = if headings[i - 1] && i - 1 > start {
                    i - 1
                } else {
                    i
                };
                pages.push((carried, start..end));
                // The section going on: its heading again.
                carried = (!headings[end])
                    .then(|| (0..end).rev().find(|&h| headings[h]))
                    .flatten();
                used = carried.map_or(0, |h| u32::from(heights[h]))
                    + heights[end..i].iter().map(|&h| u32::from(h)).sum::<u32>();
                start = end;
            }
            used += height;
        }
        pages.push((carried, start..heights.len()));
        let focus = headings.iter().position(|h| !h).unwrap_or(0);
        Self {
            area,
            heights,
            headings,
            passed: Vec::new(),
            pages,
            focus,
            before: focus,
        }
    }

    pub fn len(&self) -> usize {
        self.heights.len()
    }

    pub fn is_empty(&self) -> bool {
        self.heights.is_empty()
    }

    /// The page holding row `index`.
    fn page_of(&self, index: usize) -> usize {
        self.pages
            .iter()
            .position(|(_, rows)| rows.contains(&index))
            .unwrap_or(0)
    }

    /// The page with the focus, and the number of pages.
    pub fn paging(&self) -> super::Paging {
        super::Paging::new(self.page_of(self.focus), self.pages.len())
    }

    /// The rows of the page with the focus and where they are.
    pub fn visible(&self) -> Vec<(usize, Rect)> {
        let (carried, rows) = self.pages[self.page_of(self.focus)].clone();
        let mut y = self.area.y;
        carried
            .into_iter()
            .chain(rows)
            .map(|i| {
                let rect = Rect::new(self.area.x, y, self.area.width, self.heights[i]);
                y += self.heights[i];
                (i, rect)
            })
            .collect()
    }

    pub fn rect_of(&self, index: usize) -> Option<Rect> {
        self.visible()
            .into_iter()
            .find(|(i, _)| *i == index)
            .map(|(_, rect)| rect)
    }

    pub fn is_heading(&self, index: usize) -> bool {
        self.headings.get(index).copied().unwrap_or(false)
    }

    /// Marks the entries the focus passes over when it moves (those shown
    /// faint, which cannot be chosen now); they are still drawn and hit.
    /// Where every entry is passed, the focus moves as without marks.
    pub fn set_passed(&mut self, passed: Vec<bool>) {
        self.passed = passed;
    }

    /// Whether the focus moving stops at row `index`.
    fn stops_at(&self, index: usize, all_passed: bool) -> bool {
        !self.is_heading(index) && (all_passed || !self.passed.get(index).copied().unwrap_or(false))
    }

    /// Every entry is passed (or there are none).
    fn all_passed(&self) -> bool {
        (0..self.len()).all(|i| self.is_heading(i) || self.passed.get(i).copied().unwrap_or(false))
    }

    /// Draws the rows of the focused page with `draw_row(canvas, index,
    /// rect, focused)`, headings included: the focused entry (if `focus`
    /// shows) on black, thin lines between entries but not above a
    /// heading; with [`UiStyle::cards`] the entries between headings on
    /// rounded cards.
    pub fn draw(
        &self,
        ui: &mut super::Ui<'_>,
        focus: bool,
        mut draw_row: impl FnMut(&mut super::Ui<'_>, usize, Rect, bool),
    ) {
        let style = ui.style();
        let visible = self.visible();
        if style.cards {
            let entry = |i: usize| i < self.len() && !self.is_heading(i);
            draw_cards(
                ui.canvas(),
                style,
                &visible,
                entry,
                // An entry just before or after it, on another page.
                |i| i > 0 && entry(i - 1),
                |i| entry(i + 1),
            );
        }
        for (n, &(index, rect)) in visible.iter().enumerate() {
            // Outside the band drawn now: drawn in its own.
            if !super::shows(ui.canvas(), rect) {
                continue;
            }
            let focused = focus && index == self.focus && !self.is_heading(index);
            if !self.is_heading(index) {
                ui.canvas()
                    .describe(crate::semantics::Role::Row, "", rect)
                    .focused(focused);
            }
            if focused {
                draw_entry_focus(ui.canvas(), style, rect);
            } else if !self.is_heading(index)
                && visible
                    .get(n + 1)
                    .is_some_and(|&(next, _)| !self.is_heading(next))
            {
                crate::ui::draw_separator(ui.canvas(), style, rect, 0);
            }
            draw_row(ui, index, rect, focused);
        }
    }

    /// The focus as it last moved within the page: from the row focused
    /// before to the one focused now (both the same where it only came to
    /// show).
    pub fn focus_change(&self) -> ListChange {
        let rect = |index| self.rect_of(index).unwrap_or(self.area);
        ListChange::focus(rect(self.before), rect(self.focus))
    }

    /// The entry at `point`, if any (headings are not hit).
    pub fn hit(&self, point: Point) -> Option<usize> {
        self.visible()
            .into_iter()
            .find(|(i, rect)| rect.contains(point) && !self.is_heading(*i))
            .map(|(i, _)| i)
    }

    /// Puts the focus on row `focus`; what that changed on the screen.
    pub fn set_focus(&mut self, focus: usize) -> ListChange {
        if focus == self.focus {
            return ListChange::None;
        }
        let page = self.page_of(self.focus);
        self.before = self.focus;
        self.focus = focus;
        if self.page_of(focus) == page {
            self.focus_change()
        } else {
            ListChange::Page
        }
    }

    /// Moves the focus to the next (`forward`) or previous entry; past
    /// the last it goes round to the first, and back (quicker with few
    /// buttons).
    pub fn step(&mut self, forward: bool) -> ListChange {
        let (all, this) = (self.all_passed(), &*self);
        let entries =
            |range: core::ops::Range<usize>| range.filter(move |&i| this.stops_at(i, all));
        let next = if forward {
            entries(self.focus + 1..self.len())
                .next()
                .or_else(|| entries(0..self.len()).next())
        } else {
            entries(0..self.focus)
                .next_back()
                .or_else(|| entries(0..self.len()).next_back())
        };
        next.map_or(ListChange::None, |i| self.set_focus(i))
    }

    /// Turns to the next (`forward`) or previous page, focusing its
    /// first entry.
    pub fn turn(&mut self, forward: bool) -> ListChange {
        let page = self.page_of(self.focus);
        let target = if forward {
            page + 1
        } else {
            match page.checked_sub(1) {
                Some(page) => page,
                None => return ListChange::None,
            }
        };
        let Some((_, range)) = self.pages.get(target).cloned() else {
            return ListChange::None;
        };
        // Its first entry not passed, else its first.
        let all = self.all_passed();
        let first = range
            .clone()
            .find(|&i| self.stops_at(i, all))
            .or_else(|| range.clone().find(|&i| !self.is_heading(i)));
        match first {
            Some(first) => self.set_focus(first),
            None => ListChange::None,
        }
    }
}

/// Draws each run of entries in `visible` (rows between headings, where
/// `entry` says it is one) on a rounded card, as [`UiStyle::cards`] asks:
/// its sides where the focus of rows is ([`focus_rect`]), so the text in
/// it stays on the margin; a very light gray on panels of 16 grays, a
/// thin line round it on fewer. A run that goes on over a page break
/// (`before` its first entry, `after` its last) is open there, except
/// where its heading comes again above it.
///
/// [`focus_rect`]: crate::ui::focus_rect
pub(super) fn draw_cards<T: Copy>(
    canvas: &mut Canvas<'_>,
    style: &UiStyle,
    visible: &[(T, Rect)],
    entry: impl Fn(T) -> bool,
    before: impl Fn(T) -> bool,
    after: impl Fn(T) -> bool,
) {
    for run in visible.split(|&(item, _)| !entry(item)) {
        let (Some(&(first, top)), Some(&(last, bottom))) = (run.first(), run.last()) else {
            continue;
        };
        let side = (style.margin / 2) as u16;
        let side = side.min(top.width / 8);
        let width = top.width - 2 * side;
        let end = bottom.y + bottom.height;
        let shown = Rect::new(top.x + side, top.y, width, end - top.y);
        // Past a cut by more than a corner, so neither shows there.
        let reach = (style.corner + style.line) as i32 + 1;
        let open = visible.first().is_some_and(|&(_, at)| at == top) && before(first);
        let y = i32::from(top.y) - if open { reach } else { 0 };
        let height = (i32::from(end) + if after(last) { reach } else { 0 } - y) as u32;
        let x = i32::from(shown.x);
        let width = u32::from(width);
        let mut canvas = canvas.clipped(shown);
        if style.grays >= 16 {
            canvas.fill_round_rect(
                x,
                y,
                width,
                height,
                style.corner,
                style.ink(super::Ink::SECTION),
            );
        } else {
            let line = (style.line / 2).max(1);
            canvas.stroke_round_rect(x, y, width, height, line, style.corner, Color::BLACK);
        }
    }
}

/// Draws the focus of the entry in `rect`, kept off the edges of its
/// card with [`UiStyle::cards`], its corners following the card's.
pub(super) fn draw_entry_focus(canvas: &mut Canvas<'_>, style: &UiStyle, rect: Rect) {
    if !style.cards {
        return crate::ui::draw_focus(canvas, style, rect);
    }
    // As far in from the card's sides as from the row's ends (less what
    // the focus keeps from them anyway), drawn as for a row whose margin
    // is what is left to the text (e.g. for a marker in there).
    let inset = (style.margin / 6).max(style.line + 1).min(style.margin / 2);
    let edge = (style.line as u16).min(rect.height / 8);
    let side = (2 * inset) as u16;
    let rect = rect.inset(
        side,
        (inset as u16).saturating_sub(edge).min(rect.height / 4),
    );
    let style = UiStyle {
        margin: style.margin - 2 * inset,
        corner: style.corner.saturating_sub(inset),
        ..*style
    };
    crate::ui::draw_focus(canvas, &style, rect);
}

/// A style with cards on black and white, for tests.
#[cfg(test)]
pub(super) fn card_style() -> UiStyle {
    UiStyle {
        text: 20.0,
        small: 15.0,
        title: 23.0,
        head: 23.0,
        row: 20,
        control_row: 20,
        header: 24,
        margin: 12,
        gap: 8,
        line: 2,
        dark_header: false,
        corner: 4,
        strong: false,
        pill: false,
        focus: crate::ui::Focus::Fill,
        separators: crate::ui::Separators::None,
        plain_header: false,
        cards: true,
        grays: 2,
        finger: 0,
        look: crate::ui::look::Look::none(),
        palette: crate::ui::Palette::DEFAULT,
        direction: 0,
    }
}

/// What `draw` draws on black and white, 100 by 70.
#[cfg(test)]
pub(super) fn card_frame(draw: impl FnOnce(&mut Canvas<'_>)) -> paperpaw_graphics::Framebuffer {
    use paperpaw_platform::PixelFormat;
    let mut frame = paperpaw_graphics::Framebuffer::new(100, 70, PixelFormat::Mono1).unwrap();
    let mut canvas = frame.canvas();
    canvas.clear(Color::WHITE);
    draw(&mut canvas);
    frame
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_go_round_past_the_ends() {
        // A heading, two entries, a heading, one entry.
        let headings = alloc::vec![true, false, false, true, false];
        let mut rows = Rows::new(Rect::new(0, 0, 100, 500), alloc::vec![20; 5], headings);
        rows.focus = 1;
        rows.step(false);
        assert_eq!(rows.focus, 4, "back from the first: the last");
        rows.step(true);
        assert_eq!(
            rows.focus, 1,
            "on from the last: the first, past the heading"
        );
        rows.step(true);
        rows.step(true);
        assert_eq!(rows.focus, 4, "over the heading between");
    }

    #[test]
    fn steps_pass_faint_entries() {
        // A heading, three entries, the middle one faint.
        let headings = alloc::vec![true, false, false, false];
        let mut rows = Rows::new(Rect::new(0, 0, 100, 500), alloc::vec![20; 4], headings);
        rows.set_passed(alloc::vec![false, false, true]);
        rows.step(true);
        assert_eq!(rows.focus, 3, "over the faint one");
        rows.step(false);
        assert_eq!(rows.focus, 1);
        // All faint: the focus moves as before.
        rows.set_passed(alloc::vec![false, true, true, true]);
        rows.step(true);
        assert_eq!(rows.focus, 2);
    }

    #[test]
    fn marks_and_heights_of_different_lengths_do_not_panic() {
        let area = Rect::new(0, 0, 100, 50);
        // Fewer marks than rows: the rest are entries.
        let rows = Rows::new(area, alloc::vec![20; 4], alloc::vec![true]);
        assert_eq!(rows.len(), 4);
        assert_eq!(rows.focus, 1);
        // More marks than rows: those past them left out.
        let rows = Rows::new(area, alloc::vec![20; 2], alloc::vec![false; 6]);
        assert_eq!(rows.len(), 2);
        // Rows adding up past what a u16 holds still go on pages.
        let rows = Rows::new(area, alloc::vec![u16::MAX; 3], alloc::vec![]);
        assert_eq!(rows.paging().pages, 3);
    }

    #[test]
    fn headings_go_with_their_entries() {
        // Heading, two entries, heading, two entries; three rows a page.
        let heights = Vec::from([5, 10, 10, 5, 10, 10]);
        let headings = Vec::from([true, false, false, true, false, false]);
        let mut rows = Rows::new(Rect::new(0, 0, 100, 30), heights, headings);
        assert_eq!(rows.pages, [(None, 0..3), (None, 3..6)]);
        assert_eq!(rows.focus, 1, "the first entry");
        assert!(rows.step(true).is_focus());
        assert_eq!(rows.step(true), ListChange::Page, "over the heading");
        assert_eq!(rows.focus, 4);
        assert_eq!(rows.paging(), super::super::Paging::new(1, 2));
        assert_eq!(rows.turn(false), ListChange::Page);
        assert_eq!(rows.focus, 1);
        let visible = rows.visible();
        assert_eq!(visible[0].1, Rect::new(0, 0, 100, 5));
        assert_eq!(visible[1].1, Rect::new(0, 5, 100, 10));
        assert_eq!(rows.hit(Point { x: 3, y: 2 }), None, "a heading");
        assert_eq!(rows.hit(Point { x: 3, y: 7 }), Some(1));
    }

    #[test]
    fn a_heading_never_ends_a_page() {
        let heights = Vec::from([10, 10, 5, 10]);
        let headings = Vec::from([false, false, true, false]);
        let rows = Rows::new(Rect::new(0, 0, 100, 25), heights, headings);
        assert_eq!(rows.pages, [(None, 0..2), (None, 2..4)]);
    }

    #[test]
    fn a_section_going_on_repeats_its_heading() {
        // Heading, four entries; two entries fit a page with the heading.
        let heights = Vec::from([5, 10, 10, 10, 10]);
        let headings = Vec::from([true, false, false, false, false]);
        let mut rows = Rows::new(Rect::new(0, 0, 100, 25), heights, headings);
        assert_eq!(rows.pages, [(None, 0..3), (Some(0), 3..5)]);
        rows.turn(true);
        let visible: Vec<usize> = rows.visible().into_iter().map(|(i, _)| i).collect();
        assert_eq!(visible, [0, 3, 4], "the heading first");
        assert_eq!(rows.focus, 3);
    }

    #[test]
    fn rows_outside_the_band_are_left_out_but_still_told() {
        use paperpaw_platform::PixelFormat;
        let style = crate::ui::tests::STYLE;
        let kit = crate::test_kit::Kit::new(false);
        let env = kit.env();
        let area = Rect::new(0, 0, 100, 80);
        let rows = Rows::new(area, alloc::vec![20; 4], alloc::vec![false; 4]);
        let mut frame = paperpaw_graphics::Framebuffer::new(100, 80, PixelFormat::Mono1).unwrap();
        // A band from the second row into the third.
        let drawn = |canvas: &mut Canvas<'_>| {
            let mut band = canvas.clipped(Rect::new(0, 25, 100, 20));
            let mut drawn = Vec::new();
            let mut ui = crate::ui::Ui::new(&mut band, &env, &style);
            rows.draw(&mut ui, false, |_, index, _, _| drawn.push(index));
            drawn
        };
        assert_eq!(drawn(&mut frame.canvas()), [1, 2]);
        let mut described = Vec::new();
        let all = drawn(&mut frame.canvas().describing(&mut described));
        assert_eq!(all, [0, 1, 2, 3], "a screen reader hears every row");
        assert_eq!(described.len(), 4);
    }

    #[test]
    fn cards_are_open_where_a_page_cuts_them() {
        let style = card_style();
        let kit = crate::test_kit::Kit::new(false);
        let env = kit.env();
        // Four entries, two a page, no headings.
        let mut rows = Rows::new(
            Rect::new(0, 0, 100, 40),
            alloc::vec![20; 4],
            alloc::vec![false; 4],
        );
        let draw = |rows: &Rows| {
            card_frame(|canvas| {
                rows.draw(
                    &mut crate::ui::Ui::new(canvas, &env, &style),
                    false,
                    |_, _, _, _| {},
                )
            })
        };
        let first = draw(&rows);
        let (top, bottom) = (first.view().pixel(50, 0), first.view().pixel(50, 39));
        assert_eq!((top, bottom), (Color::BLACK, Color::WHITE), "open below");
        assert_eq!(
            first.view().pixel(6, 39),
            Color::BLACK,
            "its side down to the cut"
        );
        assert_eq!(first.view().pixel(6, 40), Color::WHITE, "not past it");
        rows.turn(true);
        let second = draw(&rows);
        let (top, bottom) = (second.view().pixel(50, 0), second.view().pixel(50, 39));
        assert_eq!((top, bottom), (Color::WHITE, Color::BLACK), "open above");
        assert_eq!(second.view().pixel(6, 0), Color::BLACK);
    }
}
