//! PaperPaw's storage on a FAT file system (FAT12, FAT16, FAT32) with long
//! file names, e.g. the X4's SD card: [`FatStorage`] over any disk that
//! reads, writes and seeks in bytes (see [`fatfs::ReadWriteSeek`]), such
//! as a [`BlockDisk`] over an SD card's blocks.
//!
//! The [`OPEN_FILES`] files read last stay open, so reading a book a piece
//! at a time, often from two files by turns (an index and its text), does
//! not look them up again for every piece. Files appended to stay open
//! the same way, written back only when they are wanted otherwise (listed,
//! renamed, removed, replaced), when they make room for another file, or on
//! [`Storage::sync`]: a picture decoded into a file in hundreds of pieces
//! is not looked up and written back hundreds of times.
//!
//! The [`FOLDERS`] folders looked up last are kept too: a file opened in
//! one is looked up there, not by walking every folder from the root again
//! (a book's data lies five folders deep, beside those of all other books).
//! Open files and kept folders are found by their paths as FAT names
//! them, without regard to case: a file is open once, however its path
//! is spelt.
//!
//! A [`BlockDisk`] keeps changed blocks too (folders, the FAT) and writes
//! them back in the order they were changed, all of them before a file's
//! data that goes past them: cut off at any point (an empty battery), the
//! card holds the changes up to one of them, none after. Shared with the
//! storage ([`SharedDisk`], [`FatStorage::with_disk`]), it is written
//! back when a call that changes the card returns (but
//! [`Storage::append`]) and on `sync`.
//!
//! Files are written through files beside them named after them
//! (`Temps`): the new content is written whole under one name (`.TMP`)
//! and synced. A new file then takes its name; a file there is replaced:
//! the content is named whole (`.NEW`), then takes the place of the old
//! file (rust-fatfs's `Dir::replace`, as patched for PaperPaw, writes the
//! new entry before it removes the old one and that of `.NEW`, and frees
//! the old clusters last). Cut off at any point, a file reads its old
//! content or its new one, whole, and is listed. What a cut left of a
//! file's write is finished (`settled`) when its folder is first looked
//! at after a start (`settle_folder`), before any file in it is opened:
//! `.NEW` takes the file's place, unless the file was changed after it
//! was made, elsewhere or not: `.NEW` is marked with the file's time,
//! size and first cluster as they were (not told by times alone, as the
//! X4 sets none). New content an earlier version left unmarked is set
//! aside as `<name>.unsaved`, the file kept: whether it changed since
//! cannot be told. A file that cannot be settled is left as it is, its
//! folder in reach; so are the paths of a move that cannot be settled,
//! held up until the next start (`held_up`).
//! The next write, move or removal of a file does the same, so at most
//! one of each is ever left beside it. They are neither listed nor taken
//! by name: one may hold the clusters of the file itself. rust-fatfs, as
//! patched, removes a file's entry before it frees its clusters: cut off
//! between, they stay taken by no file, never a file pointing at free
//! ones. A file's size is its entry's, and reading it fails where its
//! clusters end before (left so by an earlier version).
//!
//! rust-fatfs, as patched, moves an entry by adding the new one before
//! it removes the old one: cut off between, the entry is at both
//! places, its clusters held by both, never at neither, and a folder
//! that cannot grow for it (a full card) fails the move before anything
//! is changed. Where two entries share clusters, one is taken away
//! without freeing them (`unlink`), as a file's temporary file beside
//! its new content (`settled`). A move but a file's to another case of
//! its name is recorded first, in a hidden file of the root
//! ([`JOURNAL`]), and settled after a start before anything else
//! (`settle_moves`): its new place kept, the old entry taken away.
//!
//! Earlier versions wrote files through temporary files of their own
//! (`~pp<n>.tmp`). Such a file may be the only copy of a file whose
//! name is lost, so it is taken away only where it is provably no
//! file's only copy (`clean_old`). Those that stay never stand in the
//! way: none are made any more, and at most 64 were made in a folder.
//!
//! A name stands for an entry by its long name where it has one, by its
//! short name only where it has none: `PAPERP~1`, the short name FAT
//! keeps beside `.paperpaw`, does not reach PaperPaw's data. rust-fatfs
//! takes either, so names are looked up here (`lookup`) and handed to it
//! only where it cannot take another entry for them.

// What goes wrong with the user's data is told, never dropped unseen:
// a result left unread says why (see CONTRIBUTING.md).
#![deny(clippy::let_underscore_must_use)]
#![no_std]

extern crate alloc;

mod disk;
mod exfat;
mod free;
mod settle;

pub use disk::{BLOCK, BlockDisk, Blocks, DiskError, Kind, SharedDisk, VolumeError};
pub use exfat::{ExDisk, ExFatStorage};

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};

use fatfs::{
    DefaultTimeProvider, Dir, File, FileAttributes, FileSystem, LossyOemCpConverter, Read,
    ReadWriteSeek, Seek, SeekFrom, Write,
};
use paperpaw_platform::storage::is_valid_path;
use paperpaw_platform::{DirEntry, Storage, StorageError};

pub use fatfs;

type FatFile<IO> = File<'static, IO, DefaultTimeProvider, LossyOemCpConverter>;

/// Files kept open (each takes about a hundred bytes).
pub const OPEN_FILES: usize = 16;
/// Folders kept (each takes about a hundred bytes).
pub const FOLDERS: usize = 8;
type FatDir<IO> = Dir<'static, IO, DefaultTimeProvider, LossyOemCpConverter>;
type FatEntry<IO> = fatfs::DirEntry<'static, IO, DefaultTimeProvider, LossyOemCpConverter>;

/// A file kept open.
struct Open<IO: ReadWriteSeek + 'static> {
    path: String,
    file: FatFile<IO>,
    /// Its size: its entry's, as appended. Its clusters may end before
    /// (cut off in a remove by an earlier version, see `read_at`).
    len: u64,
    /// Appended to since it was last written back.
    dirty: bool,
}

impl<IO: ReadWriteSeek + 'static> Open<IO> {
    /// Writes back what was appended (its size in its folder, the disk's
    /// blocks).
    fn flush(&mut self) -> Result<(), StorageError> {
        if self.dirty {
            self.dirty = false;
            self.file.flush().map_err(error)?;
        }
        Ok(())
    }
}

