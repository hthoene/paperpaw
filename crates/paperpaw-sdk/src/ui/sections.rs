//! Entries under the headings of their sections, on pages (see
//! [`Sections`]), in memory that does not grow with the entries: only how
//! many each section has is kept, and where the entries of a page are is
//! worked out when asked. For lists of any length whose entries are read
//! a page at a time, e.g. from an index on the storage.
//!
//! Every entry is as high as every other, every heading too. A heading
//! never ends a page, and a section that goes on over a page break has its
//! heading again at the top of the next page (as [`Rows`](crate::ui::Rows)).

use alloc::vec::Vec;
use core::ops::Range;

use crate::ui::ListChange;
use crate::{Point, Rect};

/// A section: whether it has a heading, and how many entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Section {
    pub heading: bool,
    pub count: usize,
}

/// What a place on a page shows: section `n`'s heading, or entry `n`
/// (counted over all sections, in their order).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Item {
    Heading(usize),
    Entry(usize),
}

/// Where a page begins: its section, and the entry in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Start {
    section: usize,
    within: usize,
}

/// The list; see the [module documentation](self).
#[derive(Debug)]
pub struct Sections {
    area: Rect,
    heading: u16,
    row: u16,
    sections: Vec<Section>,
    /// The focused entry.
    focus: usize,
    /// The entry focused before the focus last moved.
    before: usize,
}

impl Sections {
    /// Where the rows are drawn.
    pub fn area(&self) -> Rect {
        self.area
    }

    /// The row with the focus.
    pub fn focus(&self) -> usize {
        self.focus
    }

    /// `sections` in `area`, their headings `heading` high, their entries
    /// `row`. Sections without entries are not shown.
    pub fn new(area: Rect, heading: u16, row: u16, sections: Vec<Section>) -> Self {
        Self {
            area,
            heading,
            row: row.max(1),
            sections,
            focus: 0,
            before: 0,
        }
    }

