//! A stable host context owns this scope; each event supplies its borrowed Env.
//! No environment or device pointer crosses the plugin boundary.

use super::Error;
pub use crate::abi7::settings_wire::{SettingValueRef, settings_decode, settings_encode};
pub use crate::abi7::snapshot_wire::{
    Battery, PixelFormat, SNAPSHOT_BYTES, SNAPSHOT_MAX_BYTES, SNAPSHOT_VERSION, Snapshot,
    snapshot_decode,
};
use crate::abi7::{
    context::{Error as FrameError, FrameSlot},
    core_frame::CoreFrame,
    queries, snapshot,
};
use core::{cell::Cell, fmt, pin::Pin};
use paperpaw_sdk::Env;

/// Scoped environment access inside a pinned per-plugin host context.
#[non_exhaustive]
pub struct EnvironmentScope {
    slot: FrameSlot,
    callback: Cell<u64>,
    /// What runs after each callback: the callback's blobs expire, through
    /// the resources of the context that holds this scope (#248: not a view
    /// of them of its own, one per plugin).
    cleanup: Cell<Option<Cleanup>>,
}

#[derive(Clone, Copy)]
struct Cleanup {
    context: *const core::ffi::c_void,
    run: unsafe fn(*const core::ffi::c_void),
}

struct ExpireBlobs<'a>(&'a EnvironmentScope);

impl Drop for ExpireBlobs<'_> {
    fn drop(&mut self) {
        if let Some(cleanup) = self.0.cleanup.get() {
            // SAFETY: installation ties the hook to the pinned host lifetime.
            unsafe {
                (cleanup.run)(cleanup.context);
            }
        }
    }
}

