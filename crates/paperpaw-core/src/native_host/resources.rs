//! One device-wide registry for all persistent plugin handles.
//! Values own host metadata only. No plugin pointer or borrowed font survives a call.

use super::{flat::FlatError, shared::Shared};
use crate::abi7::handles::{Handles, Kind, Release};
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};
use paperpaw_native::host_glue::{ERR_NO_MEMORY, ERR_NO_SPACE};
use paperpaw_sdk::Id;

#[path = "resources/blobs.rs"]
mod blobs;
pub(crate) use blobs::Blob;

#[path = "resources/services.rs"]
mod services;
pub(crate) use services::ServiceKey;
use services::ServiceRegistry;

enum Resource {
    Font(FontKey),
    Blob(Blob),
    Service(ServiceKey),
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
    Named {
        id: Id,
        acquired: u64,
    },
    Role(FontRole),
    /// A font file of the plugin's own data folder (`data:<path>`), opened
    /// when acquired and owned by the handle (no registry); stale once the
    /// file is changed or removed.
    Data(alloc::boxed::Box<super::fonts_data::DataFont>),
}

impl Release for FontKey {
    fn release(self) {}
}

struct Table {
    next_owner: Cell<u64>,
    /// One bit per owner key with a live [`OwnerResources`]: a key is hosted
    /// again only once its owner is gone ([`HostResources::try_owner_for`]).
    /// The first word is reserved with the table; 8 bytes a 64 keys.
    live: RefCell<Vec<u64>>,
    handles: RefCell<Handles<Resource>>,
    blob_limits: blobs::Limits,
    services: ServiceRegistry,
    /// The wired views behind `core.document`, `core.library`, `core.lookup`.
    hosts: super::host_services::HostServices,
    /// Who owns the plugin heap's blocks and the budget each may use.
    heap: super::plugin_heap::HeapState,
}

impl Table {
    /// Marks `owner` live or not; the bitmap grows to hold a new key.
    fn mark_live(&self, owner: u64, live: bool) -> Result<(), i32> {
        let word = usize::try_from(owner / 64).map_err(|_| ERR_NO_SPACE)?;
        let mut words = self
            .live
            .try_borrow_mut()
            .map_err(|_| paperpaw_native::host_glue::ERR_BUSY)?;
        if word >= words.len() {
            if !live {
                return Ok(());
            }
            words
                .try_reserve_exact(word + 1 - words.len())
                .map_err(|_| ERR_NO_MEMORY)?;
            words.resize(word + 1, 0);
        }
        let bit = 1_u64 << (owner % 64);
        if let Some(bits) = words.get_mut(word) {
            if live {
                *bits |= bit;
            } else {
                *bits &= !bit;
            }
        }
        Ok(())
    }