/// PaperPaw's storage on a FAT file system that lives as long as the
/// device runs (`Box::leak` it).
pub struct FatStorage<IO: ReadWriteSeek + 'static> {
    fs: &'static FileSystem<IO>,
    /// The files used last, the latest last.
    open: RefCell<alloc::vec::Vec<Open<IO>>>,
    /// The folders looked up last, the latest last, by their paths.
    folders: RefCell<alloc::vec::Vec<(String, FatDir<IO>)>>,
    /// Writes back the disk's changed blocks, where the disk is shared.
    flush_disk: Option<alloc::boxed::Box<dyn Fn() -> bool>>,
    /// Reads the volume's bytes, where the disk is shared: to count its
    /// free space a part at a time ([`free`]).
    read: Option<alloc::boxed::Box<free::Read>>,
    free: RefCell<free::Free>,
    /// A move recorded in [`JOURNAL`] was looked for since the start
    /// (`settle_moves`).
    moves_settled: Cell<bool>,
    /// The folders settled since the start, by a hash of their paths
    /// (`settle_folder`); none while a move is settled.
    settled_folders: RefCell<Vec<u64>>,
    settling: Cell<bool>,
    /// The paths of a move that could not be settled: held up until the
    /// next start (see `held_up`).
    stuck: RefCell<Option<(String, String)>>,
    /// The folders whose temporary files of earlier versions were looked
    /// at since the start, by a hash of their paths (`clean_old`).
    cleaned: RefCell<Vec<u64>>,
}

impl<IO: ReadWriteSeek + 'static> core::fmt::Debug for FatStorage<IO> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("FatStorage").finish_non_exhaustive()
    }
}

fn error<E>(e: fatfs::Error<E>) -> StorageError {
    match e {
        fatfs::Error::NotFound => StorageError::NotFound,
        fatfs::Error::NotEnoughSpace => StorageError::Full,
        fatfs::Error::InvalidInput
        | fatfs::Error::InvalidFileNameLength
        | fatfs::Error::UnsupportedFileNameCharacter => StorageError::InvalidPath,
        _ => StorageError::Io,
    }
}

/// Refuses a path PaperPaw does not take, or one of the storage's own:
/// [`JOURNAL`], or a file a write goes through ([`is_temp_name`]), which
/// may hold the clusters of the file itself (cut off in a write).
fn check(path: &str) -> Result<(), StorageError> {
    let own = same_path(path, JOURNAL) || path.split('/').any(is_temp_name);
    if is_valid_path(path) && !own {
        Ok(())
    } else {
        Err(StorageError::InvalidPath)
    }
}

/// The folder of `path` (`""` for the root).
fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// The name of `path` in its folder.
fn name(path: &str) -> &str {
    path.rsplit_once('/').map_or(path, |(_, name)| name)
}

/// Whether `stored` is `name` to FAT: without regard to case, compared as
/// rust-fatfs compares names (in upper case, letter by letter).
fn same_name(stored: impl Iterator<Item = char>, name: &str) -> bool {
    stored
        .flat_map(char::to_uppercase)
        .eq(name.chars().flat_map(char::to_uppercase))
}

/// Whether `path` is folder `dir` or lies in it, at any depth, to FAT:
/// name by name, without regard to case.
fn is_within(path: &str, dir: &str) -> bool {
    let mut parts = path.split('/');
    dir.split('/')
        .all(|d| parts.next().is_some_and(|p| same_name(p.chars(), d)))
}

/// Whether paths `a` and `b` name the same entry to FAT: name by name,
/// without regard to case. Quick where both are ASCII, as most are.
fn same_path(a: &str, b: &str) -> bool {
    if a.is_ascii() && b.is_ascii() {
        a.eq_ignore_ascii_case(b)
    } else {
        a.split('/').count() == b.split('/').count() && is_within(a, b)
    }
}

/// Whether `entry`'s long name is `name`; `None` where it has none.
fn long_name_is<IO: ReadWriteSeek + 'static>(entry: &FatEntry<IO>, name: &str) -> Option<bool> {
    let units = entry.long_file_name_as_ucs2_units()?;
    let long =
        char::decode_utf16(units.iter().copied()).map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER));
    Some(same_name(long, name))
}

/// Whether `entry`'s short name is `name`, its letters read as
/// [`LossyOemCpConverter`] reads them.
fn short_name_is<IO: ReadWriteSeek + 'static>(entry: &FatEntry<IO>, name: &str) -> bool {
    let short = entry.short_file_name_as_bytes().iter().map(|&b| {
        if b.is_ascii() {
            char::from(b)
        } else {
            char::REPLACEMENT_CHARACTER
        }
    });
    same_name(short, name)
}

/// Most entries a folder holds (FAT's limit). Read on past them, a
/// folder whose clusters run in a loop (a damaged card) would never end.
/// Not where the loop holds removed entries only: rust-fatfs reads past
/// those itself, giving none.
const ENTRIES: usize = 65_536;
/// The same, for exFAT's folders (which may hold more, up to 256 MiB of
/// entries; a library has far fewer).
const ENTRIES_MAX: usize = 1 << 20;

/// `items`, failing past [`ENTRIES`].
fn bounded<T>(
    items: impl Iterator<Item = Result<T, StorageError>>,
) -> impl Iterator<Item = Result<T, StorageError>> {
    items.enumerate().map(|(i, item)| match i {
        ..ENTRIES => item,
        _ => Err(StorageError::Io),
    })
}

/// The entries of `dir`, failing past [`ENTRIES`].
fn entries<IO: ReadWriteSeek + 'static>(
    dir: &FatDir<IO>,
) -> impl Iterator<Item = Result<FatEntry<IO>, StorageError>> {
    bounded(dir.iter().map(|entry| entry.map_err(error)))
}

/// The names a file is written under before it takes its place (see the
/// [crate documentation](crate)), made from its name: `~` and seven
/// letters of a hash of it, as FAT compares names. Short names of their
/// own, so they are found by them where their long names are lost, and
/// no other name stands for them.
struct Temps {
    /// While it is written.
    tmp: [u8; 12],
    /// Once it is whole.
    ready: [u8; 12],
}

