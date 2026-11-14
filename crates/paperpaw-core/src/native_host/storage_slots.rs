//! Raw storage slots over the current callback's metered storage view.
//! Inputs are checked before any storage effect. A callback borrows its folder
//! from the stack, so constructing an adapter never allocates.

use super::{
    admitted_features::AdmittedFeatures,
    environment::EnvironmentScope,
    files_service::FilesHost,
    format_scope::FormatScope,
    linked_plugins::LinkedPlugins,
    paths_in_use::PathsInUse,
    storage::{
        self, Card, MAX_DATA, MAX_FOLDER, MAX_PATH, PREFIX, ROOT_BUNDLE, ROOT_CARD, ROOT_PLUGIN,
        StorageAdapter,
    },
};
use core::mem::MaybeUninit;
use paperpaw_native::host_glue::{
    ERR_DENIED, ERR_INVALID, ERR_NO_SPACE, ERR_STATE, ERR_UNSUPPORTED, OK, PpBuffer, PpBytes,
    buffers,
};

/// What a plugin's slot call reaches beyond root 0. The format calls under
/// way and the host owner id of the plugin whose slot it is let roots 3, 4
/// and 6 name the document the plugin's call is for (a plugin outside such a
/// call has none of these roots). Its admitted features open root 1, the
/// user's files, with `uses.storage` (decision 125, `files.md` 1.1), and
/// `files`, `core.files`'s host where it is wired, says which of them the host
/// holds busy. `linked` tells whether the plugin is built into the firmware:
/// then root 2, its install folder, is empty.
#[derive(Clone, Copy)]
pub(crate) struct Format<'a> {
    pub(crate) scope: &'a FormatScope,
    pub(crate) owner: u64,
    pub(crate) features: AdmittedFeatures,
    pub(crate) files: Option<&'a dyn FilesHost>,
    pub(crate) linked: &'a LinkedPlugins,
}

fn with_adapter<R>(
    scope: &EnvironmentScope,
    owner: &str,
    in_use: Option<&PathsInUse>,
    format: Option<(Format<'_>, u32)>,
    call: impl FnOnce(&StorageAdapter<'_>) -> Result<R, i32>,
) -> Result<R, i32> {
    let length = PREFIX
        .len()
        .checked_add(owner.len())
        .and_then(|n| n.checked_add(1))
        .ok_or(ERR_INVALID)?;
    let mut folder = [0; MAX_FOLDER];
    let folder = folder.get_mut(..length).ok_or(ERR_INVALID)?;
    folder[..PREFIX.len()].copy_from_slice(PREFIX.as_bytes());
    folder[PREFIX.len()..length - 1].copy_from_slice(owner.as_bytes());
    folder[length - 1] = b'/';
    let folder = core::str::from_utf8(folder).map_err(|_| ERR_INVALID)?;
    scope.with_frame(|frame| {
        let storage = frame.storage().ok_or(ERR_STATE)?;
        // Roots 3, 4 and 6 are the call's document, if the plugin is in one.
        let Some((format, root)) = format.filter(|(_, root)| *root != ROOT_PLUGIN) else {
            let adapter =
                StorageAdapter::borrowed_folder(storage, folder, in_use).ok_or(ERR_INVALID)?;
            return call(&adapter);
        };
        // Root 1 is the card's top ([`path`] checked `uses.storage`).
        if root == ROOT_CARD {
            let adapter = StorageAdapter::borrowed_folder(storage, folder, in_use)
                .ok_or(ERR_INVALID)?
                .with_card(Card {
                    files: format.files,
                    formats: frame.kernel_env().map(paperpaw_sdk::Env::formats),
                });
            return call(&adapter);
        }
        // Root 2 is the plugin's install folder, read only.
        if root == ROOT_BUNDLE {
            let adapter = StorageAdapter::borrowed_folder(storage, folder, in_use)
                .ok_or(ERR_INVALID)?
                .with_bundle(format.linked.contains(owner));
            return call(&adapter);
        }
        let mut call = Some(call);
        let mut result = Err(ERR_UNSUPPORTED);
        format.scope.current(format.owner, &mut |book| {
            if let Some(call) = call.take() {
                result = match StorageAdapter::borrowed_folder(storage, folder, in_use) {
                    Some(adapter) => call(&adapter.with_format(book)),
                    None => Err(ERR_INVALID),
                };
            }
        });
        result
    })?
}

/// Check bounds before forming raw slices. Oversized storage inputs retain
/// StorageAdapter's ERR_NO_SPACE rule; malformed pointer pairs are ERR_INVALID.
fn pair(ptr: *const u8, len: usize, max: usize) -> Result<(), i32> {
    if len != 0
        && (ptr.is_null() || len > isize::MAX as usize || (ptr as usize).checked_add(len).is_none())
    {
        return Err(ERR_INVALID);
    }
    if len > max {
        return Err(ERR_NO_SPACE);
    }
    Ok(())
}

/// Whether a nonempty output range shares a byte with `path` (both already
/// bounded by [`pair`]).
fn overlaps(path: &[u8], out: *const u8, cap: usize) -> bool {
    let (start, end) = (path.as_ptr() as usize, path.as_ptr() as usize + path.len());
    let at = out as usize;
    cap != 0 && !path.is_empty() && at < end && start < at + cap
}

unsafe fn input<'a>(bytes: PpBytes, max: usize) -> Result<&'a [u8], i32> {
    pair(bytes.ptr, bytes.len, max)?;
    // SAFETY: pair bounds were checked and the caller guarantees live input.
    unsafe { buffers::bytes_in(bytes, max) }
}

/// The root first (`ERR_UNSUPPORTED` for one the slots do not serve; root 1
/// `ERR_DENIED` without `uses.storage`, `ERR_UNSUPPORTED` where the host
/// offers it not; root 2 `ERR_DENIED` for a `change`), then the path's size,
/// UTF-8 and grammar (`files.md` 3).
unsafe fn path<'a>(
    root: u32,
    bytes: PpBytes,
    format: Option<Format<'_>>,
    change: bool,
) -> Result<&'a [u8], i32> {
    match root {
        ROOT_PLUGIN | 3 | 4 | 6 => {}
        ROOT_BUNDLE if format.is_none() => return Err(ERR_UNSUPPORTED),
        ROOT_BUNDLE if change => return Err(ERR_DENIED),
        ROOT_BUNDLE => {}
        ROOT_CARD => format
            .ok_or(ERR_UNSUPPORTED)?
            .features
            .feature("uses.storage")?,
        _ => return Err(ERR_UNSUPPORTED),
    }
    // SAFETY: the caller guarantees readable input.
    let bytes = unsafe { input(bytes, MAX_PATH) }?;
    // Roots 3 and 4 name one file by the empty path.
    if !(bytes.is_empty() && matches!(root, 3 | 4)) {
        storage::relative_path(bytes)?;
    }
    Ok(bytes)
}

