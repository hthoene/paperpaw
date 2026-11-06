//! A stable host context owns this scope; each event supplies its borrowed Env.
//! No environment or device pointer crosses the plugin boundary.

use super::Error;
pub use crate::abi7::settings_wire::{SettingValueRef, settings_decode};
pub use crate::abi7::snapshot_wire::{
    Battery, PixelFormat, SNAPSHOT_BYTES, SNAPSHOT_MAX_BYTES, SNAPSHOT_VERSION, Snapshot,
    snapshot_decode,
};
use crate::abi7::{
    context::{Error as FrameError, FrameSlot},
    queries, snapshot,
};
use core::{fmt, pin::Pin};
use paperpaw_sdk::Env;

/// Scoped environment access inside a pinned per-plugin host context.
#[non_exhaustive]
pub struct EnvironmentScope {
    slot: FrameSlot,
}

impl EnvironmentScope {
    /// Create an empty scope before pinning the containing host context.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            slot: FrameSlot::new(),
        }
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
        slot.scope(env, call).map_err(|error| match error {
            FrameError::Busy => Error::Busy,
            FrameError::NoFrame => Error::State,
        })
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