/// FNV-1a over `bytes`.
fn fnv(bytes: impl Iterator<Item = u8>) -> u64 {
    bytes.fold(0xcbf2_9ce4_8422_2325, |hash, b| {
        (hash ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// A hash of `name` as FAT compares names: over it in upper case.
fn hash(name: &str) -> u64 {
    fnv(name.chars().flat_map(char::to_uppercase).flat_map(|c| {
        let mut utf8 = [0u8; 4];
        let len = c.encode_utf8(&mut utf8).len();
        utf8.into_iter().take(len)
    }))
}

impl Temps {
    fn of(name: &str) -> Self {
        let mut hash = hash(name);
        let mut tmp = *b"~0000000.TMP";
        for letter in &mut tmp[1..8] {
            *letter = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567"[(hash & 31) as usize];
            hash >>= 5;
        }
        let mut ready = tmp;
        ready[9..].copy_from_slice(b"NEW");
        Self { tmp, ready }
    }

    fn tmp(&self) -> &str {
        core::str::from_utf8(&self.tmp).unwrap_or_default()
    }

    fn ready(&self) -> &str {
        core::str::from_utf8(&self.ready).unwrap_or_default()
    }
}

/// What a name stands for in a folder.
struct Lookup<IO: ReadWriteSeek + 'static> {
    /// The entry it names: by its long name, or by its short name where
    /// it has no long one.
    entry: Option<FatEntry<IO>>,
    /// rust-fatfs could take another entry for the name (one whose short
    /// name it is, as `PAPERP~1` for `.paperpaw`): the name is not handed
    /// to rust-fatfs then.
    others: bool,
    /// Another entry has its long name too: the old file and the new one,
    /// a replace cut off between (see `settled`).
    twice: bool,
    /// Its file of [`Temps::tmp`], where looked for.
    tmp: Option<FatEntry<IO>>,
    /// Its file of [`Temps::ready`], where looked for.
    ready: Option<FatEntry<IO>>,
    /// A temporary file of an earlier version is in the folder, where
    /// looked for (see [`is_old_temp`]).
    old: bool,
}

/// Looks up `name` in `dir`, as PaperPaw names entries (see the [crate
/// documentation](crate)): every entry, where the name goes to rust-fatfs
/// after (`whole`), else up to the one it names.
fn lookup<IO: ReadWriteSeek + 'static>(
    dir: &FatDir<IO>,
    name: &str,
    whole: bool,
) -> Result<Lookup<IO>, StorageError> {
    lookup_with(dir, name, whole, None)
}

/// [`lookup`], looking for the files of `temps` on the way (all of them
/// where `name` is not there, or `whole`).
fn lookup_with<IO: ReadWriteSeek + 'static>(
    dir: &FatDir<IO>,
    name: &str,
    whole: bool,
    temps: Option<&Temps>,
) -> Result<Lookup<IO>, StorageError> {
    let mut found = Lookup {
        entry: None,
        others: false,
        twice: false,
        tmp: None,
        ready: None,
        old: false,
    };
    for entry in entries(dir) {
        let entry = entry?;
        let by_short = short_name_is(&entry, name);
        let (named, other) = match long_name_is(&entry, name) {
            Some(by_long) => (by_long, by_short),
            None => (by_short, false),
        };
        if named && found.entry.is_none() {
            found.entry = Some(entry);
            if !whole {
                break;
            }
            continue;
        } else if named {
            found.twice = true;
        } else if other {
            found.others = true;
        }
        let Some(temps) = temps.filter(|_| entry.is_file()) else {
            continue;
        };
        // By its short name too: a temporary's long name, if lost, was
        // the same.
        let is = |temp| long_name_is(&entry, temp) == Some(true) || short_name_is(&entry, temp);
        if is(temps.tmp()) && found.tmp.is_none() {
            found.tmp = Some(entry);
        } else if is(temps.ready()) && found.ready.is_none() {
            found.ready = Some(entry);
        } else {
            found.old |= is_old_temp(&entry);
        }
    }
    Ok(found)
}

/// Folder `name` in `dir`.
fn open_dir<IO: ReadWriteSeek + 'static>(
    dir: &FatDir<IO>,
    name: &str,
) -> Result<FatDir<IO>, StorageError> {
    match lookup(dir, name, false)?.entry {
        Some(entry) if entry.is_dir() => Ok(entry.to_dir()),
        Some(_) => Err(StorageError::InvalidPath),
        None => Err(StorageError::NotFound),
    }
}

/// File `name` in `dir` with its size (its entry's), made if it is not
/// there and `create`; not where `name` is only another entry's short
/// name then: rust-fatfs would open that entry. Where it is not there
/// but its new content is whole beside it (a write cut off before it
/// took its place), that takes its place first.
fn find_file<IO: ReadWriteSeek + 'static>(
    dir: &FatDir<IO>,
    name: &str,
    create: bool,
) -> Result<(FatFile<IO>, u64), StorageError> {
    let temps = Temps::of(name);
    let mut found = lookup_with(dir, name, create, Some(&temps))?;
    if found.entry.is_none() && found.ready.is_some() && !found.others {
        dir.replace(temps.ready(), name).map_err(error)?;
        found = lookup(dir, name, create)?;
    }
    match found {
        Lookup {
            entry: Some(entry), ..
        } if entry.is_file() => {
            let len = entry.len();
            Ok((entry.to_file(), len))
        }
        Lookup { entry: Some(_), .. } => Err(StorageError::InvalidPath),
        Lookup { others: true, .. } if create => Err(StorageError::InvalidPath),
        Lookup { .. } if create => Ok((dir.create_file(name).map_err(error)?, 0)),
        Lookup { .. } => Err(StorageError::NotFound),
    }
}

