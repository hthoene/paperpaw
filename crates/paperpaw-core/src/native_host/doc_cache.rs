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
//! no format sees a half-made or stale cache as finished. What a conversion
//! is made of includes the book's content (its fingerprint) and the CSS file's
//! ([`content`]), so another file at the same path is another cache. A book
//! keeps at most [`MAX_CONVERSIONS`]; the older goes, never one an open
//! document uses.
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
const STAMP_HEAD: &str = "paperpaw-cache 2";
/// The most of a CSS file whose bytes are hashed (its size and time are
/// always part of the key).
const CSS_HASHED: u64 = 64 * 1024;
/// The most entries of a folder book's top level the content line tells.
const LISTED: u32 = 256;

/// What one cache is for.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Identity<'a> {
    /// The book key (16 hexadecimal digits).
    pub(crate) book: &'a str,
    pub(crate) plugin: &'a str,
    pub(crate) version: &'a str,
    pub(crate) conversion: u64,
    /// What the book and the CSS file are ([`content`]).
    pub(crate) content: &'a str,
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

    /// What the stamp says of the conversion: the first lines of the stamp
    /// (the last line is what the cache holds, [`current`]).
    fn stamp(&self) -> Result<String, i32> {
        let mut digits = [0_u8; 16];
        let conversion = hex(self.conversion, &mut digits);
        let mut text = made(
            STAMP_HEAD.len()
                + self.plugin.len()
                + self.version.len()
                + conversion.len()
                + self.content.len()
                + 5
                + BYTES_ROOM,
        )?;
        for part in [
            STAMP_HEAD,
            self.plugin,
            self.version,
            conversion,
            self.content,
        ] {
            text.push_str(part);
            text.push('\n');
        }
        Ok(text)
    }
}

/// Room for the stamp's last line (a `u64` in decimal and its end).
const BYTES_ROOM: usize = 21;

fn decimal(value: u64, text: &mut String) {
    let mut digits = [0_u8; 20];
    let mut at = digits.len();
    let mut rest = value;
    loop {
        at -= 1;
        digits[at] = b'0' + (rest % 10) as u8;
        rest /= 10;
        if rest == 0 {
            break;
        }
    }
    for digit in &digits[at..] {
        text.push(char::from(*digit));
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

/// The bytes a finished cache holds, if `id`'s folder holds the stamp of
/// exactly `id`; `None` where it does not (no stamp, another conversion,
/// another book, CSS or version, a damaged stamp).
pub(crate) fn current(storage: &dyn Storage, id: &Identity<'_>) -> Result<Option<u64>, i32> {
    let folder = id.folder()?;
    let want = id.stamp()?;
    let mut path = made(folder.len() + 1 + STAMP.len())?;
    path.push_str(&folder);
    path.push('/');
    path.push_str(STAMP);
    // The stamp's lines of the conversion, then the bytes and a line end: a
    // longer file is another stamp.
    let mut buffer = Vec::new();
    buffer
        .try_reserve_exact(want.len() + BYTES_ROOM + 1)
        .map_err(|_| ERR_NO_MEMORY)?;
    buffer.resize(want.len() + BYTES_ROOM + 1, 0);
    let count = match storage.read_at(&path, 0, &mut buffer) {
        Ok(count) => count,
        Err(StorageError::NotFound) => return Ok(None),
        Err(error) => return Err(status(error)),
    };
    let Some(rest) = buffer[..count].strip_prefix(want.as_bytes()) else {
        return Ok(None);
    };
    let Some(digits) = rest.strip_suffix(b"\n") else {
        return Ok(None);
    };
    Ok(core::str::from_utf8(digits)
        .ok()
        .filter(|d| !d.is_empty() && d.len() <= 20 && d.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|d| d.parse().ok()))
}

/// Whether the cache of `id` is finished: [`current`].
#[cfg(test)]
pub(crate) fn is_complete(storage: &dyn Storage, id: &Identity<'_>) -> Result<bool, i32> {
    current(storage, id).map(|bytes| bytes.is_some())
}

/// What the book at `path` and the CSS file at `css` (card paths; `""` for
/// none) are, as one stamp line: the book's size and partial MD5 (a folder's
/// time, where it has no size), and the CSS file's size, time and the first
/// bytes' hash. A file that changed is another line.
///
/// # Errors
/// `ERR_NO_MEMORY`.
pub(crate) fn content(storage: &dyn Storage, path: &str, css: &str) -> Result<String, i32> {
    let mut text = made(128)?;
    match paperpaw_sdk::library::Fingerprint::of(storage, path) {
        Some(found) => {
            decimal(found.size, &mut text);
            text.push(' ');
            text.push_str(&found.md5);
        }
        // A folder book (or none): its top level, by names, sizes and times.
        None => match listed_hash(storage, path) {
            Some(hash) => {
                text.push('d');
                decimal(hash, &mut text);
            }
            None => text.push('-'),
        },
    }
    text.push(';');
    match storage.size(css) {
        Ok(size) if !css.is_empty() => {
            decimal(size, &mut text);
            text.push(' ');
            decimal(
                storage.modified(css).map_or(0, i64::unsigned_abs),
                &mut text,
            );
            text.push(' ');
            decimal(hash_of(storage, css, size.min(CSS_HASHED)), &mut text);
        }
        _ => text.push('-'),
    }
    Ok(text)
}

/// FNV-1a 64 of the first [`LISTED`] entries of the folder `path`: names,
/// kinds, sizes and times; `None` where it is not a folder.
fn listed_hash(storage: &dyn Storage, path: &str) -> Option<u64> {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    let mut count = 0_u32;
    storage
        .list(path, &mut |entry| {
            count += 1;
            if count > LISTED {
                return;
            }
            let numbers = [
                u64::from(entry.is_dir),
                entry.size,
                entry.modified.map_or(0, i64::unsigned_abs),
            ];
            for byte in entry
                .name
                .bytes()
                .chain(numbers.iter().flat_map(|n| n.to_le_bytes()))
                .chain([0xff])
            {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(0x0100_0000_01b3);
            }
        })
        .ok()?;
    Some(hash)
}

/// FNV-1a 64 of the first `len` bytes of the file (a short read ends it).
fn hash_of(storage: &dyn Storage, path: &str, len: u64) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    let mut chunk = [0_u8; 512];
    let mut at = 0;
    while at < len {
        let want = usize::try_from((len - at).min(512)).unwrap_or(512);
        let Ok(got) = storage.read_at(path, at, &mut chunk[..want]) else {
            break;
        };
        if got == 0 {
            break;
        }
        for byte in &chunk[..got] {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        at += got as u64;
    }
    hash
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

/// A cache made ready for a document: its folder, and what is known of it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Prepared {
    pub(crate) folder: String,
    /// The bytes it holds if it is finished (the stamp is current).
    pub(crate) bytes: Option<u64>,
    /// The folder was made new (a removed one, or none before): what is
    /// counted for it starts at 0.
    pub(crate) fresh: bool,
}

/// Makes the cache of `id` ready to be used by a new document: a folder that
/// is not finished is removed (unless `pinned`: an open document uses it, and
/// then it is shared as it is), and the book's older conversions go until
/// there is room for this one.
///
/// # Errors
/// `ERR_BUSY` when a removal is not done yet, or no conversion can go (all
/// are pinned); `ERR_IO`, `ERR_NO_MEMORY`.
pub(crate) fn prepare(
    storage: &dyn Storage,
    id: &Identity<'_>,
    pinned: &dyn Fn(&str) -> bool,
) -> Result<Prepared, i32> {
    let folder = id.folder()?;
    let bytes = current(storage, id)?;
    if pinned(&folder) {
        return Ok(Prepared {
            folder,
            bytes,
            fresh: false,
        });
    }
    let mut fresh = bytes.is_none();
    if bytes.is_none() && folder_exists(storage, &folder) {
        clear(storage, &folder)?;
        fresh = true;
    }
    make_room(storage, id, &folder, pinned)?;
    Ok(Prepared {
        folder,
        bytes,
        fresh,
    })
}

/// A conversion of the book on the card: its folder and when it was changed.
struct Found {
    folder: String,
    changed: i64,
    /// It has its stamp (a finished conversion).
    stamped: bool,
}

/// Whether `folder` holds its stamp.
fn has_stamp(storage: &dyn Storage, folder: &str) -> Result<bool, i32> {
    let mut path = made(folder.len() + 1 + STAMP.len())?;
    path.push_str(folder);
    path.push('/');
    path.push_str(STAMP);
    Ok(storage.size(&path).is_ok())
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
                let stamped = has_stamp(storage, &folder)?;
                found.push(Found {
                    folder,
                    changed,
                    stamped,
                });
            }
        }
    }
    // Oldest first by the folder's own time: on a FAT card that is when it
    // was made, which is when its conversion began.
    oldest_first(&mut found);
    Ok(found)
}

