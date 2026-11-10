//! A document's cache on the card (`docs/abi/document.md` 2.1, "The cache is
//! whole or not used"): where it is, when it counts as finished, and how many
//! conversions of one book the card keeps.
//!
//! The cache of one **conversion** (the book's key, the format plugin's id and
//! version, the conversion key) is the folder
//! `.paperpaw/cache/<book key>/<format plugin>/<conversion key>/`: the
//! format's root 6. The host writes the stamp `.complete` there once the
//! format answered done and the card was synced; a folder without a stamp
//! that says exactly this conversion is removed before a document uses it, so
//! no format sees a half-made or stale cache as finished. A book keeps at
//! most [`MAX_CONVERSIONS`]; the older goes, never one an open document uses.
//!
//! Everything here is a function of a [`Storage`]: nothing is kept in memory
//! between calls, and every step that can fail says so (`ERR_*`, never a
//! panic). Removing a tree is bounded ([`CLEAR_STEPS`] pieces a call); what is
//! left after that is `ERR_BUSY`, and the next call goes on where the card is.
use alloc::{string::String, vec::Vec};
use paperpaw_native::host_glue::{ERR_BUSY, ERR_IO, ERR_NO_MEMORY, ERR_NO_SPACE};
use paperpaw_platform::{Storage, StorageError};

/// The folder of all document caches.
pub(crate) const ROOT: &str = ".paperpaw/cache";
/// The host's stamp at the top of a finished cache (a format cannot touch it).
pub(crate) const STAMP: &str = ".complete";
/// The host's own folder in a cache, where the stamp is made.
const HOST: &str = ".host";
/// Conversions of one book on the card.
pub(crate) const MAX_CONVERSIONS: usize = 2;
/// Pieces of a tree removal one call does (each removes a bounded number of
/// entries); a bigger tree is `ERR_BUSY` and goes on with the next call.
const CLEAR_STEPS: u32 = 64;
/// The stamp's first line.
const STAMP_HEAD: &str = "paperpaw-cache 1";

/// What one cache is for.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Identity<'a> {
    /// The book key (16 hexadecimal digits).
    pub(crate) book: &'a str,
    pub(crate) plugin: &'a str,
    pub(crate) version: &'a str,
    pub(crate) conversion: u64,
}

fn hex(value: u64, out: &mut [u8; 16]) -> &str {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    for (i, digit) in out.iter_mut().enumerate() {
        *digit = DIGITS[((value >> (60 - 4 * i)) & 0xf) as usize];
    }
    core::str::from_utf8(out).unwrap_or_default()
}

fn made(len: usize) -> Result<String, i32> {
    let mut text = String::new();
    text.try_reserve_exact(len).map_err(|_| ERR_NO_MEMORY)?;
    Ok(text)
}

impl Identity<'_> {
    /// The cache folder, without a trailing slash.
    pub(crate) fn folder(&self) -> Result<String, i32> {
        let mut digits = [0_u8; 16];
        let conversion = hex(self.conversion, &mut digits);
        let mut path =
            made(ROOT.len() + self.book.len() + self.plugin.len() + conversion.len() + 3)?;
        for (i, part) in [ROOT, self.book, self.plugin, conversion]
            .iter()
            .enumerate()
        {
            if i > 0 {
                path.push('/');
            }
            path.push_str(part);
        }
        Ok(path)
    }

    /// What the stamp says: the conversion, and nothing else.
    fn stamp(&self) -> Result<String, i32> {
        let mut digits = [0_u8; 16];
        let conversion = hex(self.conversion, &mut digits);
        let mut text =
            made(STAMP_HEAD.len() + self.plugin.len() + self.version.len() + conversion.len() + 4)?;
        for part in [STAMP_HEAD, self.plugin, self.version, conversion] {
            text.push_str(part);
            text.push('\n');
        }
        Ok(text)
    }
}

/// Whether the folder is on the card (`Storage::exists` is a file's question).
fn folder_exists(storage: &dyn Storage, folder: &str) -> bool {
    storage.list_bounded(folder, 0, &mut |_| false).is_ok()
}

fn status(error: StorageError) -> i32 {
    match error {
        StorageError::Full | StorageError::TooLarge => ERR_NO_SPACE,
        StorageError::OutOfMemory => ERR_NO_MEMORY,
        _ => ERR_IO,
    }
}

/// Whether `folder` holds the stamp of exactly `id`.
pub(crate) fn is_complete(storage: &dyn Storage, id: &Identity<'_>) -> Result<bool, i32> {
    let folder = id.folder()?;
    let want = id.stamp()?;
    let mut path = made(folder.len() + 1 + STAMP.len())?;
    path.push_str(&folder);
    path.push('/');
    path.push_str(STAMP);
    // One byte more than the stamp is read: a longer file is another stamp.
    let mut buffer = Vec::new();
    buffer
        .try_reserve_exact(want.len() + 1)
        .map_err(|_| ERR_NO_MEMORY)?;
    buffer.resize(want.len() + 1, 0);
    match storage.read_at(&path, 0, &mut buffer) {
        Ok(count) => Ok(buffer[..count] == *want.as_bytes()),
        Err(StorageError::NotFound) => Ok(false),
        Err(error) => Err(status(error)),
    }
}

/// Removes the tree at `folder`: `ERR_BUSY` where more than [`CLEAR_STEPS`]
/// pieces are left.
pub(crate) fn clear(storage: &dyn Storage, folder: &str) -> Result<(), i32> {
    let mut place = 0;
    for _ in 0..CLEAR_STEPS {
        match storage.remove_tree_step(folder, false, place) {
            Ok(step) if step.done => return Ok(()),
            Ok(step) => place = step.next,
            Err(StorageError::NotFound) => return Ok(()),
            Err(error) => return Err(status(error)),
        }
    }
    Err(ERR_BUSY)
}

