//! The data plugins every PaperPaw installs by default: **English
//! hyphenation**, embedded in the program and put on the card (or the
//! shell's storage) where the plugin folders are, at the first start that
//! finds none.
//!
//! Hyphenation comes only from data plugins (`plugins/hyphenation/` holds the
//! English table, `hyphenation-<lang>` the others; `docs/abi/proposals/
//! declarative-registration.md`, "The `[hyphenation]` data-plugin kind"). The
//! web flasher puts the English one on the card of a fresh install. A device
//! that did not come through the flasher has none: the update of a firmware
//! that had English built in (Beta 3), the emulator on a new folder, the
//! Android app, the web page. [`seed_hyphenation`] makes them the same as a
//! flashed card: the same package, built from the same sources by the same
//! code (`tools/hyphenation-packs`), in `build.rs`.
//!
//! What it does, in this order, at every start (cheap: one or two file
//! lookups once it has run):
//!
//! 1. A mark `.paperpaw/seeded/hyphenation` is there: **nothing**. The pack
//!    was put there, or found there, at an earlier start; a reader who removed
//!    it keeps it removed (the mark is the firmware's own, outside the plugin's
//!    folder, which removing the plugin deletes).
//! 2. The plugin's `plugin.ini` is there (the flasher's, the store's, a
//!    newer one, one a reader copied) **and the plugin is one the reader can
//!    use** (by the loader's own rule, `paperpaw_core::hyphenation_tables`: it
//!    parses, is made for formats 5 or later, and `[hyphenation]` names a table
//!    that is in its folder; a missing `id` is the folder's name): it is
//!    **kept** as it is, and the mark written. A `plugin.ini` that is no such
//!    plugin (empty, damaged, no table) does not stop the pack: it is removed
//!    (and the card synced) before step 3 writes the files, with no mark until
//!    then, so a power cut cannot leave it naming a half-written table. A
//!    broken file must not keep a device without English for good; the
//!    reader's other files in the folder (other languages' tables) stay, also
//!    when a write fails: only the files this start made are taken back. A file this cannot judge (too
//!    big, not readable, no room to read it) is left as it is, with no mark:
//!    judged again at the next start.
//! 3. Otherwise the files are written to `plugins/hyphenation/`, `plugin.ini`
//!    **last**: a folder without it is no plugin, so a power cut while
//!    copying leaves nothing half installed, and the next start writes it all
//!    again over what is there. Then the mark is written.
//!
//! No room on the card (less than [`ROOM`] free, where the card says) leaves
//! no mark: tried again at the next start. A write that fails (the X4 does not
//! know the free space at this point of the start, so a nearly full card fails
//! part-way) removes what was written, counts the failure
//! (`.paperpaw/seeded/hyphenation.tried`) and is tried again at the next
//! start, [`TRIES`] times at most, then given up. Each file is on the card
//! (`sync`) before `plugin.ini` is written. Nothing here allocates more than a
//! path and, to judge a `plugin.ini` that is there, a copy of it (4 KB at
//! most, a refused allocation is "not judged"); the files are written
//! straight from the program's image, where they are kept deflated (half their size, #428): inflated piece by piece
//! as they are written, with 4 KiB of history and the decoder (some 11 KB) on the heap for a moment; a start
//! that has no room for them fails like one whose card is full. Nothing panics.

#![no_std]

extern crate alloc;

use alloc::format;
use alloc::vec::Vec;

use paperpaw_platform::{Storage, StorageError};

/// The package's id, and its folder under `plugins/`.
pub const HYPHENATION: &str = "hyphenation";

/// Where the firmware notes that it dealt with the English pack.
const MARK: &str = ".paperpaw/seeded/hyphenation";

/// Bytes that must be free before the files are written: the package
/// (about 60 KB) and a card's clusters.
pub const ROOM: u64 = 256 * 1024;

/// A file of a package as the program carries it: path inside its folder,
/// its length, and its content raw-deflated with a history of [`WINDOW`]
/// bytes.
type Packed = (&'static str, usize, &'static [u8]);

/// The English package, as `tools/hyphenation-packs` builds it.
static ENGLISH: &[Packed] = include!(concat!(env!("OUT_DIR"), "/english.rs"));

/// The history the files are deflated with (`build.rs`), in bytes: a power of
/// two, so that the position wraps with a mask.
const WINDOW: usize = 4096;

/// The files of the English package: path inside its folder, and content
/// (inflated: for tests and tools; [`seed_hyphenation`] does not hold them).
pub fn english() -> Vec<(&'static str, Vec<u8>)> {
    ENGLISH
        .iter()
        .map(|&(path, len, packed)| {
            let mut bytes = Vec::with_capacity(len);
            let whole = inflate_to(packed, &mut |piece| {
                bytes.extend_from_slice(piece);
                true
            });
            assert!(whole && bytes.len() == len, "{path} is inflated whole");
            (path, bytes)
        })
        .collect()
}

