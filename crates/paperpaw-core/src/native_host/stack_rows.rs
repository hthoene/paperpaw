//! Fixed host-only stack authority, refreshed outside native callbacks.
use super::ui_guard::{Row, UiStackView};
use core::cell::Cell;

pub(crate) const STACK_MAX: usize = 16;
pub(crate) const OVERLAY: u8 = 1;
pub(crate) const GUARDS: u8 = 2;
pub(crate) const SECRET: u8 = 4;
pub(crate) const HOST: u8 = 8;
pub(crate) const GUARD_DIALOG: u8 = 16;

#[derive(Clone, Copy, Default)]
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
    pub(crate) cleanup: bool,
}
const NO_PENDING: PendingUi = PendingUi {
    instance: 0,
    close: false,
    cleanup: false,
};

pub(crate) struct StackRows {
    rows: [Cell<StackRow>; STACK_MAX],
    len: Cell<u8>,
    generation: Cell<u64>,
    closes: [Cell<PendingUi>; STACK_MAX],
    close_error: Cell<i32>,
}
impl Default for StackRows {
    fn default() -> Self {
        Self {
            rows: [const { Cell::new(EMPTY) }; STACK_MAX],
            len: Cell::new(0),
            generation: Cell::new(1),
            closes: [const { Cell::new(NO_PENDING) }; STACK_MAX],
            close_error: Cell::new(0),
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
impl StackRows {
    pub(crate) fn generation(&self) -> u64 {
        self.generation.get()
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
            cleanup: false,
        })
    }
    pub(crate) fn retry_cleanup(&self, instance: u32) -> Result<(), i32> {
        if instance == 0 {
            return Err(paperpaw_native::host_glue::ERR_STATE);
        }
        self.enqueue(PendingUi {
            instance,
            close: false,
            cleanup: true,
        })
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
                cleanup: old.cleanup || requested.cleanup,
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
        self.closes.iter().any(|slot| slot.get().cleanup)
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
        self.close_error.get() != 0 || self.closes.iter().any(|slot| slot.get().instance != 0)
    }

    /// Only Kernel::sync_stack writes this snapshot, after callbacks end.
    pub(crate) fn update(&self, rows: impl IntoIterator<Item = StackRow>) {
        let mut count = 0;
        for row in rows {
            if let Some(slot) = self.rows.get(count) {
                slot.set(row);
            }
            count += 1;
            if count > STACK_MAX {
                break;
            }
        }
        self.len.set(u8::try_from(count).unwrap_or(u8::MAX));
        for pending in &self.closes {
            let mut action = pending.get();
            let instance = action.instance;
            if instance != 0
                && !self.rows[..count.min(STACK_MAX)]
                    .iter()
                    .any(|row| row.get().instance == instance)
            {
                action.close = false;
                pending.set(if action.cleanup { action } else { NO_PENDING });
            }
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
}