/// Finishes in `dir` a write of file `name` cut off before (see the
/// [crate documentation](crate)): its new content, whole, takes its
/// place, or is taken away where it is already there (the same file);
/// its temporary file, not yet whole, is removed. What `name` stands
/// for after.
fn settled<IO: ReadWriteSeek + 'static>(
    dir: &FatDir<IO>,
    name: &str,
) -> Result<Lookup<IO>, StorageError> {
    let temps = Temps::of(name);
    let found = lookup_with(dir, name, true, Some(&temps))?;
    if let Some(tmp) = &found.tmp {
        // Renamed whole, cut off before its own entry went: its clusters
        // are those of its new content (or of the file it replaced).
        let held = [&found.ready, &found.entry]
            .into_iter()
            .flatten()
            .any(|other| shares(tmp, other));
        match held {
            true => dir.unlink(temps.tmp()),
            false => dir.remove(temps.tmp()),
        }
        .map_err(error)?;
    }
    // Not where rust-fatfs could take another entry for `name` (no
    // write ever began there then), nor over a folder.
    let folder = found.entry.as_ref().is_some_and(FatEntry::is_dir);
    if let Some(ready) = found.ready.as_ref().filter(|_| !found.others && !folder) {
        // The file changed since its new content was made, elsewhere or
        // by PaperPaw (not as that is marked, see `state`): that is let
        // go. Not by times: the X4 sets none, so any file a computer
        // wrote is newer. Unmarked (left by an earlier version), whether
        // the file changed cannot be told: the file stays, and the new
        // content is set aside under a name of its own.
        let other = !found.twice && found.entry.as_ref().is_some_and(|e| !shares(e, ready));
        let mark = ready.mark();
        let changed = other
            && found
                .entry
                .as_ref()
                .is_some_and(|entry| mark.is_some_and(|mark| mark != state(entry)));
        if other && mark.is_none() {
            if let Some(aside) = aside_name(dir, name)? {
                dir.rename(temps.ready(), dir, &aside).map_err(error)?;
            }
        } else {
            match changed {
                true => dir.remove(temps.ready()),
                false => dir.replace(temps.ready(), name),
            }
            .map_err(error)?;
        }
    }
    if found.tmp.is_some() || found.ready.is_some() {
        let mut found_now = lookup(dir, name, true)?;
        found_now.old = found.old;
        Ok(found_now)
    } else {
        Ok(found)
    }
}

/// A free name beside file `name` for new content set aside: `<name>.unsaved`,
/// else with a number; `None` where none of a few is free (it stays
/// hidden beside the file then).
fn aside_name<IO: ReadWriteSeek + 'static>(
    dir: &FatDir<IO>,
    name: &str,
) -> Result<Option<String>, StorageError> {
    for n in 1..10 {
        let aside = match n {
            1 => format!("{name}.unsaved"),
            n => format!("{name}.unsaved{n}"),
        };
        if lookup(dir, &aside, false)?.entry.is_none() {
            return Ok(Some(aside));
        }
    }
    Ok(None)
}

/// A FAT date and time (the device's local time) in seconds since
/// 1970-01-01, read as UTC.
fn seconds(at: fatfs::DateTime) -> i64 {
    let (year, month, day) = (
        i64::from(at.date.year),
        i64::from(at.date.month),
        i64::from(at.date.day),
    );
    // Howard Hinnant's days from civil.
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    days * 86_400
        + i64::from(at.time.hour) * 3600
        + i64::from(at.time.min) * 60
        + i64::from(at.time.sec)
}

/// What tells whether file `entry` was changed: a hash of its time,
/// size and first cluster. The new content of a file is marked with it
/// ([`fatfs::File::set_mark`]): where the file is no longer so after a
/// cut, it was changed since and is kept (see `settled`).
fn state<IO: ReadWriteSeek + 'static>(entry: &FatEntry<IO>) -> u32 {
    let fatfs::DateTime { date, time, .. } = entry.modified();
    let cluster = entry.first_cluster().unwrap_or(0);
    let words = [
        date.year, date.month, date.day, time.hour, time.min, time.sec,
    ];
    let bytes = words
        .into_iter()
        .flat_map(u16::to_le_bytes)
        .chain(cluster.to_le_bytes())
        .chain(entry.len().to_le_bytes());
    fnv(bytes) as u32
}

/// Whether `name` is one of a file a write goes through ([`Temps`]):
/// `~`, seven of its letters, `.TMP` or `.NEW`, without regard to case.
fn is_temp_name(name: &str) -> bool {
    let name = name.as_bytes();
    let letter = |b: &u8| matches!(b.to_ascii_uppercase(), b'A'..=b'Z' | b'2'..=b'7');
    name.len() == 12
        && name[0] == b'~'
        && name[1..8].iter().all(letter)
        && (name[8..].eq_ignore_ascii_case(b".TMP") || name[8..].eq_ignore_ascii_case(b".NEW"))
}

/// Whether entries `a` and `b` hold the same clusters (a move or a
/// rename cut off between): never freed for one while the other stays.
fn shares<IO: ReadWriteSeek + 'static>(a: &FatEntry<IO>, b: &FatEntry<IO>) -> bool {
    a.first_cluster().is_some() && a.first_cluster() == b.first_cluster()
}

/// Whether entry `b` is entry `a` at another place, as a move cut off
/// between leaves it: of one kind, holding the same clusters, or both
/// files that hold none (empty, the same content).
fn holds_same<IO: ReadWriteSeek + 'static>(a: &FatEntry<IO>, b: &FatEntry<IO>) -> bool {
    let empty = |e: &FatEntry<IO>| e.is_file() && e.first_cluster().is_none() && e.len() == 0;
    a.is_dir() == b.is_dir() && (shares(a, b) || (empty(a) && empty(b)))
}

/// Whether `entry` is a temporary file of an earlier version: named
/// `~pp<n>.tmp` (`n` a number below 64, as they were) by its short name
/// and by its long name, where it has one.
fn is_old_temp<IO: ReadWriteSeek + 'static>(entry: &FatEntry<IO>) -> bool {
    let short = entry
        .short_file_name_as_bytes()
        .iter()
        .map(|&b| char::from(b));
    let long = entry.long_file_name_as_ucs2_units().map(|units| {
        char::decode_utf16(units.iter().copied()).map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER))
    });
    entry.is_file() && is_old_temp_name(short) && long.is_none_or(is_old_temp_name)
}

/// Whether `name` is `~pp<n>.tmp`, `n` a number below 64, without regard
/// to case.
fn is_old_temp_name(name: impl Iterator<Item = char>) -> bool {
    let mut letters = [0u8; 9];
    let mut len = 0;
    for c in name {
        if len == letters.len() || !c.is_ascii() {
            return false;
        }
        letters[len] = c.to_ascii_uppercase() as u8;
        len += 1;
    }
    let number = letters[..len]
        .strip_prefix(b"~PP")
        .and_then(|rest| rest.strip_suffix(b".TMP"));
    match number {
        Some([d]) => d.is_ascii_digit(),
        Some([t @ b'1'..=b'6', d]) => d.is_ascii_digit() && (t - b'0') * 10 + (d - b'0') < 64,
        _ => false,
    }
}

