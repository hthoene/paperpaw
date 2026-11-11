//! Fixed host-only stack authority, refreshed outside native callbacks.
use super::ui_guard::{Row, UiStackView};
use core::cell::Cell;

pub(crate) const STACK_MAX: usize = 16;
pub(crate) const OVERLAY: u8 = 1;
pub(crate) const GUARDS: u8 = 2;
pub(crate) const SECRET: u8 = 4;
pub(crate) const HOST: u8 = 8;
pub(crate) const GUARD_DIALOG: u8 = 16;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct StackRow {
    pub(crate) instance: u32,
    pub(crate) owner: u64,
    pub(crate) flags: u8,
}
const EMPTY: StackRow = StackRow {
    instance: 0,
    owner: 0,
    flags: 0,
};

#[derive(Clone, Copy, Default)]
pub(crate) struct PendingUi {
    pub(crate) instance: u32,
    pub(crate) close: bool,
}
const NO_PENDING: PendingUi = PendingUi {
    instance: 0,
    close: false,
};

pub(crate) struct StackRows {
    rows: [Cell<StackRow>; STACK_MAX],
    len: Cell<u8>,
    generation: Cell<u64>,
    /// How often the stack differed after an update (wrapping): what
    /// `event.stack_changed` is raised from (`stack_events`).
    changes: Cell<u32>,
    /// The `changes` each subscriber of `event.stack_changed` was last told
    /// (`(owner, changes)`), kept by `stack_events`.
    pub(super) told: core::cell::RefCell<alloc::vec::Vec<(u64, u32)>>,
    closes: [Cell<PendingUi>; STACK_MAX],
    close_error: Cell<i32>,
    cleanup_sweep: Cell<bool>,
    /// The screen callback on the stack: `(owner, instance)`, `(0, 0)` for none.
    running: Cell<(u64, u32)>,
    /// What the open instances relabel in the row of button hints
    /// (`command.set_hint`); an instance's labels go when it leaves the stack.
    labels: core::cell::RefCell<super::hint_labels::HintLabels>,
}
impl Default for StackRows {
    fn default() -> Self {
        Self {
            rows: [const { Cell::new(EMPTY) }; STACK_MAX],
            len: Cell::new(0),
            generation: Cell::new(1),
            changes: Cell::new(0),
            told: core::cell::RefCell::default(),
            closes: [const { Cell::new(NO_PENDING) }; STACK_MAX],
            close_error: Cell::new(0),
            cleanup_sweep: Cell::new(false),
            running: Cell::new((0, 0)),
            labels: core::cell::RefCell::default(),
        }
    }
}
impl core::fmt::Debug for StackRows {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("StackRows")
            .field("generation", &self.generation.get())
            .finish_non_exhaustive()
    }
}
/// See [`StackRows::enter_callback`].
pub(crate) struct ScreenCallback<'a> {
    rows: &'a StackRows,
    previous: (u64, u32),
}
impl Drop for ScreenCallback<'_> {
    fn drop(&mut self) {
        self.rows.running.set(self.previous);
    }
}

impl StackRows {
    /// The (owner, instance) of the callback running now (tests).
    #[cfg(test)]
    pub(crate) fn running(&self) -> (u64, u32) {
        self.running.get()
    }

