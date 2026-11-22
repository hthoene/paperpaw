//! Which page of an [`ItemList`] a row is on, where an arrow key takes the
//! focus where the arrow keys stay on the page, and the heading of a section
//! that goes on over a page break, shown again at the top of the next page.
use super::{Error, Font, Item, ItemList, Paint, Rect, Style, heading};
use alloc::vec::Vec;

impl ItemList {
    /// Where an arrow key that steps to `target` (onwards with `forward`)
    /// takes the focus: there, unless the arrow keys stay on the page and it
    /// is off the page of the last frame (or there is none): then to the
    /// page's other end (`focus::round_page`).
    pub(super) fn on_page(&self, target: Option<usize>, forward: bool) -> Option<usize> {
        if self.arrows_stay != Some(true) {
            return target;
        }
        let page = self.page_of(self.focus);
        let start = self.starts.get(page).copied().unwrap_or(0);
        let end = self
            .starts
            .get(page + 1)
            .copied()
            .unwrap_or(self.items.len());
        let rests = |i: &usize| self.items.get(*i).is_some_and(Item::selectable);
        let first = (start..end).find(rests)?;
        let last = (start..end).rev().find(rests)?;
        Some(crate::focus::round_page(target, forward, (first, last)))
    }

    pub(super) fn page_of(&self, item: usize) -> usize {
        self.starts
            .iter()
            .rposition(|start| *start <= item)
            .unwrap_or(0)
    }
}

/// The heading of the section that goes on at the top of the page starting at
/// item `start`: shown there again, as Beta 3 did. None where the page starts
/// with a heading of its own or no heading comes before it.
#[must_use]
pub fn carried_heading(items: &[Item], start: usize) -> Option<usize> {
    if matches!(items.get(start), None | Some(Item::Heading(_))) {
        return None;
    }
    (0..start)
        .rev()
        .find(|i| matches!(items[*i], Item::Heading(_)))
}

/// Draws the heading a page starting at item `start` carries over
/// ([`carried_heading`]) at the top of `rect`, as high as its own; gives where
/// the rows under it begin.
pub(crate) fn draw_carried(
    paint: &mut Vec<Paint>,
    (font, style): (&Font<'_>, &Style),
    (items, heights): (&[Item], &[u16]),
    start: usize,
    rect: Rect,
) -> Result<u16, Error> {
    let Some(at) = carried_heading(items, start) else {
        return Ok(rect.y);
    };
    let (Some(Item::Heading(title)), Some(&high)) = (items.get(at), heights.get(at)) else {
        return Ok(rect.y);
    };
    heading(
        paint,
        font,
        style,
        Rect::new(rect.x, rect.y, rect.width, high),
        title,
    )?;
    Ok(rect.y.saturating_add(high))
}