/// Folder `name` in `dir`, made if it is not there (as [`find_file`]
/// makes a file).
fn create_dir_in<IO: ReadWriteSeek + 'static>(
    dir: &FatDir<IO>,
    name: &str,
) -> Result<FatDir<IO>, StorageError> {
    match lookup(dir, name, true)? {
        Lookup {
            entry: Some(entry), ..
        } if entry.is_dir() => Ok(entry.to_dir()),
        Lookup { entry: Some(_), .. } | Lookup { others: true, .. } => {
            Err(StorageError::InvalidPath)
        }
        Lookup { .. } => dir.create_dir(name).map_err(error),
    }
}

/// Removes `name` from `dir` (a file, or an empty folder); nothing to do
/// where it is not there.
fn remove_in<IO: ReadWriteSeek + 'static>(
    dir: &FatDir<IO>,
    name: &str,
) -> Result<(), StorageError> {
    remove_found(dir, name, lookup(dir, name, true)?)
}

/// [`remove_in`], `name` looked up whole (`found`).
fn remove_found<IO: ReadWriteSeek + 'static>(
    dir: &FatDir<IO>,
    name: &str,
    found: Lookup<IO>,
) -> Result<(), StorageError> {
    match found {
        Lookup { entry: None, .. } => Ok(()),
        Lookup { others: true, .. } => Err(StorageError::InvalidPath),
        Lookup { .. } => match dir.remove(name) {
            Ok(()) | Err(fatfs::Error::NotFound) => Ok(()),
            Err(e) => Err(error(e)),
        },
    }
}

/// The file in the root that records a move while it is made (see the
/// [crate documentation](crate)). Hidden, and written in its place once
/// made, so recording takes no cluster: `PPMV`, the length of what
/// follows and its hash (`fnv`'s lower half) as 4 bytes each, then the
/// paths moved from and to, a line each. Cleared, it begins with zeros.
pub const JOURNAL: &str = "~PPMOVE.BIN";

/// Folders whose temporary files of earlier versions are known to have
/// been looked at (see `clean_old`).
const CLEANED: usize = 64;
/// Folders known to be settled (see `settle_folder`).
const SETTLED: usize = 128;

/// `name` in folder `dir` (`""` for the root).
fn child(dir: &str, name: &str) -> String {
    match dir {
        "" => name.to_string(),
        _ => format!("{dir}/{name}"),
    }
}

/// Moves `name` of `dir` to `to_name` in `to`, where nothing is.
fn rename_in<IO: ReadWriteSeek + 'static>(
    dir: &FatDir<IO>,
    name: &str,
    to: &FatDir<IO>,
    to_name: &str,
) -> Result<(), StorageError> {
    match lookup(dir, name, true)? {
        Lookup { entry: None, .. } => return Err(StorageError::NotFound),
        Lookup { others: true, .. } => return Err(StorageError::InvalidPath),
        Lookup { .. } => {}
    }
    match lookup(to, to_name, true)? {
        Lookup {
            entry: None,
            others: false,
            ..
        } => dir.rename(name, to, to_name).map_err(error),
        Lookup { .. } => Err(StorageError::InvalidPath),
    }
}

