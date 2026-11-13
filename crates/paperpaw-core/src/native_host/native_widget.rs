//! One `core.widget` stage call to a widget's owner (`docs/abi/ui.md` 5.2,
//! behind [`Frame::widget_call`]): ops 7 `widget_targets`, 12 `widget_measure`
//! and 13 `widget_place` each make exactly one.
//!
//! The call goes to the owner's own instance and token only, as the `core.status`
//! call does (`native_status.rs`), through the declaration directory's lease
//! held for the call: the owner's stage dispatch enters it on the usage meter
//! and enforces the depth budget. The checks come in this order and a refusal
//! leaves everything untouched:
//!
//! 1. a callback is running (`ERR_STATE`);
//! 2. the X4 answers `ERR_UNSUPPORTED` until its stack gate passes;
//! 3. the caller is at depth 1 (the owner runs at depth 2), else `ERR_BUSY`;
//! 4. the owner is not the caller (`ERR_BUSY`, same-instance re-entry);
//! 5. the directory knows the owner and its instance is live and itself at
//!    depth 1 (`ERR_NOT_FOUND`, `ERR_BUSY`).
//!
//! No declaration borrow crosses the owner's code. Memory: the input is the
//! caller's encoded record inside one fallible heap call (about 100 bytes)
//! and the reply goes straight into the caller's buffer (the op's one
//! fallible 3,840-byte buffer), so the call adds no second one; a refused
//! allocation is `ERR_NO_MEMORY` with nothing called. A reply the
//! stage's codec refuses (a wrong tag, more than 32 rows, too long) is dropped
//! to the owner and is this call's failure.
use super::{
    codec, native_screen::Dispatch, native_status::scratch, registrations as wire,
    stages_wire::core_widget_v1 as widget,
};
use crate::abi7::{context::Frame, core_frame::CoreFrame};
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_INVALID, ERR_NO_SPACE, ERR_NOT_FOUND, ERR_UNSUPPORTED,
};

/// Runs `token` of `owner` with `call`; the checked reply is the first bytes
/// of `out`.
pub(crate) fn call(
    frame: &CoreFrame<'_>,
    owner: u64,
    token: u32,
    payload: &[u8],
    out: &mut [u8],
) -> Result<usize, i32> {
    frame.callback_epoch()?;
    if super::ui_store::nested_closed(frame.env.device().memory) {
        return Err(ERR_UNSUPPORTED);
    }
    if frame.callback_depth()? != 1 || frame.caller()? == owner {
        return Err(ERR_BUSY);
    }
    let directory = frame.directory.ok_or(ERR_UNSUPPORTED)?;
    let lease = directory
        .store
        .try_borrow()
        .map_err(|_| ERR_BUSY)?
        .native_lease(owner)
        .map_err(|error| {
            if error == ERR_BUSY {
                error
            } else {
                ERR_NOT_FOUND
            }
        })?;
    if lease.checked_depth()? != 1 {
        return Err(ERR_BUSY);
    }
    let input = input(payload)?;
    // The owner hears of the instance before it is asked about it (ui.md 5.2).
    if let (Some(ui), Some(instance)) = (frame.ui, instance_of(payload)) {
        super::widget_drops::tell_made(&ui.widgets.handles, &lease, instance, frame.env);
    }
    // The owner writes straight into the caller's buffer (the one 3,840-byte
    // buffer of the op); the reply is checked where it lies.
    let result = lease
        .stage(token, &input, out, frame.env)
        .and_then(|length| {
            let length = usize::try_from(length).map_err(|_| length)?;
            let bytes = out.get(..length).ok_or(ERR_INVALID)?;
            widget::reply_decode(bytes)?;
            Ok(length)
        });
    if let Err(error) = result {
        lease.dropped(token, error);
    }
    result
}

/// The instance a `core.widget` call is about.
fn instance_of(payload: &[u8]) -> Option<u32> {
    match widget::input_decode(payload).ok()?.next()?.ok()? {
        widget::Record::WidgetMeasure(call) => Some(call.instance),
        widget::Record::WidgetPlace(call) => Some(call.instance),
        widget::Record::WidgetTargets(call) => Some(call.instance),
        _ => None,
    }
}

/// The call as the stage's framed input: a `stage.call` naming `core.widget`
/// around the one stage-local record `payload` (checked as a stage input).
fn input(payload: &[u8]) -> Result<alloc::vec::Vec<u8>, i32> {
    widget::input_decode(payload)?;
    let record = wire::Record::StageCall(wire::StageCall {
        stage: widget::KEY,
        schema: u32::from(widget::MAJOR),
        payload,
    });
    let mut framed = scratch(
        codec::framed_len(&record)?
            .checked_add(1)
            .ok_or(ERR_NO_SPACE)?,
    )?;
    codec::encode_batch(&[record], &mut framed)?;
    Ok(framed)
}

#[cfg(test)]
#[path = "native_widget_tests.rs"]
mod tests;
