//! Slot 32 validates raw buffers and delegates through the callback's frame.
use super::Context;
use crate::abi7::context::Frame;
use core::ffi::c_void;
use paperpaw_native::host_glue::{ERR_INVALID, PpBuffer, PpBytes, buffers};

/// # Safety
/// The context and all nonempty pairs are live and disjoint for this call.
pub(super) unsafe extern "C" fn run<S: 'static>(
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
    // The plugin waits for the host (and the broker's subscribers, which are
    // plugin calls of their own) from here to the return.
    let _slot = crate::native_host::host_slot::enter();
    context
        .environment
        .with_frame(|frame| {
            // SAFETY: the generated slot contract supplies live, disjoint buffers.
            unsafe { in_frame(frame, context.plugin_id(), stage, cursor, input, out) }
        })
        .and_then(core::convert::identity)
        .map_or_else(|error| error, |n| n as i32)
}

/// # Safety
/// Nonempty pairs are live for their declared access; overlaps are refused
/// before any reference is formed.
unsafe fn in_frame(
    frame: &dyn Frame,
    caller: &str,
    stage: PpBytes,
    cursor: u32,
    input: PpBytes,
    out: PpBuffer,
) -> Result<usize, i32> {
    let pairs = [
        (stage.ptr.addr(), stage.len),
        (input.ptr.addr(), input.len),
        (out.ptr.addr(), out.cap),
    ];
    for (pointer, length) in pairs {
        if length != 0 && (pointer == 0 || pointer.checked_add(length).is_none()) {
            return Err(ERR_INVALID);
        }
    }
    for (index, &(start, length)) in pairs.iter().enumerate() {
        for &(other, other_length) in &pairs[index + 1..] {
            if length != 0
                && other_length != 0
                && start < other + other_length
                && other < start + length
            {
                return Err(ERR_INVALID);
            }
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
    // The broker validates before publishing, initializes only the returned
    // prefix and leaves output untouched on error. No temporary buffer is needed.
    let count = frame.stage_run(caller, stage, cursor, input, output)?;
    debug_assert!(
        count <= output.len(),
        "broker returned a count beyond output capacity"
    );
    if count > output.len() {
        return Err(ERR_INVALID);
    }
    Ok(count)
}

#[cfg(test)]
#[path = "shell_stage_slot_tests.rs"]
mod tests;
