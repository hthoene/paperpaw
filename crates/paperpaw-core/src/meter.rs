//! What each plugin takes (see [`paperpaw_sdk::usage::PluginUsage`]): the
//! time its code runs (its screens drawing and handling input, its event
//! and action handlers, opening its screens), the storage read and written
//! meanwhile, and the time its screen is on top with the refreshes then.
//! The kernel itself counts as `core`.
//!
//! Time is measured with the shell's clock
//! ([`KernelBuilder::clock`](crate::KernelBuilder::clock)); without one,
//! only what needs none is counted.

use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};

use paperpaw_platform::{DirEntry, Rect, RefreshMode, Storage, StorageError};
use paperpaw_sdk::Id;
use paperpaw_sdk::usage::PluginUsage;

/// What the kernel's own work counts as.
pub(crate) const CORE: &str = "core";

/// Places made for the plugins running at once when the meter is built, so
/// entering one allocates nothing: the kernel's own work, a screen, and the
/// calls inside it (at most [`crate::abi7::calls::MAX_LIVE`] more) fit many
/// times over.
const RUNNING_ROOM: usize = 16;

pub(crate) struct Meter {
    clock: Option<fn() -> u64>,
    generation: Cell<u32>,
    /// By plugin, `core` first; plugins read it through
    /// `Env::plugin_usage`, the one copy kept.
    pub(crate) table: RefCell<Vec<PluginUsage>>,
    /// The plugins whose code runs, the innermost last, with when it began
    /// (or went on after an inner one), in microseconds. Made with
    /// [`RUNNING_ROOM`] places.
    running: RefCell<Vec<(usize, u64)>>,
    /// `Storage::sync` calls by plugin, in the order of `table` (kept
    /// apart: `PluginUsage` is a published type).
    syncs: RefCell<Vec<u32>>,
}

impl core::fmt::Debug for Meter {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Meter").finish_non_exhaustive()
    }
}

/// A plugin's code running, until dropped.
pub(crate) struct Running(Rc<Meter>);

impl Drop for Running {
    fn drop(&mut self) {
        self.0.leave();
    }
}

/// A plugin's code running, until dropped, on a meter held by reference.
pub(crate) struct Scoped<'a>(&'a Meter);

impl Drop for Scoped<'_> {
    fn drop(&mut self) {
        self.0.leave();
    }
}

impl Meter {
    pub(crate) fn new(clock: Option<fn() -> u64>) -> Rc<Self> {
        let meter = Rc::new(Self {
            clock,
            generation: Cell::new(0),
            table: RefCell::new(Vec::new()),
            running: RefCell::new(Vec::with_capacity(RUNNING_ROOM)),
            syncs: RefCell::new(Vec::new()),
        });
        meter.index(CORE);
        meter.generation.set(0);
        meter
    }

