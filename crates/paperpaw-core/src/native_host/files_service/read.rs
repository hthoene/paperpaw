//! The reads of `core.files`: `list` (op 1), `stat` (op 2) and `space`
//! (op 3). Nothing here allocates: a listing writes each row into the reply
//! as the storage visits its entry.
use super::{
    View, encode,
    target::{self, BOOK, CARD, Intent, Target},
};
use crate::native_host::codec;
use crate::native_host::{
    registrations::{self as wire, Record},
    storage::{reserve, status},
};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_IO, ERR_NOT_FOUND, ERR_UNSUPPORTED};
use paperpaw_platform::{
    Storage, StorageError,
    tree::{Kind, is_usable_name, kind},
};

/// Most rows of one page (`services_wire::core_files_v1::PAGE_MAX_ROWS`).
const PAGE_ROWS: usize = crate::abi7::services_wire::core_files_v1::PAGE_MAX_ROWS;
/// `fs_entry.flags`: a folder.
const ENTRY_FOLDER: u32 = 1;
/// `fs_entry.flags`: a name the other ops cannot reach.
const ENTRY_UNADDRESSABLE: u32 = 2;
/// `service.end.flags`: the folder is done.
const END_LAST: u32 = 1;
/// `fs_space.flags`.
const SPACE_FREE_KNOWN: u32 = 1;
const SPACE_TOTAL_KNOWN: u32 = 2;
const SPACE_IGNORES_CASE: u32 = 4;
/// Longest `fs_entry.name`, in bytes.
const NAME_MAX: usize = wire::SERVICE_FS_ENTRY_NAME_MAX_BYTES;
/// A path the card answers for whatever it holds, to tell a card that is gone.
const PROBE: &str = ".paperpaw";

/// Whether the card answers (`stat` of root 1 `""`, the `files` plugin's
/// probe); `ERR_IO` when it is gone.
fn card_present(storage: &dyn Storage) -> Result<(), i32> {
    match storage.size(PROBE) {
        Ok(_) | Err(StorageError::NotFound | StorageError::InvalidPath) => Ok(()),
        Err(error) => Err(status(error)),
    }
}

/// Op 1.
#[inline(never)]
pub(super) fn list(view: &View<'_>, record: Record<'_>, out: &mut [u8]) -> Result<usize, i32> {
    let Record::ServiceFsList(input) = record else {
        return Err(ERR_INVALID);
    };
    let mut target = Target::EMPTY;
    target::resolve(view, &mut target, input.root, input.path, Intent::Read)?;
    if input.flags != 0 {
        return Err(ERR_UNSUPPORTED);
    }
    let end_room =
        codec::framed_len(&Record::ServiceEnd(end(0, Some(u64::MAX)))).map_err(|_| ERR_INVALID)?;
    let room = out.len().checked_sub(end_room).ok_or(ERR_INVALID)?;
    if target.empty_bundle {
        // A built-in plugin's install folder: no rows, the last page.
        let closing = Record::ServiceEnd(end(END_LAST, None));
        return codec::encode_one(&closing, out);
    }
    let hide_protected = target.is_card_top();
    let (mut used, mut rows, mut failure) = (0usize, 0usize, None);
    let listed = view
        .storage
        .list_from(target.joined(), input.cursor, &mut |entry| {
            if hide_protected && target::reaches_protected(view, entry.name) {
                return true;
            }
            if rows == PAGE_ROWS {
                return false;
            }
            let row = match entry_row(&entry) {
                Ok(row) => row,
                Err(error) => {
                    failure = Some(error);
                    return false;
                }
            };
            let length = match codec::framed_len(&row) {
                Ok(length) => length,
                Err(_) => {
                    failure = Some(ERR_INVALID);
                    return false;
                }
            };
            // A row that does not fit ends the page there: its place is the
            // next cursor, nothing is cut.
            if length > room - used {
                return false;
            }
            match codec::encode_one(&row, &mut out[used..]) {
                Ok(written) => {
                    used += written;
                    rows += 1;
                    true
                }
                Err(error) => {
                    failure = Some(error);
                    false
                }
            }
        });
    if let Some(error) = failure {
        return Err(error);
    }
    let last = match listed {
        Ok(None) => None,
        Ok(Some(place)) if rows > 0 && place != 0 => Some(place),
        // No row fitted, or a place the reader cannot tell from the start.
        Ok(Some(_)) => return Err(ERR_IO),
        Err(StorageError::NotFound)
            if target.is_root() && target.root != CARD && target.root != BOOK =>
        {
            None
        }
        Err(error) => return Err(list_error(view, &target, error)),
    };
    let closing = Record::ServiceEnd(end(if last.is_none() { END_LAST } else { 0 }, last));
    Ok(used + codec::encode_one(&closing, &mut out[used..])?)
}

/// `service.end` of a page: no total, no generation, no ordinal `next`.
fn end(flags: u32, cursor: Option<u64>) -> wire::ServiceEnd {
    wire::ServiceEnd {
        flags,
        total: 0,
        generation: 0,
        next: 0,
        cursor,
    }
}