fn status(result: Result<(), i32>) -> i32 {
    result.map_or_else(|error| error, |()| OK)
}

/// Slot 20: copy the file size only on success.
///
/// # Safety
/// Nonempty path storage must be live and readable. A non-null aligned output
/// must be writable for one u64, exclusive and disjoint from input and host state.
/// Pointers are borrowed only for this call; output may be uninitialized.
pub(crate) unsafe fn size(
    scope: &EnvironmentScope,
    owner: &str,
    format: Option<Format<'_>>,
    root: u32,
    bytes: PpBytes,
    out: *mut u64,
) -> i32 {
    let result = (|| {
        if out.is_null()
            || !(out as usize).is_multiple_of(core::mem::align_of::<u64>())
            || (out as usize)
                .checked_add(core::mem::size_of::<u64>())
                .is_none()
        {
            return Err(ERR_INVALID);
        }
        // SAFETY: the caller guarantees live input storage.
        let path = unsafe { path(root, bytes, format, false) }?;
        with_adapter(scope, owner, None, format.map(|f| (f, root)), |adapter| {
            adapter.size(root, path)
        })
    })();
    match result {
        Ok(value) => {
            // SAFETY: non-null and aligned above, writable and exclusive by contract.
            unsafe { out.write(value) };
            OK
        }
        Err(error) => error,
    }
}

/// Slot 21: read a bounded chunk into possibly uninitialized caller storage.
/// The card's adapter reads straight into the caller's buffer (no copy of it is
/// held on the heap, #248): its bytes are zeroed first (it may be
/// uninitialized), and the whole of it again on a read error, as the adapter
/// may use it as scratch (sectors, directory entries) before it fails. On
/// success the bytes after `count` are zeroed too, for the same reason. A
/// refusal before the read (the path, the buffer, the route) leaves the
/// buffer untouched (`files.md` 1: `out` is unspecified on an error). An `out`
/// that overlaps the path is `ERR_INVALID`: the path stays borrowed while
/// `out` is written.
///
/// # Safety
/// Nonempty input must be live and readable; nonempty output must be live,
/// writable, exclusive and disjoint from all input and host state for this call.
pub(crate) unsafe fn read_at(
    scope: &EnvironmentScope,
    owner: &str,
    format: Option<Format<'_>>,
    root: u32,
    bytes: PpBytes,
    offset: u64,
    out: PpBuffer,
) -> i32 {
    let result = (|| {
        // SAFETY: the caller guarantees live input storage.
        let path = unsafe { path(root, bytes, format, false) }?;
        pair(out.ptr, out.cap, MAX_DATA)?;
        if overlaps(path, out.ptr, out.cap) {
            return Err(ERR_INVALID);
        }
        // SAFETY: validated bounds, disjoint from the path just above, and the
        // caller's exclusive output contract.
        let out = unsafe { buffers::buffer_out(out, MAX_DATA) }?;
        with_adapter(scope, owner, None, format.map(|f| (f, root)), |adapter| {
            out.fill(MaybeUninit::new(0));
            // SAFETY: every byte was initialized just above, and
            // `MaybeUninit<u8>` has the size and alignment of `u8`.
            let into = unsafe { &mut *(core::ptr::from_mut(out) as *mut [u8]) };
            match adapter.read_at(root, path, offset, into) {
                Ok(count) if count <= into.len() => {
                    if let Some(rest) = into.get_mut(count..) {
                        rest.fill(0);
                    }
                    i32::try_from(count).map_err(|_| ERR_INVALID)
                }
                Ok(_) => {
                    into.fill(0);
                    Err(ERR_INVALID)
                }
                Err(error) => {
                    into.fill(0);
                    Err(error)
                }
            }
        })
    })();
    result.unwrap_or_else(|error| error)
}

