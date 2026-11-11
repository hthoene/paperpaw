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
use core::cell::{Cell, RefCell};
use core::ops::ControlFlow;
use paperpaw_native::host_glue::ERR_NO_MEMORY;
use paperpaw_platform::Storage;

use super::shell_wiring::WorkDevice;

mod commit;
mod receipt;
mod stage;

/// How many books' paths are remembered for the change event: a transaction
/// lives for 10 minutes at most and the device holds two.
const PATHS: usize = 4;

/// What one call of the transaction service adds to the logical clock where
/// the host has none: the 10 minutes of idle time are 60 calls.
const TICK_MS: u64 = 10_000;

pub(crate) struct LibEdits {
    stage: stage::Stage,
    transactions: Option<Transactions>,
    env: Rc<dyn CatalogEnv>,
    /// The device's monotonic clock where the shell wired `core.work`: the
    /// clock of the idle time and of the receipts' life.
    clock: Option<Rc<dyn WorkDevice>>,
    /// The logical clock of a host that has none ([`TICK_MS`] per call).
    ticks: Cell<u64>,
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
            clock: None,
            ticks: Cell::new(0),
            paths: RefCell::new(Vec::new()),
        }
    }

    /// The X4's smaller transactions: a commit's memory grows with its edits.
    pub(crate) fn small(self) -> Self {
        if let Some(transactions) = &self.transactions {
            transactions.limit_to_x4();
        }
        self
    }

    /// Reads time from the device's monotonic clock instead of the catalog's.
    pub(crate) fn with_clock(mut self, clock: Rc<dyn WorkDevice>) -> Self {
        self.clock = Some(clock);
        self
    }

    pub(crate) fn transactions(&self) -> Option<&Transactions> {
        self.transactions.as_ref()
    }

    /// The clock of the receipts: the device's, else the catalog's; 0 where
    /// the host has none (a receipt then ends at the next begin or the stop).
    pub(crate) fn clock_ms(&self) -> u64 {
        self.clock
            .as_ref()
            .map_or_else(|| self.env.now_ms(), |clock| clock.now_ms())
    }

    /// The clock of the transactions' idle time. Where the host has no clock
    /// (0) every call of the transaction service counts [`TICK_MS`]: an
    /// abandoned transaction still ends, after 60 calls by anyone, instead of
    /// holding its slot for good.
    pub(crate) fn now_ms(&self) -> u64 {
        let real = self.clock_ms();
        if real != 0 {
            return real;
        }
        let ticks = self.ticks.get().saturating_add(TICK_MS);
        self.ticks.set(ticks);
        ticks
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
