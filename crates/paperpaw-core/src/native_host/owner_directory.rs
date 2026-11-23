//! The one row each native plugin owner has (#405, D4): its key, plugin id,
//! lease and lifecycle state. The kernel's owner registry
//! ([`super::native_instances::NativeInstances`]) keeps its plugins here; the
//! host services that must name a plugin or ask about its subscriptions
//! (`core.net`: the `.received/` folder, the root-0 saves, "did the plugin
//! subscribe to the done event") and `core.files` (which ids are linked into
//! the firmware) read the same rows: no second table copies a key or an id.
//!
//! Only the registry adds or takes rows away, outside every visit; the state
//! of a row is in cells, so that stopping or installing an owner is a shared
//! borrow, as a service's read is. Nothing here runs plugin code, and a read
//! never allocates. A row's lease holds the plugin, whose host context holds
//! this table: the registry empties it when it is dropped (no cycle stays).

use super::{native_instances::NativeLease, stack_rows::StackRows, work_sched::WorkSched};
use alloc::{boxed::Box, rc::Rc, vec::Vec};
use core::cell::{Cell, OnceCell, Ref, RefCell, RefMut};
use paperpaw_native::host_glue::{ERR_BUSY, ERR_INVALID, ERR_NO_MEMORY, ERR_STATE};
use paperpaw_sdk::Id;

/// In [`Row::flags`]: the owner is enabled (admitted, not stopped).
pub(super) const ENABLED: u8 = 1;
/// Its start is asked for (a lazy owner, made at the next pass).
pub(super) const START: u8 = 1 << 1;
/// It is being retired.
pub(super) const RETIRING: u8 = 1 << 2;
/// It declared `uses.install` (it may answer plugin requests).
pub(super) const INSTALLER: u8 = 1 << 3;
/// It is linked into the firmware (`Kernel::register_linked`).
pub(super) const LINKED: u8 = 1 << 4;

/// The bit of [`Row::retry_at`] that says its failed retirement was said
/// already (a time never reaches it).
pub(super) const FAILURE_REPORTED: u64 = 1 << 63;

/// One plugin owner: 40 bytes on the X4 where its instance row, its
/// directory row and its linked-set entry took 80 (#405, D4).
pub(crate) struct Row {
    pub(super) key: u64,
    /// A linked plugin's id is kept in the firmware: no heap (#248, D4).
    pub(super) id: Id,
    /// Set once: at the admission, or by the install of a lazy owner.
    pub(super) instance: OnceCell<NativeLease>,
    /// The folder of the version that replaces it once it is retired: a box
    /// of its bytes, not a `String`.
    pub(super) replacement: OnceCell<Box<str>>,
    /// When a failed retirement is tried again (ms), with
    /// [`FAILURE_REPORTED`].
    pub(super) retry_at: Cell<u64>,
    /// [`ENABLED`], [`START`], [`RETIRING`], [`INSTALLER`], [`LINKED`].
    pub(super) flags: Cell<u8>,
    /// The registry the row is of ([`OwnerDirectory::new_registry`]): a
    /// pool may outlive a kernel, and tests share one between kernels.
    pub(super) registry: u16,
}

impl Row {
    pub(super) fn new(
        registry: u16,
        key: u64,
        id: Id,
        instance: Option<NativeLease>,
        flags: u8,
    ) -> Self {
        let lease = OnceCell::new();
        if let Some(instance) = instance {
            let _ = lease.set(instance);
        }
        Self {
            key,
            id,
            instance: lease,
            replacement: OnceCell::new(),
            retry_at: Cell::new(0),
            flags: Cell::new(flags),
            registry,
        }
    }

    pub(super) fn has(&self, flag: u8) -> bool {
        self.flags.get() & flag != 0
    }

    pub(super) fn set(&self, flag: u8, on: bool) {
        let flags = self.flags.get();
        self.flags
            .set(if on { flags | flag } else { flags & !flag });
    }

    pub(super) fn enabled(&self) -> bool {
        self.has(ENABLED)
    }

    pub(super) fn lease(&self) -> Option<&NativeLease> {
        self.instance.get()
    }