    /// Read rows directly while one host visitor runs; no borrow crosses plugin code.
    pub(crate) fn visit_rows(
        &self,
        from: u32,
        visitor: &mut dyn FnMut(&crate::abi7::context::PluginRow<'_>) -> core::ops::ControlFlow<()>,
    ) -> Result<crate::abi7::context::RowsEnd, i32> {
        use paperpaw_native::host_glue::ERR_STATE;
        let table = self.table.try_borrow().map_err(|_| ERR_STATE)?;
        let syncs = self.syncs.try_borrow().map_err(|_| ERR_STATE)?;
        let total = u32::try_from(table.len()).map_err(|_| ERR_STATE)?;
        let mut next = from.min(total);
        for row in table
            .iter()
            .skip(usize::try_from(next).unwrap_or(usize::MAX))
        {
            let synced = syncs.get(next as usize).copied().unwrap_or(0);
            let row = crate::abi7::context::PluginRow::new(row, synced);
            if visitor(&row).is_break() {
                break;
            }
            next += 1;
        }
        Ok(crate::abi7::context::RowsEnd {
            total,
            generation: self.generation.get(),
            next,
        })
    }

    fn now(&self) -> u64 {
        self.clock.map_or(0, |clock| clock())
    }

    /// The clock's microseconds, where the shell gave one.
    pub(crate) fn micros(&self) -> Option<u64> {
        self.clock.map(|clock| clock())
    }

    /// The clock as plugins get it ([`Env::clock`](paperpaw_sdk::Env::clock)).
    pub(crate) fn clock(&self) -> paperpaw_sdk::time::Clock {
        self.clock.map_or(
            paperpaw_sdk::time::Clock::NONE,
            paperpaw_sdk::time::Clock::new,
        )
    }

    /// The row of `plugin`, added the first time.
    fn index(&self, plugin: &str) -> usize {
        let mut table = self.table.borrow_mut();
        if let Some(i) = table.iter().position(|p| p.plugin.as_str() == plugin) {
            return i;
        }
        let id = Id::parse(plugin).unwrap_or(Id::new_static(CORE));
        table.push(PluginUsage::new(id));
        self.generation.set(self.generation.get().wrapping_add(1));
        table.len() - 1
    }

    /// A `Storage::sync` by the code running now.
    fn synced(&self) {
        let index = self.running.borrow().last().map_or(0, |&(i, _)| i);
        let mut syncs = self.syncs.borrow_mut();
        if syncs.len() <= index {
            syncs.resize(index + 1, 0);
        }
        syncs[index] = syncs[index].saturating_add(1);
    }

    /// The `Storage::sync` calls `plugin`'s code made.
    #[allow(
        dead_code,
        reason = "read by the storage adapter tests until Battery shows it"
    )]
    pub(crate) fn syncs_of(&self, plugin: &str) -> u32 {
        // Read only: a plugin not in the table gets no row.
        let table = self.table.borrow();
        let Some(index) = table.iter().position(|p| p.plugin.as_str() == plugin) else {
            return 0;
        };
        self.syncs.borrow().get(index).copied().unwrap_or(0)
    }

    /// `plugin`'s code runs until the result is dropped; the code it was
    /// called from pauses meanwhile.
    pub(crate) fn enter(self: &Rc<Self>, plugin: &str) -> Running {
        self.begin(plugin);
        Running(self.clone())
    }

    /// Like [`Self::enter`], for a frame that holds the meter by reference
    /// (the callee of a nested native call): the code ends with the result.
    pub(crate) fn enter_scoped(&self, plugin: &str) -> Scoped<'_> {
        self.begin(plugin);
        Scoped(self)
    }

    fn begin(&self, plugin: &str) {
        let index = self.index(plugin);
        let now = self.now();
        let mut running = self.running.borrow_mut();
        if let Some(&(outer, since)) = running.last() {
            self.add_busy(outer, now.saturating_sub(since));
        }
        running.push((index, now));
        let mut table = self.table.borrow_mut();
        table[index].calls = table[index].calls.saturating_add(1);
    }

    /// The calls `plugin`'s code made so far; no row is added.
    #[cfg(test)]
    pub(crate) fn calls_of(&self, plugin: &str) -> u32 {
        self.table
            .borrow()
            .iter()
            .find(|p| p.plugin.as_str() == plugin)
            .map_or(0, |p| p.calls)
    }

    /// As [`Self::enter`], unless `plugin`'s code runs now already: a top-level
    /// callback entered it, and a nested call of the same plugin is still it.
    /// For the dispatch paths a screen's own callbacks share with nested calls.
    pub(crate) fn enter_unless_running(self: &Rc<Self>, plugin: &str) -> Option<Running> {
        use paperpaw_sdk::secrets::Running as _;
        (!self.runs(plugin)).then(|| self.enter(plugin))
    }

    fn leave(&self) {
        let now = self.now();
        let mut running = self.running.borrow_mut();
        if let Some((index, since)) = running.pop() {
            self.add_busy(index, now.saturating_sub(since));
        }
        if let Some(last) = running.last_mut() {
            last.1 = now;
        }
    }

    fn add_busy(&self, index: usize, micros: u64) {
        if let Some(p) = self.table.borrow_mut().get_mut(index) {
            p.busy_us = p.busy_us.saturating_add(micros);
        }
    }

    /// Storage used by the code running now.
    fn io(&self, read: usize, written: usize) {
        let index = self.running.borrow().last().map_or(0, |&(i, _)| i);
        if let Some(p) = self.table.borrow_mut().get_mut(index) {
            p.read = p
                .read
                .saturating_add(u32::try_from(read).unwrap_or(u32::MAX));
            p.written = p
                .written
                .saturating_add(u32::try_from(written).unwrap_or(u32::MAX));
        }
    }

    /// `plugin` reached for the network: it went out, or was refused.
    pub(crate) fn network(&self, plugin: &str, refused: bool) {
        let index = self.index(plugin);
        let mut table = self.table.borrow_mut();
        let p = &mut table[index];
        if refused {
            p.refused = p.refused.saturating_add(1);
        } else {
            p.requests = p.requests.saturating_add(1);
        }
    }

    /// `plugin`'s holds kept the device awake for `millis`, the last for
    /// `reason`.
    pub(crate) fn awake(
        &self,
        plugin: &str,
        millis: u64,
        reason: alloc::borrow::Cow<'static, str>,
    ) {
        let index = self.index(plugin);
        let mut table = self.table.borrow_mut();
        let millis = u32::try_from(millis).unwrap_or(u32::MAX);
        table[index].awake_ms = table[index].awake_ms.saturating_add(millis);
        table[index].awake_reason = reason;
    }

    /// `plugin`'s screen was on top for `millis`.
    pub(crate) fn shown(&self, plugin: &str, millis: u64) {
        let index = self.index(plugin);
        let mut table = self.table.borrow_mut();
        let millis = u32::try_from(millis).unwrap_or(u32::MAX);
        table[index].shown_ms = table[index].shown_ms.saturating_add(millis);
    }

    /// A refresh while `plugin`'s screen was on top.
    pub(crate) fn refreshed(&self, plugin: &str, mode: RefreshMode, area: Rect) {
        let index = self.index(plugin);
        let mut table = self.table.borrow_mut();
        let p = &mut table[index];
        p.refreshes.count(mode);
        p.pixels = p
            .pixels
            .saturating_add(u64::from(area.width) * u64::from(area.height));
    }
}

