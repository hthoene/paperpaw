//! Copy-out query backends for a live scoped frame.
//!
//! Pointer validation precedes lookups. Results are copied while the frame
//! and its catalog borrow remain live. These adapters add no temporary String;
//! existing Settings placement resolution and catalog reads may allocate.

use super::{
    context::FrameSlot,
    settings_wire::{SettingValueRef, settings_encode},
};
use paperpaw_native::host_glue::{
    ERR_NO_SPACE, ERR_NOT_FOUND, ERR_STATE, ERR_UNSUPPORTED, PpBuffer, PpBytes, buffers,
};
use paperpaw_sdk::SettingValue;

/// # Safety
/// Input and output meet `buffers::text_in` and `buffers::buffer_out` contracts:
/// live for the call, nonoverlapping, and output disjoint from host state.
/// The output may be uninitialized. Null with zero length/capacity is allowed.
pub(crate) unsafe fn settings_read(slot: &FrameSlot, key: PpBytes, out: PpBuffer) -> i32 {
    // SAFETY: the caller supplies valid, nonoverlapping pairs.
    let key = match unsafe { buffers::text_in(key, buffers::TEXT_MAX) } {
        Ok(key) => key,
        Err(error) => return error,
    };
    // SAFETY: the output validity and exclusive access are caller obligations.
    let out = match unsafe { buffers::buffer_out(out, buffers::TEXT_MAX) } {
        Ok(out) => out,
        Err(error) => return error,
    };
    slot.with(|frame| {
        let value = match frame.settings().get(key) {
            None => SettingValueRef::Absent,
            Some(SettingValue::Bool(value)) => SettingValueRef::Bool(*value),
            Some(SettingValue::Int(value)) => SettingValueRef::Int(*value),
            Some(SettingValue::Text(value)) => SettingValueRef::Text(value),
            Some(_) => return ERR_UNSUPPORTED,
        };
        match settings_encode(value, out) {
            Ok(needed) => i32::try_from(needed).unwrap_or(ERR_NO_SPACE),
            Err(error) => error,
        }
    })
    .unwrap_or(ERR_STATE)
}

/// # Safety
/// Same pair-validity and no-alias requirements as `settings_read`.
pub(crate) unsafe fn i18n_copy(slot: &FrameSlot, key: PpBytes, out: PpBuffer) -> i32 {
    // SAFETY: the caller supplies valid, nonoverlapping pairs.
    let key = match unsafe { buffers::text_in(key, buffers::TEXT_MAX) } {
        Ok(key) => key,
        Err(error) => return error,
    };
    // SAFETY: output validity and exclusive access are caller obligations.
    let out = match unsafe { buffers::buffer_out(out, buffers::TEXT_MAX) } {
        Ok(out) => out,
        Err(error) => return error,
    };
    slot.with(|frame| {
        let Some(message) = frame.translator().get(key) else {
            return ERR_NOT_FOUND;
        };
        buffers::copy_required(message.as_bytes(), out, buffers::TEXT_MAX)
    })
    .unwrap_or(ERR_STATE)
}
