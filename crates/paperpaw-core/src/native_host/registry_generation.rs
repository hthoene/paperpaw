//! The registry generation of `core.registry` (`registry.md` 2.1): one counter
//! per device, shared by the kernel (which bumps it) and every frame (which
//! reads it), like the font removals' tracker.
use core::cell::Cell;

/// It changes when the set or order of a list `core.registry` reads changes:
/// a plugin loaded, stopped, enabled, removed or updated, a declaration
/// admitted or retired, an action, screen, slot, offer, group, input, menu,
/// binding or preset added, removed or reordered. Values and pushed item
/// state are not under it. Never 0: it wraps past it.
#[derive(Debug)]
pub(crate) struct RegistryGeneration(Cell<u32>);

impl RegistryGeneration {
    pub(crate) const fn new() -> Self {
        Self(Cell::new(1))
    }

    /// The current generation; never 0.
    pub(crate) fn get(&self) -> u32 {
        self.0.get()
    }

    /// Something a list is made of changed.
    pub(crate) fn bump(&self) {
        self.0.set(self.0.get().wrapping_add(1).max(1));
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
        generation.0.set(u32::MAX);
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
}
