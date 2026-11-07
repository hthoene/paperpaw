//! Integer declaration handles; only DeclarationStore retains plugin owners.
use super::{
    declaration_store::{Allocations, DeclKey, DeclarationStore},
    registrations::Record,
};
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};
use paperpaw_native::host_glue::{ERR_BUSY, ERR_NO_SPACE, ERR_STATE};

#[derive(Default)]
struct Data {
    rows: Vec<DeclKey>,
    registry_order: Vec<u16>,
    scope_order: Vec<u16>,
}

/// Shared owns this catalog; it contains no dispatch, screens or Registry.
/// The callback supplies the retained store, so this creates no owner cycle.
pub(crate) struct RegCatalog {
    generation: Cell<u32>,
    data: RefCell<Data>,
}
impl RegCatalog {
    pub(crate) fn new() -> Self {
        Self {
            generation: Cell::new(1),
            data: RefCell::new(Data::default()),
        }
    }
    pub(crate) fn generation(&self) -> u32 {
        self.generation.get()
    }

    /// Called only at kernel apply points. All reservations and checked record
    /// visits precede publication; a refused replacement leaves the old view.
    pub(crate) fn replace(
        &self,
        store: &RefCell<DeclarationStore>,
        rows: &[DeclKey],
        allocations: &mut Allocations,
    ) -> Result<u32, i32> {
        let mut current = self.data.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        let store = store.try_borrow().map_err(|_| ERR_BUSY)?;
        u16::try_from(rows.len()).map_err(|_| ERR_NO_SPACE)?;
        let generation = self.generation.get().wrapping_add(1).max(1);
        let mut next = Data::default();
        allocations.reserve(&mut next.rows, rows.len())?;
        allocations.reserve(&mut next.registry_order, rows.len())?;
        allocations.reserve(&mut next.scope_order, rows.len())?;
        for (index, row) in rows.iter().enumerate() {
            visit_row(&store, *row, &mut |_| Ok(()))?;
            let index = u16::try_from(index).map_err(|_| ERR_NO_SPACE)?;
            next.rows.push(*row);
            // Scope sorting uses these integer indexes at the same apply point.
            next.registry_order.push(index);
            next.scope_order.push(index);
        }
        *current = next;
        self.generation.set(generation);
        Ok(generation)
    }

    /// The callback may encode into reply scratch but must not invoke plugins,
    /// reenter the store or retain metadata. Visibility is checked by the query.
    pub(crate) fn with_record(
        &self,
        store: &RefCell<DeclarationStore>,
        generation: u32,
        index: usize,
        visit: &mut dyn for<'a> FnMut(Record<'a>) -> Result<(), i32>,
    ) -> Result<(), i32> {
        let data = self.data.try_borrow().map_err(|_| ERR_BUSY)?;
        if generation != self.generation.get() {
            return Err(ERR_STATE);
        }
        let row = data.rows.get(index).ok_or(ERR_STATE)?;
        let store = store.try_borrow().map_err(|_| ERR_BUSY)?;
        visit_row(&store, *row, visit)
    }
}

fn visit_row(
    store: &DeclarationStore,
    row: DeclKey,
    visit: &mut dyn for<'a> FnMut(Record<'a>) -> Result<(), i32>,
) -> Result<(), i32> {
    let mut result = Err(ERR_STATE);
    if store.visit_record(row, &mut |record| result = visit(record)) {
        result
    } else {
        Err(ERR_STATE)
    }
}

#[cfg(test)]
#[path = "reg_catalog_tests.rs"]
mod tests;

#[cfg(target_pointer_width = "32")]
const _: () = assert!(core::mem::size_of::<DeclKey>() == 16);