    /// A callback of the screen `instance` of `owner` begins on the stack; the
    /// guard ends it. Host-only: the pair comes from the dispatcher, never from
    /// plugin bytes.
    pub(crate) fn enter_callback(&self, owner: u64, instance: u32) -> ScreenCallback<'_> {
        ScreenCallback {
            previous: self.running.replace((owner, instance)),
            rows: self,
        }
    }

    /// The screen instance whose callback runs for `owner`, if it is showing
    /// (`network.md` 5: no server the user cannot see). It is on the snapshot,
    /// the owner's own non-host row, and nothing above it covers it:
    /// - a row above that is no overlay, is a host row (sleep screen, device
    ///   state) or guards (`ui.md` 4.3) covers it; rows above a guard are
    ///   covered along with everything at or below it;
    /// - an overlay of another owner covers a screen that is `secret`
    ///   (`ui.md` 1.2) or itself guards;
    /// - the owner's own non-guarding dialogs above it do not.
    ///
    /// `None` elsewhere, also for a stale, missing or overflowed snapshot (fail
    /// closed). No allocation.
    pub(crate) fn showing_screen(&self, owner: u64) -> Option<u64> {
        let (running_owner, instance) = self.running.get();
        if owner == 0
            || running_owner != owner
            || instance == 0
            || !self.valid(self.generation.get())
        {
            return None;
        }
        let len = self.len();
        let at =
            (0..len).find(|&index| self.row(index).is_some_and(|row| row.instance == instance))?;
        self.uncovered(owner, at)
    }

    /// The topmost screen of `owner`, if it is showing: the owner's own
    /// non-host, non-overlay row nearest the top (its own dialogs above it do
    /// not hide it) with nothing covering it, by the rules of
    /// [`Self::showing_screen`] (`network.md` 5.1: a server started outside a
    /// screen callback is tied to it). `None` for another owner's or a host
    /// screen on top, a covered one, no screen, and for a stale, missing or
    /// overflowed snapshot. No allocation.
    pub(crate) fn topmost_screen(&self, owner: u64) -> Option<u64> {
        if owner == 0 || !self.valid(self.generation.get()) {
            return None;
        }
        let at = (0..self.len()).rev().find(|&index| {
            self.row(index)
                .is_some_and(|row| !row.host && !row.overlay && row.owner == owner)
        })?;
        self.uncovered(owner, at)
    }

    /// Whether `owner` has a screen of its own on the stack, shown or covered
    /// (its dialogs count). `None` for a stale or overflowed snapshot. No
    /// allocation.
    pub(crate) fn has_screen(&self, owner: u64) -> Option<bool> {
        if owner == 0 || !self.valid(self.generation.get()) {
            return None;
        }
        Some((0..self.len()).any(|index| {
            self.row(index)
                .is_some_and(|row| !row.host && row.owner == owner)
        }))
    }

    /// The instance of the row at `at` when it is `owner`'s own non-host row
    /// and nothing above it covers it (see [`Self::showing_screen`]).
    fn uncovered(&self, owner: u64, at: usize) -> Option<u64> {
        let len = self.len();
        let row = self.row(at)?;
        if row.host || row.owner != owner {
            return None;
        }
        let covered = (at + 1..len).any(|index| {
            self.row(index).is_none_or(|above| {
                !above.overlay
                    || above.host
                    || above.guards
                    || (above.owner != owner && (row.secret || row.guards))
            })
        });
        (!covered).then_some(u64::from(row.instance))
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation.get()
    }
    /// How often the stack changed (wrapping), for `event.stack_changed`.
    pub(super) fn changes(&self) -> u32 {
        self.changes.get()
    }
    pub(crate) fn valid(&self, generation: u64) -> bool {
        generation != 0
            && self.generation.get() == generation
            && usize::from(self.len.get()) <= STACK_MAX
    }
    pub(crate) fn defer_close(&self, instance: u32, generation: u64) -> Result<(), i32> {
        self.instance(instance, generation)
            .ok_or(paperpaw_native::host_glue::ERR_STATE)?;
        self.enqueue(PendingUi {
            instance,
            close: true,
        })
    }
    pub(crate) fn retry_cleanup(&self, instance: u32) -> Result<(), i32> {
        if instance == 0 {
            return Err(paperpaw_native::host_glue::ERR_STATE);
        }
        // Sweep the retained store instead of keeping closed ids in the
        // bounded dialog-close queue. Closures can outnumber the live stack.
        self.cleanup_sweep.set(true);
        Ok(())
    }

    fn enqueue(&self, requested: PendingUi) -> Result<(), i32> {
        if let Some(slot) = self
            .closes
            .iter()
            .find(|slot| slot.get().instance == requested.instance)
        {
            let old = slot.get();
            slot.set(PendingUi {
                close: old.close || requested.close,
                ..old
            });
            return Ok(());
        }
        let slot = self
            .closes
            .iter()
            .find(|slot| slot.get().instance == 0)
            .ok_or(paperpaw_native::host_glue::ERR_FULL)?;
        slot.set(requested);
        Ok(())
    }
    pub(crate) fn cleanup_pending(&self) -> bool {
        self.cleanup_sweep.get()
    }
    pub(crate) fn take_cleanup_sweep(&self) -> bool {
        self.cleanup_sweep.replace(false)
    }
    pub(crate) fn request_cleanup_sweep(&self) {
        self.cleanup_sweep.set(true);
    }
    pub(crate) fn note_close_error(&self, error: i32) {
        if error != paperpaw_native::host_glue::ERR_STATE && self.close_error.get() == 0 {
            self.close_error.set(error);
        }
    }
    pub(crate) fn take_close_error(&self) -> i32 {
        self.close_error.replace(0)
    }
    pub(crate) fn take_pending(&self) -> [PendingUi; STACK_MAX] {
        core::array::from_fn(|index| self.closes[index].replace(NO_PENDING))
    }
    pub(crate) fn closes_pending(&self) -> bool {
        self.cleanup_sweep.get()
            || self.close_error.get() != 0
            || self.closes.iter().any(|slot| slot.get().instance != 0)
    }

    /// Only Kernel::sync_stack writes this snapshot, after callbacks end.
    /// The labels of `command.set_hint` of the open instances.
    pub(crate) fn labels(&self) -> &core::cell::RefCell<super::hint_labels::HintLabels> {
        &self.labels
    }
    pub(crate) fn update(&self, rows: impl IntoIterator<Item = StackRow>) {
        let mut count = 0;
        let mut differs = false;
        for row in rows {
            if let Some(slot) = self.rows.get(count) {
                differs |= slot.get() != row;
                slot.set(row);
            }
            count += 1;
            if count > STACK_MAX {
                break;
            }
        }
        differs |= usize::from(self.len.get()) != count;
        self.len.set(u8::try_from(count).unwrap_or(u8::MAX));
        if differs {
            self.changes.set(self.changes.get().wrapping_add(1));
        }
        for pending in &self.closes {
            let action = pending.get();
            let instance = action.instance;
            if instance != 0
                && !self.rows[..count.min(STACK_MAX)]
                    .iter()
                    .any(|row| row.get().instance == instance)
            {
                pending.set(NO_PENDING);
            }
        }
        // Labels of instances that left the stack go with them. A labels
        // call in progress (a hint being drawn) keeps what it has: the next
        // change of the stack prunes.
        if let Ok(mut labels) = self.labels.try_borrow_mut() {
            let open = &self.rows[..count.min(STACK_MAX)];
            labels.retain_open(|instance| open.iter().any(|row| row.get().instance == instance));
        }
        let generation = self.generation.get();
        self.generation.set(if generation == 0 {
            0
        } else {
            generation.checked_add(1).unwrap_or(0)
        });
    }
    pub(crate) fn instance(&self, instance: u32, generation: u64) -> Option<Row> {
        if !self.valid(generation) || instance == 0 {
            return None;
        }
        (0..self.len())
            .filter_map(|index| self.row(index))
            .find(|row| row.instance == instance)
    }
}
impl UiStackView for StackRows {
    fn len(&self) -> usize {
        usize::from(self.len.get())
    }
    fn row(&self, index: usize) -> Option<Row> {
        if index >= self.len() {
            return None;
        }
        let row = self.rows.get(index).map_or(
            StackRow {
                instance: 0,
                owner: 0,
                flags: HOST | GUARDS | SECRET,
            },
            Cell::get,
        );
        let invalid = row.owner == 0 && row.flags & HOST == 0 || self.generation.get() == 0;
        Some(Row {
            instance: row.instance,
            owner: row.owner,
            overlay: row.flags & OVERLAY != 0,
            guards: invalid || row.flags & GUARDS != 0,
            secret: invalid || row.flags & SECRET != 0,
            host: row.flags & HOST != 0,
            guard_dialog: row.flags & GUARD_DIALOG != 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_host::ui_guard;

    #[test]
    fn snapshot_keeps_full_owner_identity_and_all_authority_flags() {
        let rows = StackRows::default();
        rows.update([StackRow {
            instance: 9,
            owner: u64::MAX,
            flags: OVERLAY | GUARDS | SECRET | GUARD_DIALOG,
        }]);
        let row = rows
            .instance(9, rows.generation())
            .expect("present instance");
        assert_eq!(row.owner, u64::MAX);
        assert!(row.overlay && row.guards && row.secret && row.guard_dialog && !row.host);
        assert_eq!(ui_guard::guard_owner(&rows), Some(u64::MAX));
    }
    #[test]
    fn old_generations_and_missing_instances_have_no_authority() {
        let rows = StackRows::default();
        rows.update([StackRow {
            instance: 1,
            owner: 8,
            flags: 0,
        }]);
        let generation = rows.generation();
        rows.update([StackRow {
            instance: 2,
            owner: 8,
            flags: 0,
        }]);
        assert!(rows.instance(2, generation).is_none());
        assert!(rows.instance(1, rows.generation()).is_none());
        assert!(rows.instance(0, rows.generation()).is_none());
    }
    #[test]
    fn set_hint_labels_go_when_their_instance_leaves_the_stack() {
        use crate::native_host::hint_labels::{HintLabels, make};
        let rows = StackRows::default();
        let row = |instance| StackRow {
            instance,
            owner: 8,
            flags: 0,
        };
        rows.update([row(1), row(2)]);
        let sets = alloc::vec![
            make("probe", 1, "probe.go", 1, "One").unwrap(),
            make("probe", 2, "probe.go", 1, "Two").unwrap()
        ];
        rows.labels().borrow_mut().reserve(&sets).unwrap();
        rows.labels().borrow_mut().apply(sets);
        let hint = |labels: &HintLabels, instance| labels.hint(instance, "probe.go");
        rows.update([row(1)]);
        assert!(hint(&rows.labels().borrow(), 1).is_some());
        assert!(hint(&rows.labels().borrow(), 2).is_none());
        rows.update([]);
        assert!(rows.labels().borrow().is_empty());
    }
    #[test]
    fn unknown_plugin_owner_and_overflow_are_secret_and_guarding() {
        let rows = StackRows::default();
        rows.update([StackRow {
            instance: 1,
            owner: 0,
            flags: 0,
        }]);
        assert!(ui_guard::top_is_secret(&rows));
        assert!(ui_guard::topmost_guard(&rows).is_some());
        rows.update((0..17).map(|instance| StackRow {
            instance,
            owner: 8,
            flags: 0,
        }));
        assert!(ui_guard::top_is_secret(&rows));
        assert!(rows.instance(1, rows.generation()).is_none());
        assert!(!rows.valid(rows.generation()));
    }
    #[test]
    fn exhausted_generation_never_revives_an_old_snapshot() {
        let rows = StackRows::default();
        rows.generation.set(u64::MAX);
        rows.update([StackRow {
            instance: 1,
            owner: 8,
            flags: 0,
        }]);
        assert_eq!(rows.generation(), 0);
        assert!(ui_guard::top_is_secret(&rows));
        rows.update([StackRow {
            instance: 2,
            owner: 8,
            flags: 0,
        }]);
        assert!(!rows.valid(0));
    }

    fn row(instance: u32, owner: u64, flags: u8) -> StackRow {
        StackRow {
            instance,
            owner,
            flags,
        }
    }

    #[test]
    fn a_showing_screen_is_the_running_one_with_only_overlays_above() {
        let rows = StackRows::default();
        rows.update([row(1, 8, 0), row(2, 9, 0), row(3, 9, OVERLAY)]);
        assert_eq!(rows.showing_screen(9), None, "no callback runs");
        let guard = rows.enter_callback(9, 2);
        assert_eq!(
            rows.showing_screen(9),
            Some(2),
            "a dialog above does not hide it"
        );
        assert_eq!(
            rows.showing_screen(8),
            None,
            "another owner's view is empty"
        );
        assert_eq!(rows.showing_screen(0), None);
        drop(guard);
        assert_eq!(rows.showing_screen(9), None, "the guard ended the callback");
    }

    #[test]
    fn a_covered_missing_host_or_foreign_screen_is_not_showing() {
        let rows = StackRows::default();
        rows.update([row(1, 9, 0), row(2, 8, 0)]);
        {
            let _call = rows.enter_callback(9, 1);
            assert_eq!(rows.showing_screen(9), None, "another screen covers it");
        }
        {
            let _call = rows.enter_callback(9, 7);
            assert_eq!(rows.showing_screen(9), None, "not on the stack");
        }
        rows.update([row(4, 9, HOST)]);
        {
            let _call = rows.enter_callback(9, 4);
            assert_eq!(
                rows.showing_screen(9),
                None,
                "a host row is no plugin screen"
            );
        }
        rows.update([row(5, 8, 0)]);
        {
            let _call = rows.enter_callback(9, 5);
            assert_eq!(
                rows.showing_screen(9),
                None,
                "the row belongs to another owner"
            );
        }
    }

    fn showing(rows: &[StackRow], owner: u64, instance: u32) -> Option<u64> {
        let stack = StackRows::default();
        stack.update(rows.iter().copied());
        let _call = stack.enter_callback(owner, instance);
        stack.showing_screen(owner)
    }

    #[test]
    fn a_host_overlay_covers_the_screen() {
        let sleep = row(7, 0, HOST | OVERLAY);
        assert_eq!(showing(&[row(1, 9, 0), sleep], 9, 1), None);
        assert_eq!(showing(&[row(1, 9, 0), row(7, 0, HOST)], 9, 1), None);
    }

    #[test]
    fn another_plugins_guard_overlay_covers_the_screen() {
        let guard = row(2, 8, OVERLAY | GUARDS);
        assert_eq!(showing(&[row(1, 9, 0), guard], 9, 1), None);
        // Everything above the guard is covered with what is at or below it.
        let keyboard = row(3, 8, OVERLAY);
        assert_eq!(showing(&[row(1, 9, 0), guard, keyboard], 9, 1), None);
    }

    #[test]
    fn an_own_dialog_or_a_foreign_plain_menu_leaves_the_screen_showing() {
        assert_eq!(showing(&[row(1, 9, 0), row(2, 9, OVERLAY)], 9, 1), Some(1));
        assert_eq!(showing(&[row(1, 9, 0), row(2, 8, OVERLAY)], 9, 1), Some(1));
    }

    #[test]
    fn a_secret_screen_is_covered_by_a_foreign_overlay_not_by_its_own() {
        let secret = row(1, 9, SECRET);
        assert_eq!(showing(&[secret, row(2, 8, OVERLAY)], 9, 1), None);
        assert_eq!(showing(&[secret, row(2, 9, OVERLAY)], 9, 1), Some(1));
    }

    #[test]
    fn a_guarding_own_screen_is_showing_with_its_own_dialogs_above() {
        let lock = row(1, 9, GUARDS);
        assert_eq!(showing(&[lock], 9, 1), Some(1));
        assert_eq!(showing(&[lock, row(2, 9, OVERLAY)], 9, 1), Some(1));
        assert_eq!(
            showing(&[lock, row(2, 8, OVERLAY)], 9, 1),
            None,
            "fail closed"
        );
        assert_eq!(showing(&[lock, row(2, 9, GUARDS | OVERLAY)], 9, 1), None);
    }

    #[test]
    fn an_unknown_row_above_covers_the_screen() {
        assert_eq!(showing(&[row(1, 9, 0), row(2, 0, OVERLAY)], 9, 1), None);
    }

    #[test]
    fn nested_callbacks_restore_the_outer_one() {
        let rows = StackRows::default();
        rows.update([row(1, 9, 0)]);
        let outer = rows.enter_callback(9, 1);
        {
            let _inner = rows.enter_callback(8, 6);
            assert_eq!(rows.showing_screen(9), None);
        }
        assert_eq!(rows.showing_screen(9), Some(1));
        drop(outer);
        assert_eq!(rows.showing_screen(9), None);
    }

    #[test]
    fn an_overflowed_snapshot_has_no_showing_screen() {
        let rows = StackRows::default();
        rows.update((1..=17).map(|instance| row(instance, 9, 0)));
        let _call = rows.enter_callback(9, 3);
        assert_eq!(rows.showing_screen(9), None);
    }

    fn topmost(rows: &[StackRow], owner: u64) -> Option<u64> {
        let stack = StackRows::default();
        stack.update(rows.iter().copied());
        stack.topmost_screen(owner)
    }

    #[test]
    fn the_topmost_own_screen_is_found_without_a_callback_and_its_own_dialogs_do_not_hide_it() {
        assert_eq!(topmost(&[row(1, 9, 0)], 9), Some(1));
        // The nearest own screen counts, not a lower one.
        assert_eq!(topmost(&[row(1, 9, 0), row(2, 9, 0)], 9), Some(2));
        // Its own non-guarding dialog above it does not cover it.
        assert_eq!(topmost(&[row(1, 9, 0), row(2, 9, OVERLAY)], 9), Some(1));
        // Another plugin's overlay does not cover a plain screen.
        assert_eq!(topmost(&[row(1, 9, 0), row(2, 8, OVERLAY)], 9), Some(1));
    }

    #[test]
    fn a_dialog_callback_is_still_a_showing_screen_of_its_own() {
        // The running dialog is the screen the server ties to, not the one
        // below it (`showing_screen` comes first in `serve`).
        let rows = [row(1, 9, 0), row(2, 9, OVERLAY)];
        assert_eq!(showing(&rows, 9, 2), Some(2));
        assert_eq!(topmost(&rows, 9), Some(1));
    }

    #[test]
    fn the_topmost_own_screen_is_none_when_covered_or_not_there() {
        // Another plugin's or the host's screen on top.
        assert_eq!(topmost(&[row(1, 9, 0), row(2, 8, 0)], 9), None);
        assert_eq!(topmost(&[row(1, 9, 0), row(2, 0, HOST)], 9), None);
        // A guard above, an unknown row above (fail closed).
        assert_eq!(
            topmost(&[row(1, 9, 0), row(2, 8, GUARDS | OVERLAY)], 9),
            None
        );
        assert_eq!(topmost(&[row(1, 9, 0), row(2, 0, OVERLAY)], 9), None);
        // No screen of the owner, the unknown owner 0, an empty stack.
        assert_eq!(topmost(&[row(1, 8, 0)], 9), None);
        assert_eq!(topmost(&[row(1, 0, HOST)], 0), None);
        assert_eq!(topmost(&[], 9), None);
        // An overflowed snapshot fails closed.
        let many: alloc::vec::Vec<StackRow> = (1..=17).map(|n| row(n, 9, 0)).collect();
        assert_eq!(topmost(&many, 9), None);
    }
}

#[cfg(target_pointer_width = "32")]
const _: () = assert!(core::mem::size_of::<StackRows>() <= 512);
