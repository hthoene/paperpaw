//! Typed `core.files`: folders, file information, moves and free space, with
//! caller-owned buffers (`docs/abi/files.md`).
//!
//! Roots are those of [`Root`]: the plugin's data folder, the card (with the
//! signed `uses.storage` feature) and, read only, the plugin's install
//! folder. Every call is one bounded piece of card work; a long folder is read
//! page by page ([`Files::list`]) and a long tree removed step by step
//! ([`Files::remove_step`], or [`Files::remove_tree`] for all of them).
use super::storage::Root;
use super::wire::services_wire::core_files_v1 as schema;
use super::{
    Error, Host,
    declare::records_wire::{self as wire, WireEncode},
    services::Service,
};

/// Most entries one page of [`Files::list`] holds.
pub const PAGE_MAX_ROWS: usize = schema::PAGE_MAX_ROWS;
/// The longest path, in UTF-8 bytes.
pub const MAX_PATH: usize = wire::SERVICE_FS_PATH_PATH_MAX_BYTES;
/// The longest name of an entry, in UTF-8 bytes.
pub const MAX_NAME: usize = wire::SERVICE_FS_ENTRY_NAME_MAX_BYTES;
/// Input buffer a call needs.
pub const INPUT_BYTES: usize = 1536;
/// Output buffer a call needs (the largest reply of any op).
pub const OUTPUT_BYTES: usize = 4096;

const ENTRY_FOLDER: u32 = 1;
const ENTRY_UNADDRESSABLE: u32 = 2;
const MOVE_REPLACE: u32 = 1;
const SPACE_FREE_KNOWN: u32 = 1;
const SPACE_TOTAL_KNOWN: u32 = 2;
const SPACE_IGNORES_CASE: u32 = 4;
const PROGRESS_DONE: u32 = 1;
const END_LAST: u32 = 1;
/// Calls of a whole-tree removal in a row that remove nothing before it gives up.
const IDLE_CALLS: u32 = 1024;
/// Most removals one `remove_tree` call makes.
const REMOVED_MAX: u32 = 64;

/// A `core.files` service handle. Release it explicitly within a callback.
#[derive(Debug)]
#[must_use = "release the files handle when finished"]
pub struct Files {
    service: Service,
}

/// One entry of a folder, or the one [`Files::stat`] describes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry<'a> {
    name: &'a str,
    flags: u32,
    size: u64,
    modified: Option<u64>,
}

impl<'a> Entry<'a> {
    /// `root` is true for the one row that may have no name: `stat` of a root.
    fn decode(record: wire::RawRecord<'a>, root: bool) -> Result<Self, Error> {
        let row: wire::ServiceFsEntry<'a> = typed(record)?;
        let nameless = row.name.is_empty() || matches!(row.name, "." | "..");
        if row.flags & !(ENTRY_FOLDER | ENTRY_UNADDRESSABLE) != 0
            || row.name.contains('/')
            || nameless && !(root && row.name.is_empty())
            || row.flags & ENTRY_FOLDER != 0 && row.size != 0
        {
            return Err(Error::Invalid);
        }
        Ok(Self {
            name: row.name,
            flags: row.flags,
            size: row.size,
            modified: row.modified,
        })
    }
    /// The last segment of the path (`""` for a root).
    #[must_use]
    pub const fn name(&self) -> &'a str {
        self.name
    }
    /// Whether it is a folder.
    #[must_use]
    pub const fn is_folder(&self) -> bool {
        self.flags & ENTRY_FOLDER != 0
    }
    /// Whether its name cannot be used in a path (too long, or a character the
    /// path rules refuse): it can be shown, not opened, moved or removed.
    #[must_use]
    pub const fn unaddressable(&self) -> bool {
        self.flags & ENTRY_UNADDRESSABLE != 0
    }
    /// A file's bytes; 0 for a folder.
    #[must_use]
    pub const fn size(&self) -> u64 {
        self.size
    }
    /// Seconds since 1970-01-01 as the storage keeps them (FAT: the device's
    /// local time read as UTC); none where the storage keeps no time.
    #[must_use]
    pub const fn modified(&self) -> Option<u64> {
        self.modified
    }
}

/// One page of a folder's entries.
#[derive(Clone, Copy, Debug)]
pub struct Listing<'a> {
    bytes: &'a [u8],
    count: usize,
    next: Option<u64>,
}