    /// When its failed retirement is tried again (ms; 0: at once).
    pub(super) fn retry_at(&self) -> u64 {
        self.retry_at.get() & !FAILURE_REPORTED
    }
}

/// The plugin owners, one row each, with what the services read beside them.
#[derive(Default)]
pub(crate) struct OwnerDirectory {
    rows: RefCell<Vec<Row>>,
    /// Linked plugin ids without a row: registered but not admitted yet (its
    /// `make` runs before the row is added), or retired. Empty almost always.
    linked_loose: RefCell<Vec<Id>>,
    /// The kernel's screen stack, once a native owner was admitted.
    screens: RefCell<Option<Rc<StackRows>>>,
    /// The scheduler, once `core.work` is wired.
    sched: RefCell<Option<super::shared::Shared<WorkSched>>>,
    /// Closes of work runs that overflowed [`Self::late_closes`] since the
    /// kernel last asked ([`Self::take_missed`]).
    missed: Cell<u32>,
    /// Closes of a request or socket of a work run that the scheduler, in
    /// use at that moment, could not take: asked again at the next pass
    /// ([`Self::retry_closes`]). Fixed room, so that letting go never
    /// allocates; a close past it is counted in `missed`.
    late_closes: Cell<[Option<u32>; LATE_CLOSES]>,
    /// The registries made so far ([`Self::new_registry`]).
    registries: Cell<u16>,
    /// The subscriptions the service tests give an owner.
    #[cfg(test)]
    test_tags: RefCell<Vec<(u64, super::event_tags::EventTags)>>,
}

/// How many closes wait for the scheduler at most.
const LATE_CLOSES: usize = 8;

impl core::fmt::Debug for OwnerDirectory {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("OwnerDirectory").finish_non_exhaustive()
    }
}

impl OwnerDirectory {
    pub(crate) const fn new() -> Self {
        Self {
            rows: RefCell::new(Vec::new()),
            linked_loose: RefCell::new(Vec::new()),
            screens: RefCell::new(None),
            sched: RefCell::new(None),
            missed: Cell::new(0),
            late_closes: Cell::new([None; LATE_CLOSES]),
            registries: Cell::new(0),
            #[cfg(test)]
            test_tags: RefCell::new(Vec::new()),
        }
    }

    /// A tag no other registry of this pool has (never 0).
    pub(super) fn new_registry(&self) -> u16 {
        let next = self.registries.get().wrapping_add(1).max(1);
        self.registries.set(next);
        next
    }