/// Slot 22: replace a file, preserving the reserve and path rules.
///
/// # Safety
/// Nonempty input regions must be live, readable and unchanged for this call,
/// and must not overlap mutable host state. No pointer is retained.
pub(crate) unsafe fn write(
    scope: &EnvironmentScope,
    owner: &str,
    format: Option<Format<'_>>,
    in_use: &PathsInUse,
    root: u32,
    bytes: PpBytes,
    data: PpBytes,
) -> i32 {
    // SAFETY: both operations use the same caller-owned readable inputs.
    unsafe { put(scope, owner, format, in_use, root, bytes, data, false) }
}

/// Slot 23: append a chunk, preserving the reserve and path rules.
///
/// # Safety
/// Nonempty input regions must be live, readable and unchanged for this call,
/// and must not overlap mutable host state. No pointer is retained.
pub(crate) unsafe fn append(
    scope: &EnvironmentScope,
    owner: &str,
    format: Option<Format<'_>>,
    in_use: &PathsInUse,
    root: u32,
    bytes: PpBytes,
    data: PpBytes,
) -> i32 {
    // SAFETY: both operations use the same caller-owned readable inputs.
    unsafe { put(scope, owner, format, in_use, root, bytes, data, true) }
}

#[allow(
    clippy::too_many_arguments,
    reason = "the slot's inputs, as the other slots take them"
)]
unsafe fn put(
    scope: &EnvironmentScope,
    owner: &str,
    format: Option<Format<'_>>,
    in_use: &PathsInUse,
    root: u32,
    bytes: PpBytes,
    data: PpBytes,
    append: bool,
) -> i32 {
    status((|| {
        // SAFETY: the caller guarantees live readable inputs.
        let path = unsafe { path(root, bytes, format, true) }?;
        // SAFETY: the caller guarantees live readable inputs.
        let data = unsafe { input(data, MAX_DATA) }?;
        with_adapter(
            scope,
            owner,
            Some(in_use),
            format.map(|f| (f, root)),
            |adapter| {
                if append {
                    adapter.append(root, path, data)
                } else {
                    adapter.write(root, path, data)
                }
            },
        )
    })())
}

/// Slot 24: rename within the owner's data folder.
///
/// # Safety
/// Nonempty input regions must be live, readable and unchanged for this call,
/// and must not overlap mutable host state. No pointer is retained.
pub(crate) unsafe fn rename(
    scope: &EnvironmentScope,
    owner: &str,
    format: Option<Format<'_>>,
    in_use: &PathsInUse,
    root: u32,
    from: PpBytes,
    to: PpBytes,
) -> i32 {
    status((|| {
        // SAFETY: the caller guarantees live readable inputs.
        let from = unsafe { path(root, from, format, true) }?;
        // SAFETY: the caller guarantees live readable inputs.
        let to = unsafe { path(root, to, format, true) }?;
        with_adapter(
            scope,
            owner,
            Some(in_use),
            format.map(|f| (f, root)),
            |adapter| adapter.rename(root, from, to),
        )
    })())
}

/// Slot 25: remove a file, accepting an absent file.
///
/// # Safety
/// Nonempty path storage must be live, readable and unchanged for this call,
/// and must not overlap mutable host state. No pointer is retained.
pub(crate) unsafe fn remove(
    scope: &EnvironmentScope,
    owner: &str,
    format: Option<Format<'_>>,
    in_use: &PathsInUse,
    root: u32,
    bytes: PpBytes,
) -> i32 {
    status((|| {
        // SAFETY: the caller guarantees live readable input.
        let path = unsafe { path(root, bytes, format, true) }?;
        with_adapter(
            scope,
            owner,
            Some(in_use),
            format.map(|f| (f, root)),
            |adapter| adapter.remove(root, path),
        )
    })())
}

/// Slot 26: flush the current callback's storage view.
pub(crate) fn sync(scope: &EnvironmentScope, owner: &str) -> i32 {
    status(with_adapter(scope, owner, None, None, |adapter| {
        adapter.sync()
    }))
}

#[cfg(test)]
#[path = "storage_slots_bundle_tests.rs"]
mod bundle_tests;
#[cfg(test)]
#[path = "storage_slots_card_tests.rs"]
mod card_tests;
#[cfg(test)]
#[path = "storage_slots_format_tests.rs"]
mod format_tests;
#[cfg(test)]
#[path = "storage_slots_tests.rs"]
mod tests;
