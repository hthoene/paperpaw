//! Who the running plugin owners are, for the host services that must name a
//! plugin or ask about its subscriptions (`core.net`: the `.received/` folder,
//! the root-0 saves, "did the plugin subscribe to the done event").
//!
//! The owner registry ([`super::native_instances::NativeInstances`]) lives in
//! the kernel and cannot be borrowed by a device adapter, so it keeps this
//! read-only copy up to date: a row when an enabled owner is admitted, its
//! subscribed event tags when its instance is installed, no row once it
//! stops. Nothing here runs plugin code, and a read never allocates.

use super::{event_tags::EventTags, stack_rows::StackRows, work_sched::WorkSched};
use alloc::{rc::Rc, vec::Vec};
use core::cell::{Cell, RefCell};
use paperpaw_native::host_glue::{ERR_BUSY, ERR_INVALID, ERR_NO_MEMORY, ERR_STATE};
use paperpaw_sdk::Id;

struct Row {
    key: u64,
    /// A linked plugin's id is kept in the firmware: no heap (#248, D4).
    id: Id,
    tags: EventTags,
}

/// The enabled owners: key, plugin id and subscribed event tags.
#[derive(Default)]
pub(crate) struct OwnerDirectory {
    rows: RefCell<Vec<Row>>,
    /// The kernel's screen stack, once a native owner was admitted.
    screens: RefCell<Option<Rc<StackRows>>>,
    /// The scheduler, once `core.work` is wired.
    sched: RefCell<Option<super::shared::Shared<WorkSched>>>,
    /// Updates ([`Self::subscribe`], [`Self::withdraw`]) that found the table
    /// held and were skipped; the kernel reports them ([`Self::take_missed`]).
    /// A missed subscribe fails closed (nothing is admitted); a missed
    /// withdraw leaves a row for an owner that never comes back (keys never
    /// repeat).
    missed: Cell<u32>,
    /// Closes of a request or socket of a work run that the scheduler, in
    /// use at that moment, could not take: asked again at the next pass
    /// ([`Self::retry_closes`]). Fixed room, so that letting go never
    /// allocates; a close past it is counted in `missed`.
    late_closes: Cell<[Option<u32>; LATE_CLOSES]>,
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
            screens: RefCell::new(None),
            sched: RefCell::new(None),
            missed: Cell::new(0),
            late_closes: Cell::new([None; LATE_CLOSES]),
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
            .filter(|row| row.id.as_str() == id)
            .any(|row| screens.has_screen(row.key).unwrap_or(true))
    }

    /// Whether the work scheduler can be asked now (attached and not in use):
    /// when it cannot, a caller that needs to know that no run is current
    /// fails closed (`current_run` alone says `None` for both).
    pub(crate) fn run_known(&self) -> bool {
        self.sched
            .try_borrow()
            .ok()
            .is_some_and(|sched| sched.is_some())
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
    }

    /// Adds the row of an enabled owner (no subscriptions yet).
    ///
    /// # Errors
    /// `ERR_NO_MEMORY` (nothing is added), `ERR_BUSY` (a reader is visiting).
    pub(crate) fn admit(&self, key: u64, id: &Id) -> Result<(), i32> {
        super::net_service::gate()?;
        let mut rows = self.rows.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        rows.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
        super::net_service::gate()?;
        let copy = id.try_clone().ok_or(ERR_NO_MEMORY)?;
        rows.retain(|row| row.key != key);
        rows.push(Row {
            key,
            id: copy,
            tags: EventTags::default(),
        });
        Ok(())
    }

    /// Records the event tags the owner's instance subscribed to. An owner
    /// that has no row (stopped) is left alone.
    pub(crate) fn subscribe(&self, key: u64, tags: EventTags) {
        let Ok(mut rows) = self.rows.try_borrow_mut() else {
            self.miss();
            return;
        };
        if let Some(row) = rows.iter_mut().find(|row| row.key == key) {
            row.tags = tags;
        }
    }

    /// Drops the owner's row (it stopped or was retired).
    pub(crate) fn withdraw(&self, key: u64) {
        let Ok(mut rows) = self.rows.try_borrow_mut() else {
            self.miss();
            return;
        };
        rows.retain(|row| row.key != key);
    }

    fn miss(&self) {
        self.missed.set(self.missed.get().saturating_add(1));
    }

    /// How many updates were skipped since the last call (the table was held,
    /// or the closes of work runs waiting for the scheduler overflowed).
    pub(crate) fn take_missed(&self) -> u32 {
        self.missed.replace(0)
    }

    /// Hands the plugin id of an enabled owner to `visit`.
    ///
    /// # Errors
    /// `ERR_STATE` for an owner that is unknown or stopped, or while another
    /// reader or writer holds the table.
    pub(crate) fn with_id(&self, key: u64, visit: &mut dyn FnMut(&str)) -> Result<(), i32> {
        let rows = self.rows.try_borrow().map_err(|_| ERR_STATE)?;
        let row = rows.iter().find(|row| row.key == key).ok_or(ERR_STATE)?;
        visit(row.id.as_str());
        Ok(())
    }

    /// Whether the running instance of `key` subscribed to the event `tag`;
    /// `false` for an owner that is unknown, stopped or busy.
    pub(crate) fn subscribed(&self, key: u64, tag: u32) -> bool {
        self.rows.try_borrow().ok().is_some_and(|rows| {
            rows.iter()
                .any(|row| row.key == key && row.tags.contains(tag))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_host::net_service::fail_nth;
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
        // Nothing attached: nothing is known, nothing is showing.
        assert_eq!(dir.topmost_screen(9), None);
        assert!(!dir.run_known());
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
            dir.withdraw(1);
        })
        .unwrap();
        assert_eq!(inner, Ok(()), "readers share");
        assert_eq!(
            id_of(&dir, 1).as_deref(),
            Ok("a"),
            "the busy withdraw was skipped, so the row stays"
        );
        assert_eq!(dir.take_missed(), 1, "and the skip is counted");
        assert_eq!(dir.take_missed(), 0, "once");
        dir.withdraw(1);
        assert_eq!(id_of(&dir, 1), Err(ERR_STATE), "free again: it goes");
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
