//! Scoped immutable service snapshots. Only successful queries publish a handle.
use super::{
    data_services::{self, Query},
    declared_services::DeclaredServices,
    environment::EnvironmentScope,
    resources::{Blob, OwnerResources},
};
use alloc::vec::Vec;
use paperpaw_native::host_glue::{
    ERR_INVALID, ERR_NO_MEMORY, ERR_UNSUPPORTED, OK, PpBuffer, PpBytes, buffers,
};

/// # Safety
/// Input and output are live, aligned, disjoint storage for this call.
#[allow(
    clippy::too_many_arguments,
    reason = "mirrors the scalar service slot and host state"
)]
pub(super) unsafe fn query(
    scope: &EnvironmentScope,
    owner: &OwnerResources,
    declared: &DeclaredServices,
    key: PpBytes,
    major: u16,
    minor: u16,
    out: *mut u32,
) -> i32 {
    let result = scope
        .with_frame(|frame| {
            if out.is_null() || !(out as usize).is_multiple_of(core::mem::align_of::<u32>()) {
                return Err(ERR_INVALID);
            }
            // SAFETY: the slot caller supplies readable key storage.
            let key = unsafe { buffers::text_in(key, 128) }?;
            let query = Query::parse(key)?;
            let (declared_major, revision) =
                declared.declared(query.base()).ok_or(ERR_UNSUPPORTED)?;
            if major != 1 || minor != 0 || major != declared_major || minor > revision {
                return Err(ERR_UNSUPPORTED);
            }
            // Lookup failures precede budget refusal without allocating a page.
            match query {
                Query::Plugin(id) => frame.plugin_name(id, &mut |_| {})?,
                Query::WorkNote(id) => frame.work_note(id, &mut |_, _| {})?,
                Query::Usage | Query::PluginUsage(_) => {}
            }
            let page = owner.blob_page_bytes();
            owner.check_blob_capacity(page)?;
            let mut bytes = Vec::new();
            bytes.try_reserve_exact(page).map_err(|_| ERR_NO_MEMORY)?;
            bytes.resize(page, 0);
            let length = data_services::encode(frame, query, &mut bytes)?;
            bytes.truncate(length);
            let handle = owner.insert_blob(Blob {
                bytes,
                callback: scope.callback_id(),
            })?;
            // SAFETY: validated scalar output; only success publishes the handle.
            unsafe {
                out.write(handle);
            }
            Ok(())
        })
        .and_then(core::convert::identity);
    result.map_or_else(|error| error, |()| OK)
}

/// # Safety
/// Output is live, exclusive storage, disjoint from host state.
pub(super) unsafe fn read(
    scope: &EnvironmentScope,
    owner: &OwnerResources,
    handle: u32,
    offset: u64,
    out: PpBuffer,
) -> i32 {
    scope
        .with_frame(|_| {
            // SAFETY: slot caller supplies exclusive writable storage.
            let out = unsafe { buffers::buffer_out(out, buffers::TEXT_MAX) }?;
            owner.with_blob(handle, scope.callback_id(), |bytes| {
                let offset = usize::try_from(offset).unwrap_or(usize::MAX);
                Ok(buffers::copy_count(
                    bytes.get(offset..).unwrap_or_default(),
                    out,
                ))
            })
        })
        .and_then(core::convert::identity)
        .unwrap_or_else(|error| error)
}

pub(super) fn release(scope: &EnvironmentScope, owner: &OwnerResources, handle: u32) -> i32 {
    scope
        .with_frame(|_| owner.release_blob(handle, scope.callback_id()))
        .and_then(core::convert::identity)
        .map_or_else(|error| error, |()| OK)
}

#[cfg(test)]
mod tests;
