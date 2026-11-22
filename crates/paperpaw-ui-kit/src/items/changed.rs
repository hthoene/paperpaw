//! Drawing again only the rows of an [`ItemList`] that say something else.

use super::{Item, ItemList};
use alloc::vec::Vec;
use paperpaw_sdk::native::{
    Error,
    callbacks::FrameRect,
    commands::{CommandSender, RedrawMode},
};
use paperpaw_sdk::{Rect, native::Host};

/// A key of what a row says: the same texts and flags give the same key.
pub type RowKey = u64;

/// What a list needs drawn after its rows were replaced
/// ([`ItemList::swap_rows`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowsChange {
    /// No row on the page says something else.
    None,
    /// Only the rows in this area (of the frame's region) changed.
    Rows(FrameRect),
    /// The list as a whole: the rows were not drawn before, their number or
    /// the focus changed.
    All,
}

impl RowsChange {
    /// Asks the host for what the change needs: the area, the screen for
    /// [`RowsChange::All`], nothing for [`RowsChange::None`].
    ///
    /// # Errors
    /// Propagates command queueing errors.
    pub fn queue(self, host: &Host<'_>) -> Result<(), Error> {
        let mut scratch = [0; 64];
        let mut sender = CommandSender::new(host, &mut scratch);
        match self {
            Self::None => Ok(()),
            Self::Rows(area) => sender.queue_redraw_area(RedrawMode::Partial, area),
            Self::All => sender.queue_redraw(RedrawMode::Partial),
        }
    }
}

/// FNV-1a, continued over `bytes` and a separator.
fn fold(hash: u64, bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .chain(core::iter::once(&b'\t'))
        .fold(hash, |h, b| {
            (h ^ u64::from(*b)).wrapping_mul(0x0000_0100_0000_01b3)
        })
}

fn key_of(item: &Item) -> RowKey {
    let seed = 0xcbf2_9ce4_8422_2325;
    match item {
        Item::Heading(title) => fold(fold(seed, b"#"), title.as_bytes()),
        Item::Row(row) => {
            let flags = [
                u8::from(row.stepper),
                u8::from(row.track().is_some()),
                u8::from(row.focus_steps()),
                u8::from(row.disabled),
                u8::from(row.field),
                u8::from(row.whole),
                u8::from(row.small),
                u8::from(row.plain),
                row.on.map_or(2, u8::from),
                row.zone.unwrap_or(255),
                u8::from(row.icon.is_some()),
            ];
            let hash = fold(seed, &flags);
            [&row.title, &row.value, &row.note]
                .into_iter()
                .fold(hash, |h, text| fold(h, text.as_bytes()))
        }
    }
}

impl ItemList {
    /// A key of each row, in order, for [`Self::swap_rows`]; `None` where
    /// there is no room for them.
    #[must_use]
    pub fn row_keys(&self) -> Option<Vec<RowKey>> {
        let mut keys = Vec::new();
        keys.try_reserve(self.items.len()).ok()?;
        keys.extend(self.items.iter().map(key_of));
        Some(keys)
    }

    /// Frees the rows and keeps where the last frame drew them, so that the
    /// new rows can be made without the old (the heap has room for one) and
    /// [`Self::swap_rows`] still knows the places. The focus stays.
    pub fn free_rows(&mut self) {
        self.items = Vec::new();
        self.drop_heights();
    }

    /// Takes the rows of `fresh` (made after [`Self::free_rows`]) and says
    /// what needs drawing, `before` being the [`Self::row_keys`] of the rows
    /// replaced: only the rows on the page that say something else, the
    /// whole list where the number of rows or the focus changed or the
    /// page was not drawn. Assumes the rows keep their height (a note wraps
    /// to the same lines).
    pub fn swap_rows(&mut self, mut fresh: Self, before: Option<&[RowKey]>) -> RowsChange {
        let focus = self.focus;
        self.items = core::mem::take(&mut fresh.items);
        self.drop_heights();
        self.focus = self.nearest(focus);
        let Some(before) = before else {
            return RowsChange::All;
        };
        if before.len() != self.items.len() || self.focus != focus || self.hits.is_empty() {
            return RowsChange::All;
        }
        let mut area: Option<Rect> = None;
        for (rect, index) in &self.hits {
            let same = before
                .get(*index)
                .is_some_and(|key| self.items.get(*index).map(key_of) == Some(*key));
            if !same {
                area = Some(area.map_or(*rect, |a| a.union(rect)));
            }
        }
        // A row off the page that changed leaves the page as it is.
        area.map_or(RowsChange::None, |rect| {
            RowsChange::Rows(FrameRect::new(
                i32::from(rect.x).saturating_sub(self.origin.0),
                i32::from(rect.y).saturating_sub(self.origin.1),
                u32::from(rect.width),
                u32::from(rect.height),
            ))
        })
    }
}
