//! Registration wire bridge for a loader's explicitly typed context.
//!
//! The loader installs its construction scope only around `make`. This module
//! never casts a foreign context, retains input bytes, or invokes plugin code.
//! A refused submission poisons construction even if the plugin ignores it.

#[path = "../abi7/records_wire.rs"]
#[allow(
    clippy::all,
    clippy::pedantic,
    missing_docs,
    reason = "schema-generated bounded wire codec"
)]
mod wire;

pub use wire::{
    EVENT_TICK_TAG, REGISTRATION_MAX_BYTES, REGISTRATION_MAX_RECORDS, Registration,
    RegistrationBatch as DecodedRegistrations, registrations_decode, registrations_encode,
    tick_decode, tick_encode,
};
// Mixed registration and declaration batches: borrowed records with capped
// strings. `registrations_decode` still admits callback registrations only.
pub use wire::{
    Record, RecordBatch, RecordIter, ResourceFontExtension, ResourceFontFace, ResourceHyphenation,
    SettingExtra, SettingGroup, SettingSpec, records_decode, records_encode,
};

use super::{Error, registration_scope::RegistrationScope};
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_INVALID, ERR_NO_SPACE, ERR_STATE, ERR_UNSUPPORTED, OK, PpBytes, buffers,
};

/// Decode and validate an entire borrowed batch, then stage it: callback
/// registrations and declaration records (tags 5-10) together, all or nothing.
/// All records remain private until the loader commits validated `make` output.
/// Every decode, validation, unsupported-event or staging refusal poisons that
/// transaction; the loader must abort it even when the foreign caller ignores
/// this status. The plugin id for name checks is the one given to `begin`.
/// Only the bounded staging copies are allocated and no input pointer is kept.
///
/// # Safety
/// When non-null with nonzero length, `bytes` must designate initialized storage
/// readable for `len` bytes, stable and unmodified until this call returns.
/// The caller must use its own pinned context's scope, on the dispatch thread.
/// Host state cannot be mutated through an alias to the input during this call.
#[must_use]
pub unsafe fn submit(scope: &RegistrationScope<Registration>, bytes: *const u8, len: usize) -> i32 {
    if let Err(error) = scope.admission() {
        return refuse(scope, status(error));
    }
    let result = if len > REGISTRATION_MAX_BYTES {
        Err(ERR_NO_SPACE)
    } else {
        // SAFETY: caller guarantees readable, stable storage; the helper also
        // checks null and size bounds before constructing any nonempty slice.
        unsafe { buffers::bytes_in(PpBytes { ptr: bytes, len }, REGISTRATION_MAX_BYTES) }
            .and_then(wire::records_decode)
    };
    let batch = match result {
        Ok(batch) => batch,
        Err(error) => return refuse(scope, error),
    };
    // Only the tick event is supported; refuse before staging anything.
    let supported = batch.iter().all(|record| match record {
        Record::Registration(Registration::Event { event_tag, .. }) => event_tag == EVENT_TICK_TAG,
        _ => true,
    });
    if !supported {
        return refuse(scope, ERR_UNSUPPORTED);
    }
    match scope.submit_batch(&batch, |registration| registration) {
        Ok(()) => OK,
        // The construction latched itself; this also covers an overlapping call.
        Err(error) => refuse(scope, error),
    }
}

fn refuse(scope: &RegistrationScope<Registration>, error: i32) -> i32 {
    // A missing scope cannot be poisoned and is a lifecycle error regardless
    // of the bytes supplied. Active scopes latch before the status is returned.
    match scope.reject() {
        Ok(()) => error,
        Err(error) => status(error),
    }
}

pub(super) fn status(error: Error) -> i32 {
    match error {
        Error::State => ERR_STATE,
        Error::Busy | Error::Depth => ERR_BUSY,
        Error::Capacity => ERR_NO_SPACE,
        Error::Invalid => ERR_INVALID,
    }
}
