//! Which page of an [`ItemList`] a row is on, and where an arrow key takes
//! the focus where the arrow keys stay on the page.
use super::{Item, ItemList};

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
