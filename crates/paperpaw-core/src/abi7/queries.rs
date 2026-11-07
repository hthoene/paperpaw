//! Copy-out query backends for a live scoped frame.
//!
//! Pointer validation precedes lookups. Results are copied while the frame
//! and its catalog borrow remain live. These adapters add no temporary String;
//! existing Settings placement resolution and catalog reads may allocate.

use super::settings_visibility::{Visibility, visibility};
use super::{
    context::FrameSlot,
    settings_wire::{SettingValueRef, settings_encode},
};
use paperpaw_native::host_glue::{
    ERR_NO_SPACE, ERR_NOT_FOUND, ERR_STATE, ERR_UNSUPPORTED, PpBuffer, PpBytes, buffers,
};
use paperpaw_sdk::{Id, SettingValue};

/// Generic slot 5 uses this host-bound caller path. Output is published only
/// after framing, identity and visibility checks succeed.
///
/// # Safety
/// The same pointer and alias rules as `settings_read` apply.
pub(crate) unsafe fn settings_read_for(
    slot: &FrameSlot,
    caller: &Id,
    key: PpBytes,
    out: PpBuffer,
) -> i32 {
    // SAFETY: the slot caller provides live nonoverlapping storage.
    let key = match unsafe { buffers::text_in(key, buffers::TEXT_MAX) } {
        Ok(key) => key,
        Err(error) => return error,
    };
    // SAFETY: the slot caller provides exclusive writable storage.
    let out = match unsafe { buffers::buffer_out(out, buffers::TEXT_MAX) } {
        Ok(out) => out,
        Err(error) => return error,
    };
    slot.with(|frame| read_for(frame, caller, key, out))
        .unwrap_or(ERR_STATE)
}

fn read_for(
    frame: &dyn super::context::Frame,
    caller: &Id,
    key: &str,
    out: &mut [core::mem::MaybeUninit<u8>],
) -> i32 {
    use paperpaw_native::host_glue::{ERR_DENIED, ERR_INVALID};
    // Only the short identity route may name a grant without editor authority.
    if let Some(kind) = key.strip_prefix("core.grant.")
        && !kind.contains('.')
    {
        if !matches!(kind, "style" | "install") {
            return ERR_UNSUPPORTED;
        }
        let length = key
            .len()
            .saturating_add(1)
            .saturating_add(caller.as_str().len());
        let mut full = [0; 128];
        if length > full.len() {
            return ERR_INVALID;
        }
        full[..key.len()].copy_from_slice(key.as_bytes());
        full[key.len()] = b'.';
        full[key.len() + 1..length].copy_from_slice(caller.as_str().as_bytes());
        let Ok(full) = core::str::from_utf8(&full[..length]) else {
            return ERR_INVALID;
        };
        let granted = matches!(frame.settings().get(full), Some(SettingValue::Bool(true)));
        return encode(SettingValueRef::Bool(granted), out);
    }
    let secret = frame
        .settings()
        .spec(key)
        .is_some_and(|spec| spec.extra().secret);
    match visibility(caller, key, secret, frame.settings_editor()) {
        Visibility::Hidden => ERR_DENIED,
        Visibility::Redacted if key.split('.').next() != Some(caller.as_str()) => ERR_DENIED,
        Visibility::Redacted | Visibility::Readable => copy_value(frame, key, out),
    }
}

fn encode(value: SettingValueRef<'_>, out: &mut [core::mem::MaybeUninit<u8>]) -> i32 {
    match settings_encode(value, out) {
        Ok(needed) => i32::try_from(needed).unwrap_or(ERR_NO_SPACE),
        Err(error) => error,
    }
}

fn copy_value(
    frame: &dyn super::context::Frame,
    key: &str,
    out: &mut [core::mem::MaybeUninit<u8>],
) -> i32 {
    let value = match frame.settings().get(key) {
        None => SettingValueRef::Absent,
        Some(SettingValue::Bool(value)) => SettingValueRef::Bool(*value),
        Some(SettingValue::Int(value)) => SettingValueRef::Int(*value),
        Some(SettingValue::Text(value)) => SettingValueRef::Text(value),
        Some(_) => return ERR_UNSUPPORTED,
    };
    encode(value, out)
}

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
    slot.with(|frame| copy_value(frame, key, out))
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