impl EnvironmentScope {
    /// Create an empty scope before pinning the containing host context.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            slot: FrameSlot::new(),
            callback: Cell::new(0),
            cleanup: Cell::new(None),
        }
    }

    pub(crate) fn callback_id(&self) -> u64 {
        self.callback.get()
    }

    /// Install what runs after each callback (its blobs expire, a failure is
    /// reported) once its containing context is pinned.
    ///
    /// # Safety
    /// Context must remain live and immovable while this scope can run. The
    /// run function must use that context's type and never panic or reenter.
    pub(crate) unsafe fn set_cleanup(
        &self,
        context: *const core::ffi::c_void,
        run: unsafe fn(*const core::ffi::c_void),
    ) {
        self.cleanup.set(Some(Cleanup { context, run }));
    }

    /// Tests: the blobs of `view` expire after each callback, as a hosted
    /// plugin's do.
    ///
    /// # Safety
    /// `view` stays in place while this scope can run.
    #[cfg(test)]
    pub(crate) unsafe fn expire_view(&self, view: &super::resources::OwnerResourceView) {
        unsafe fn run(raw: *const core::ffi::c_void) {
            // SAFETY: `expire_view`'s caller keeps the view in place.
            let view = unsafe { &*raw.cast::<super::resources::OwnerResourceView>() };
            let _ = view.expire_blobs();
        }
        // SAFETY: passed on to the caller.
        unsafe { self.set_cleanup(core::ptr::from_ref(view).cast(), run) };
    }

    fn run_callback<R>(&self, call: impl FnOnce() -> R) -> Result<R, Error> {
        // This cannot wrap in a device lifetime. Refusal preserves stale-handle checks.
        let next = self.callback.get().checked_add(1).ok_or(Error::Capacity)?;
        self.callback.set(next);
        let _expire = ExpireBlobs(self);
        // Cleanup reports separately: the callback ran, even if reclamation fails.
        Ok(call())
    }

    /// Supply the event's environment only for this synchronous dispatch.
    /// Clears borrowed state on return and unwind; nested dispatch is refused.
    ///
    /// # Errors
    /// Returns Busy if an environment is already installed.
    pub fn scope<R>(self: Pin<&Self>, env: &Env<'_>, call: impl FnOnce() -> R) -> Result<R, Error> {
        // SAFETY: the slot is structurally pinned; no method moves it, and
        // EnvironmentScope has no destructor that moves the field.
        let slot = unsafe { self.map_unchecked(|this| &this.slot) };
        slot.scope(env, || self.run_callback(call))
            .map_err(|error| match error {
                FrameError::Busy => Error::Busy,
                FrameError::NoFrame => Error::State,
            })?
    }

    /// Core dispatch supplies separate shared field borrows for one callback.
    /// Usage-table borrows exist only inside individual host visitor calls.
    pub(crate) fn scope_core<R>(
        self: Pin<&Self>,
        frame: &CoreFrame<'_>,
        call: impl FnOnce() -> R,
    ) -> Result<R, Error> {
        // SAFETY: the slot is structurally pinned and never moved by this scope.
        let slot = unsafe { self.map_unchecked(|this| &this.slot) };
        slot.scope(frame, || self.run_callback(call))
            .map_err(|error| match error {
                FrameError::Busy => Error::Busy,
                FrameError::NoFrame => Error::State,
            })?
    }

    /// Test frames lent for one synchronous call, as `scope_core` lends core's.
    #[cfg(test)]
    pub(crate) fn scope_frame<R>(
        self: Pin<&Self>,
        frame: &dyn crate::abi7::context::Frame,
        call: impl FnOnce() -> R,
    ) -> Result<R, Error> {
        // SAFETY: the slot is structurally pinned and never moved by this scope.
        let slot = unsafe { self.map_unchecked(|this| &this.slot) };
        slot.scope(frame, || self.run_callback(call))
            .map_err(|error| match error {
                FrameError::Busy => Error::Busy,
                FrameError::NoFrame => Error::State,
            })?
    }

    /// Internal host operations borrow a frame only for this synchronous call.
    pub(crate) fn with_frame<R>(
        &self,
        read: impl FnOnce(&dyn crate::abi7::context::Frame) -> R,
    ) -> Result<R, i32> {
        self.slot
            .with(read)
            .map_err(|_| paperpaw_native::host_glue::ERR_STATE)
    }

    /// Plugin reads supply the identity bound by Hosted::new, never input bytes.
    ///
    /// # Safety
    /// Input/output meet the same contract as `settings_read`.
    pub(crate) unsafe fn settings_read_for(
        &self,
        caller: &paperpaw_sdk::Id,
        key: paperpaw_native::host_glue::PpBytes,
        out: paperpaw_native::host_glue::PpBuffer,
    ) -> i32 {
        // SAFETY: the shell forwards the slot pointer contract unchanged.
        unsafe { queries::settings_read_for(&self.slot, caller, key, out) }
    }

    /// Read a typed setting from the current dispatch frame.
    /// Returns the complete required byte count, copying only the fitting prefix.
    /// Decode only a complete value; absent, false, zero and empty text are distinct.
    /// A valid call outside dispatch returns ERR_STATE.
    ///
    /// # Safety
    /// Key storage must contain key_len initialized bytes in one live allocation,
    /// readable and unchanged for the entire call. Output must be writable
    /// for capacity bytes with exclusive access; it may be uninitialized.
    /// Both regions must be live and nonoverlapping, and output must be disjoint
    /// from host state. Null with zero length/capacity is accepted. No pointer
    /// is retained. Limits and UTF-8 are validated before looking up the key.
    #[must_use]
    pub unsafe fn settings_read(
        &self,
        key: *const u8,
        key_len: usize,
        output: *mut u8,
        capacity: usize,
    ) -> i32 {
        // SAFETY: the caller provides the same pair validity and exclusion
        // guarantees required by the existing reviewed query backend.
        unsafe {
            queries::settings_read(
                &self.slot,
                paperpaw_native::host_glue::PpBytes {
                    ptr: key,
                    len: key_len,
                },
                paperpaw_native::host_glue::PpBuffer {
                    ptr: output,
                    cap: capacity,
                },
            )
        }
    }

    /// Copy the current snapshot, returning OK or a negative error status.
    /// The generated total_bytes header field carries the encoded length.
    /// This does not expose a native table type in the host API.
    ///
    /// # Safety
    /// Output must be writable for capacity bytes, with exclusive access, and
    /// disjoint from host state and borrowed inputs. It may be uninitialized.
    /// Null with zero capacity is accepted; no pointer is retained.
    #[must_use]
    pub unsafe fn copy(&self, output: *mut u8, capacity: usize) -> i32 {
        // SAFETY: identical output validity and exclusion contract.
        unsafe {
            snapshot::snapshot(
                &self.slot,
                paperpaw_native::host_glue::PpBuffer {
                    ptr: output,
                    cap: capacity,
                },
            )
        }
    }
}

impl Default for EnvironmentScope {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for EnvironmentScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EnvironmentScope").finish_non_exhaustive()
    }
}

#[cfg(test)]
#[path = "flat/event_tests.rs"]
mod event_tests;
