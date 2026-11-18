//! The place a typed page number or percentage names (`native_goto`), and
//! the jump there: the page is laid out, reported and drawn again, as a turn.
use super::{Body, Documents, Error, Host, Open, RedrawMode, View};
use crate::native_goto::{Jump, page_at};
use paperpaw_sdk::native::storage::DataStorage;

impl View {
    /// The pages counted so far in this layout and whether that is all of
    /// them; none where no book is open.
    #[must_use]
    pub fn pages_counted(&self) -> Option<(u32, bool)> {
        let Body::Open(open) = &self.body else {
            return None;
        };
        Some(open.counter.as_ref().map_or((0, false), |c| c.counted()))
    }

    /// Goes where `jump` says: the page before is kept to turn back to, and
    /// the page is laid out, reported and drawn again. Whether it moved.
    ///
    /// # Errors
    /// `NoMemory`, and the host's refusal to redraw.
    pub fn jump(&mut self, host: &Host<'_>, docs: &Documents, jump: Jump) -> Result<bool, Error> {
        let Body::Open(open) = &self.body else {
            return Ok(false);
        };
        let Some(place) = open.place_of(host, jump) else {
            return Ok(false);
        };
        if place == open.at {
            return Ok(false);
        }
        self.go_to(place)?;
        let Body::Open(open) = &mut self.body else {
            return Ok(false);
        };
        // Told now, not when the screen shows again: the keyboard may have
        // closed before its answer came.
        open.report_due = false;
        open.announce(host, docs);
        open.redraw_page(host, docs, RedrawMode::Partial)?;
        Ok(true)
    }
}

impl Open {
    /// The place `jump` names: a page's start from the count, or the place so
    /// many thousandths into the book (the start of its page where the count
    /// is complete, as Beta 3 had it).
    fn place_of(&self, host: &Host<'_>, jump: Jump) -> Option<(u32, u32)> {
        let storage = DataStorage::new(host);
        let counter = self.counter.as_ref();
        match jump {
            Jump::Page(page) => counter?.start(&storage, page),
            Jump::Permille(permille) => match counter.map(|c| c.counted()) {
                Some((pages @ 1.., true)) => counter?.start(&storage, page_at(permille, pages)),
                _ => Some(
                    self.progress
                        .as_ref()?
                        .place_at(permille.saturating_mul(1000)),
                ),
            },
        }
    }
}