    /// Whether `owner` has a live [`OwnerResources`].
    fn is_live(&self, owner: u64) -> Result<bool, i32> {
        let words = self
            .live
            .try_borrow()
            .map_err(|_| paperpaw_native::host_glue::ERR_BUSY)?;
        let word = usize::try_from(owner / 64).unwrap_or(usize::MAX);
        Ok(words
            .get(word)
            .is_some_and(|bits| bits & (1_u64 << (owner % 64)) != 0))
    }
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
        Self::with_services(
            blob_limits,
            ServiceRegistry::device().map_err(|_| FlatError::Allocation)?,
        )
    }

    fn with_services(
        blob_limits: blobs::Limits,
        services: ServiceRegistry,
    ) -> Result<Self, FlatError> {
        let mut live = Vec::new();
        live.try_reserve_exact(1)
            .map_err(|_| FlatError::Allocation)?;
        live.push(0);
        Shared::try_new(Table {
            next_owner: Cell::new(1),
            live: RefCell::new(live),
            handles: RefCell::new(Handles::new()),
            blob_limits,
            services,
            hosts: super::host_services::HostServices::new(),
            heap: super::plugin_heap::HeapState::new(),
        })
        .map(Self)
        .map_err(|_| FlatError::Allocation)
    }

    /// Sets the per-plugin heap budget (`named`: the shell named the limit
    /// itself, which wins over the default of a memory class).
    pub(crate) fn set_plugin_heap(&self, bytes: usize, named: bool) {
        self.0.heap.set_limit(bytes, named);
    }

    /// The per-plugin heap budget now.
    #[cfg(test)]
    pub(crate) fn plugin_heap_limit(&self) -> usize {
        self.0.heap.limit()
    }

    /// Retain this same device table without allocation.
    #[must_use]
    pub fn try_clone(&self) -> Option<Self> {
        self.0.try_clone().map(Self)
    }

    #[allow(dead_code, reason = "a shell wires its host views; tests do today")]
    /// The wired host views, to wire a family once (a shell, a test).
    pub(crate) fn hosts(&self) -> &super::host_services::HostServices {
        &self.0.hosts
    }

    /// The main loop's retry for owners whose retirement was busy when their
    /// handle table dropped: call it again after `Ok(true)` (some are left) and
    /// after `Err(ERR_BUSY)` (one is still busy).
    ///
    /// # Errors
    /// The first error of an owner that stays queued (`ERR_BUSY`).
    pub fn retry_retirements(&self) -> Result<bool, i32> {
        self.0.hosts.retry_retirements()
    }

    /// Assign a never-reused host owner, without allocation.
    ///
    /// # Errors
    /// Returns ERR_NO_SPACE on owner exhaustion, ERR_NO_MEMORY on count exhaustion.
    pub fn try_owner(&self) -> Result<OwnerResources, i32> {
        let owner = self.0.next_owner.get();
        let next = owner.checked_add(1).ok_or(ERR_NO_SPACE)?;
        let resources = self.admit(owner)?;
        self.0.next_owner.set(next);
        Ok(resources)
    }

    /// Admits the owner `owner`: marked live, room for its retirement
    /// reserved (see the queue), its place in the heap's accounting. On a
    /// refusal nothing of it is left.
    fn admit(&self, owner: u64) -> Result<OwnerResources, i32> {
        let table = self.0.try_clone().ok_or(ERR_NO_MEMORY)?;
        self.0.mark_live(owner, true)?;
        if let Err(status) = self.0.hosts.admit_owner() {
            let _ = self.0.mark_live(owner, false);
            return Err(status);
        }
        let Ok(heap_owner) = self.0.heap.admit() else {
            // Its reserved retirement is given back, and its mark.
            self.0.hosts.owner_gone(owner, true);
            let _ = self.0.mark_live(owner, false);
            return Err(ERR_NO_MEMORY);
        };
        Ok(OwnerResources {
            table,
            owner,
            heap_owner,
            caller: None,
        })
    }

    /// Admits `owner` again, a key this pool assigned before whose resources
    /// are gone: a parked plugin made again (#248, `parked-plugins.md`). No
    /// key is drawn, so what the kernel keys by it (the instance row, the
    /// owner directory, timers, queued events) stays as it is. Only one
    /// `OwnerResources` of a key may live at a time: the caller dropped the
    /// earlier one (its handles are released, its retirement ran); a key
    /// whose owner still lives is refused. Each hosting takes a new place in
    /// the heap's accounting (that table only grows): phase 1 hosts a plugin
    /// again once at most; parking again (phase 2) must reuse the old place.
    ///
    /// # Errors
    /// `ERR_INVALID` for a key this pool never assigned; `ERR_BUSY` while the
    /// owner still lives, holds a handle or its retirement is queued, or a
    /// table is in use; `ERR_STATE` once a retirement was lost; as
    /// [`Self::try_owner`] otherwise.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the kernel's unpark recipe calls it (#2638)")
    )]
    pub(crate) fn try_owner_for(&self, owner: u64) -> Result<OwnerResources, i32> {
        if owner == 0 || owner >= self.0.next_owner.get() {
            return Err(paperpaw_native::host_glue::ERR_INVALID);
        }
        if self.0.is_live(owner)? {
            return Err(paperpaw_native::host_glue::ERR_BUSY);
        }
        let holds = self
            .0
            .handles
            .try_borrow()
            .map_err(|_| paperpaw_native::host_glue::ERR_BUSY)?
            .holds(owner);
        if holds || self.0.hosts.retiring(owner)? {
            return Err(paperpaw_native::host_glue::ERR_BUSY);
        }
        self.admit(owner)
    }
}