impl<IO: ReadWriteSeek + 'static> FatStorage<IO> {
    pub fn new(fs: &'static FileSystem<IO>) -> Self {
        Self {
            fs,
            open: RefCell::new(alloc::vec::Vec::new()),
            folders: RefCell::new(alloc::vec::Vec::new()),
            flush_disk: None,
            read: None,
            free: RefCell::default(),
            moves_settled: Cell::new(false),
            settled_folders: RefCell::new(Vec::new()),
            settling: Cell::new(false),
            stuck: RefCell::new(None),
            cleaned: RefCell::new(Vec::new()),
        }
    }

    /// The same, with the file system's disk shared ([`SharedDisk`]): its
    /// changed blocks are written back when a call that changes the card
    /// returns (but [`Storage::append`]) and on [`Storage::sync`].
    pub fn with_disk<B: Blocks + 'static>(mut self, disk: SharedDisk<B>) -> Self {
        let reader = disk.clone();
        self.read = Some(alloc::boxed::Box::new(move |at, buf: &mut [u8]| {
            // Where the file system was, kept: it seeks before it reads,
            // but nothing of its own is moved.
            let mut disk = reader.clone();
            let Ok(was) = disk.seek(SeekFrom::Current(0)) else {
                return false;
            };
            let read = disk.seek(SeekFrom::Start(at)).is_ok() && disk.read_exact(buf).is_ok();
            disk.seek(SeekFrom::Start(was)).is_ok() && read
        }));
        self.flush_disk = Some(alloc::boxed::Box::new(move || disk.flush_all().is_ok()));
        self
    }

    /// Follows a change of the free space: `taken` bytes in files written,
    /// `freed` in files removed (each in whole clusters).
    fn follow(&self, taken: u64, freed: u64) {
        if self.read.is_some() {
            let mut free = self.free.borrow_mut();
            let (taken, freed) = (free.clusters(taken), free.clusters(freed));
            free.changed(taken, freed);
        }
    }

    /// The bytes of the file at `path` (0 where there is none), where the
    /// free space is followed.
    fn len_of(&self, path: &str) -> u64 {
        if self.read.is_none() {
            return 0;
        }
        Storage::size(self, path).unwrap_or(0)
    }

    /// The clusters the files and folders below `path` take, as bytes in
    /// whole clusters, where the free space is followed.
    fn tree_len(&self, path: &str) -> u64 {
        if self.read.is_none() {
            return 0;
        }
        let mut taken = 0;
        let mut dirs = alloc::vec![String::from(path)];
        while let Some(dir) = dirs.pop() {
            taken += self.free.borrow().clusters(1) * self.cluster();
            #[expect(
                clippy::let_underscore_must_use,
                reason = "a folder that cannot be listed counts as empty"
            )]
            let _ = Storage::list(self, &dir, &mut |entry| {
                let path = format!("{dir}/{}", entry.name);
                if entry.is_dir {
                    dirs.push(path);
                } else {
                    taken += self.free.borrow().clusters(entry.size) * self.cluster();
                }
            });
        }
        taken
    }

    /// The bytes the folders of `dir` that are not there yet take once
    /// made (a cluster each), where the free space is followed.
    fn new_dirs(&self, dir: &str) -> u64 {
        if self.read.is_none() || dir.is_empty() {
            return 0;
        }
        // From the deepest up: a folder there has its parents (a kept
        // one is there), so a file's folders are not each looked for in
        // theirs again, some with hundreds of names.
        let mut made = 0;
        let mut path = dir;
        while !self.is_kept(path) && !self.is_there(path) {
            made += 1;
            match path.rfind('/') {
                Some(at) => path = &path[..at],
                None => break,
            }
        }
        made * self.cluster()
    }

    /// Whether folder `path` is kept (see [`FOLDERS`]): it is there.
    fn is_kept(&self, path: &str) -> bool {
        let path = path.trim_matches('/');
        self.folders
            .borrow()
            .iter()
            .any(|(p, _)| same_path(p, path))
    }

    /// Bytes of a cluster, where known.
    fn cluster(&self) -> u64 {
        let free = self.free.borrow();
        // One byte takes one cluster: its size is what `clusters` rounds to.
        if free.clusters(1) == 0 {
            return 0;
        }
        self.fs.cluster_size().into()
    }

    /// Whether a file or a folder is at `path`, looked up in its folder
    /// (no file opened).
    fn is_there(&self, path: &str) -> bool {
        self.dir(parent(path))
            .and_then(|dir| lookup(&dir, name(path), false))
            .is_ok_and(|found| found.entry.is_some())
    }

    fn root(&self) -> FatDir<IO> {
        self.fs.root_dir()
    }

    /// Folder `path`: kept from before, or looked up from the root and
    /// kept.
    fn dir(&self, path: &str) -> Result<FatDir<IO>, StorageError> {
        let path = path.trim_matches('/');
        if path.is_empty() {
            let root = self.root();
            self.settle_folder("", &root);
            return Ok(root);
        }
        {
            let mut folders = self.folders.borrow_mut();
            if let Some(i) = folders.iter().position(|(p, _)| same_path(p, path)) {
                // The latest last.
                let folder = folders.remove(i);
                let dir = folder.1.clone();
                folders.push(folder);
                return Ok(dir);
            }
        }
        // Looked up in its folder, kept too: the folders of many books'
        // data are not each looked up from the root.
        let dir = open_dir(&self.dir(parent(path))?, name(path))?;
        self.settle_folder(path, &dir);
        let mut folders = self.folders.borrow_mut();
        if folders.len() == FOLDERS {
            folders.remove(0);
        }
        folders.push((path.to_string(), dir.clone()));
        Ok(dir)
    }

    /// Forgets the folders kept at `path` or below it (removed, renamed),
    /// by any case of its names.
    fn forget(&self, path: &str) {
        let path = path.trim_matches('/');
        self.folders
            .borrow_mut()
            .retain(|(p, _)| !is_within(p, path));
    }

    /// Runs `f` with `path` open (created first with its folders if
    /// `create`), keeping it open for the next call.
    fn with_open<T>(
        &self,
        path: &str,
        create: bool,
        f: impl FnOnce(&mut Open<IO>) -> Result<T, StorageError>,
    ) -> Result<T, StorageError> {
        // Spelt as it was opened first: a book is read 4 bytes at a time,
        // many times a page. Then in any case, as FAT names it: one file
        // is open once.
        let kept = {
            let open = self.open.borrow();
            open.iter()
                .position(|o| o.path == path)
                .or_else(|| open.iter().position(|o| same_path(&o.path, path)))
        };
        match kept {
            // The latest last.
            Some(i) => {
                let mut open = self.open.borrow_mut();
                let file = open.remove(i);
                open.push(file);
            }
            None => {
                // Checked once, when opened: a book is read 4 bytes at a
                // time, many times a page.
                check(path)?;
                self.hold(path)?;
                if create {
                    self.create_dirs(parent(path))?;
                }
                // Before the open files are taken: a folder looked up
                // first is settled, closing files (see `settle_folder`).
                let dir = self.dir(parent(path))?;
                let mut open = self.open.borrow_mut();
                if open.len() == OPEN_FILES {
                    open.remove(0).flush()?;
                }
                let (file, len) = find_file(&dir, name(path), create)?;
                open.push(Open {
                    path: path.to_string(),
                    file,
                    len,
                    dirty: false,
                });
            }
        }
        match self.open.borrow_mut().last_mut() {
            Some(file) => f(file),
            None => Err(StorageError::Io),
        }
    }

    /// Closes the open files at `path` (or below it), by any case of its
    /// names, writing back what was appended to them.
    fn close(&self, path: &str) -> Result<(), StorageError> {
        let mut result = Ok(());
        self.open.borrow_mut().retain_mut(|o| {
            let keep = !is_within(&o.path, path);
            if !keep && let Err(e) = o.flush() {
                result = Err(e);
            }
            keep
        });
        result
    }

    fn write_all(file: &mut FatFile<IO>, data: &[u8]) -> Result<(), StorageError> {
        file.write_all(data).map_err(error)?;
        file.flush().map_err(error)
    }

    /// Writes back the disk's changed blocks, where it is shared.
    fn write_back(&self) -> Result<(), StorageError> {
        match &self.flush_disk {
            Some(flush) if !flush() => Err(StorageError::Io),
            _ => Ok(()),
        }
    }

    /// `result`, the disk's changed blocks written back after it: what a
    /// call changed is on the card when it returns.
    fn written(&self, result: Result<(), StorageError>) -> Result<(), StorageError> {
        let back = self.write_back();
        result.and(back)
    }

    /// [`Storage::write`], its blocks not yet written back.
    fn write_whole(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.close(path)?;
        self.create_dirs(parent(path))?;
        let dir = self.dir(parent(path))?;
        let name = name(path);
        // Refused before anything is written: no temporary file is left.
        if !fatfs::is_valid_long_name(name) {
            return Err(StorageError::InvalidPath);
        }
        let found = settled(&dir, name)?;
        let old = found.old;
        let there = found.entry.is_some();
        let mark = found.entry.as_ref().map(state);
        if found.others || found.entry.is_some_and(|entry| entry.is_dir()) {
            return Err(StorageError::InvalidPath);
        }
        // Whole under one name of the moment, synced, then under another
        // (see the crate documentation): settled, a cut is told by them.
        let temps = Temps::of(name);
        let written = dir
            .create_file(temps.tmp())
            .map_err(error)
            .and_then(|mut file| {
                // The file it replaces, as it is now: changed after a
                // cut, it is kept (see `settled`).
                if let Some(mark) = mark {
                    file.set_mark(mark);
                }
                Self::write_all(&mut file, data)
            });
        if let Err(e) = written {
            // A card that is full keeps no part of the new content.
            #[expect(
                clippy::let_underscore_must_use,
                reason = "cleaning up after a failed write, whose own error is the one told"
            )]
            let _ = dir.remove(temps.tmp());
            return Err(e);
        }
        if there {
            dir.rename(temps.tmp(), &dir, temps.ready())
                .map_err(error)?;
            dir.replace(temps.ready(), name).map_err(error)?;
        } else {
            // A new file: cut off, it is not there yet, or is there whole
            // (its temporary file beside it, taken away by `settled`).
            dir.rename(temps.tmp(), &dir, name).map_err(error)?;
        }
        if old {
            self.clean_old(parent(path), &dir);
        }
        Ok(())
    }

    /// [`Storage::remove`], its blocks not yet written back; settled
    /// first where `settle` (see [`settled`]): no new content of it is
    /// left to take its place later.
    fn remove_entry(&self, path: &str, settle: bool) -> Result<(), StorageError> {
        check(path)?;
        self.hold(path)?;
        self.close(path)?;
        self.forget(path);
        match self.dir(parent(path)) {
            Ok(dir) if settle => remove_found(&dir, name(path), settled(&dir, name(path))?),
            Ok(dir) => remove_in(&dir, name(path)),
            Err(StorageError::NotFound) => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// [`Storage::rename`], its blocks not yet written back.
    fn move_entry(&self, from: &str, to: &str) -> Result<(), StorageError> {
        check(from)?;
        check(to)?;
        // The same entry (FAT names it without regard to case): its name
        // spelt anew, if at all.
        let same = same_path(from, to);
        if same && name(from) == name(to) {
            return Ok(());
        }
        // Not into itself: it would be cut off from the root.
        if !same && is_within(to, from) {
            return Err(StorageError::InvalidPath);
        }
        // Not to a name FAT does not take: rust-fatfs would find out only
        // once the entry is gone from where it was.
        if !fatfs::is_valid_long_name(name(to)) {
            return Err(StorageError::InvalidPath);
        }
        self.close(from)?;
        self.close(to)?;
        self.forget(from);
        self.forget(to);
        // No new content of it is left behind (see `settled`).
        let dir = self.dir(parent(from))?;
        let is_file = match settled(&dir, name(from))? {
            // Nothing to move (a file or a folder): what is at `to` stays.
            Lookup { entry: None, .. } => return Err(StorageError::NotFound),
            Lookup { others: true, .. } => return Err(StorageError::InvalidPath),
            Lookup {
                entry: Some(entry), ..
            } => entry.is_file(),
        };
        // A file goes through the name of its new content: cut off, it
        // takes its place when looked for (see the crate documentation).
        let temps = Temps::of(name(to));
        if same && is_file {
            // rust-fatfs takes a move to its own entry for nothing to do:
            // it goes through a name of the moment. Cut off, `settled`
            // keeps the file where it was.
            rename_in(&dir, name(from), &dir, temps.ready())?;
            return dir.replace(temps.ready(), name(to)).map_err(error);
        }
        if same {
            return self.journaled(from, to, || {
                rename_in(&dir, name(from), &dir, temps.ready())?;
                rename_in(&dir, temps.ready(), &dir, name(to))
            });
        }
        self.create_dirs(parent(to))?;
        let to_dir = self.dir(parent(to))?;
        let there = settled(&to_dir, name(to))?;
        let replaced = match there.entry {
            _ if there.others => return Err(StorageError::InvalidPath),
            Some(entry) if entry.is_dir() => return Err(StorageError::InvalidPath),
            // A file there is replaced.
            Some(_) => true,
            None => false,
        };
        self.journaled(from, to, || match (replaced, is_file) {
            (true, true) => {
                rename_in(&dir, name(from), &to_dir, temps.ready())?;
                to_dir.replace(temps.ready(), name(to)).map_err(error)
            }
            (true, false) => {
                remove_in(&to_dir, name(to))?;
                rename_in(&dir, name(from), &to_dir, name(to))
            }
            (false, _) => rename_in(&dir, name(from), &to_dir, name(to)),
        })
    }

    /// [`Storage::create_dir`], its blocks not yet written back.
    fn create_dirs(&self, path: &str) -> Result<(), StorageError> {
        let path = path.trim_matches('/');
        if path.is_empty() {
            return Ok(());
        }
        check(path)?;
        // From the deepest kept folder on (see [`FOLDERS`]), not from the
        // root: its parents are there, some with hundreds of names.
        let mut kept = Some(path);
        while let Some(at) = kept.filter(|at| !self.is_kept(at)) {
            kept = at.rfind('/').map(|end| &at[..end]);
        }
        let (mut dir, rest) = match kept {
            Some(at) => (self.dir(at)?, &path[at.len()..]),
            None => (self.root(), path),
        };
        for part in rest.split('/').filter(|p| !p.is_empty()) {
            dir = create_dir_in(&dir, part)?;
        }
        Ok(())
    }

    /// [`Storage::remove_dir_all`], its blocks not yet written back.
    fn remove_tree(&self, path: &str) -> Result<(), StorageError> {
        check(path)?;
        self.hold(path)?;
        self.close(path)?;
        self.forget(path);
        let dir = match self.dir(path) {
            Ok(dir) => dir,
            Err(StorageError::NotFound) => return Ok(()),
            Err(e) => return Err(e),
        };
        let mut names: alloc::vec::Vec<(String, bool)> = alloc::vec::Vec::new();
        // Names of new content left by cuts (see `settled`).
        let mut ready = alloc::vec::Vec::new();
        for entry in entries(&dir) {
            let entry = entry?;
            let name = entry.file_name();
            if name != "." && name != ".." {
                if entry.is_file() && name.len() == 12 && name.starts_with('~') {
                    ready.push(name.to_ascii_uppercase());
                }
                names.push((name, entry.is_dir()));
            }
        }
        // A file and its new content may be the same file: settled first,
        // its clusters are not freed twice.
        for (name, _) in names
            .iter()
            .filter(|(_, is_dir)| !is_dir && !ready.is_empty())
        {
            let temps = Temps::of(name);
            if ready.iter().any(|n| n == temps.ready()) {
                dir.replace(temps.ready(), name).map_err(error)?;
            }
        }
        for (name, is_dir) in names {
            let child = format!("{path}/{name}");
            if is_temp_name(&name) {
                // Not taken by name (see `check`). A folder's new name in a
                // move cut off holds that folder's clusters: left to it.
                match is_dir {
                    true => dir.unlink(&name).map_err(error)?,
                    false => remove_in(&dir, &name)?,
                }
            } else if is_dir {
                self.remove_tree(&child)?;
            } else {
                self.remove_entry(&child, false)?;
            }
        }
        self.forget(path);
        remove_in(&self.dir(parent(path))?, name(path))
    }
}

impl<IO: ReadWriteSeek + 'static> Storage for FatStorage<IO> {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        // Past what FAT holds (4 GiB less a byte): the end of any file.
        let past_any_file = offset > u64::from(u32::MAX);
        self.with_open(path, false, |Open { file, len, .. }| {
            if past_any_file || offset >= *len {
                return Ok(0);
            }
            // Up to its size: clusters that end before (freed in a remove
            // cut off by an earlier version) do not pass for its end.
            let want = buf
                .len()
                .min(usize::try_from(*len - offset).unwrap_or(usize::MAX));
            if file.seek(SeekFrom::Start(offset)).map_err(error)? != offset {
                return Err(StorageError::Io);
            }
            let mut read = 0;
            while read < want {
                match file.read(&mut buf[read..want]).map_err(error)? {
                    0 => return Err(StorageError::Io),
                    n => read += n,
                }
            }
            Ok(read)
        })
    }

    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.with_open(path, false, |o| Ok(o.len))
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        check(path)?;
        self.settle_moves()?;
        self.hold(path)?;
        let before = self.len_of(path);
        let made = self.new_dirs(parent(path));
        let written = self.written(self.write_whole(path, data));
        if written.is_ok() {
            self.follow(made + data.len() as u64, before);
        }
        written
    }

    /// Kept in the open file until it is written back (see the [crate
    /// documentation](crate)).
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.settle_moves()?;
        let before = self.len_of(path);
        let made = self.new_dirs(parent(path));
        self.with_open(path, true, |o| {
            // Not after clusters that end before its size (see `read_at`).
            if o.file.seek(SeekFrom::End(0)).map_err(error)? != o.len {
                return Err(StorageError::Io);
            }
            o.dirty = true;
            let written = o.file.write_all(data).map_err(error);
            // As far as it came, where the card is full.
            o.len = o.file.seek(SeekFrom::Current(0)).map_err(error)?;
            written
        })?;
        // In whole clusters: what the file takes more, and its folders.
        let more = self.free.borrow().clusters(before + data.len() as u64)
            - self.free.borrow().clusters(before);
        self.follow(made + more * self.cluster(), 0);
        Ok(())
    }

    fn sync(&self) -> Result<(), StorageError> {
        let mut result = Ok(());
        for o in self.open.borrow_mut().iter_mut() {
            if let Err(e) = o.flush() {
                result = Err(e);
            }
        }
        // Any block still changed: a file's flush writes back all of the
        // disk's, but there may be no file appended to.
        if let Err(e) = self.write_back() {
            result = Err(e);
        }
        result
    }

    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        if !dir.is_empty() {
            check(dir)?;
        }
        // The sizes in the folders, as appended.
        self.sync()?;
        for entry in entries(&self.dir(dir)?) {
            let entry = entry?;
            let name = entry.file_name();
            // Nor the storage's own (see `check`).
            let own = is_temp_name(&name) || (dir.is_empty() && same_path(&name, JOURNAL));
            if name == "." || name == ".." || own {
                continue;
            }
            let is_dir = entry.is_dir();
            let modified = Some(seconds(entry.modified()));
            visit(
                DirEntry::new(&name, is_dir, if is_dir { 0 } else { entry.len() })
                    .with_modified(modified),
            );
        }
        Ok(())
    }

    fn modified(&self, path: &str) -> Option<i64> {
        let path = path.trim_matches('/');
        check(path).ok()?;
        let (dir, name) = path.rsplit_once('/').unwrap_or(("", path));
        let entry = lookup(&self.dir(dir).ok()?, name, false).ok()?.entry?;
        Some(seconds(entry.modified()))
    }

    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.settle_moves()?;
        let before = self.len_of(path);
        let removed = self.written(self.remove_entry(path, true));
        if removed.is_ok() {
            self.follow(0, before);
        }
        removed
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.settle_moves()?;
        // None while a move is held up: it would be recorded over it.
        if self.stuck.borrow().is_some() {
            return Err(StorageError::Io);
        }
        // A file it takes the place of is gone.
        let replaced = if from.eq_ignore_ascii_case(to) {
            0
        } else {
            self.len_of(to)
        };
        let moved = self.written(self.move_entry(from, to));
        if moved.is_ok() {
            self.follow(0, replaced);
        }
        moved
    }

    /// FAT names entries without regard to case.
    fn ignores_case(&self) -> bool {
        true
    }

    /// With the disk shared ([`FatStorage::with_disk`], as on the
    /// devices): at first what the card says of itself (FAT32), counted
    /// exactly a part of the FAT at each ask, then followed as files are
    /// written and removed; never all of the FAT at once (see `free`).
    /// Without it, rust-fatfs's count (which may read all of the FAT).
    fn free_space(&self) -> Option<u64> {
        match &self.read {
            Some(read) => self.free.borrow_mut().ask(read.as_ref()),
            None => {
                let stats = self.fs.stats().ok()?;
                Some(u64::from(stats.free_clusters()) * u64::from(stats.cluster_size()))
            }
        }
    }

    /// The bytes for files in all, from the boot sector.
    fn total_space(&self) -> Option<u64> {
        match &self.read {
            Some(read) => self.free.borrow_mut().total(read.as_ref()),
            None => {
                let stats = self.fs.stats().ok()?;
                Some(u64::from(stats.total_clusters()) * u64::from(stats.cluster_size()))
            }
        }
    }

    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.settle_moves()?;
        let made = self.new_dirs(path);
        let created = self.written(self.create_dirs(path));
        if created.is_ok() {
            self.follow(made, 0);
        }
        created
    }

    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        self.settle_moves()?;
        let before = self.tree_len(path);
        let removed = self.written(self.remove_tree(path));
        if removed.is_ok() {
            self.follow(0, before);
        }
        removed
    }
}

#[cfg(test)]
mod tests;
