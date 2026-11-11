//! One call of a format plugin's `core.format` token (`docs/abi/document.md`
//! section 3), run by the document service for the opener's call.
//!
//! The service is entered from the opener's callback, so this is a nested
//! dispatch of another plugin's token through the retained instance registry:
//! the lease keeps the pinned owner and module alive, the shared depth budget
//! bounds the nesting, and a plugin that is its own format answers `ERR_BUSY`
//! (same-instance reentry). Framing follows the stage broker: one `stage.call`
//! record in, a counted batch of stage-local records out; the typed `fmt.call`
//! payload and the tag translation belong to the generated `core.format`
//! schema and are layered on these bytes.
//!
//! Peak memory of one call: the framed call (at most 4 KiB) and the reply
//! buffer (at most 3,840 bytes), both allocated fallibly before the format is
//! entered and freed on return. Nothing is retained.
use super::{
    native_instances::{InstanceState, NativeInstances},
    native_screen::Dispatch,
    registrations::{self as wire, Record},
    stage_wire,
};
use crate::native_host::codec;
use alloc::vec::Vec;
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_CANCELLED, ERR_INVALID, ERR_NO_MEMORY, ERR_NO_SPACE,
};
use paperpaw_sdk::Env;

/// The host-defined stage key and its major.
pub(crate) const STAGE: &str = "core.format";
pub(crate) const MAJOR: u32 = 1;
/// A fragment is a stage payload at most (`document.md` 4).
pub(crate) const PAYLOAD_MAX: usize = 3840;
/// The framed call: one `stage.call` record and the batch count byte.
const CALL_MAX: usize = 4096;
/// Most records of one reply (the stage maximum).
const REPLY_ROWS: u32 = 64;

/// Where a format's token lives: the plugin's index in the instance registry
/// and the token it declared in `format.spec`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Target {
    pub(crate) plugin: u64,
    pub(crate) token: u32,
}

fn scratch(capacity: usize) -> Result<Vec<u8>, i32> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(capacity)
        .map_err(|_| ERR_NO_MEMORY)?;
    bytes.resize(capacity, 0);
    Ok(bytes)
}

/// `payload` as the one `stage.call` record the format's `core.format` token
/// takes, framed as a batch.
///
/// # Errors
/// `ERR_NO_SPACE` for a payload over 3,840 bytes or a framed call over 4 KiB,
/// `ERR_NO_MEMORY`.
pub(crate) fn framed(payload: &[u8]) -> Result<Vec<u8>, i32> {
    if payload.len() > PAYLOAD_MAX {
        return Err(ERR_NO_SPACE);
    }
    let record = Record::StageCall(wire::StageCall {
        stage: STAGE,
        schema: MAJOR,
        payload,
    });
    let capacity = codec::framed_len(&record)?
        .checked_add(1)
        .filter(|length| *length <= CALL_MAX)
        .ok_or(ERR_NO_SPACE)?;
    let mut framed = scratch(capacity)?;
    if codec::encode_batch(&[record], &mut framed)? != capacity {
        return Err(ERR_INVALID);
    }
    Ok(framed)
}

/// The reply of a format call is whole frames of at most the stage's rows.
pub(crate) fn accept_reply(bytes: &[u8]) -> Result<(), i32> {
    stage_wire::validate_reply(bytes, PAYLOAD_MAX, REPLY_ROWS)
}

/// Call the format with `payload` (the encoded `fmt.call` and its options) and
/// write its counted reply batch at the front of `out`; returns its length.
///
/// # Errors
/// `ERR_NO_SPACE` for a payload over 3,840 bytes, `ERR_NO_MEMORY` when a
/// buffer cannot be had (the format is not entered), `ERR_CANCELLED` when the
/// format plugin is not running, `ERR_BUSY` when it has not started yet (the
/// start is requested; the caller asks again) or is the calling plugin
/// itself, `ERR_INVALID` for a reply that is longer than the buffer or not
/// whole frames (the format is told with `dropped`), and the format's own
/// negative status otherwise. `out` is written only on success.
pub(crate) fn call(
    registry: &NativeInstances,
    target: Target,
    payload: &[u8],
    out: &mut [u8],
    env: &Env<'_>,
) -> Result<usize, i32> {
    let framed = framed(payload)?;
    let mut reply = scratch(out.len().min(PAYLOAD_MAX))?;
    // The retained epoch keeps start requests from flushing under this call.
    let _epoch = registry.callback()?;
    let lease = match registry.state(target.plugin)? {
        InstanceState::Running(lease) => lease,
        InstanceState::Lazy => {
            registry.request_start(target.plugin)?;
            return Err(ERR_BUSY);
        }
        InstanceState::Stopped => return Err(ERR_CANCELLED),
    };
    let result = lease.stage(target.token, &framed, &mut reply, env);
    // A negative result is the format's own status and passes through; it is
    // not a misbehaviour. Only a reply the host cannot take is dropped.
    let length = result?;
    let Ok(length) = usize::try_from(length) else {
        return Err(length);
    };
    let valid = reply
        .get(..length)
        .ok_or(ERR_INVALID)
        .and_then(|bytes| stage_wire::validate_reply(bytes, PAYLOAD_MAX, REPLY_ROWS));
    match valid {
        Ok(()) => {
            out[..length].copy_from_slice(&reply[..length]);
            Ok(length)
        }
        Err(status) => {
            lease.dropped(target.token, status);
            Err(status)
        }
    }
}

#[cfg(test)]
#[path = "format_call_tests.rs"]
pub(crate) mod tests;
