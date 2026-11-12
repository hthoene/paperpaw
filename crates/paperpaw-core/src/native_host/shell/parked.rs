//! What a parked plugin needs of its host (#248,
//! `docs/measurements/parked-plugins.md`): hosted again under the owner key it
//! had, and whether its `make` only registered.
use super::{Context, DeclaredServices, Error, Hosted, Make, Report};
use crate::native_host::HostResources;
use crate::native_host::resources::OwnerSeat;
use core::cell::Cell;
use paperpaw_native::host_glue::Services;

/// What a plugin's own code called the host for, besides the heap: in
/// `make` other than `registrations.submit`, and in anything after `make`
/// (a callback, its destructor). Set by the frame owner's record
/// (`frame_owner::within_recorded`), read before a plugin is parked.
#[derive(Default)]
pub(super) struct HostCalls {
    pub(super) make: Cell<bool>,
    pub(super) after_make: Cell<bool>,
}

impl<S: Services + 'static, M> Hosted<S, M> {
    /// As [`new`](Self::new), with the id already checked: a linked plugin's
    /// is the firmware's text, so no copy of it takes heap (#248, D4).
    ///
    /// # Safety
    /// As [`new`](Self::new).
    #[allow(clippy::too_many_arguments, reason = "as `new`")]
    pub(crate) unsafe fn with_caller(
        services: S,
        module: M,
        make: Make,
        id: paperpaw_sdk::Id,
        image: (*const u8, usize),
        report: Report<S>,
        resources: HostResources,
        declared_services: DeclaredServices,
    ) -> Result<Self, Error> {
        let resources = resources.try_owner().map_err(Error::Status)?;
        // SAFETY: the caller's contract, passed on.
        unsafe {
            Self::host(
                services,
                module,
                make,
                id,
                image,
                report,
                resources,
                declared_services,
            )
        }
    }

    /// As [`new`](Self::new), under the owner key and heap place `seat` of
    /// the plugin's earlier `Hosted` ([`Self::seat`], kept while it is
    /// parked): a parked plugin made again (#248,
    /// `docs/measurements/parked-plugins.md`). No key is drawn, so the
    /// instance row, the owner directory and what the kernel keys by the key
    /// stay, and the plugin is charged on its old heap row. The earlier
    /// `Hosted` must be dropped first.
    ///
    /// # Errors
    /// As [`new`](Self::new); `Status(ERR_BUSY)` while the earlier owner
    /// lives, holds a handle or its retirement is queued,
    /// `Status(ERR_INVALID)` for a seat of another pool, `Status(ERR_STATE)`
    /// once a retirement was lost.
    ///
    /// # Safety
    /// As [`new`](Self::new).
    #[allow(clippy::too_many_arguments, reason = "as `new`, with the seat")]
    pub(crate) unsafe fn new_for_owner(
        services: S,
        module: M,
        make: Make,
        id: paperpaw_sdk::Id,
        image: (*const u8, usize),
        report: Report<S>,
        resources: HostResources,
        seat: OwnerSeat,
        declared_services: DeclaredServices,
    ) -> Result<Self, Error> {
        let resources = resources.try_owner_for(seat).map_err(Error::Status)?;
        // SAFETY: the caller's contract, passed on.
        unsafe {
            Self::host(
                services,
                module,
                make,
                id,
                image,
                report,
                resources,
                declared_services,
            )
        }
    }

    /// The owner key and heap place to host the plugin again under
    /// ([`Self::new_for_owner`]), kept while it is parked.
    pub(crate) fn seat(&self) -> OwnerSeat {
        self.owner.as_ref().context().resources.seat()
    }

    /// Whether `make` ran and called the host only to register (and for the
    /// heap): nothing it read can make a second `make` register differently
    /// (#248, parked plugins condition 1). `false` before `make` ran.
    #[must_use]
    pub(crate) fn make_registered_only(&self) -> bool {
        let context = self.owner.as_ref().context();
        context.attempted.get() && !context.calls.make.get()
    }

    /// Whether the owner has nothing the host keeps for it (#248, parked
    /// plugins condition 2, host side): `make` only registered, the plugin's
    /// code has not called the host since, and it holds no handle. Every
    /// other host resource (an open operation, file, socket, secret,
    /// document, request) needs a host call of its own, so none can be held
    /// then. The kernel adds its own tables (timers, work, queued events).
    /// It does not see the plugin's own memory: sound for a plugin parked
    /// before any callback ran (phase 1), not on its own for parking one
    /// again after it ran (phase 2), whose state would be lost.
    ///
    /// # Errors
    /// `ERR_BUSY` while the handle table is in use.
    pub(crate) fn owner_idle(&self) -> Result<bool, i32> {
        let context = self.owner.as_ref().context();
        if !self.make_registered_only() || context.calls.after_make.get() {
            return Ok(false);
        }
        context.resources.holds_handles().map(|holds| !holds)
    }

    /// Whether the owner holds nothing now (#248, D3: parking a plugin again
    /// after it ran): no blob (its font and service handles are released and
    /// opened again by the remade plugin), and nothing any host service would
    /// end with it (`HostServices::owner_holds`). What the plugin kept in its own
    /// memory is not seen here: only plugins whose shell lets them sleep,
    /// each with its parity case, are parked again. `false` where a table is
    /// in use.
    #[must_use]
    pub(crate) fn holds_nothing(&self) -> bool {
        let resources = &self.owner.as_ref().context().resources;
        resources.holds_blobs() == Ok(false)
            && resources.hosts().owner_holds(resources.owner_id()) == Ok(false)
    }
}

#[cfg(test)]
#[path = "parked_tests.rs"]
mod tests;

impl<S: 'static> Context<S> {
    /// A copy of the plugin's id: a linked plugin's is the shell's text and
    /// copies without allocating (#248, D4).
    ///
    /// # Errors
    /// `ERR_NO_MEMORY` when a loaded plugin's id cannot be copied.
    pub(crate) fn plugin(&self) -> Result<paperpaw_sdk::Id, i32> {
        self.resources
            .bound_caller()?
            .try_clone()
            .ok_or(paperpaw_native::host_glue::ERR_NO_MEMORY)
    }
}