    /// The rows, for the registry's reads (`None` while it adds or takes one).
    pub(super) fn rows(&self) -> Option<Ref<'_, Vec<Row>>> {
        self.rows.try_borrow().ok()
    }

    /// The rows, for the registry to add or take one: never held while a
    /// row's plugin can run.
    pub(super) fn rows_mut(&self) -> Result<RefMut<'_, Vec<Row>>, i32> {
        self.rows.try_borrow_mut().map_err(|_| ERR_BUSY)
    }

    /// Adds `row`. A linked id waiting for its row ([`Self::add_linked`])
    /// goes into it. Nothing is added on a failure.
    ///
    /// # Errors
    /// `ERR_NO_MEMORY`, `ERR_BUSY` (the rows are in use).
    pub(super) fn push(&self, row: Row) -> Result<(), i32> {
        let mut rows = self.rows_mut()?;
        rows.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
        if let Ok(mut loose) = self.linked_loose.try_borrow_mut()
            && let Some(at) = loose.iter().position(|id| *id == row.id)
        {
            loose.swap_remove(at);
            row.set(LINKED, true);
        }
        rows.push(row);
        Ok(())
    }

    /// Takes the row of `key` out, to be dropped by the caller once no borrow
    /// is held (its plugin may run as it goes). A linked plugin's id stays
    /// linked; where there is no room to keep it, the row stays.
    ///
    /// # Errors
    /// `ERR_STATE` (no such row), `ERR_NO_MEMORY`, `ERR_BUSY`.
    pub(super) fn take(&self, key: u64) -> Result<Row, i32> {
        let mut rows = self.rows_mut()?;
        let at = rows
            .iter()
            .position(|row| row.key == key)
            .ok_or(ERR_STATE)?;
        if rows[at].has(LINKED) {
            let mut loose = self.linked_loose.try_borrow_mut().map_err(|_| ERR_BUSY)?;
            loose.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
            loose.push(rows[at].id.try_clone().ok_or(ERR_NO_MEMORY)?);
        }
        Ok(rows.swap_remove(at))
    }

    /// Every row of `registry`, to be dropped by the caller (the registry
    /// goes). Moves rows only: nothing is allocated.
    pub(super) fn take_all(&self, registry: u16) -> Vec<Row> {
        let Ok(mut rows) = self.rows.try_borrow_mut() else {
            return Vec::new();
        };
        if rows.iter().all(|row| row.registry == registry) {
            return core::mem::take(&mut *rows);
        }
        // Another registry's rows stay: take its own one by one (they are
        // dropped by the caller, outside the borrow).
        let mut mine = Vec::new();
        while let Some(at) = rows.iter().position(|row| row.registry == registry) {
            if mine.try_reserve(1).is_err() {
                // No room to hand it out: dropped here, under the borrow.
                drop(rows.swap_remove(at));
                continue;
            }
            mine.push(rows.swap_remove(at));
        }
        mine
    }

    /// Room for `more` rows, asked for once before the plugins are hosted.
    pub(super) fn reserve_exact(&self, more: usize) {
        if let Ok(mut rows) = self.rows.try_borrow_mut() {
            let _ = rows.try_reserve_exact(more);
        }
    }

    /// Keeps the screen stack for [`Self::showing_screen`].
    pub(crate) fn attach_screens(&self, screens: &Rc<StackRows>) {
        if let Ok(mut slot) = self.screens.try_borrow_mut() {
            *slot = Some(Rc::clone(screens));
        }
    }

    /// Keeps the scheduler for [`Self::current_run`].
    pub(crate) fn attach_sched(&self, sched: super::shared::Shared<WorkSched>) {
        if let Ok(mut slot) = self.sched.try_borrow_mut() {
            *slot = Some(sched);
        }
    }

    /// The screen instance of `owner` whose callback runs, if it is showing
    /// (`None` when unknown, covered, or no stack is attached).
    pub(crate) fn showing_screen(&self, owner: u64) -> Option<u64> {
        self.screens
            .try_borrow()
            .ok()?
            .as_ref()?
            .showing_screen(owner)
    }

    /// The screen instance of `owner` whose callback runs, shown or covered
    /// (`None` when unknown, not a screen's callback, or no stack is attached).
    pub(crate) fn calling_screen(&self, owner: u64) -> Option<u64> {
        self.screens
            .try_borrow()
            .ok()?
            .as_ref()?
            .calling_screen(owner)
    }

    /// The topmost screen instance of `owner` when it is showing (`None` when
    /// unknown, covered, or no stack is attached).
    pub(crate) fn topmost_screen(&self, owner: u64) -> Option<u64> {
        self.screens
            .try_borrow()
            .ok()?
            .as_ref()?
            .topmost_screen(owner)
    }

    /// Whether an enabled plugin with the id `id` has a screen on the stack
    /// (shown or covered). Unknown state (the table or the snapshot held or
    /// stale) says `true`: the caller holds back; no stack attached says
    /// `false` (no screens). No allocation.
    pub(crate) fn plugin_has_screen(&self, id: &str) -> bool {
        let Ok(screens) = self.screens.try_borrow() else {
            return true;
        };
        let Some(screens) = screens.as_ref() else {
            return false;
        };
        let Ok(rows) = self.rows.try_borrow() else {
            return true;
        };
        rows.iter()
            .filter(|row| row.enabled() && row.id.as_str() == id)
            .any(|row| screens.has_screen(row.key).unwrap_or(true))
    }

    /// Whether [`Self::current_run`] can tell now: `false` only while the
    /// scheduler is in use, and then a caller that needs to know that no run
    /// is current fails closed (`current_run` alone says `None` for both). No
    /// scheduler attached is known: `core.work` is made at its first call
    /// (#248) and attached then, a scheduler not made has no runs (#469).
    pub(crate) fn run_known(&self) -> bool {
        self.sched.try_borrow().is_ok()
    }

    /// The work run `owner`'s running callback belongs to.
    pub(crate) fn current_run(&self, owner: u64) -> Option<u64> {
        self.sched.try_borrow().ok()?.as_ref()?.current_run(owner)
    }

    /// A request or socket of work run `run` opened: the run is kept going.
    /// Without a scheduler attached there are no runs and nothing to keep.
    ///
    /// # Errors
    /// `ERR_BUSY` (the scheduler is in use), and the scheduler's own.
    pub(crate) fn resource_opened(&self, run: u64) -> Result<(), i32> {
        let sched = self.sched.try_borrow().map_err(|_| ERR_BUSY)?;
        let Some(sched) = sched.as_ref() else {
            return Ok(());
        };
        sched.resource_opened(u32::try_from(run).map_err(|_| ERR_INVALID)?)
    }

    /// The request or socket of work run `run` ended; the run may end now.
    /// Never allocates. When the scheduler is in use the close waits for
    /// the next pass ([`Self::retry_closes`]).
    pub(crate) fn resource_closed(&self, run: u64, now_ms: u64) {
        let Ok(run) = u32::try_from(run) else {
            return;
        };
        let done = match self.sched.try_borrow() {
            Ok(sched) => sched
                .as_ref()
                .is_none_or(|sched| sched.resource_closed(run, now_ms)),
            Err(_) => false,
        };
        if !done {
            self.defer_close(run);
        }
    }

    fn defer_close(&self, run: u32) {
        let mut waiting = self.late_closes.get();
        if let Some(room) = waiting.iter_mut().find(|slot| slot.is_none()) {
            *room = Some(run);
            self.late_closes.set(waiting);
        } else {
            // The run then ends at its longest time (`take_expired`).
            self.missed.set(self.missed.get().saturating_add(1));
        }
    }

    /// Whether a close waits for the scheduler.
    pub(crate) fn closes_wait(&self) -> bool {
        self.late_closes.get().iter().any(Option::is_some)
    }

    /// Asks the scheduler again for the closes it could not take.
    pub(crate) fn retry_closes(&self, now_ms: u64) {
        let waiting = self.late_closes.replace([None; LATE_CLOSES]);
        for run in waiting.into_iter().flatten() {
            self.resource_closed(u64::from(run), now_ms);
        }
    }

    /// Gives back the room the rows kept while owners were admitted one by
    /// one (the list grows by doubling): called once the plugins are hosted.
    /// A list in use is left as it is.
    pub(crate) fn shrink_to_fit(&self) {
        if let Ok(mut rows) = self.rows.try_borrow_mut() {
            super::shrink::shrink(&mut rows);
        }
        if let Ok(mut loose) = self.linked_loose.try_borrow_mut() {
            super::shrink::shrink(&mut loose);
        }
    }

    /// How many closes of work runs found no room since the last call.
    pub(crate) fn take_missed(&self) -> u32 {
        self.missed.replace(0)
    }

    /// Hands the plugin id of an enabled owner to `visit`.
    ///
    /// # Errors
    /// `ERR_STATE` for an owner that is unknown or stopped, or while the
    /// rows are being changed.
    pub(crate) fn with_id(&self, key: u64, visit: &mut dyn FnMut(&str)) -> Result<(), i32> {
        let rows = self.rows.try_borrow().map_err(|_| ERR_STATE)?;
        let row = rows
            .iter()
            .find(|row| row.key == key && row.enabled())
            .ok_or(ERR_STATE)?;
        visit(row.id.as_str());
        Ok(())
    }

    /// Whether the running instance of `key` subscribed to the event `tag`
    /// (asked of its lease, which reads the committed registrations or a
    /// parked plugin's recipe: nothing is made); `false` for an owner that
    /// is unknown, stopped, not installed yet or busy.
    pub(crate) fn subscribed(&self, key: u64, tag: u32) -> bool {
        self.rows.try_borrow().ok().is_some_and(|rows| {
            rows.iter().any(|row| {
                row.key == key
                    && row.enabled()
                    && (row.lease().is_some_and(|lease| lease.subscribed(tag))
                        || self.test_subscribed(key, tag))
            })
        })
    }

    /// Records `id` as a linked plugin's before the plugin can run; a
    /// registration that fails takes it back ([`Self::remove_linked`]).
    ///
    /// # Errors
    /// `ERR_NO_MEMORY`; `ERR_BUSY` when the table is in use.
    pub(crate) fn add_linked(&self, id: &'static str) -> Result<(), i32> {
        if let Ok(rows) = self.rows.try_borrow()
            && let Some(row) = rows.iter().find(|row| row.id.as_str() == id)
        {
            row.set(LINKED, true);
            return Ok(());
        }
        let mut loose = self.linked_loose.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        if loose.iter().any(|have| have.as_str() == id) {
            return Ok(());
        }
        loose.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
        loose.push(Id::new_static(id));
        Ok(())
    }

    /// Takes the linked id `id` back (a registration that failed).
    pub(crate) fn remove_linked(&self, id: &str) {
        if let Ok(mut loose) = self.linked_loose.try_borrow_mut() {
            loose.retain(|have| have.as_str() != id);
        }
        if let Ok(rows) = self.rows.try_borrow() {
            for row in rows.iter().filter(|row| row.id.as_str() == id) {
                row.set(LINKED, false);
            }
        }
    }

    /// Whether `id` is a linked plugin's. A table in use says yes.
    pub(crate) fn is_linked(&self, id: &str) -> bool {
        let (Ok(loose), Ok(rows)) = (self.linked_loose.try_borrow(), self.rows.try_borrow()) else {
            return true;
        };
        loose.iter().any(|have| have.as_str() == id)
            || rows
                .iter()
                .any(|row| row.has(LINKED) && row.id.as_str() == id)
    }
}

