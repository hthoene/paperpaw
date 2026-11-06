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
    registrations_encode, tick_decode, tick_encode,
};

use super::{Error, registration_scope::RegistrationScope};
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_INVALID, ERR_NO_SPACE, ERR_STATE, ERR_UNSUPPORTED, OK, PpBytes, buffers,
};

/// Validate an entire borrowed batch, then stage owned scalar registrations.
/// All records remain private until the loader commits validated `make` output.
/// Every decode, unsupported-event or staging refusal poisons that transaction;
/// the loader must abort it even when the foreign caller ignores this status.
/// No allocation occurs here and no input pointer is retained.
///
/// # Safety
/// When non-null with nonzero length, `bytes` must designate initialized storage
/// readable for `len` bytes, stable and unmodified until this call returns.
/// The caller must use its own pinned context's scope, on the dispatch thread.
/// Host state cannot be mutated through an alias to the input during this call.
#[must_use]
pub unsafe fn submit(scope: &RegistrationScope<Registration>, bytes: *const u8, len: usize) -> i32 {
    if !scope.is_active() {
        return ERR_STATE;
    }
    let result = if len > REGISTRATION_MAX_BYTES {
        Err(ERR_NO_SPACE)
    } else {
        // SAFETY: caller guarantees readable, stable storage; the helper also
        // checks null and size bounds before constructing any nonempty slice.
        unsafe { buffers::bytes_in(PpBytes { ptr: bytes, len }, REGISTRATION_MAX_BYTES) }
            .and_then(wire::registrations_decode)
    };
    let batch = match result {
        Ok(batch) => batch,
        Err(error) => return refuse(scope, error),
    };
    // Validate support for the whole batch before staging even its first item.
    for record in batch.iter() {
        match record {
            Registration::Event {
                event_tag: EVENT_TICK_TAG,
                ..
            }
            | Registration::Draw { .. } => {}
            _ => return refuse(scope, ERR_UNSUPPORTED),
        }
    }
    for record in batch.iter() {
        if let Err((error, _record)) = scope.submit(record) {
            return refuse(scope, status(error));
        }
    }
    OK
}

fn refuse(scope: &RegistrationScope<Registration>, error: i32) -> i32 {
    // A missing scope cannot be poisoned and is a lifecycle error regardless
    // of the bytes supplied. Active scopes latch before the status is returned.
    match scope.reject() {
        Ok(()) => error,
        Err(error) => status(error),
    }
}

fn status(error: Error) -> i32 {
    match error {
        Error::State => ERR_STATE,
        Error::Busy | Error::Depth => ERR_BUSY,
        Error::Capacity => ERR_NO_SPACE,
        Error::Invalid => ERR_INVALID,
    }
}
