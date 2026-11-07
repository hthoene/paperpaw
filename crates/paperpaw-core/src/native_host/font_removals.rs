//! Host-only epochs for named font handles. No font reference survives a call.
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};
use paperpaw_sdk::Id;

#[derive(Debug)]
struct Removal {
    provider: Id,
    generation: u64,
}

/// One tracker is shared by the kernel and every flat host on a device.
/// The Rc header and this empty tracker allocate once before admission;
/// removals add one checked Id copy and one vector entry per provider.
#[derive(Debug)]
pub(crate) struct FontRemovals {
    generation: Cell<Option<u64>>,
    floor: Cell<u64>,
    providers: RefCell<Vec<Removal>>,
    #[cfg(test)]
    refuse_growth: Cell<bool>,
}

impl FontRemovals {
    pub(crate) const fn new() -> Self {
        Self {
            generation: Cell::new(Some(0)),
            floor: Cell::new(0),
            providers: RefCell::new(Vec::new()),
            #[cfg(test)]
            refuse_growth: Cell::new(false),
        }
    }

    /// The epoch stored by an acquired named handle. Exhaustion refuses acquire.
    pub(crate) fn generation(&self) -> Option<u64> {
        self.generation.get()
    }

    /// A missing tracker or a failed registry borrow must fail closed upstream.
    pub(crate) fn valid(&self, provider: &str, acquired: u64) -> bool {
        let Some(current) = self.generation.get() else {
            return false;
        };
        if acquired < self.floor.get() || acquired > current {
            return false;
        }
        if acquired == current {
            return true;
        }
        let Ok(providers) = self.providers.try_borrow() else {
            return false;
        };
        !providers
            .iter()
            .any(|entry| entry.provider.as_str() == provider && entry.generation > acquired)
    }

    /// Call only after removing `removed` font entries owned by this provider.
    /// A no-op removal does not invalidate a handle. On memory refusal the
    /// floor invalidates all earlier handles, so no removal can be forgotten.
    pub(crate) fn removed(&self, provider: &Id, removed: usize) {
        if removed == 0 {
            return;
        }
        let Some(generation) = self
            .generation
            .get()
            .and_then(|old| old.checked_add(1))
            .filter(|next| *next < u64::MAX)
        else {
            // No epoch is reused, including under injected exhaustion.
            self.generation.set(None);
            return;
        };
        self.generation.set(Some(generation));
        let Ok(mut providers) = self.providers.try_borrow_mut() else {
            self.floor.set(generation);
            return;
        };
        if let Some(entry) = providers
            .iter_mut()
            .find(|entry| entry.provider == *provider)
        {
            entry.generation = generation;
            return;
        }
        if let Some(provider) = self.copy_provider(provider, &mut providers) {
            providers.push(Removal {
                provider,
                generation,
            });
        } else {
            self.floor.set(generation);
            providers.clear();
        }
    }

    fn copy_provider(&self, provider: &Id, providers: &mut Vec<Removal>) -> Option<Id> {
        #[cfg(test)]
        if self.refuse_growth.replace(false) {
            return None;
        }
        providers.try_reserve_exact(1).ok()?;
        provider.try_clone()
    }
}

#[cfg(test)]
#[path = "font_removals_tests.rs"]
mod tests;
