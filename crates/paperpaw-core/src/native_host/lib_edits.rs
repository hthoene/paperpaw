//! The staged annotation transactions of `core.library` on the card (ops 17
//! to 21 and 26, `library.md` 2.2): what the service's table does not hold.
//!
//! - [`stage`]: the edits and the parts of a transaction go to one file on the
//!   card, `.paperpaw/library/edits/<transaction>.stage`, and are read back
//!   record by record at the commit; a power cut leaves them behind and the
//!   next start removes them.
//! - [`commit`]: compares the book's annotation generation, checks every edit
//!   (unknown ids, repeated ids and client keys), then rewrites the book's
//!   annotation file as version 3 through `paperpaw_sdk::annotations::write`
//!   (a temporary file, renamed over the old one): all the edits or none.
//! - [`receipt`]: the ids of a committed transaction, a file of fixed rows,
//!   kept until the plugin's next `edit_begin`, 10 minutes or its stop.
//!
//! Memory: a table of the transaction's edits (at most 1,024 of 40 bytes,
//! exactly as many as it has), a buffer of 5 KiB for one staged record and
//! the texts of the one row being written (at most a line of 8 KiB); every
//! allocation is fallible and the answer is `ERR_NO_MEMORY`, nothing changed.
use super::lib_reads::CatalogEnv;
use super::lib_service::{BookId, CommitRequest, Committed, StageItem, Transactions};
use crate::native_host::registrations::Record;
use alloc::{rc::Rc, string::String, vec::Vec};
use core::cell::RefCell;
use core::ops::ControlFlow;
use paperpaw_native::host_glue::ERR_NO_MEMORY;
use paperpaw_platform::Storage;

mod commit;
mod receipt;
mod stage;

/// How many books' paths are remembered for the change event: a transaction
/// lives for 10 minutes at most and the device holds two.
const PATHS: usize = 4;

pub(crate) struct LibEdits {
    stage: stage::Stage,
    transactions: Option<Transactions>,
    env: Rc<dyn CatalogEnv>,
    /// The books transactions were begun on, by [`BookId`]: the commit
    /// names the book by its key, the change event by its path.
    paths: RefCell<Vec<(u64, String)>>,
}

impl LibEdits {
    /// The transactions on the card `storage`; what a power cut left of the
    /// last run is removed now.
    pub(crate) fn new(storage: Rc<dyn Storage>, env: Rc<dyn CatalogEnv>) -> Self {
        let stage = stage::Stage::new(storage);
        stage.sweep();
        Self {
            stage,
            transactions: Transactions::try_new().ok(),
            env,
            paths: RefCell::new(Vec::new()),
        }
    }

    pub(crate) fn transactions(&self) -> Option<&Transactions> {
        self.transactions.as_ref()
    }

    pub(crate) fn now_ms(&self) -> u64 {
        self.env.now_ms()
    }

    /// The identity of the book at `path` for as long as a transaction on it
    /// lives: its key, the number the annotation file is named by.
    pub(crate) fn book_id(&self, path: &str) -> Result<BookId, i32> {
        let mut digits = [0_u8; paperpaw_sdk::library::KEY_LEN];
        let key = paperpaw_sdk::library::key_into(path, &mut digits);
        let id = u64::from_str_radix(key, 16).map_err(|_| paperpaw_native::host_glue::ERR_IO)?;
        self.remember(id, path)?;
        Ok(BookId(id))
    }

    fn remember(&self, id: u64, path: &str) -> Result<(), i32> {
        let Ok(mut paths) = self.paths.try_borrow_mut() else {
            return Ok(());
        };
        if paths.iter().any(|(known, _)| *known == id) {
            return Ok(());
        }
        let mut copy = String::new();
        copy.try_reserve_exact(path.len())
            .map_err(|_| ERR_NO_MEMORY)?;
        copy.push_str(path);
        if paths.len() >= PATHS {
            paths.remove(0);
        }
        paths.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
        paths.push((id, copy));
        Ok(())
    }

    /// The path the book was resolved from, while it is remembered.
    pub(crate) fn path_of(&self, id: BookId, visit: &mut dyn FnMut(&str)) {
        if let Ok(paths) = self.paths.try_borrow()
            && let Some((_, path)) = paths.iter().find(|(known, _)| *known == id.0)
        {
            visit(path);
        }
    }

    pub(crate) fn stage_item(&self, transaction: u32, item: &StageItem<'_>) -> Result<(), i32> {
        self.stage.append(transaction, item)
    }

    /// Drops what the transaction staged, and its receipt if it made one.
    pub(crate) fn abort_staged(&self, transaction: u32) {
        self.stage.remove(transaction);
        receipt::remove(self, transaction);
    }

    pub(crate) fn commit(&self, request: &CommitRequest) -> Result<Committed, i32> {
        let done = commit::run(self, request);
        self.stage.remove_staged(request.transaction);
        done
    }

    pub(crate) fn receipt(
        &self,
        owner: u64,
        transaction: u32,
        cursor: u32,
        visit: &mut dyn FnMut(Record<'_>) -> ControlFlow<()>,
    ) -> Result<crate::abi7::context::RowsEnd, i32> {
        receipt::read(self, owner, transaction, cursor, visit)
    }

    /// The receipts of `owner` end (its next `edit_begin`, its stop).
    pub(crate) fn receipts_ended(&self, owner: u64) {
        receipt::forget(self, owner);
    }
}

#[cfg(test)]
#[path = "lib_edits_tests.rs"]
mod tests;