/// Why a listing failed: a file at the path is `ERR_INVALID`, a missing
/// folder `ERR_NOT_FOUND`.
fn list_error(view: &View<'_>, target: &Target, error: StorageError) -> i32 {
    match error {
        StorageError::NotFound => ERR_NOT_FOUND,
        StorageError::InvalidPath => match kind(view.storage, target.joined()) {
            Ok(Kind::File) => ERR_INVALID,
            Ok(_) => ERR_NOT_FOUND,
            Err(error) => status(error),
        },
        other => status(other),
    }
}

/// The row of a listed entry. A name the path rules refuse or over 255 bytes
/// is listed whole (up to 765 bytes) and marked `unaddressable`.
fn entry_row<'a>(entry: &paperpaw_platform::DirEntry<'a>) -> Result<Record<'a>, i32> {
    let mut flags = if entry.is_dir { ENTRY_FOLDER } else { 0 };
    if !is_usable_name(entry.name) {
        flags |= ENTRY_UNADDRESSABLE;
    }
    let mut name = entry.name;
    if name.len() > NAME_MAX {
        let mut cut = NAME_MAX;
        while !name.is_char_boundary(cut) {
            cut -= 1;
        }
        name = &name[..cut];
    }
    Ok(Record::ServiceFsEntry(wire::ServiceFsEntry {
        name,
        flags,
        size: if entry.is_dir { 0 } else { entry.size },
        modified: entry.modified.and_then(|at| u64::try_from(at).ok()),
    }))
}

/// Op 2.
#[inline(never)]
pub(super) fn stat(view: &View<'_>, record: Record<'_>, out: &mut [u8]) -> Result<usize, i32> {
    let Record::ServiceFsPath(input) = record else {
        return Err(ERR_INVALID);
    };
    let mut target = Target::EMPTY;
    target::resolve(view, &mut target, input.root, input.path, Intent::Read)?;
    let (flags, size, modified) = if target.is_root() {
        root_present(view, &target)?;
        (ENTRY_FOLDER, 0, None)
    } else {
        let joined = target.joined();
        match kind(view.storage, joined).map_err(status)? {
            Kind::Missing => return Err(ERR_NOT_FOUND),
            Kind::File => (
                0,
                view.storage.size(joined).map_err(status)?,
                view.storage.modified(joined),
            ),
            Kind::Folder => (ENTRY_FOLDER, 0, view.storage.modified(joined)),
            _ => return Err(ERR_IO),
        }
    };
    encode(
        &Record::ServiceFsEntry(wire::ServiceFsEntry {
            name: target.name(),
            flags,
            size,
            modified: modified.and_then(|at| u64::try_from(at).ok()),
        }),
        out,
    )
}

/// A root always exists; the card being away is `ERR_IO`. The data and install
/// folders may not be made yet.
pub(super) fn root_present(view: &View<'_>, target: &Target) -> Result<(), i32> {
    if target.root == CARD || (target.root == BOOK && target.base().is_empty()) {
        return card_present(view.storage);
    }
    if target.root == BOOK {
        // The book's folder is the user's: it is there or it is not.
        return match kind(view.storage, target.base()).map_err(status)? {
            Kind::Folder => Ok(()),
            Kind::Missing => Err(ERR_NOT_FOUND),
            _ => Err(ERR_INVALID),
        };
    }
    match view.storage.list_from(target.base(), 0, &mut |_| false) {
        Ok(_) | Err(StorageError::NotFound | StorageError::InvalidPath) => {
            card_present(view.storage)
        }
        Err(error) => Err(status(error)),
    }
}

/// Op 3.
#[inline(never)]
pub(super) fn space(view: &View<'_>, record: Record<'_>, out: &mut [u8]) -> Result<usize, i32> {
    let Record::ServiceFsPath(input) = record else {
        return Err(ERR_INVALID);
    };
    // Root 3 is for `list` and `stat` only.
    if input.root == BOOK {
        return Err(ERR_UNSUPPORTED);
    }
    let mut target = Target::EMPTY;
    target::resolve(view, &mut target, input.root, input.path, Intent::Read)?;
    if !input.path.is_empty() {
        return Err(ERR_INVALID);
    }
    let storage = view.storage;
    let free = storage.free_space();
    let total = storage.total_space();
    if free.is_none() && total.is_none() {
        card_present(storage)?;
    }
    let mut flags = 0;
    if storage.ignores_case() {
        flags |= SPACE_IGNORES_CASE;
    }
    // The room the kernel keeps for its own saves is not the plugin's.
    let free = free.map(|free| {
        flags |= SPACE_FREE_KNOWN;
        free.saturating_sub(reserve(storage))
    });
    if total.is_some() {
        flags |= SPACE_TOTAL_KNOWN;
    }
    encode(
        &Record::ServiceFsSpace(wire::ServiceFsSpace {
            flags,
            free: free.unwrap_or(0),
            total: total.unwrap_or(0),
        }),
        out,
    )
}
