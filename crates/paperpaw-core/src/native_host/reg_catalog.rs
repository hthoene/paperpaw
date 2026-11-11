//! The generation of the kernel's declaration catalog.
//!
//! The declaration store keeps every record and its keys; this keeps no copy
//! of them. It only counts the changes, so a reader can tell that what it
//! read under an older generation may be out of date.
use super::declaration_store::DeclarationStore;
use core::cell::Cell;

/// Shared owns this counter; it holds no plugin owner and no store.
pub(crate) struct RegCatalog {
    generation: Cell<u32>,
}

/// The next generation, published by `commit` and not before.
pub(crate) struct PreparedCatalog<'a> {
    generation: &'a Cell<u32>,
    next: u32,
}
impl PreparedCatalog<'_> {
    pub(crate) fn commit(&self) -> u32 {
        self.generation.set(self.next);
        self.next
    }
}

impl Default for RegCatalog {
    fn default() -> Self {
        Self::new()
    }
}

impl RegCatalog {
    pub(crate) fn new() -> Self {
        Self {
            generation: Cell::new(1),
        }
    }

    pub(crate) fn generation(&self) -> u32 {
        self.generation.get()
    }

    /// Called only at kernel apply points; it allocates nothing.
    pub(crate) fn prepare(&self) -> PreparedCatalog<'_> {
        PreparedCatalog {
            generation: &self.generation,
            next: next(self.generation.get()),
        }
    }

    /// An owner left the store: the generation moves on. It cannot fail;
    /// the result stays for the uninstall path, which checks it.
    pub(crate) fn prune(&self, _store: &DeclarationStore) -> Result<(), i32> {
        self.generation.set(next(self.generation.get()));
        Ok(())
    }
}

/// Zero is never a generation, so a reader can keep 0 for "never read".
fn next(generation: u32) -> u32 {
    generation.wrapping_add(1).max(1)
}

#[cfg(test)]
#[path = "reg_catalog_tests.rs"]
mod tests;