impl OwnerDirectory {
    #[cfg(not(test))]
    #[allow(clippy::unused_self, reason = "the tests' subscriptions")]
    fn test_subscribed(&self, _: u64, _: u32) -> bool {
        false
    }

    #[cfg(test)]
    fn test_subscribed(&self, key: u64, tag: u32) -> bool {
        self.test_tags.try_borrow().is_ok_and(|told| {
            told.iter()
                .any(|(owner, tags)| *owner == key && tags.contains(tag))
        })
    }
}

#[cfg(test)]
impl OwnerDirectory {
    /// An empty table of its own, shared.
    pub(crate) fn fresh() -> super::shared::Shared<Self> {
        super::shared::Shared::try_new(Self::new()).unwrap_or_else(|_| unreachable!())
    }

    /// Adds the row of an enabled owner (no subscriptions yet), as the
    /// registry does when it admits one; an owner the registry admitted
    /// keeps its row.
    ///
    /// # Errors
    /// `ERR_NO_MEMORY` (nothing is added), `ERR_BUSY` (a reader is visiting).
    pub(crate) fn admit(&self, key: u64, id: &Id) -> Result<(), i32> {
        super::net_service::gate()?;
        let copy = id.try_clone().ok_or(ERR_NO_MEMORY)?;
        super::net_service::gate()?;
        if self
            .rows()
            .ok_or(ERR_BUSY)?
            .iter()
            .any(|row| row.key == key)
        {
            return Ok(());
        }
        self.push(Row::new(0, key, copy, None, ENABLED))
    }

