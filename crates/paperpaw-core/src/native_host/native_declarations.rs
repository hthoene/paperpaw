//! One declaration producer retaining the same lease as native callbacks.
use super::{
    declaration_store::{Allocations, ImageOwner, PendingDeclarations},
    native_instances::NativeLease,
    native_screen::Dispatch,
    raw_declarations::RawDeclarations,
};
use paperpaw_native::host_glue::{ERR_NO_MEMORY, ERR_STATE};
use paperpaw_sdk::Id;

struct Source {
    lease: NativeLease,
    plugin: Id,
    owner: u64,
}
impl ImageOwner for Source {
    fn plugin_id(&self) -> &str {
        self.plugin.as_str()
    }
    fn generation(&self) -> u32 {
        u32::from(self.lease.live().is_ok())
    }
    fn owner_id(&self) -> Result<u64, i32> {
        Ok(self.owner)
    }
    fn native_lease(&self) -> Result<NativeLease, i32> {
        self.lease.live()?;
        self.lease.try_clone()
    }
    fn with_image(&self, visit: &mut dyn FnMut(Option<&[u8]>)) {
        match self.lease.live() {
            Ok(instance) => instance.with_image(visit),
            Err(_) => visit(None),
        }
    }
    fn invalid_declaration(&self, error: i32) {
        self.lease.dropped(0, error);
    }
}

pub(crate) fn pending(
    plugin: &Id,
    owner: u64,
    lease: NativeLease,
    raw: RawDeclarations,
    allocations: &mut Allocations,
) -> Result<PendingDeclarations, i32> {
    lease.live()?;
    let id = plugin.try_clone().ok_or(ERR_NO_MEMORY)?;
    let source = Source {
        lease,
        plugin: plugin.try_clone().ok_or(ERR_NO_MEMORY)?,
        owner,
    };
    if source.generation() == 0 {
        return Err(ERR_STATE);
    }
    let mut pending = PendingDeclarations::native(id, source)?;
    for batch in raw.batches {
        pending.stage_committed(batch, allocations)?;
    }
    Ok(pending)
}

/// The registry and callback frames share this one store and its keyed catalog.
#[derive(Default)]
pub(crate) struct NativeDeclarations {
    pub(crate) store: core::cell::RefCell<super::declaration_store::DeclarationStore>,
    pub(crate) catalog: super::reg_catalog::RegCatalog,
    /// What a plugin's callback froze of the stages it runs.
    pub(crate) members: core::cell::RefCell<super::stage_members::StageMembers>,
    /// The item states plugins pushed (`command.set_item_state`).
    pub(crate) items: core::cell::RefCell<super::item_state::ItemStates>,
    /// The entries callbacks in flight reserved in `items` and have not
    /// applied yet (a nested callback adds to its caller's).
    pub(crate) item_room: core::cell::Cell<usize>,
    /// The declaration cap each plugin was last refused at (`reg_plugin`
    /// problem 14 and 15).
    pub(crate) refusals: core::cell::RefCell<super::declaration_refusals::Refusals>,
}
impl core::fmt::Debug for NativeDeclarations {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("NativeDeclarations")
            .field("generation", &self.catalog.generation())
            .finish_non_exhaustive()
    }
}

pub(crate) struct Publication<'a> {
    store: core::cell::RefMut<'a, super::declaration_store::DeclarationStore>,
    catalog: Option<super::reg_catalog::PreparedCatalog<'a>>,
    owner: u64,
    committed: bool,
}
impl Publication<'_> {
    pub(crate) fn commit(mut self) {
        if let Some(catalog) = self.catalog.take() {
            catalog.commit();
        }
        self.committed = true;
    }
}
impl Drop for Publication<'_> {
    fn drop(&mut self) {
        if !self.committed {
            self.store.remove_host_owner(self.owner);
        }
    }
}
impl NativeDeclarations {
    /// Keeps the cap `plugin` was refused at, for its `reg_plugin` row.
    pub(crate) fn record_refusal(&self, plugin: &str, cap: super::declaration_checks::Cap) {
        if let Ok(mut refusals) = self.refusals.try_borrow_mut() {
            refusals.record(plugin, cap);
        }
    }

    /// `plugin` is admitted again: what was kept of an earlier refusal goes.
    pub(crate) fn clear_refusal(&self, plugin: &str) {
        if let Ok(mut refusals) = self.refusals.try_borrow_mut() {
            refusals.clear(plugin);
        }
    }

    /// The cap `plugin` was refused at, while it stays failed.
    pub(crate) fn refusal(&self, plugin: &str) -> Option<super::declaration_checks::Cap> {
        self.refusals.try_borrow().ok()?.get(plugin)
    }

    pub(crate) fn stage(
        &self,
        pending: PendingDeclarations,
        owner: u64,
        allocations: &mut Allocations,
    ) -> Result<Publication<'_>, i32> {
        let mut store = self
            .store
            .try_borrow_mut()
            .map_err(|_| paperpaw_native::host_glue::ERR_BUSY)?;
        if pending.host_owner_id()? != owner {
            return Err(ERR_STATE);
        }
        if store.host_owner(pending.plugin_id()).is_ok() {
            return Err(paperpaw_native::host_glue::ERR_INVALID);
        }
        store.prune_stopped_native();
        // The plugin ids are at most 64 bytes: kept on the stack for the
        // refusal, which comes after `pending` is gone.
        let id = super::registry_service::fixed::Fixed::<64>::of(&[pending.plugin_id()]);
        let prepared = match store.prepare(pending, allocations) {
            Ok(prepared) => prepared,
            Err(error) => {
                if let (Some(cap), Some(id)) = (allocations.cap, id) {
                    self.record_refusal(id.as_str(), cap);
                }
                return Err(error);
            }
        };
        prepared.commit();
        let prepared = store
            .all_keys(allocations)
            .and_then(|keys| self.catalog.prepare_store(&store, &keys, allocations));
        match prepared {
            Ok(catalog) => Ok(Publication {
                store,
                catalog: Some(catalog),
                owner,
                committed: false,
            }),
            Err(error) => {
                store.remove_host_owner(owner);
                Err(error)
            }
        }
    }
}

pub(crate) type DirectoryHandle = super::shared::Shared<NativeDeclarations>;

pub(crate) type DirectoryWeak = super::shared::Weak<NativeDeclarations>;
