//! The changes of `core.files`: `make_dir` (op 4), `remove_tree` (op 5) and
//! `move` (op 6). Every one checks root, path, exclusions and the host's
//! `ERR_BUSY` before it touches anything; a refused call changes nothing.
use super::{
    Access, View, encode, library_follows,
    target::{self, CARD, DATA, Intent, Target},
};
use crate::native_host::{
    registrations::{self as wire, Record},
    storage::{check_room, status},
};
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_DENIED, ERR_INVALID, ERR_NOT_FOUND, ERR_STATE, ERR_UNSUPPORTED,
};
use paperpaw_platform::StorageError;
use paperpaw_platform::tree::{Kind, kind};

/// `fs_move.flags`: replace an existing file at `to`.
const MOVE_REPLACE: u32 = 1;
/// `fs_progress.flags`: the tree is gone.
const PROGRESS_DONE: u32 = 1;

/// Op 4.
#[inline(never)]
pub(super) fn make_dir(view: &View<'_>, record: Record<'_>) -> Result<usize, i32> {
    let Record::ServiceFsPath(input) = record else {
        return Err(ERR_INVALID);
    };
    let mut target = Target::EMPTY;
    target::resolve(view, &mut target, input.root, input.path, Intent::Change)?;
    // The root itself always exists, while the card is there.
    if target.is_root() {
        return super::read::root_present(view, &target).map(|()| 0);
    }
    // The host's folder: a plugin reads it, never makes names in it.
    if target.is_received() {
        return Err(ERR_DENIED);
    }
    let joined = target.joined();
    // Only the old name of a move job is closed to new names; below the new
    // name they go on (`files.md` 2.6).
    if view.host.busy(joined, Access::Write) {
        return Err(ERR_BUSY);
    }
    match kind(view.storage, joined).map_err(status)? {
        Kind::Folder => return Ok(0),
        Kind::File => return Err(ERR_INVALID),
        Kind::Missing => {}
        _ => return Err(ERR_STATE),
    }
    // A folder takes a cluster: the reserve is kept (S4).
    check_room(view.storage, 0)?;
    view.storage.create_dir(joined).map_err(status)?;
    Ok(0)
}

/// Op 5.
#[inline(never)]
pub(super) fn remove_tree(
    view: &View<'_>,
    record: Record<'_>,
    out: &mut [u8],
) -> Result<usize, i32> {
    let Record::ServiceFsList(input) = record else {
        return Err(ERR_INVALID);
    };
    let mut target = Target::EMPTY;
    target::resolve(view, &mut target, input.root, input.path, Intent::Change)?;
    if input.flags != 0 {
        return Err(ERR_UNSUPPORTED);
    }
    // Never the whole card.
    if target.root == CARD && target.is_root() {
        return Err(ERR_INVALID);
    }
    let joined = target.joined();
    if view.host.busy(joined, Access::Remove) {
        return Err(ERR_BUSY);
    }
    // Root 0 `""` empties the data folder; the folder itself stays.
    let keep = target.root == DATA && target.is_root();
    let step = view
        .storage
        .remove_tree_step(joined, keep, input.cursor)
        .map_err(|error| match error {
            // A folder nested deeper than the storage goes (files.md 2.5).
            StorageError::InvalidData => ERR_STATE,
            other => status(other),
        })?;
    encode(
        &Record::ServiceFsProgress(wire::ServiceFsProgress {
            removed: step.removed,
            flags: if step.done { PROGRESS_DONE } else { 0 },
            next: step.next,
        }),
        out,
    )
}

/// Op 6.
#[inline(never)]
pub(super) fn move_entry(view: &View<'_>, record: Record<'_>) -> Result<usize, i32> {
    let Record::ServiceFsMove(input) = record else {
        return Err(ERR_INVALID);
    };
    let (mut from, mut to) = (Target::EMPTY, Target::EMPTY);
    target::resolve(view, &mut from, input.from_root, input.from, Intent::Change)?;
    target::resolve(view, &mut to, input.to_root, input.to, Intent::Change)?;
    if input.flags & !MOVE_REPLACE != 0 {
        return Err(ERR_UNSUPPORTED);
    }
    // A root is never moved.
    if from.is_root() || to.is_root() {
        return Err(ERR_INVALID);
    }
    // Names in the host's folder are not the plugin's to make or take away.
    if to.is_received() || from.is_received_folder() {
        return Err(ERR_DENIED);
    }
    let replace = input.flags & MOVE_REPLACE != 0;
    let (from_path, to_path) = (from.joined(), to.joined());
    if view.host.moving()
        || view.host.busy(from_path, Access::Rename)
        || view.host.busy(to_path, Access::Rename)
    {
        return Err(ERR_BUSY);
    }
    let storage = view.storage;
    let ignore_case = storage.ignores_case();
    // The entry under another spelling (FAT): a rename, not a replace.
    let same = target::same_spelling(ignore_case, from_path, to_path);
    let identical = from_path == to_path;
    let from_kind = kind(storage, from_path).map_err(status)?;
    if from_kind == Kind::Missing {
        return Err(ERR_NOT_FOUND);
    }
    if !same && view.host.within(storage, to_path, from_path) {
        return Err(ERR_INVALID);
    }
    if identical {
        // Onto itself: a folder is moved into itself, a file is there.
        return match (from_kind, replace) {
            (Kind::Folder, _) => Err(ERR_INVALID),
            (_, true) => Ok(0),
            _ => Err(ERR_STATE),
        };
    }
    check_parent(view, &to)?;
    if !same {
        match kind(storage, to_path).map_err(status)? {
            Kind::Missing => {}
            Kind::File if replace && from_kind == Kind::File => {}
            // An existing folder is never merged, a file is never replaced
            // by a folder.
            _ => return Err(ERR_STATE),
        }
    }
    // The library follows the user's folders and books (2.6); any other
    // single file there is the card's rename alone.
    let library = from.root == CARD
        && to.root == CARD
        && library_follows(
            view.formats,
            storage,
            from_kind == Kind::Folder,
            from_path,
            to_path,
        );
    view.host
        .rename(storage, from_path, to_path, replace, library)?;
    Ok(0)
}

/// The folder above `to` must exist (`ERR_NOT_FOUND`); a file there is
/// `ERR_INVALID`. A root's own folder counts as there.
fn check_parent(view: &View<'_>, to: &Target) -> Result<(), i32> {
    let Some(parent) = to.parent_joined() else {
        return Ok(());
    };
    if to.parent_rel().is_empty() {
        // The root: the card's, or a plugin's own folder, made on first use.
        return Ok(());
    }
    match kind(view.storage, parent).map_err(status)? {
        Kind::Folder => Ok(()),
        Kind::File => Err(ERR_INVALID),
        Kind::Missing => Err(ERR_NOT_FOUND),
        _ => Err(ERR_STATE),
    }
}