/// Makes the cache of `id` ready to be used by a new document: a folder that
/// is not finished is removed (unless `pinned`: an open document uses it, and
/// then it is shared as it is), and the book's older conversions go until
/// there is room for this one. Returns the folder.
///
/// # Errors
/// `ERR_BUSY` when a removal is not done yet, or no conversion can go (all
/// are pinned); `ERR_IO`, `ERR_NO_MEMORY`.
pub(crate) fn prepare(
    storage: &dyn Storage,
    id: &Identity<'_>,
    pinned: &dyn Fn(&str) -> bool,
) -> Result<String, i32> {
    let folder = id.folder()?;
    if pinned(&folder) {
        return Ok(folder);
    }
    if folder_exists(storage, &folder) && !is_complete(storage, id)? {
        clear(storage, &folder)?;
    }
    make_room(storage, id, &folder, pinned)?;
    Ok(folder)
}

/// A conversion of the book on the card: its folder and when it was changed.
struct Found {
    folder: String,
    changed: i64,
}

/// The book's conversions but `own`, oldest first.
fn others(storage: &dyn Storage, id: &Identity<'_>, own: &str) -> Result<Vec<Found>, i32> {
    let mut book = made(ROOT.len() + 1 + id.book.len())?;
    book.push_str(ROOT);
    book.push('/');
    book.push_str(id.book);
    let mut plugins: Vec<String> = Vec::new();
    let mut failed = None;
    let listed = storage.list(&book, &mut |entry| {
        if entry.is_dir && failed.is_none() {
            failed = push_name(&mut plugins, entry.name).err();
        }
    });
    match listed {
        Ok(()) => {}
        Err(StorageError::NotFound) => return Ok(Vec::new()),
        Err(error) => return Err(status(error)),
    }
    if let Some(error) = failed {
        return Err(error);
    }
    let mut found: Vec<Found> = Vec::new();
    for plugin in &plugins {
        let mut dir = made(book.len() + 1 + plugin.len())?;
        dir.push_str(&book);
        dir.push('/');
        dir.push_str(plugin);
        let mut names: Vec<(String, i64)> = Vec::new();
        let mut failed = None;
        storage
            .list(&dir, &mut |entry| {
                if entry.is_dir && failed.is_none() {
                    failed = push_changed(&mut names, entry.name, entry.modified).err();
                }
            })
            .map_err(status)?;
        if let Some(error) = failed {
            return Err(error);
        }
        found.try_reserve(names.len()).map_err(|_| ERR_NO_MEMORY)?;
        for (name, changed) in names {
            let mut folder = made(dir.len() + 1 + name.len())?;
            folder.push_str(&dir);
            folder.push('/');
            folder.push_str(&name);
            if folder != own {
                found.push(Found { folder, changed });
            }
        }
    }
    found.sort_by_key(|f| f.changed);
    Ok(found)
}

fn push_name(into: &mut Vec<String>, name: &str) -> Result<(), i32> {
    into.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
    let mut copy = made(name.len())?;
    copy.push_str(name);
    into.push(copy);
    Ok(())
}

fn push_changed(
    into: &mut Vec<(String, i64)>,
    name: &str,
    changed: Option<i64>,
) -> Result<(), i32> {
    into.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
    let mut copy = made(name.len())?;
    copy.push_str(name);
    into.push((copy, changed.unwrap_or(0)));
    Ok(())
}

/// Removes the book's oldest conversions that no open document uses until
/// `own` has room among [`MAX_CONVERSIONS`].
fn make_room(
    storage: &dyn Storage,
    id: &Identity<'_>,
    own: &str,
    pinned: &dyn Fn(&str) -> bool,
) -> Result<(), i32> {
    let mut found = others(storage, id, own)?;
    while found.len() + 1 > MAX_CONVERSIONS {
        // Oldest first; a conversion an open document uses stays.
        let at = found
            .iter()
            .position(|f| !pinned(&f.folder))
            .ok_or(ERR_BUSY)?;
        clear(storage, &found[at].folder)?;
        found.remove(at);
    }
    Ok(())
}

/// The format answered done: syncs the card and makes the stamp of `id`
/// (temp file, sync, rename, sync: a cut leaves the old stamp or none). Nothing
/// is stamped for a conversion that made no cache folder.
///
/// # Errors
/// `ERR_NO_SPACE` (card full), `ERR_IO`, `ERR_NO_MEMORY`.
pub(crate) fn complete(storage: &dyn Storage, id: &Identity<'_>) -> Result<(), i32> {
    let folder = id.folder()?;
    if !folder_exists(storage, &folder) {
        return Ok(());
    }
    let stamp = id.stamp()?;
    let mut temp = made(folder.len() + 2 + HOST.len() + STAMP.len())?;
    temp.push_str(&folder);
    temp.push('/');
    temp.push_str(HOST);
    let host = temp.clone();
    temp.push('/');
    temp.push_str(STAMP);
    let mut done = made(folder.len() + 1 + STAMP.len())?;
    done.push_str(&folder);
    done.push('/');
    done.push_str(STAMP);
    storage.sync().map_err(status)?;
    storage.create_dir(&host).map_err(status)?;
    storage.write(&temp, stamp.as_bytes()).map_err(status)?;
    storage.sync().map_err(status)?;
    storage.rename(&temp, &done).map_err(status)?;
    storage.sync().map_err(status)
}

#[cfg(test)]
#[path = "doc_cache_tests.rs"]
mod tests;
