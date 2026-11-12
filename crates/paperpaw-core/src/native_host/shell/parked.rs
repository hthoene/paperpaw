//! What a parked plugin needs of its host (#248,
//! `docs/measurements/parked-plugins.md`): hosted again under the owner key it
//! had, and whether its `make` only registered.
use super::{DeclaredServices, Error, Hosted, Make, Report};
use crate::native_host::HostResources;
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
    /// As [`new`](Self::new), under the host owner key `owner` this pool
    /// assigned before: a parked plugin made again (#248,
    /// `docs/measurements/parked-plugins.md`). No key is drawn, so the
    /// instance row, the owner directory and what the kernel keys by `owner`
    /// stay. The plugin's earlier `Hosted` must be dropped first.
    ///
    /// # Errors
    /// As [`new`](Self::new); `Status(ERR_BUSY)` while the earlier owner
    /// still holds a handle or its retirement is queued, `Status(ERR_INVALID)`
    /// for a key the pool never assigned, `Status(ERR_STATE)` once a
    /// retirement was lost.
    ///
    /// # Safety
    /// As [`new`](Self::new).
    #[allow(clippy::too_many_arguments, reason = "as `new`, with the owner key")]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the kernel's unpark recipe calls it (#2638)")
    )]
    pub(crate) unsafe fn new_for_owner(
        services: S,
        module: M,
        make: Make,
        id: &str,
        image: (*const u8, usize),
        report: Report<S>,
        resources: HostResources,
        owner: u64,
        declared_services: DeclaredServices,
    ) -> Result<Self, Error> {
        let resources = resources.try_owner_for(owner).map_err(Error::Status)?;
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

    /// Whether `make` ran and called the host only to register (and for the
    /// heap): nothing it read can make a second `make` register differently
    /// (#248, parked plugins condition 1). `false` before `make` ran.
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the kernel's unpark recipe calls it (#2638)")
    )]
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
    ///
    /// # Errors
    /// `ERR_BUSY` while the handle table is in use.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the kernel's unpark recipe calls it (#2638)")
    )]
    pub(crate) fn owner_idle(&self) -> Result<bool, i32> {
        let context = self.owner.as_ref().context();
        if !self.make_registered_only() || context.calls.after_make.get() {
            return Ok(false);
        }
        context.resources.holds_handles().map(|holds| !holds)
    }
}

#[cfg(test)]
#[path = "parked_tests.rs"]
mod tests;