    /// The entries in all.
    pub fn len(&self) -> usize {
        self.sections.iter().map(|s| s.count).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The section of entry `entry`, and its place in it.
    pub fn section_of(&self, entry: usize) -> Option<(usize, usize)> {
        let mut first = 0;
        for (n, section) in self.sections.iter().enumerate() {
            if entry < first + section.count {
                return Some((n, entry - first));
            }
            first += section.count;
        }
        None
    }

    /// The first entry of `start`.
    fn entry_at(&self, start: Start) -> usize {
        self.sections[..start.section]
            .iter()
            .map(|s| s.count)
            .sum::<usize>()
            + start.within
    }

    /// Lays out the page beginning at `start`, telling `place` each item
    /// and where it goes; where the next page begins, `None` after the
    /// last.
    fn lay_out(&self, start: Start, place: &mut dyn FnMut(Item, Rect)) -> Option<Start> {
        let height = self.area.height;
        let mut y = 0u16;
        let mut placed = false;
        let mut at = start;
        let mut entry = self.entry_at(start);
        while at.section < self.sections.len() {
            let section = self.sections[at.section];
            if at.within >= section.count {
                at = Start {
                    section: at.section + 1,
                    within: 0,
                };
                continue;
            }
            if section.heading && (at.within == 0 || !placed) {
                // A heading goes with at least one of its entries.
                if placed && y + self.heading + self.row > height {
                    return Some(at);
                }
                place(Item::Heading(at.section), self.rect(y, self.heading));
                y += self.heading;
            }
            while at.within < section.count {
                if placed && y + self.row > height {
                    return Some(at);
                }
                place(Item::Entry(entry), self.rect(y, self.row));
                placed = true;
                y += self.row;
                at.within += 1;
                entry += 1;
            }
        }
        None
    }

    fn rect(&self, y: u16, height: u16) -> Rect {
        Rect::new(self.area.x, self.area.y + y, self.area.width, height)
    }

    /// The page holding entry `entry`, where it begins, and how many
    /// pages there are before it.
    fn page_of(&self, entry: usize) -> (usize, Start) {
        let mut start = Start::default();
        let mut page = 0;
        loop {
            let first = self.entry_at(start);
            let next = self.lay_out(start, &mut |_, _| {});
            match next {
                Some(next) if self.entry_at(next) <= entry => {
                    if self.entry_at(next) == first {
                        // Nothing fits; no endless pages.
                        return (page, start);
                    }
                    start = next;
                    page += 1;
                }
                _ => return (page, start),
            }
        }
    }

    /// The page with the focus, and the number of pages.
    pub fn paging(&self) -> super::Paging {
        let (page, _) = self.page_of(self.focus);
        let (last, _) = self.page_of(self.len().saturating_sub(1));
        super::Paging::new(page, last + 1)
    }

    /// What the page with the focus shows, and where.
    pub fn visible(&self) -> Vec<(Item, Rect)> {
        let (_, start) = self.page_of(self.focus);
        let mut items = Vec::new();
        self.lay_out(start, &mut |item, rect| items.push((item, rect)));
        items
    }

    /// The entries on the page with the focus.
    pub fn page_entries(&self) -> Range<usize> {
        let (_, start) = self.page_of(self.focus);
        let first = self.entry_at(start);
        let end = self
            .lay_out(start, &mut |_, _| {})
            .map_or(self.len(), |next| self.entry_at(next));
        first..end
    }

    /// Where entry `entry` is, if on the page with the focus.
    pub fn rect_of(&self, entry: usize) -> Option<Rect> {
        self.visible()
            .into_iter()
            .find(|(item, _)| *item == Item::Entry(entry))
            .map(|(_, rect)| rect)
    }

    /// The entry at `point`, if any (headings are not hit).
    pub fn hit(&self, point: Point) -> Option<usize> {
        self.visible()
            .into_iter()
            .find_map(|(item, rect)| match item {
                Item::Entry(entry) if rect.contains(point) => Some(entry),
                _ => None,
            })
    }

    /// Draws the page with the focus with `draw_row(canvas, item, rect,
    /// focused)`: the focused entry (if `focus` shows) on black, thin lines
    /// between entries but not above a heading; with
    /// [`UiStyle::cards`](crate::ui::UiStyle::cards) the entries between headings on cards.
    pub fn draw(
        &self,
        ui: &mut super::Ui<'_>,
        focus: bool,
        mut draw_row: impl FnMut(&mut super::Ui<'_>, Item, Rect, bool),
    ) {
        let style = ui.style();
        let visible = self.visible();
        if style.cards {
            // Entry `e`'s section, its place there, whether it has a heading.
            let at = |e: usize| {
                self.section_of(e)
                    .map(|(s, w)| (s, w, self.sections[s].heading))
            };
            super::rows::draw_cards(
                ui.canvas(),
                style,
                &visible,
                |item| matches!(item, Item::Entry(_)),
                // An entry just before or after it, no heading between,
                // on another page.
                |item| match item {
                    Item::Entry(e) => e > 0 && at(e).is_some_and(|(_, w, h)| w > 0 || !h),
                    Item::Heading(_) => false,
                },
                |item| match item {
                    Item::Entry(e) => match (at(e), at(e + 1)) {
                        (Some((s, ..)), Some((next, _, h))) => next == s || !h,
                        _ => false,
                    },
                    Item::Heading(_) => false,
                },
            );
        }
        for (n, &(item, rect)) in visible.iter().enumerate() {
            // Outside the band drawn now: drawn in its own.
            if !super::shows(ui.canvas(), rect) {
                continue;
            }
            let focused = focus && item == Item::Entry(self.focus);
            if let Item::Entry(_) = item {
                ui.canvas()
                    .describe(crate::semantics::Role::Row, "", rect)
                    .focused(focused);
            }
            if focused {
                super::rows::draw_entry_focus(ui.canvas(), style, rect);
            } else if matches!(item, Item::Entry(_))
                && matches!(visible.get(n + 1), Some((Item::Entry(_), _)))
            {
                crate::ui::draw_separator(ui.canvas(), style, rect, 0);
            }
            draw_row(ui, item, rect, focused);
        }
    }

    /// The focus as it last moved within the page: from the entry
    /// focused before to the one focused now.
    pub fn focus_change(&self) -> ListChange {
        let rect = |entry| self.rect_of(entry).unwrap_or(self.area);
        ListChange::focus(rect(self.before), rect(self.focus))
    }

    /// Puts the focus on entry `focus`; what that changed on the screen.
    pub fn set_focus(&mut self, focus: usize) -> ListChange {
        if focus == self.focus {
            return ListChange::None;
        }
        let page = self.page_of(self.focus).0;
        self.before = self.focus;
        self.focus = focus;
        if self.page_of(focus).0 == page {
            self.focus_change()
        } else {
            ListChange::Page
        }
    }

    /// Moves the focus to entry `entry` (the last where there are fewer).
    pub fn focus_on(&mut self, entry: usize) -> ListChange {
        self.set_focus(entry.min(self.len().saturating_sub(1)))
    }

    /// Moves the focus to the next (`forward`) or previous entry; past
    /// the last it goes round to the first, and back.
    pub fn step(&mut self, forward: bool) -> ListChange {
        let len = self.len();
        if len == 0 {
            return ListChange::None;
        }
        let next = if forward {
            (self.focus + 1) % len
        } else {
            (self.focus + len - 1) % len
        };
        self.set_focus(next)
    }

    /// Turns to the next (`forward`) or previous page, focusing its first
    /// entry.
    pub fn turn(&mut self, forward: bool) -> ListChange {
        let here = self.page_entries();
        if forward {
            if here.end >= self.len() {
                return ListChange::None;
            }
            self.set_focus(here.end)
        } else {
            let Some(before) = here.start.checked_sub(1) else {
                return ListChange::None;
            };
            let (_, start) = self.page_of(before);
            let first = self.entry_at(start);
            self.set_focus(first)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_platform::Color;

    /// Headings 10 high, entries 20, a page 60: a heading and two entries,
    /// or three entries.
    fn list(sections: &[(bool, usize)]) -> Sections {
        let sections = sections
            .iter()
            .map(|&(heading, count)| Section { heading, count })
            .collect();
        Sections::new(Rect::new(0, 0, 100, 60), 10, 20, sections)
    }

    fn page(list: &Sections) -> Vec<Item> {
        list.visible().into_iter().map(|(item, _)| item).collect()
    }

    #[test]
    fn headings_go_with_their_entries_and_come_again() {
        // No heading, one entry; a heading and three; one empty; a heading
        // and one.
        let mut list = list(&[(false, 1), (true, 3), (true, 0), (true, 1)]);
        assert_eq!(list.len(), 5);
        assert_eq!(
            page(&list),
            [Item::Entry(0), Item::Heading(1), Item::Entry(1)]
        );
        assert_eq!(list.turn(true), ListChange::Page);
        // The section goes on: its heading again.
        assert_eq!(
            page(&list),
            [Item::Heading(1), Item::Entry(2), Item::Entry(3)]
        );
        list.turn(true);
        assert_eq!(page(&list), [Item::Heading(3), Item::Entry(4)]);
        assert_eq!(list.paging(), crate::ui::Paging::new(2, 3));
        assert_eq!(list.turn(true), ListChange::None);
        assert_eq!(list.section_of(4), Some((3, 0)));
        list.turn(false);
        assert_eq!(list.focus, 2);
    }

    #[test]
    fn steps_go_round_past_the_ends() {
        let mut list = list(&[(true, 2), (true, 2)]);
        assert_eq!(list.step(false), ListChange::Page);
        assert_eq!(list.focus, 3);
        assert_eq!(list.step(true), ListChange::Page);
        assert_eq!(list.focus, 0);
        assert!(list.step(true).is_focus());
        assert_eq!(list.page_entries(), 0..2);
        assert_eq!(list.hit(Point::new(5, 35)), Some(1));
        assert_eq!(list.hit(Point::new(5, 5)), None);
    }

    #[test]
    fn cards_are_open_where_a_page_cuts_them() {
        use crate::ui::rows::{card_frame, card_style};
        let style = card_style();
        let kit = crate::test_kit::Kit::new(false);
        let env = kit.env();
        // Black where a card's line is across the list at `y`.
        let line = |list: &Sections, y: u16| {
            let frame = card_frame(|canvas| {
                list.draw(
                    &mut crate::ui::Ui::new(canvas, &env, &style),
                    false,
                    |_, _, _, _| {},
                )
            });
            frame.view().pixel(50, y) == Color::BLACK
        };
        // A heading and two entries a page, its heading again on the next.
        let mut headed = list(&[(true, 4)]);
        assert!(line(&headed, 10) && !line(&headed, 49), "open below");
        headed.turn(true);
        assert!(
            line(&headed, 10) && line(&headed, 49),
            "closed below its heading"
        );
        // No headings: three entries a page.
        let mut plain = list(&[(false, 5)]);
        assert!(line(&plain, 0) && !line(&plain, 59));
        plain.turn(true);
        assert!(!line(&plain, 0) && line(&plain, 39), "open above");
    }

    #[test]
    fn many_entries_take_no_more_memory() {
        let mut list = list(&[(true, 100_000)]);
        list.focus_on(99_999);
        assert!(list.paging().is_last());
        assert_eq!(list.page_entries().end, 100_000);
        let empty = Sections::new(Rect::new(0, 0, 100, 5), 10, 20, alloc::vec![]);
        assert!(empty.is_empty());
        assert!(empty.visible().is_empty());
    }
}
