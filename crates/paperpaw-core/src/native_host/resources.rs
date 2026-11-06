//! One device-wide registry for all persistent plugin handles.
//! Values own host metadata only. No plugin pointer or borrowed font survives a call.

use super::{flat::FlatError, shared::Shared};
use crate::abi7::handles::{Handles, Kind, Release};
use core::cell::{Cell, RefCell};
use paperpaw_native::host_glue::{ERR_NO_MEMORY, ERR_NO_SPACE};
use paperpaw_sdk::Id;

#[derive(Clone, Copy, Debug)]
pub(crate) enum FontRole {
    Ui,
    Reading,
    ReadingBold,
    ReadingItalic,
    ReadingBoldItalic,
    Status,
    Icons,
}

pub(crate) enum FontKey {
    Named { id: Id, acquired: u64 },
    Role(FontRole),
}

impl Release for FontKey {
    fn release(self) {}
}

struct Table {
    next_owner: Cell<u64>,
    handles: RefCell<Handles<FontKey>>,
}

/// Device-wide handle pool, shared by every hosted plugin on that device.
/// Construction is fallible; retaining the pool allocates nothing.
#[derive(Debug)]
pub struct HostResources(Shared<Table>);

impl HostResources {
    /// Allocate the actual bounded table before admitting a plugin.
    ///
    /// # Errors
    /// Returns Allocation when the table cannot be allocated.
    pub fn try_new() -> Result<Self, FlatError> {
        Shared::try_new(Table {
            next_owner: Cell::new(1),
            handles: RefCell::new(Handles::new()),
        })
        .map(Self)
        .map_err(|_| FlatError::Allocation)
    }

    /// Retain this same device table without allocation.
    #[must_use]
    pub fn try_clone(&self) -> Option<Self> {
        self.0.try_clone().map(Self)
    }

    /// Assign a never-reused host owner, without allocation.
    ///
    /// # Errors
    /// Returns ERR_NO_SPACE on owner exhaustion, ERR_NO_MEMORY on count exhaustion.
    pub fn try_owner(&self) -> Result<OwnerResources, i32> {
        let owner = self.0.next_owner.get();
        let next = owner.checked_add(1).ok_or(ERR_NO_SPACE)?;
        let table = self.0.try_clone().ok_or(ERR_NO_MEMORY)?;
        self.0.next_owner.set(next);
        Ok(OwnerResources { table, owner })
    }
}

/// Host-assigned ownership of one plugin's handles in the device-wide pool.
/// The lifecycle calls release_all before destroying its pinned context.
#[derive(Debug)]
pub struct OwnerResources {
    table: Shared<Table>,
    owner: u64,
}

impl OwnerResources {
    pub(crate) fn insert_font(&self, key: FontKey) -> Result<u32, i32> {
        self.table
            .handles
            .try_borrow_mut()
            .map_err(|_| paperpaw_native::host_glue::ERR_BUSY)?
            .insert(self.owner, Kind::Font, key)
            .map_err(|(error, _)| error.status())
    }

    /// Host-only visitor: never call a plugin or retain the key reference.
    pub(crate) fn with_font<R>(
        &self,
        handle: u32,
        visit: impl FnOnce(&FontKey) -> Result<R, i32>,
    ) -> Result<R, i32> {
        let handles = self
            .table
            .handles
            .try_borrow()
            .map_err(|_| paperpaw_native::host_glue::ERR_BUSY)?;
        visit(
            handles
                .get(self.owner, Kind::Font, handle)
                .map_err(crate::abi7::handles::Error::status)?,
        )
    }

    pub(crate) fn release(&self, handle: u32) -> Result<(), i32> {
        crate::abi7::handles::release(&self.table.handles, self.owner, Kind::Font, handle)
            .map_err(crate::abi7::handles::Error::status)
    }

    pub(crate) fn cancel_all(&self) -> Result<(), i32> {
        crate::abi7::handles::cancel_owner(&self.table.handles, self.owner)
            .map_err(crate::abi7::handles::Error::status)
    }

    pub(crate) fn release_all(&self) -> Result<(), i32> {
        crate::abi7::handles::release_owner(&self.table.handles, self.owner)
            .map_err(crate::abi7::handles::Error::status)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_native::host_glue::{ERR_BUSY, ERR_FULL, ERR_STATE};

    #[test]
    fn owners_share_capacity_and_cannot_use_each_others_handles() {
        let resources = HostResources::try_new().unwrap();
        let owners: alloc::vec::Vec<_> = (0..129).map(|_| resources.try_owner().unwrap()).collect();
        let mut first = 0;
        for (index, owner) in owners[..128].iter().enumerate() {
            for _ in 0..8 {
                let handle = owner.insert_font(FontKey::Role(FontRole::Ui)).unwrap();
                if index == 0 {
                    first = handle;
                }
            }
            assert_eq!(
                owner.insert_font(FontKey::Role(FontRole::Ui)),
                Err(ERR_FULL)
            );
        }
        assert_eq!(
            owners[128].insert_font(FontKey::Role(FontRole::Ui)),
            Err(ERR_FULL)
        );
        assert_eq!(owners[1].release(first), Err(ERR_STATE));
        owners[0].release_all().unwrap();
        assert_eq!(owners[0].with_font(first, |_| Ok(())), Err(ERR_STATE));
        let fresh = owners[128]
            .insert_font(FontKey::Role(FontRole::Ui))
            .unwrap();
        assert_ne!(fresh, first);
    }

    #[test]
    fn teardown_busy_is_retryable_and_owner_ids_do_not_wrap() {
        let resources = HostResources::try_new().unwrap();
        let owner = resources.try_owner().unwrap();
        let handle = owner.insert_font(FontKey::Role(FontRole::Ui)).unwrap();
        let borrowed = owner.table.handles.borrow();
        assert_eq!(owner.release_all(), Err(ERR_BUSY));
        drop(borrowed);
        owner.release_all().unwrap();
        assert_eq!(owner.release(handle), Err(ERR_STATE));
        resources.0.next_owner.set(u64::MAX);
        assert_eq!(resources.try_owner().unwrap_err(), ERR_NO_SPACE);
        assert_eq!(resources.0.next_owner.get(), u64::MAX);
    }
}