/// Inflates the raw deflate stream `packed`, handing what comes out to
/// `sink` in pieces of at most [`WINDOW`] bytes, in order; `false` where the
/// data is corrupt or short, `sink` said stop (`false`) or the room for the
/// history and the decoder is not there.
fn inflate_to(packed: &[u8], sink: &mut dyn FnMut(&[u8]) -> bool) -> bool {
    use miniz_oxide::inflate::TINFLStatus;
    use miniz_oxide::inflate::core::{DecompressorOxide, decompress};

    let mut window: Vec<u8> = Vec::new();
    if window.try_reserve_exact(WINDOW).is_err() {
        return false;
    }
    window.resize(WINDOW, 0);
    // Made in place on the heap: the decoder is 11 KB, no frame may hold it.
    let mut decoder: Vec<DecompressorOxide> = Vec::new();
    if decoder.try_reserve_exact(1).is_err() {
        return false;
    }
    decoder.resize_with(1, DecompressorOxide::default);
    let Some(state) = decoder.first_mut() else {
        return false;
    };
    let (mut input, mut at) = (packed, 0_usize);
    loop {
        let (status, used, made) = decompress(state, input, &mut window, at, 0);
        input = input.get(used..).unwrap_or_default();
        if made > 0 && !window.get(at..at + made).is_some_and(&mut *sink) {
            return false;
        }
        at = (at + made) & (WINDOW - 1);
        match status {
            TINFLStatus::Done => return true,
            TINFLStatus::HasMoreOutput => {}
            _ => return false,
        }
    }
}

/// Writes the file `path` from its deflated form: the first piece writes it,
/// the others are appended. `false` where a write failed or the stream was
/// not `len` bytes.
fn write_file(storage: &dyn Storage, path: &str, len: usize, packed: &[u8]) -> bool {
    let (mut written, mut failed) = (0_usize, false);
    let whole = inflate_to(packed, &mut |piece| {
        let result = if written == 0 {
            storage.write(path, piece)
        } else {
            storage.append(path, piece)
        };
        failed = result.is_err();
        written += piece.len();
        !failed
    });
    if written == 0 && !failed {
        // An empty file: nothing came out to write.
        return len == 0 && storage.write(path, b"").is_ok();
    }
    whole && !failed && written == len
}

/// What [`seed_hyphenation`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Seeded {
    /// Nothing to do: it was dealt with before, or the plugin is on the card.
    Kept,
    /// The package was written.
    Installed,
    /// Less than [`ROOM`] is free: nothing written, tried again next start.
    NoRoom,
    /// A file could not be written (the card filled up, a read-only card): what
    /// was written is removed again, and it is tried again at the next start,
    /// at most [`TRIES`] times in all.
    Failed,
    /// [`TRIES`] starts failed: given up, nothing written. Remove
    /// `.paperpaw/seeded/` to try once more.
    GaveUp,
}

/// Starts that may fail to write the package before it is given up (each
/// writes and removes up to 60 KB: a card that is nearly full would be worn
/// by it at every start).
pub const TRIES: usize = 3;

/// Where the failed starts are counted (one byte each).
const TRIED: &str = ".paperpaw/seeded/hyphenation.tried";

/// The longest `plugin.ini` that is read to judge it (the package's own is
/// about 0.5 KB).
const INI_MAX: u64 = 4096;

/// What the plugin found in the folder is.
enum Found {
    /// A plugin the reader can use.
    Usable,
    /// Not one: written over.
    Broken,
    /// Could not be told (too big, a read failed, no room to read it, made for
    /// a later PaperPaw).
    Unknown,
}

/// Whether the plugin in `folder` is one the loader takes as hyphenation:
/// the loader's own rule (`paperpaw_core::hyphenation_tables`: the manifest
/// parses, a missing `id` is the folder's name, made for formats 5 or later,
/// `[hyphenation]` names tables) and at least one of the tables it names is a
/// file of its folder. (Whether a table's content holds is checked when its
/// language is first needed, as the loader does.)
fn judge(storage: &dyn Storage, folder: &str) -> Found {
    let ini = format!("{folder}/plugin.ini");
    let size = match storage.size(&ini) {
        Ok(0) => return Found::Broken,
        Ok(size) if size <= INI_MAX => size as usize,
        _ => return Found::Unknown,
    };
    let mut bytes = Vec::new();
    if bytes.try_reserve_exact(size).is_err() {
        return Found::Unknown;
    }
    bytes.resize(size, 0);
    match storage.read_at(&ini, 0, &mut bytes) {
        Ok(read) if read == size => {}
        _ => return Found::Unknown,
    }
    let Ok(text) = core::str::from_utf8(&bytes) else {
        return Found::Broken;
    };
    match paperpaw_core::hyphenation_tables(text, HYPHENATION) {
        Ok(tables)
            if tables
                .iter()
                .any(|t| storage.exists(&format!("{folder}/{t}"))) =>
        {
            Found::Usable
        }
        // A pack made for a later PaperPaw (a newer firmware's, or the
        // store's, after a downgrade) is not this one's to judge or to
        // replace: left as it is, no mark, looked at again at the next start.
        Err(paperpaw_core::ManifestRefusal::LaterFormats { .. }) => Found::Unknown,
        _ => Found::Broken,
    }
}

