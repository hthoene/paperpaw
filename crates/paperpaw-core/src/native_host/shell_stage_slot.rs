//! Slot 32 validates raw buffers and delegates through the callback's frame.
use super::Context;
use crate::abi7::context::Frame;
use alloc::vec::Vec;
use core::ffi::c_void;
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_MEMORY, PpBuffer, PpBytes, buffers};

/// # Safety
/// The context and all nonempty pairs are live and disjoint for this call.
pub(super) unsafe extern "C" fn run<S>(
    ctx: *mut c_void,
    stage: PpBytes,
    cursor: u32,
    input: PpBytes,
    out: PpBuffer,
) -> i32 {
    // SAFETY: a nonnull context is the pinned shell Context supplied to make.
    let Some(context) = (unsafe { ctx.cast::<Context<S>>().as_ref() }) else {
        return ERR_INVALID;
    };
    context
        .environment
        .with_frame(|frame| {
            // SAFETY: the generated slot contract supplies live, disjoint buffers.
            unsafe { in_frame(frame, context.id.as_ref(), stage, cursor, input, out) }
        })
        .and_then(core::convert::identity)
        .map_or_else(|error| error, |n| n as i32)
}

/// # Safety
/// Every nonempty input/output pair is live, bounded and disjoint.
unsafe fn in_frame(
    frame: &dyn Frame,
    caller: &str,
    stage: PpBytes,
    cursor: u32,
    input: PpBytes,
    out: PpBuffer,
) -> Result<usize, i32> {
    for (pointer, length) in [
        (stage.ptr as usize, stage.len),
        (input.ptr as usize, input.len),
        (out.ptr as usize, out.cap),
    ] {
        if length != 0 && (pointer == 0 || pointer.checked_add(length).is_none()) {
            return Err(ERR_INVALID);
        }
    }
    // SAFETY: numeric checks above precede bounded slice creation; the caller
    // supplies readable input and exclusive, possibly uninitialized output.
    let (stage, input, output) = unsafe {
        (
            buffers::text_in(stage, 128)?,
            buffers::bytes_in(input, buffers::TEXT_MAX)?,
            buffers::buffer_out(out, buffers::TEXT_MAX)?,
        )
    };
    super::super::declarations::full_id(stage)?;
    // Raw caller output may be uninitialized. The frame gets initialized scratch,
    // and only a successful bounded prefix is copied back to the caller.
    let mut scratch = Vec::new();
    scratch
        .try_reserve_exact(output.len())
        .map_err(|_| ERR_NO_MEMORY)?;
    scratch.resize(output.len(), 0);
    let count = frame.stage_run(caller, stage, cursor, input, &mut scratch)?;
    let reply = scratch.get(..count).ok_or(ERR_INVALID)?;
    for (slot, byte) in output.iter_mut().zip(reply) {
        slot.write(*byte);
    }
    Ok(count)
}

#[cfg(test)]
#[path = "shell_stage_slot_tests.rs"]
mod tests;
