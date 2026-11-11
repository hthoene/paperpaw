//! Op 6 `next_in_series` over the catalog (`docs/abi/library.md` 2.1): of the
//! books in the given book's series that were never opened, the one with the
//! lowest number in it (ties by catalog key); the given book itself when it is
//! that one. `ERR_NOT_FOUND` for a book with no series or a series all of whose
//! books were opened.
//!
//! One scan of the catalog that holds the best book's number and where its
//! line starts, nothing else; the row is then read from its line.
use super::query::{ENTRY_BUFFER, row};
use super::{CatalogReads, fail, zeroed};
use crate::native_host::lib_service::{BookRef, One};
use core::ops::ControlFlow;
use paperpaw_native::host_glue::{ERR_NO_SPACE, ERR_NOT_FOUND, ERR_STATE};
use paperpaw_sdk::library::catalog;

/// The bytes of a series read to compare it (a value is a kilobyte at most).
const SERIES: usize = 1024;

impl CatalogReads {
    /// Op 6, leaving out the books `hide` names (a pending delete holds them).
    pub(crate) fn next_in_series_hiding(
        &self,
        book: BookRef<'_>,
        hide: &dyn Fn(&str) -> bool,
        visit: One<'_>,
    ) -> Result<(), i32> {
        let changes = catalog::try_changes(&*self.storage).map_err(fail)?;
        let offset = self.locate_with(book, &changes)?;
        let mut series = zeroed(SERIES)?;
        let mut seen = None;
        catalog::try_book_at_with(
            &*self.storage,
            &changes,
            offset,
            catalog::BookTexts::new(&mut [], &mut [], &mut series),
            &mut |view, filled| seen = Some((filled.series, view.series_bytes)),
        )
        .map_err(fail)?;
        let (length, whole) = seen.ok_or(ERR_NOT_FOUND)?;
        // A series cut at the buffer is not compared.
        if u32::try_from(length).ok() != Some(whole) {
            return Err(ERR_NO_SPACE);
        }
        let series = core::str::from_utf8(&series[..length]).map_err(|_| ERR_NOT_FOUND)?;
        if series.is_empty() {
            return Err(ERR_NOT_FOUND);
        }
        let mut best = None::<(u32, u64)>;
        catalog::try_each_at(&*self.storage, &mut |at, record| {
            // Opened is the flag or a progress: a book opened and closed
            // before a position was kept has no progress but is opened, as
            // in Beta 3 (`Index`'s unread) and as its `lib_entry` says (flag
            // `opened`); it is not the next one.
            let opened = record.status.opened || record.status.progress.is_some();
            if record.series != series || opened || hide(&record.path) {
                return;
            }
            let order = catalog::series_order(&record.number);
            if best.is_none_or(|found| (order, at) < found) {
                best = Some((order, at));
            }
        })
        .map_err(fail)?;
        let (_, at) = best.ok_or(ERR_NOT_FOUND)?;
        let path = catalog::try_path_at(&*self.storage, at)
            .map_err(fail)?
            .ok_or(ERR_STATE)?;
        let mut buffer = zeroed(ENTRY_BUFFER)?;
        // One row: the page is never full.
        let _ = row(self, &changes, (at, &path), &mut buffer, &mut |record| {
            visit(record);
            ControlFlow::Continue(())
        })?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "series_tests.rs"]
mod tests;
