//! Host side of the service `core.files` (`docs/abi/files.md` 2): folders,
//! file information, moves and free space over the callback's storage view.
//!
//! Stateless like the other service backends: [`backend`] gets the frame (whose
//! [`storage`](Frame::storage) is the caller's metered view of the one card), the
//! caller and the op, decodes and checks the input, and answers in the
//! scratch the dispatcher lends. Nothing is allocated and nothing is kept
//! between calls: paths are checked and joined in a stack buffer ([`target`]),
//! a listing is written row by row as the storage visits entries, and a tree
//! removal hands back the place it needs next. So there is nothing to end
//! when a plugin is retired.
//!
//! The ops split in [`read`] (`list`, `stat`, `space`) and [`change`]
//! (`make_dir`, `remove_tree`, `move`). What only the shell knows is
//! behind [`FilesHost`]: the paths the host holds in use and the library's
//! move job (`ERR_BUSY`), how FAT names reach the protected folders, and the
//! journalled rename of the user's files. Until a shell wires it, the
//! family answers `ERR_UNSUPPORTED`.
use super::{host_services::Caller as HostCaller, registrations::Record};
use crate::abi7::{context::Frame, services_wire::ServiceOp};
use crate::native_host::codec;
// The tests build their inputs with the wire types.
#[cfg(test)]
use super::registrations as wire;
use alloc::rc::Rc;
use paperpaw_native::host_glue::{ERR_INVALID, ERR_STATE, ERR_UNSUPPORTED};
use paperpaw_platform::Storage;

mod change;
mod read;
mod target;

pub(crate) use target::{same_fold, within};

pub(crate) use super::lib_service::moves::Access;

/// The calling plugin: its host owner id, its plugin id, and whether the
/// instance may use root 1.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Caller<'a> {
    pub(crate) owner: u64,
    pub(crate) plugin: &'a str,
    /// `uses.storage` (decision 125): `Ok`, `ERR_DENIED` when the instance
    /// did not ask for it, `ERR_UNSUPPORTED` when the host offers no root 1.
    pub(crate) storage: Result<(), i32>,
}

/// What the shell adds to the storage view: the state it keeps about paths,
/// and the parts of a move that are the library's.
pub(crate) trait FilesHost {
    /// Whether the first segment of a path of `root` (0 or 1) names, by the
    /// storage's own rules, a name the root never reaches: `.received` on root 0
    /// (the host's, `files.md` 1.1) and `plugins` and `.paperpaw` on root 1.
    /// The service already refuses their spelling in any case; this is the
    /// identity beyond it (an 8.3 alias such as `PAPERP~1`).
    ///
    /// `parent` is the storage folder `first_segment` is in (root 0: the
    /// plugin's data folder; root 1: `""`, the card's top).
    ///
    /// Required: a host that forgets it would let an alias reach them.
    fn excluded(&self, root: u32, parent: &str, first_segment: &str) -> bool;

    /// Whether `name` is the folder of a running data plugin (`plugins/<name>`):
    /// root 1 shows it, read only (`files.md` 1.1). Data plugins hold published
    /// files only; the folders of code plugins stay hidden. The default is "no".
    fn data_folder(&self, _name: &str) -> bool {
        false
    }

    /// Whether `entry`, right below the shown plugin folder `folder`
    /// (`plugins/<name>`), is its code or one of the host's files, which root
    /// 1 never shows ([`super::data_plugin_folders::host_file`]).
    ///
    /// Required: a host that forgets it would show a plugin's code.
    fn host_file(&self, folder: &str, entry: &str) -> bool;

    /// Whether `plugin` is built into the firmware (a linked plugin). Its
    /// install folder (root 2) counts as empty whatever the card holds at
    /// `plugins/<id>/` (a leftover of an older install): every path in it is
    /// `ERR_NOT_FOUND` and a listing of it has no rows.
    ///
    /// Required: a host that forgets it would let a leftover folder be read.
    fn built_in(&self, plugin: &str) -> bool;