/// Orders `found` oldest first, keeping the order of equal times (a stable
/// sort). By hand: the standard library's stable sort keeps 4 KiB of scratch
/// on the stack, which the firmware's frame limit for the host's code (2,048
/// bytes) does not allow.
fn oldest_first(found: &mut [Found]) {
    for at in 1..found.len() {
        let mut to = at;
        while to > 0 && found[to - 1].changed > found[to].changed {
            found.swap(to - 1, to);
            to -= 1;
        }
    }
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
        // Oldest first, unstamped ones (a `details` open's cover file, a
        // half-made conversion) before finished ones; a conversion an open
        // document uses stays.
        let at = found
            .iter()
            .position(|f| !f.stamped && !pinned(&f.folder))
            .or_else(|| found.iter().position(|f| !pinned(&f.folder)))
            .ok_or(ERR_BUSY)?;
        clear(storage, &found[at].folder)?;
        found.remove(at);
    }
    Ok(())
}

/// The format answered done: syncs the card and makes the stamp of `id`,
/// which says the cache holds `bytes` (temp file, sync, rename, sync: a cut
/// leaves the old stamp or none). Nothing is stamped for a conversion that
/// made no cache folder.
///
/// # Errors
/// `ERR_NO_SPACE` (card full), `ERR_IO`, `ERR_NO_MEMORY`.
pub(crate) fn complete(storage: &dyn Storage, id: &Identity<'_>, bytes: u64) -> Result<(), i32> {
    let folder = id.folder()?;
    if !folder_exists(storage, &folder) {
        return Ok(());
    }
    let mut stamp = id.stamp()?;
    decimal(bytes, &mut stamp);
    stamp.push('\n');
    let mut host = made(folder.len() + 1 + HOST.len())?;
    host.push_str(&folder);
    host.push('/');
    host.push_str(HOST);
    let mut temp = made(host.len() + 1 + STAMP.len())?;
    temp.push_str(&host);
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