    /// The services see the owner subscribed to `tags` besides what its
    /// lease says. An owner that has no row is left alone.
    pub(crate) fn subscribe(&self, key: u64, tags: super::event_tags::EventTags) {
        if self
            .rows()
            .is_some_and(|rows| rows.iter().any(|row| row.key == key))
        {
            let mut told = self.test_tags.borrow_mut();
            told.retain(|(owner, _)| *owner != key);
            told.push((key, tags));
        }
    }

    /// Drops the owner's row (it stopped or was retired).
    pub(crate) fn withdraw(&self, key: u64) {
        drop(self.take(key));
        self.test_tags
            .borrow_mut()
            .retain(|(owner, _)| *owner != key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_host::{event_tags::EventTags, net_service::fail_nth};
    use alloc::string::String;

    fn tags(of: &[u32]) -> EventTags {
        let mut set = EventTags::default();
        for tag in of {
            set.insert(*tag).unwrap();
        }
        set
    }

    fn id_of(dir: &OwnerDirectory, key: u64) -> Result<String, i32> {
        let mut out = String::new();
        dir.with_id(key, &mut |id| out.push_str(id))?;
        Ok(out)
    }

    #[test]
    fn an_admitted_owner_has_its_id_and_a_stopped_one_has_none() {
        let dir = OwnerDirectory::new();
        assert_eq!(id_of(&dir, 7), Err(ERR_STATE));
        dir.admit(7, &Id::new_static("net.probe")).unwrap();
        assert_eq!(id_of(&dir, 7).as_deref(), Ok("net.probe"));
        assert_eq!(id_of(&dir, 8), Err(ERR_STATE));
        dir.withdraw(7);
        assert_eq!(id_of(&dir, 7), Err(ERR_STATE));
    }

    #[test]
    fn the_topmost_screen_and_the_run_state_come_from_what_is_attached() {
        use crate::native_host::{
            stack_rows::{StackRow, StackRows},
            work_sched::Limits,
        };
        let dir = OwnerDirectory::new();
        // Nothing attached: nothing is showing, and no scheduler has no run.
        assert_eq!(dir.topmost_screen(9), None);
        assert!(dir.run_known());
        assert_eq!(dir.current_run(9), None);
        let rows = Rc::new(StackRows::default());
        dir.attach_screens(&rows);
        rows.update([StackRow {
            instance: 4,
            owner: 9,
            flags: 0,
        }]);
        assert_eq!(dir.topmost_screen(9), Some(4));
        assert_eq!(dir.topmost_screen(8), None);
        let sched =
            crate::native_host::shared::Shared::try_new(WorkSched::new(Limits::OTHER)).unwrap();
        dir.attach_sched(sched.try_clone().unwrap());
        assert!(dir.run_known());
        assert_eq!(dir.current_run(9), None);
        // The scheduler in use: not known (never "no run").
        let _busy = dir.sched.try_borrow_mut().unwrap();
        assert!(!dir.run_known());
    }

    #[test]
    fn a_plugin_has_a_screen_while_one_of_its_rows_is_on_the_stack() {
        use crate::native_host::stack_rows::{HOST, OVERLAY, StackRow, StackRows};
        let row = |instance, owner, flags| StackRow {
            instance,
            owner,
            flags,
        };
        let dir = OwnerDirectory::new();
        dir.admit(5, &Id::new_static("files")).unwrap();
        dir.admit(6, &Id::new_static("home")).unwrap();
        assert!(!dir.plugin_has_screen("files"), "no stack, no screens");
        let rows = Rc::new(StackRows::default());
        dir.attach_screens(&rows);
        rows.update([row(1, 6, 0)]);
        assert!(!dir.plugin_has_screen("files") && dir.plugin_has_screen("home"));
        // Covered by another owner's screen, or by a dialog: still up.
        rows.update([
            row(1, 5, 0),
            row(2, 6, 0),
            row(3, 5, OVERLAY),
            row(4, 0, HOST),
        ]);
        assert!(dir.plugin_has_screen("files"));
        rows.update([row(2, 6, 0), row(4, 0, HOST)]);
        assert!(!dir.plugin_has_screen("files"), "gone from the stack");
        assert!(!dir.plugin_has_screen("nobody"));
    }

    #[test]
    fn subscriptions_belong_to_their_owner_only() {
        let dir = OwnerDirectory::new();
        dir.admit(1, &Id::new_static("a")).unwrap();
        dir.admit(2, &Id::new_static("b")).unwrap();
        assert!(!dir.subscribed(1, 99), "nothing before the install");
        dir.subscribe(1, tags(&[99]));
        dir.subscribe(3, tags(&[99]));
        assert!(dir.subscribed(1, 99));
        assert!(!dir.subscribed(1, 101) && !dir.subscribed(2, 99));
        assert!(!dir.subscribed(3, 99), "no row, no subscription");
        dir.withdraw(1);
        assert!(!dir.subscribed(1, 99));
    }

    #[test]
    fn a_busy_table_answers_state_and_never_panics() {
        let dir = OwnerDirectory::new();
        dir.admit(1, &Id::new_static("a")).unwrap();
        let mut inner = Ok(());
        dir.with_id(1, &mut |_| {
            inner = dir.with_id(1, &mut |_| {});
            assert_eq!(dir.admit(2, &Id::new_static("b")), Err(ERR_BUSY));
            assert!(matches!(dir.take(1), Err(ERR_BUSY)));
        })
        .unwrap();
        assert_eq!(inner, Ok(()), "readers share");
        assert_eq!(
            id_of(&dir, 1).as_deref(),
            Ok("a"),
            "the busy take was refused, so the row stays"
        );
        dir.withdraw(1);
        assert_eq!(id_of(&dir, 1), Err(ERR_STATE), "free again: it goes");
    }

    #[test]
    fn a_linked_id_is_known_with_or_without_its_row() {
        let dir = OwnerDirectory::new();
        assert!(!dir.is_linked("home"));
        // Before its row: the plugin's `make` runs first.
        dir.add_linked("home").unwrap();
        dir.add_linked("home").unwrap();
        assert!(dir.is_linked("home") && !dir.is_linked("hom") && !dir.is_linked("homes"));
        // Its row takes it over; the id stays linked while the row lives
        // and once it is retired.
        dir.admit(3, &Id::new_static("home")).unwrap();
        assert!(dir.linked_loose.borrow().is_empty(), "no second copy");
        assert!(dir.is_linked("home"));
        dir.withdraw(3);
        assert!(dir.is_linked("home"));
        dir.remove_linked("home");
        assert!(!dir.is_linked("home"));
        // A registration that failed after its row was added takes it back.
        dir.add_linked("news").unwrap();
        dir.admit(4, &Id::new_static("news")).unwrap();
        dir.remove_linked("news");
        assert!(!dir.is_linked("news"));
        // A card plugin's row is no linked one.
        dir.admit(5, &Id::new_static("card")).unwrap();
        assert!(!dir.is_linked("card"));
    }

    #[test]
    fn a_plugin_owner_is_one_row_of_five_words_on_the_x4() {
        // Key, id, lease, replacement, retry time and flags: the instance
        // row, the directory row and the linked-set entry together took 80
        // bytes on the X4 (#405, D4); 64-bit hosts take 64.
        let expected = if usize::BITS == 32 { 40 } else { 64 };
        assert_eq!(core::mem::size_of::<Row>(), expected);
    }

    #[test]
    fn admission_under_memory_pressure_adds_no_row() {
        let dir = OwnerDirectory::new();
        for nth in [1, 2] {
            fail_nth(nth);
            assert_eq!(
                dir.admit(1, &Id::new_static("a")),
                Err(ERR_NO_MEMORY),
                "failure {nth}"
            );
            fail_nth(0);
            assert_eq!(id_of(&dir, 1), Err(ERR_STATE), "no half row after {nth}");
        }
        dir.admit(1, &Id::new_static("a")).unwrap();
        assert_eq!(id_of(&dir, 1).as_deref(), Ok("a"));
    }
}

#[cfg(test)]
mod shrink_tests {
    extern crate std;
    use super::*;

    #[test]
    fn the_rows_give_back_what_doubling_kept() {
        let directory = OwnerDirectory::new();
        for key in 1..=13_u64 {
            directory.admit(key, &Id::new_static("plugin")).unwrap();
        }
        let capacity = |d: &OwnerDirectory| d.rows.borrow().capacity();
        assert!(capacity(&directory) >= 16);
        directory.shrink_to_fit();
        assert_eq!(capacity(&directory), 13);
        // A list that is being read is left as it is.
        let reading = directory.rows.borrow();
        directory.shrink_to_fit();
        drop(reading);
        assert_eq!(directory.rows.borrow().len(), 13);
    }
}
