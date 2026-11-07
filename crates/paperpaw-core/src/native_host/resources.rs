//! One device-wide registry for all persistent plugin handles.
//! Values own host metadata only. No plugin pointer or borrowed font survives a call.

use super::{flat::FlatError, shared::Shared};
use crate::abi7::handles::{Handles, Kind, Release};
use core::cell::{Cell, RefCell};
use paperpaw_native::host_glue::{ERR_NO_MEMORY, ERR_NO_SPACE};
use paperpaw_sdk::Id;

#[path = "resources/blobs.rs"]
mod blobs;
pub(crate) use blobs::Blob;

enum Resource {
    Font(FontKey),
    Blob(Blob),
}

impl Release for Resource {
    fn release(self) {
        if let Self::Font(font) = self {
            font.release();
        }
    }
}

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
    handles: RefCell<Handles<Resource>>,
    blob_limits: blobs::Limits,
}

/// Device-wide handle pool, shared by every hosted plugin on that device.
/// Construction is fallible; retaining the pool allocates nothing.
/// The X4 admits one live blob device-wide. A nested plugin's service query
/// returns ERR_BUSY while an outer callback holds a blob. Release it before
/// calling another plugin, or retry after the outer callback ends.
#[derive(Debug)]
pub struct HostResources(Shared<Table>);

impl HostResources {
    /// Allocate the actual bounded table before admitting a plugin.
    ///
    /// # Errors
    /// Returns Allocation when the table cannot be allocated.
    pub fn try_new() -> Result<Self, FlatError> {
        Self::with_blob_limits(blobs::Limits::X4)
    }

    fn with_blob_limits(blob_limits: blobs::Limits) -> Result<Self, FlatError> {
        Shared::try_new(Table {
            next_owner: Cell::new(1),
            handles: RefCell::new(Handles::new()),
            blob_limits,
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

/// Retained callback view. Dropping this releases only the table reference.
/// It never owns teardown of any handle family or changes owner identity.
#[derive(Debug)]
pub(crate) struct OwnerResourceView {
    table: Shared<Table>,
    owner: u64,
}

impl OwnerResources {
    pub(crate) fn try_view(&self) -> Option<OwnerResourceView> {
        self.table.try_clone().map(|table| OwnerResourceView {
            table,
            owner: self.owner,
        })
    }

    pub(crate) fn insert_font(&self, key: FontKey) -> Result<u32, i32> {
        self.table
            .handles
            .try_borrow_mut()
            .map_err(|_| paperpaw_native::host_glue::ERR_BUSY)?
            .insert(self.owner, Kind::Font, Resource::Font(key))
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
        match handles
            .get(self.owner, Kind::Font, handle)
            .map_err(crate::abi7::handles::Error::status)?
        {
            Resource::Font(font) => visit(font),
            Resource::Blob(_) => Err(paperpaw_native::host_glue::ERR_STATE),
        }
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

impl Drop for OwnerResources {
    fn drop(&mut self) {
        loop {
            // A conflicting borrow means teardown could not finish. Entries
            // are host-owned and cannot call the dead plugin; leave them put.
            let Ok(mut handles) = self.table.handles.try_borrow_mut() else {
                return;
            };
            let resource = handles.take_owner(self.owner);
            drop(handles);
            let Some(resource) = resource else {
                return;
            };
            resource.release();
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
    extern crate std;
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
    fn worst_case_named_fonts_are_bounded_but_slots_do_not_shrink() {
        let resources = HostResources::try_new().unwrap();
        let owners: alloc::vec::Vec<_> = (0..128).map(|_| resources.try_owner().unwrap()).collect();
        let key = alloc::format!("fonts.{}", "x".repeat(122));
        assert_eq!(key.len(), 128);
        for owner in &owners {
            for _ in 0..8 {
                let id = Id::parse(&key).unwrap();
                owner
                    .insert_font(FontKey::Named { id, acquired: 0 })
                    .unwrap();
            }
        }
        let slot_bytes = crate::abi7::handles::slot_bytes::<FontKey>();
        // Exact per-target slot layout is counted, not estimated from a key.
        assert!(slot_bytes >= core::mem::size_of::<FontKey>());
        std::println!(
            "font slots: {slot_bytes} B each, {} B device-wide, {} B per-owner; names: 131072 B device-wide, 1024 B per-owner",
            slot_bytes * 1024,
            slot_bytes * 8
        );
        let capacity = resources.0.handles.borrow().capacity();
        assert!(capacity >= 1024);
        drop(owners);
        let mut handles = resources.0.handles.borrow_mut();
        assert_eq!(handles.capacity(), capacity);
        let successor = resources.try_owner().unwrap();
        // The pool remains usable at maximum capacity after all owners drop.
        for _ in 0..8 {
            handles
                .insert(
                    successor.owner,
                    Kind::Font,
                    Resource::Font(FontKey::Role(FontRole::Ui)),
                )
                .map_err(|(error, _)| error)
                .unwrap();
        }
    }

    #[test]
    fn dropping_an_owner_frees_its_slots_without_explicit_teardown() {
        let resources = HostResources::try_new().unwrap();
        let owner = resources.try_owner().unwrap();
        let survivor = resources.try_owner().unwrap();
        let dead_id = owner.owner;
        let old = owner.insert_font(FontKey::Role(FontRole::Ui)).unwrap();
        let live = survivor.insert_font(FontKey::Role(FontRole::Ui)).unwrap();
        drop(owner);
        assert!(
            resources
                .0
                .handles
                .borrow_mut()
                .take_owner(dead_id)
                .is_none()
        );
        assert_eq!(survivor.with_font(live, |_| Ok(())), Ok(()));
        let replacement = resources.try_owner().unwrap();
        let fresh = replacement
            .insert_font(FontKey::Role(FontRole::Ui))
            .unwrap();
        assert_ne!(fresh, old);
        assert_eq!(replacement.with_font(old, |_| Ok(())), Err(ERR_STATE));
    }

    #[test]
    fn dropping_an_owner_during_a_table_borrow_does_not_panic() {
        let resources = HostResources::try_new().unwrap();
        let owner = resources.try_owner().unwrap();
        let dead_id = owner.owner;
        owner.insert_font(FontKey::Role(FontRole::Ui)).unwrap();
        let borrowed = resources.0.handles.borrow();
        drop(owner);
        drop(borrowed);
        // Best effort deliberately leaves this host-only entry under a dead
        // owner. A later successful explicit cleanup can still reclaim it.
        assert!(
            resources
                .0
                .handles
                .borrow_mut()
                .take_owner(dead_id)
                .is_some()
        );
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