impl<'a> Listing<'a> {
    fn decode(bytes: &'a [u8], from: u64) -> Result<Self, Error> {
        let mut rest = bytes;
        let mut count = 0;
        let end = loop {
            let (record, n) = wire::RawRecord::prefix(rest).map_err(|_| Error::Invalid)?;
            rest = rest.get(n..).ok_or(Error::Invalid)?;
            if record.tag() == wire::SERVICE_END_TAG {
                break typed::<wire::ServiceEnd>(record)?;
            }
            count += 1;
            if count > PAGE_MAX_ROWS {
                return Err(Error::Invalid);
            }
            Entry::decode(record, false)?;
        };
        if !rest.is_empty()
            || end.flags & !END_LAST != 0
            || (end.next, end.total, end.generation) != (0, 0, 0)
        {
            return Err(Error::Invalid);
        }
        let next = if end.flags & END_LAST != 0 {
            None
        } else {
            // A page that is not the last names where to go on, and goes on
            // from somewhere else than it came from, or it would repeat for ever.
            let cursor = end.cursor.ok_or(Error::Invalid)?;
            if count == 0 && cursor == from {
                return Err(Error::Invalid);
            }
            Some(cursor)
        };
        Ok(Self { bytes, count, next })
    }
    /// The entries of this page, in the storage's order (no sorting; `.`
    /// and `..` never; hidden names included).
    pub fn entries(&self) -> impl Iterator<Item = Entry<'a>> + use<'a> {
        let mut rest = self.bytes;
        let mut left = self.count;
        core::iter::from_fn(move || {
            if left == 0 {
                return None;
            }
            left -= 1;
            let (record, n) = wire::RawRecord::prefix(rest).ok()?;
            rest = rest.get(n..)?;
            Entry::decode(record, false).ok()
        })
    }
    /// How many entries the page holds.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.count
    }
    /// Whether the page holds none.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }
    /// Where to go on (the `cursor` of the next [`Files::list`]), none when
    /// the folder is done. Valid only for the same folder.
    #[must_use]
    pub const fn next(&self) -> Option<u64> {
        self.next
    }
}

/// Free and total bytes of a root's medium.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Space {
    free: u64,
    total: u64,
    flags: u32,
}

impl Space {
    /// Bytes free for new files, after the host's reserve; none while it is
    /// not known (on the X4 until the FAT has been counted).
    #[must_use]
    pub const fn free(&self) -> Option<u64> {
        if self.flags & SPACE_FREE_KNOWN != 0 {
            Some(self.free)
        } else {
            None
        }
    }
    /// The medium's size, if known.
    #[must_use]
    pub const fn total(&self) -> Option<u64> {
        if self.flags & SPACE_TOTAL_KNOWN != 0 {
            Some(self.total)
        } else {
            None
        }
    }
    /// Whether names that differ only in case name one entry (FAT).
    #[must_use]
    pub const fn ignores_case(&self) -> bool {
        self.flags & SPACE_IGNORES_CASE != 0
    }
}

/// What one step of [`Files::remove_step`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    removed: u32,
    done: bool,
    next: u64,
}

impl Progress {
    /// How many entries this step removed.
    #[must_use]
    pub const fn removed(&self) -> u32 {
        self.removed
    }
    /// Whether the tree is gone.
    #[must_use]
    pub const fn done(&self) -> bool {
        self.done
    }
    /// Where the next step goes on from (`cursor` of the next call).
    #[must_use]
    pub const fn next(&self) -> u64 {
        self.next
    }
}

/// [`check`] for the reading ops `list` and `stat`, which also take the book
/// root (3) inside a `core.format` call of a folder or siblings document
/// (`files.md` 1.1); the host answers Unsupported outside one.
fn check_read(root: Root, path: &str) -> Result<(), Error> {
    check(if root == Root::Book { Root::Data } else { root }, path)
}

fn check(root: Root, path: &str) -> Result<(), Error> {
    // Roots 3 and above are reserved: the service answers Unsupported, and so
    // does this check, without a call.
    if !matches!(root, Root::Data | Root::Files | Root::Install) {
        return Err(Error::Unsupported);
    }
    if path.len() > MAX_PATH {
        return Err(Error::NoSpace);
    }
    Ok(())
}

fn one(bytes: &[u8]) -> Result<wire::RawRecord<'_>, Error> {
    wire::RawRecord::one(bytes).map_err(|_| Error::Invalid)
}

