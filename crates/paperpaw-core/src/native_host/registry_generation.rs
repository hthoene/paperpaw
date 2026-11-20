//! The registry generation of `core.registry` (`registry.md` 2.1): one counter
//! per device, shared by the kernel (which bumps it) and every frame (which
//! reads it), like the font removals' tracker.
use super::declaration_store::DeclKey;
use alloc::rc::Rc;
use core::cell::{Cell, RefCell, RefMut};

/// It changes when the set or order of a list `core.registry` reads changes:
/// a plugin loaded, stopped, enabled, removed or updated, a declaration
/// admitted or retired, an action, screen, slot, offer, group, input, menu,
/// binding or preset added, removed or reordered. Values and pushed item
/// state are not under it. Never 0: it wraps past it.
#[derive(Debug)]
pub(crate) struct RegistryGeneration {
    generation: Cell<u32>,
    memo: RefCell<ListMemo>,
}

/// What the settings ops read of every setting and declaration of the
/// device, kept from one op call to the next of a round while the generation
/// holds: the settings screen asks for its list a page at a time and a row at
/// a time, and each call read all of it again (#422: opening the screen read
/// the whole store some fifty times). None of it depends on values. Let go at
/// the end of the round, as the described specs are (#248), and at a bump.
#[derive(Debug)]
pub(crate) struct ListMemo {
    /// The generation it was read under (0: none).
    of: u32,
    /// The `menu.need` rows by the hash of their item
    /// (`menu_native::Native::need_keys`).
    pub(crate) needs: Option<Rc<[(u32, DeclKey)]>>,
    /// The hashes of the ids of the settings whose value another setting's
    /// showing depends on, sorted (`settings::governing`).
    pub(crate) governing: Option<Rc<[u64]>>,
    /// The hashes of the settings with a declared `setting.extra`, sorted
    /// (`menu_native::Native::extra_hashes`).
    pub(crate) extras: Option<Rc<[u32]>>,
    /// The `icon.spec` rows (`menu_native::Native::sharing`).
    pub(crate) icons: Option<Rc<[(u32, u16, DeclKey)]>>,
    /// The rows op 1 counted last: the hash of its scope, how many, and
    /// whether every row it walked frames (then a page passes those before
    /// its cursor without asking).
    pub(crate) total: Option<(u64, u32, bool)>,
    /// The last row op 1 visited: the hash of its scope, the row's index and
    /// its place in the settings' order (`settings::resume_place`).
    pub(crate) last_row: Option<(u64, u32, usize)>,
}

impl ListMemo {
    const fn new() -> Self {
        Self {
            of: 0,
            needs: None,
            governing: None,
            extras: None,
            icons: None,
            total: None,
            last_row: None,
        }
    }
}

impl RegistryGeneration {
    pub(crate) const fn new() -> Self {
        Self {
            generation: Cell::new(1),
            memo: RefCell::new(ListMemo::new()),
        }
    }

    /// The current generation; never 0.
    pub(crate) fn get(&self) -> u32 {
        self.generation.get()
    }

    /// Something a list is made of changed.
    pub(crate) fn bump(&self) {
        self.generation
            .set(self.generation.get().wrapping_add(1).max(1));
        if let Ok(mut memo) = self.memo.try_borrow_mut() {
            *memo = ListMemo::new();
        }
    }

    /// Lets go of the memo: the end of a round.
    pub(crate) fn rest(&self) {
        if let Ok(mut memo) = self.memo.try_borrow_mut() {
            *memo = ListMemo::new();
        }
    }

    /// The memo of the current generation (emptied if it was read under
    /// another); `None` while it is held already.
    pub(crate) fn memo(&self) -> Option<RefMut<'_, ListMemo>> {
        let mut memo = self.memo.try_borrow_mut().ok()?;
        if memo.of != self.get() {
            *memo = ListMemo::new();
            memo.of = self.get();
        }
        Some(memo)
    }
}

/// Bumps the generation when dropped: held by a change that goes on after
/// its first step (a plugin's code runs in between), so a reader never sees
/// the new generation with the old lists.
pub(crate) struct BumpOnExit(alloc::rc::Rc<RegistryGeneration>);

impl BumpOnExit {
    pub(crate) fn new(generation: &alloc::rc::Rc<RegistryGeneration>) -> Self {
        Self(alloc::rc::Rc::clone(generation))
    }
}

impl Drop for BumpOnExit {
    fn drop(&mut self) {
        self.0.bump();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_starts_at_one_and_skips_zero_when_it_wraps() {
        let generation = RegistryGeneration::new();
        assert_eq!(generation.get(), 1);
        generation.bump();
        assert_eq!(generation.get(), 2);
        generation.generation.set(u32::MAX);
        generation.bump();
        assert_eq!(generation.get(), 1);
    }

    #[test]
    fn a_change_bumps_on_every_way_out() {
        let generation = alloc::rc::Rc::new(RegistryGeneration::new());
        let change = |fail: bool| -> Result<(), ()> {
            let _bump = BumpOnExit::new(&generation);
            if fail { Err(()) } else { Ok(()) }
        };
        assert_eq!(generation.get(), 1);
        let _ = change(false);
        assert_eq!(generation.get(), 2);
        let _ = change(true);
        assert_eq!(generation.get(), 3);
    }

    #[test]
    fn the_memo_holds_for_one_generation() {
        let generation = RegistryGeneration::new();
        generation.memo().unwrap().total = Some((7, 3, true));
        assert_eq!(generation.memo().unwrap().total, Some((7, 3, true)));
        let held = generation.memo();
        assert!(generation.memo().is_none());
        drop(held);
        generation.bump();
        assert_eq!(generation.memo().unwrap().total, None);
        generation.memo().unwrap().total = Some((7, 3, true));
        generation.rest();
        assert_eq!(generation.memo().unwrap().total, None);
    }
}