/// The most files of the package whose being there before is remembered (a
/// larger package leaves the rest alone when a start fails).
const MAX_FILES: usize = 32;

/// What a failed start wrote is taken back, as far as the card lets it. Where
/// the folder was not there (or empty), all of it goes. Where it held files
/// before (a broken `plugin.ini` beside the reader's other tables), only the
/// files this start made: never the folder, and never a file that was there.
fn take_back(storage: &dyn Storage, folder: &str, before: Option<&[bool; MAX_FILES]>) {
    for (at, &(path, ..)) in ENGLISH.iter().enumerate() {
        let there_before = before.is_some_and(|b| b.get(at).copied().unwrap_or(true));
        if before.is_none() || !there_before {
            let _ = storage.remove(&format!("{folder}/{path}"));
        }
    }
    if before.is_none() {
        let _ = storage.remove_dir_all(folder);
    }
}

/// A start that failed to write: counted, to be tried again.
fn failed(storage: &dyn Storage, folder: &str, before: Option<&[bool; MAX_FILES]>) -> Seeded {
    take_back(storage, folder, before);
    let _ = storage.append(TRIED, &[1]);
    Seeded::Failed
}

/// Puts the English hyphenation package on `storage` if this device never
/// had it dealt with (see the [crate documentation](self)).
pub fn seed_hyphenation(storage: &dyn Storage) -> Seeded {
    if storage.exists(MARK) {
        return Seeded::Kept;
    }
    let folder = format!("plugins/{HYPHENATION}");
    let ini = format!("{folder}/plugin.ini");
    let mut broken = false;
    if storage.exists(&ini) {
        match judge(storage, &folder) {
            // Found, not made: only note it.
            Found::Usable => {
                let _ = storage.write(MARK, b"");
                return Seeded::Kept;
            }
            // Not judged: left as it is, looked at again at the next start.
            Found::Unknown => return Seeded::Kept,
            // No plugin: the pack goes on over it.
            Found::Broken => broken = true,
        }
    }
    let tried = storage.size(TRIED).unwrap_or(0) as usize;
    if tried >= TRIES {
        return Seeded::GaveUp;
    }
    if storage.free_space().is_some_and(|free| free < ROOM) {
        return Seeded::NoRoom;
    }
    // Files that are there already (a broken `plugin.ini` beside the reader's
    // other tables, tables with no manifest) are not this start's to take
    // back if it fails.
    // A listing that fails for any other reason than "no such folder" counts
    // as files there: a failed write then never takes the folder away.
    let mut held = false;
    match storage.list(&folder, &mut |_| held = true) {
        Ok(()) | Err(StorageError::NotFound) => {}
        Err(_) => held = true,
    }
    let mut before = [false; MAX_FILES];
    if held {
        for (at, &(path, ..)) in ENGLISH.iter().enumerate().take(MAX_FILES) {
            before[at] = storage.exists(&format!("{folder}/{path}"));
        }
    }
    let before = held.then_some(&before);
    // The old, broken `plugin.ini` goes first, and is on the card (`sync`)
    // before a table is written: a power cut while the tables are copied
    // must not leave a manifest that names a half-written table, which the
    // next start would take for a plugin and mark for good.
    if broken && (storage.remove(&ini).is_err() || storage.sync().is_err()) {
        return failed(storage, &folder, before);
    }
    // `plugin.ini` last: without it the folder is no plugin. Everything
    // else is on the card (`sync`) before it is written, so a power cut
    // cannot leave a manifest over files that were not written back.
    let manifest = |path: &str| path == "plugin.ini";
    for pass in [false, true] {
        for &(path, len, packed) in ENGLISH.iter().filter(|(p, ..)| manifest(p) == pass) {
            if !write_file(storage, &format!("{folder}/{path}"), len, packed) {
                return failed(storage, &folder, before);
            }
        }
        if !pass && storage.sync().is_err() {
            return failed(storage, &folder, before);
        }
    }
    let _ = storage.sync();
    let _ = storage.write(MARK, b"");
    Seeded::Installed
}

#[cfg(test)]
mod tests;