/// Host-assigned ownership of one plugin's handles in the device-wide pool.
/// The lifecycle calls release_all before destroying its pinned context.
#[derive(Debug)]
pub struct OwnerResources {
    table: Shared<Table>,
    owner: u64,
    /// This owner's place in the plugin heap's owner table.
    heap_owner: u32,
    caller: Option<Id>,
}

/// Retained callback view. Dropping this releases only the table reference.
/// It never owns teardown of any handle family or changes owner identity.
#[derive(Debug)]
pub(crate) struct OwnerResourceView {
    table: Shared<Table>,
    owner: u64,
}

impl OwnerResourceView {
    /// The `core.ui` service, once a shell wired it.
    pub(crate) fn ui(&self) -> Option<&super::ui_service::UiServices> {
        self.table.hosts.ui()
    }

    /// The device's wired host views.
    pub(crate) fn hosts(&self) -> &super::host_services::HostServices {
        &self.table.hosts
    }

    /// Another view of the same pool, without allocation.
    pub(crate) fn try_clone(&self) -> Option<Self> {
        self.table.try_clone().map(|table| Self {
            table,
            owner: self.owner,
        })
    }

    pub(crate) fn ui_store(&self) -> Option<&super::ui_store::UiStore> {
        self.table.hosts.ui().map(|ui| &*ui.store)
    }

    pub(crate) fn owner_id(&self) -> u64 {
        self.owner
    }
    /// The device's owner directory (see [`super::owner_directory`]).
    pub(crate) fn owners(&self) -> Result<&Shared<super::owner_directory::OwnerDirectory>, i32> {
        self.table.hosts.owners()
    }
    pub(crate) fn retry_retirements(&self) -> Result<bool, i32> {
        self.table.hosts.retry_retirements()
    }
    pub(crate) fn retirements_pending(&self) -> bool {
        self.table.hosts.retirements_pending()
    }
    pub(crate) fn same_pool(&self, other: &Self) -> bool {
        core::ptr::eq(&*self.table, &*other.table)
    }
}

impl OwnerResources {
    /// Bind the host-admitted identity once, before plugin construction.
    /// Preflight resource owners remain unbound and cannot call a service.
    pub(crate) fn bind_caller(&mut self, id: &str) -> Result<(), i32> {
        if self.caller.is_some() {
            return Err(paperpaw_native::host_glue::ERR_STATE);
        }
        self.caller = Some(Id::try_parse(id).map_err(|error| match error {
            paperpaw_sdk::IdError::NoMemory => ERR_NO_MEMORY,
            _ => paperpaw_native::host_glue::ERR_INVALID,
        })?);
        Ok(())
    }

    pub(crate) fn bound_caller(&self) -> Result<&Id, i32> {
        self.caller
            .as_ref()
            .ok_or(paperpaw_native::host_glue::ERR_DENIED)
    }

    pub(crate) fn owner_id(&self) -> u64 {
        self.owner
    }