    /// Whether `access` to `path` (card relative) is `ERR_BUSY` now: the host
    /// holds it in use (the reader's book, a staged install, a download's
    /// file, a served request body) or the library's move job runs and says
    /// so (`Moves::busy`). A folder that holds (or is below) an in-use path
    /// is busy too: removing or moving it would take that path away, and the
    /// service asks about the folder only, never about what is inside it.
    /// Reading is never asked about.
    fn busy(&self, _path: &str, _access: Access) -> bool {
        false
    }

    /// Whether the library's move job runs: then every `move` is `ERR_BUSY`,
    /// wherever it points (one move job at a time, device-wide).
    fn moving(&self) -> bool {
        false
    }

    /// Whether `path` is `folder` or below it as the storage names entries
    /// (case beyond ASCII, 8.3 aliases). The default compares spellings, with
    /// case ignored where the storage ignores it.
    fn within(&self, storage: &dyn Storage, path: &str, folder: &str) -> bool {
        target::within(storage.ignores_case(), path, folder)
    }

    /// Moves `from` to `to` after the service has checked them (both exist
    /// as it said, `to` is free or a file to replace, no folder moves into
    /// itself). `library`: both are the user's files (root 1); the library
    /// follows such a move with its journal entry and job (`files.md` 2.6),
    /// which the host starts here. The default is the storage's rename.
    ///
    /// # Errors
    /// An `ERR_*` status; a refusal changes nothing.
    fn rename(
        &self,
        storage: &dyn Storage,
        from: &str,
        to: &str,
        replace: bool,
        library: bool,
    ) -> Result<(), i32> {
        let _ = (replace, library);
        storage.rename(from, to).map_err(super::storage::status)
    }
}

/// What the shell hands the frame under the key `core.files`.
pub(crate) struct FilesServices(pub(crate) Rc<dyn FilesHost>);

/// Ops of `core.files` major 1.
mod op {
    pub(super) const LIST: u32 = 1;
    pub(super) const STAT: u32 = 2;
    pub(super) const SPACE: u32 = 3;
    pub(super) const MAKE_DIR: u32 = 4;
    pub(super) const REMOVE_TREE: u32 = 5;
    pub(super) const MOVE: u32 = 6;
}

/// The `core.files` backend.
pub(crate) fn backend(
    frame: &dyn Frame,
    caller: &HostCaller<'_>,
    op: &ServiceOp,
    input: &[u8],
    out: &mut [u8],
) -> Result<usize, i32> {
    let services = caller.hosts.files().ok_or(ERR_UNSUPPORTED)?;
    // Outside a callback the frame has no storage view (the slot rules).
    let storage = frame.storage().ok_or(ERR_STATE)?;
    let who = Caller {
        owner: caller.owner,
        plugin: caller.plugin,
        storage: caller.features.feature("uses.storage"),
    };
    let view = View {
        host: &*services.0,
        storage,
        who: &who,
        scope: caller.hosts.format_scope(),
    };
    let record = codec::decode_single(input)?;
    match op.id {
        op::LIST => read::list(&view, record, out),
        op::STAT => read::stat(&view, record, out),
        op::SPACE => read::space(&view, record, out),
        op::MAKE_DIR => change::make_dir(&view, record),
        op::REMOVE_TREE => change::remove_tree(&view, record, out),
        op::MOVE => change::move_entry(&view, record),
        _ => Err(ERR_UNSUPPORTED),
    }
}

/// What every op works with.
struct View<'a> {
    host: &'a dyn FilesHost,
    storage: &'a dyn Storage,
    who: &'a Caller<'a>,
    /// The format calls under way: root 3 is the book of the caller's own.
    scope: &'a super::format_scope::FormatScope,
}

/// A record the host built, framed into `out`; its length.
fn encode(record: &Record<'_>, out: &mut [u8]) -> Result<usize, i32> {
    let length = codec::framed_len(record).map_err(|_| ERR_INVALID)?;
    if length > out.len() {
        return Err(paperpaw_native::host_glue::ERR_NO_SPACE);
    }
    codec::encode_one(record, out)
}

#[cfg(test)]
#[path = "files_service/tests.rs"]
mod tests;