/// The storage, with what is read and written counted for the code
/// running.
pub(crate) struct Metered {
    pub inner: Box<dyn Storage>,
    pub meter: Rc<Meter>,
}

impl Storage for Metered {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        let n = self.inner.read_at(path, offset, buf)?;
        self.meter.io(n, 0);
        Ok(n)
    }

    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.inner.size(path)
    }

    fn modified(&self, path: &str) -> Option<i64> {
        self.inner.modified(path)
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.meter.io(0, data.len());
        self.inner.write(path, data)
    }

    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.meter.io(0, data.len());
        self.inner.append(path, data)
    }

    fn write_at(&self, path: &str, offset: u64, data: &[u8]) -> Result<(), StorageError> {
        self.meter.io(0, data.len());
        self.inner.write_at(path, offset, data)
    }

    fn truncate(&self, path: &str, len: u64) -> Result<(), StorageError> {
        self.inner.truncate(path, len)
    }

    fn sync(&self) -> Result<(), StorageError> {
        self.meter.synced();
        self.inner.sync()
    }

    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.inner.list(dir, visit)
    }

    fn list_from(
        &self,
        dir: &str,
        from: u64,
        visit: &mut dyn FnMut(DirEntry<'_>) -> bool,
    ) -> Result<Option<u64>, StorageError> {
        self.inner.list_from(dir, from, visit)
    }

    fn list_bounded(
        &self,
        dir: &str,
        from: u64,
        visit: &mut dyn FnMut(DirEntry<'_>) -> bool,
    ) -> Result<Option<u64>, StorageError> {
        self.inner.list_bounded(dir, from, visit)
    }

    fn remove_listed(&self, dir: &str, place: u64) -> Result<(), StorageError> {
        self.inner.remove_listed(dir, place)
    }

    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.inner.remove(path)
    }

    fn exists(&self, path: &str) -> bool {
        self.inner.exists(path)
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.inner.rename(from, to)
    }

    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.inner.create_dir(path)
    }

    fn remove_tree_step(
        &self,
        root: &str,
        keep_root: bool,
        place: u64,
    ) -> Result<paperpaw_platform::tree::TreeStep, StorageError> {
        self.inner.remove_tree_step(root, keep_root, place)
    }

    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        self.inner.remove_dir_all(path)
    }

    fn ignores_case(&self) -> bool {
        self.inner.ignores_case()
    }

    fn same_entry(&self, a: &str, b: &str) -> Option<bool> {
        self.inner.same_entry(a, b)
    }

    fn entry_identity(&self, path: &str) -> Result<Option<u64>, StorageError> {
        self.inner.entry_identity(path)
    }

    fn free_space(&self) -> Option<u64> {
        self.inner.free_space()
    }

    fn total_space(&self) -> Option<u64> {
        self.inner.total_space()
    }

    fn local_path(&self, path: &str, with: &mut dyn FnMut(&str)) {
        self.inner.local_path(path, with);
    }
}

