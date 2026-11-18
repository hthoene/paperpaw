//! One directed stage call to another plugin's native instance, behind
//! [`Frame::call_native`] (`docs/abi/document.md` 3 and 7, ADR 0034).
//!
//! This is the one way the document and lookup services run plugin code: the
//! call goes through the owner's own instance (its stage dispatch pushes the
//! owner's frame and enforces the depth budget) from the declaration
//! directory's lease, held for the call only. The checks come in this order,
//! and a refusal leaves everything untouched:
//!
//! 1. a callback is running (`ERR_STATE`);
//! 2. the caller is at depth 1, so the callee runs at depth 2; a deeper
//!    caller is `ERR_BUSY` (no third level);
//! 3. the callee is not the caller itself (`ERR_BUSY`, same-instance
//!    re-entry);
//! 4. the directory knows the owner (`ERR_NOT_FOUND`), its instance is live
//!    and itself at depth 1 (`ERR_BUSY`).
//!
//! No declaration borrow crosses the call. The reply buffer is the caller's;
//! an answer the caller's `accept` refuses, or one longer than the buffer, is
//! reported to the callee as dropped; its own negative status is not.
use super::native_screen::Dispatch;
use crate::abi7::{context::Frame, core_frame::CoreFrame};
use paperpaw_native::host_glue::{ERR_BUSY, ERR_INVALID, ERR_NOT_FOUND, ERR_UNSUPPORTED};

/// Runs `token` of `owner` with `input`; the answer is the first bytes of
/// `reply`, accepted by `accept`.
pub(crate) fn call(
    frame: &CoreFrame<'_>,
    owner: u64,
    token: u32,
    input: &[u8],
    reply: &mut [u8],
    accept: &mut dyn FnMut(&[u8]) -> Result<(), i32>,
) -> Result<usize, i32> {
    frame.callback_epoch()?;
    if frame.callback_depth()? != 1 {
        return Err(ERR_BUSY);
    }
    if frame.caller()? == owner {
        return Err(ERR_BUSY);
    }
    let directory = frame.directory.ok_or(ERR_UNSUPPORTED)?;
    // The borrow ends with the statement: the callee may declare or stop.
    let lease = directory
        .store
        .try_borrow()
        .map_err(|_| ERR_BUSY)?
        .native_lease(owner)?;
    if lease.is_parked() {
        // A parked callee (#248, D2: a format) is not made inside a
        // callback: it is asked to start at the next safe point, and what
        // called it waits (a pending open is sent once it is made).
        let _ = lease.live();
        return Err(ERR_BUSY);
    }
    if lease.checked_depth()? != 1 {
        return Err(ERR_BUSY);
    }
    // The callee's own negative status passes through: it is not a
    // misbehaviour. Only an answer the host cannot take is dropped.
    // The callee's run is metered to the callee: the caller's time stops
    // while it runs, and what it uses is its own.
    let store = directory.store.try_borrow().map_err(|_| ERR_BUSY)?;
    let running = store
        .plugin_id_of_owner(owner)
        .map(|id| frame.meter.enter_scoped(id))
        .ok_or(ERR_NOT_FOUND)?;
    drop(store);
    let length = lease.stage(token, input, reply, frame.env);
    drop(running);
    let length = length?;
    let length = usize::try_from(length).map_err(|_| length)?;
    let taken = reply.get(..length).ok_or(ERR_INVALID).and_then(accept);
    match taken {
        Ok(()) => Ok(length),
        Err(status) => {
            lease.dropped(token, status);
            Err(status)
        }
    }
}

#[cfg(test)]
#[path = "native_call_tests.rs"]
mod tests;