    /// Whether this owner holds any handle (a font, blob or service).
    ///
    /// # Errors
    /// `ERR_BUSY` while the handle table is in use.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the kernel's unpark recipe calls it (#2638)")
    )]
    pub(crate) fn holds_handles(&self) -> Result<bool, i32> {
        self.table
            .handles
            .try_borrow()
            .map(|handles| handles.holds(self.owner))
            .map_err(|_| paperpaw_native::host_glue::ERR_BUSY)
    }

    /// The plugin heap's accounting and this owner's place in it.
    pub(crate) fn heap(&self) -> (&super::plugin_heap::HeapState, u32) {
        (&self.table.heap, self.heap_owner)
    }

    /// The device's wired host views (a slot reaches its family here).
    pub(crate) fn hosts(&self) -> &super::host_services::HostServices {
        &self.table.hosts
    }

    pub(crate) fn try_view(&self) -> Option<OwnerResourceView> {
        self.table.try_clone().map(|table| OwnerResourceView {
            table,
            owner: self.owner_id(),
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
            Resource::Blob(_) | Resource::Service(_) => Err(paperpaw_native::host_glue::ERR_STATE),
        }
    }

    pub(crate) fn release(&self, handle: u32) -> Result<(), i32> {
        let mut resource = {
            let mut table = self
                .table
                .handles
                .try_borrow_mut()
                .map_err(|_| paperpaw_native::host_glue::ERR_BUSY)?;
            match table.remove(self.owner, Kind::Font, handle) {
                Ok(resource) => resource,
                Err(crate::abi7::handles::Error::State) => table
                    .remove(self.owner, Kind::Service, handle)
                    .map_err(crate::abi7::handles::Error::status)?,
                Err(error) => return Err(error.status()),
            }
        };
        resource.cancel();
        resource.release();
        Ok(())
    }

    pub(crate) fn cancel_all(&self) -> Result<(), i32> {
        crate::abi7::handles::cancel_owner(&self.table.handles, self.owner)
            .map_err(crate::abi7::handles::Error::status)
    }

    pub(crate) fn release_all(&self) -> Result<(), i32> {
        let released = crate::abi7::handles::release_owner(&self.table.handles, self.owner)
            .map_err(crate::abi7::handles::Error::status);
        // The owner is retired whether or not every handle could be released
        // (a busy table is retried): what the document and library services
        // held for it ends now, so no slot or transaction stays behind.
        let retired = self.table.hosts.owner_retired(self.owner);
        released.and(retired)
    }

    /// The caller of a service call: this owner and the plugin `plugin`.
    pub(crate) fn caller<'a>(
        &'a self,
        plugin: &'a str,
        features: super::admitted_features::AdmittedFeatures,
    ) -> super::host_services::Caller<'a> {
        super::host_services::Caller {
            owner: self.owner,
            plugin,
            features,
            hosts: &self.table.hosts,
        }
    }
}

impl OwnerResources {
    /// Best effort: release every handle of the owner.
    fn release_leftovers(&self) {
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

impl Drop for OwnerResources {
    fn drop(&mut self) {
        self.release_leftovers();
        // Whatever a busy table left, the owner is retired: its documents,
        // image ids and transactions end (idempotent after `release_all`).
        // A table that is busy here has no caller left to retry: the owner is
        // queued and the main loop retries it until it is done.
        let retired = self.table.hosts.owner_retired(self.owner).is_ok();
        self.table.hosts.owner_gone(self.owner, retired);
        // Not live any more (a busy mark keeps it live: never hosted again).
        let _ = self.table.mark_live(self.owner, false);
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

    /// Review of #308 (SF1): a busy handle table must not keep the owner's
    /// documents (an X4 slot) or transactions alive, nor leave close calls
    /// into a retired plugin.
    #[test]
    fn a_busy_handle_table_still_retires_the_owner() {
        use crate::native_host::{
            admitted_features::AdmittedFeatures,
            doc_service::{
                DocHost, Limits, PREPARING,
                tests::{FORMAT, Fake},
            },
            host_services::tests::Lib,
        };
        for by_drop in [false, true] {
            let resources = HostResources::try_new().unwrap();
            let fake = Fake::new(Limits::LARGE);
            let host: alloc::rc::Rc<dyn DocHost> = fake.clone();
            resources.hosts().wire_documents(host).unwrap();
            let lib = alloc::rc::Rc::new(Lib::new());
            resources.hosts().wire_library(lib.clone()).unwrap();
            let owner = resources.try_owner().unwrap();
            let id = owner.caller("opener", AdmittedFeatures::NONE).owner;
            let document = fake
                .table()
                .open(id, false, FORMAT, PREPARING, 0, &|_| false)
                .unwrap()
                .0;
            let transaction = lib.transactions.begin_for_test(id, 0);
            owner.insert_font(FontKey::Role(FontRole::Ui)).unwrap();
            let borrowed = resources.0.handles.borrow();
            if by_drop {
                drop(owner);
            } else {
                assert_eq!(owner.release_all(), Err(ERR_BUSY));
            }
            drop(borrowed);
            assert!(
                fake.table().lease(id, document).is_err(),
                "the slot is free"
            );
            assert_eq!(
                *fake.ended.borrow(),
                [document],
                "the format closed it once"
            );
            assert_eq!(*lib.aborted.borrow(), [transaction]);
        }
    }
}