/// The plugin whose code runs now is the innermost entered: whose
/// secrets [`Env::secrets`](paperpaw_sdk::Env) hands out.
impl paperpaw_sdk::secrets::Running for Meter {
    fn runs(&self, plugin: &str) -> bool {
        let Some(&(index, _)) = self.running.borrow().last() else {
            return false;
        };
        self.table
            .borrow()
            .get(index)
            .is_some_and(|p| p.plugin.as_str() == plugin)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::sync::atomic::{AtomicU64, Ordering};

    static NOW: AtomicU64 = AtomicU64::new(0);

    fn clock() -> u64 {
        NOW.load(Ordering::Relaxed)
    }

    #[test]
    fn entering_unless_running_looks_only_at_the_innermost_and_allocates_nothing() {
        let meter = Meter::new(Some(clock));
        let room = meter.running.borrow().capacity();
        assert!(room >= RUNNING_ROOM);
        let outer = meter.enter("reader");
        let inner = meter.enter("stats");
        // `reader` is lower on the stack, not running now: it is entered.
        let again = meter.enter_unless_running("reader");
        assert!(again.is_some());
        assert!(meter.enter_unless_running("reader").is_none());
        drop(again);
        assert!(meter.enter_unless_running("stats").is_none());
        drop((inner, outer));
        assert_eq!(meter.running.borrow().capacity(), room);
    }

    #[test]
    fn inner_code_is_its_own_not_the_outer_ones() {
        let meter = Meter::new(Some(clock));
        NOW.store(100, Ordering::Relaxed);
        let outer = meter.enter("reader");
        NOW.store(130, Ordering::Relaxed);
        {
            let _inner = meter.enter("stats");
            meter.io(10, 0);
            NOW.store(180, Ordering::Relaxed);
        }
        meter.io(5, 7);
        NOW.store(200, Ordering::Relaxed);
        drop(outer);
        // Outside every plugin: the kernel's.
        meter.io(1, 0);
        meter.refreshed("reader", RefreshMode::Partial, Rect::new(0, 0, 10, 20));
        meter.shown("reader", 1500);
        let table = meter.table.borrow().clone();
        let row = |id: &str| table.iter().find(|p| p.plugin.as_str() == id).unwrap();
        assert_eq!(row("reader").busy_us, 30 + 20);
        assert_eq!(row("stats").busy_us, 50);
        assert_eq!((row("stats").read, row("reader").read), (10, 5));
        assert_eq!((row("reader").written, row("core").read), (7, 1));
        assert_eq!(
            row("reader").refreshes,
            paperpaw_sdk::usage::Refreshes::new(0, 1, 0)
        );
        assert_eq!((row("reader").pixels, row("reader").shown_ms), (200, 1500));
        assert_eq!((row("reader").calls, row("stats").calls), (1, 1));
    }

    /// Narrow counts stop at their largest value, never wrap.
    #[test]
    fn counts_stop_at_their_largest_value() {
        let meter = Meter::new(None);
        meter.shown("reader", u64::MAX);
        meter.shown("reader", 5);
        meter.awake("reader", u64::from(u32::MAX) + 9, "clock".into());
        meter.io(usize::MAX, 3);
        meter.io(2, 0);
        let table = meter.table.borrow();
        let row = |id: &str| table.iter().find(|p| p.plugin.as_str() == id).unwrap();
        assert_eq!(row("reader").shown_ms, u32::MAX);
        assert_eq!(row("reader").awake_ms, u32::MAX);
        assert_eq!((row("core").read, row("core").written), (u32::MAX, 3));
    }
}

#[cfg(test)]
#[path = "meter_usage_tests.rs"]
mod usage_tests;