/// The record `raw` holds as `T`; another tag is as malformed as a bad body.
fn typed<'a, T: wire::WireRecord<'a>>(raw: wire::RawRecord<'a>) -> Result<T, Error> {
    raw.decode().map_err(|_| Error::Invalid)
}

impl Files {
    /// Open the metadata-declared files service.
    ///
    /// # Errors
    /// Returns the host's admission, availability or allocation refusal.
    pub fn open(host: &Host<'_>) -> Result<Self, Error> {
        Service::open(host, schema::KEY, schema::MAJOR, 0).map(|service| Self { service })
    }

    /// Release this service handle using the current callback's host.
    ///
    /// # Errors
    /// Returns the host's stale-handle or ownership refusal.
    pub fn release(self, host: &Host<'_>) -> Result<(), Error> {
        self.service.release(host)
    }

    fn call<R: WireEncode + Copy>(
        &self,
        host: &Host<'_>,
        op: u32,
        record: R,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<usize, Error> {
        let operation = self.service.operation(op).ok_or(Error::Unsupported)?;
        let len = wire::records_encode_counted_of(
            core::iter::once(record),
            usize::from(operation.input_rows),
            operation.max_input,
            input,
        )
        .map_err(Error::from_status)?;
        let n = self
            .service
            .call(host, op, input.get(..len).ok_or(Error::Invalid)?, out)?;
        // A larger scratch buffer does not widen what the operation may answer.
        if n > operation.max_reply {
            return Err(Error::Invalid);
        }
        Ok(n)
    }

    fn mutate<R: WireEncode + Copy>(
        &self,
        host: &Host<'_>,
        op: u32,
        record: R,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<(), Error> {
        match self.call(host, op, record, input, out)? {
            0 => Ok(()),
            _ => Err(Error::Invalid),
        }
    }

    /// One page of the folder `path`, from `cursor` (0 starts; then the
    /// [`Listing::next`] of the page before).
    ///
    /// # Errors
    /// NotFound for a missing folder, Invalid for a file or a malformed reply
    /// (also a page that does not go on), NoSpace for a long path, Unsupported
    /// for a root the service does not have (the user CSS and cache roots) and
    /// for the book root ([`Root::Book`]) outside a `core.format` call of a
    /// folder or siblings document, where a format lists its book's folder,
    /// Denied for the card without `uses.storage`, Busy while the host uses the
    /// folder; otherwise the host's refusal.
    pub fn list<'a>(
        &self,
        host: &Host<'_>,
        root: Root,
        path: &str,
        cursor: u64,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<Listing<'a>, Error> {
        check_read(root, path)?;
        let record = wire::ServiceFsList {
            root: root.wire(),
            path,
            cursor,
            flags: 0,
        };
        let n = self.call(host, schema::OP_LIST, record, input, out)?;
        Listing::decode(out.get(..n).ok_or(Error::Invalid)?, cursor)
    }

    /// Describe the file or folder at `path` (`""` is the root).
    ///
    /// # Errors
    /// NotFound where there is none; otherwise as [`Files::list`].
    pub fn stat<'a>(
        &self,
        host: &Host<'_>,
        root: Root,
        path: &str,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<Entry<'a>, Error> {
        check_read(root, path)?;
        let record = wire::ServiceFsPath {
            root: root.wire(),
            path,
        };
        let n = self.call(host, schema::OP_STAT, record, input, out)?;
        Entry::decode(one(out.get(..n).ok_or(Error::Invalid)?)?, path.is_empty())
    }

    /// Free and total bytes of the medium `root` lives on.
    ///
    /// # Errors
    /// As [`Files::list`].
    pub fn space(
        &self,
        host: &Host<'_>,
        root: Root,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<Space, Error> {
        check(root, "")?;
        let record = wire::ServiceFsPath {
            root: root.wire(),
            path: "",
        };
        let n = self.call(host, schema::OP_SPACE, record, input, out)?;
        let row: wire::ServiceFsSpace = typed(one(out.get(..n).ok_or(Error::Invalid)?)?)?;
        if row.flags & !(SPACE_FREE_KNOWN | SPACE_TOTAL_KNOWN | SPACE_IGNORES_CASE) != 0 {
            return Err(Error::Invalid);
        }
        Ok(Space {
            free: row.free,
            total: row.total,
            flags: row.flags,
        })
    }

    /// Create the folder `path` and every missing parent; an existing folder
    /// is fine.
    ///
    /// # Errors
    /// Invalid where a file is in the way, Denied on the install folder,
    /// Full for a full card; otherwise as [`Files::list`].
    pub fn make_dir(
        &self,
        host: &Host<'_>,
        root: Root,
        path: &str,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<(), Error> {
        check(root, path)?;
        let record = wire::ServiceFsPath {
            root: root.wire(),
            path,
        };
        self.mutate(host, schema::OP_MAKE_DIR, record, input, out)
    }

    /// One bounded step of removing the folder (or file) at `path` and all in
    /// it: `cursor` is 0 to start, then the [`Progress::next`] of the step
    /// before.
    ///
    /// # Errors
    /// Denied on the install folder, Invalid for the card's root; otherwise as
    /// [`Files::list`].
    pub fn remove_step(
        &self,
        host: &Host<'_>,
        root: Root,
        path: &str,
        cursor: u64,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<Progress, Error> {
        check(root, path)?;
        let record = wire::ServiceFsList {
            root: root.wire(),
            path,
            cursor,
            flags: 0,
        };
        let n = self.call(host, schema::OP_REMOVE_TREE, record, input, out)?;
        let row: wire::ServiceFsProgress = typed(one(out.get(..n).ok_or(Error::Invalid)?)?)?;
        if row.flags & !PROGRESS_DONE != 0 || row.removed > REMOVED_MAX {
            return Err(Error::Invalid);
        }
        Ok(Progress {
            removed: row.removed,
            done: row.flags & PROGRESS_DONE != 0,
            next: row.next,
        })
    }

    /// Remove the folder (or file) at `path` and all in it, a bounded step at
    /// a time until it is gone; how many entries were removed. What was removed
    /// stays removed where it stops with an error.
    ///
    /// This is the "loop until done" helper: it is bounded, but it has no
    /// budget and cannot be cancelled. A caller that needs either, or a
    /// progress display (a file manager, a transfer), calls
    /// [`Files::remove_step`] across work runs or ticks instead.
    ///
    /// # Errors
    /// As [`Files::remove_step`]; Invalid where the host makes more than
    /// `IDLE_CALLS` calls in a row that remove nothing.
    pub fn remove_tree(
        &self,
        host: &Host<'_>,
        root: Root,
        path: &str,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<u64, Error> {
        let (mut cursor, mut total, mut idle) = (0, 0u64, 0u32);
        loop {
            let step = self.remove_step(host, root, path, cursor, input, out)?;
            total = total.saturating_add(u64::from(step.removed));
            if step.done {
                return Ok(total);
            }
            // Why this ends: every call that removes something is progress on a
            // finite tree, and a correct host reads at most 256 entries a call
            // (`docs/abi/files.md` 2.5), so IDLE_CALLS calls without one removal
            // are over 256k entries read, four full FAT folders. Whatever `next`
            // says (a cursor that changes may still go in circles), such a host
            // broke "every loop advances".
            if step.removed > 0 {
                idle = 0;
            } else {
                idle += 1;
                if idle > IDLE_CALLS {
                    return Err(Error::Invalid);
                }
            }
            cursor = step.next;
        }
    }

    /// Move or rename the file or folder `from` to `to` (also between the data
    /// folder and the card). With `replace`, an existing file at `to` is
    /// replaced; a folder there never is.
    ///
    /// # Errors
    /// NotFound for a missing source or parent of `to`, State where `to`
    /// exists, Busy while the host uses a path, Invalid for a root or a folder
    /// moved into itself, Denied on the install folder.
    #[allow(
        clippy::too_many_arguments,
        reason = "two paths with their roots, and the call's buffers"
    )]
    pub fn move_entry(
        &self,
        host: &Host<'_>,
        from_root: Root,
        from: &str,
        to_root: Root,
        to: &str,
        replace: bool,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<(), Error> {
        check(from_root, from)?;
        check(to_root, to)?;
        let record = wire::ServiceFsMove {
            from_root: from_root.wire(),
            from,
            to_root: to_root.wire(),
            to,
            flags: if replace { MOVE_REPLACE } else { 0 },
        };
        self.mutate(host, schema::OP_MOVE, record, input, out)
    }
}

#[cfg(test)]
mod tests;
