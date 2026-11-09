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
//!    newer one, one a reader copied): it is **kept** as it is, and the mark
//!    written.
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
//! path, and the files are written straight from the program's image. Nothing
//! panics.

#![no_std]

extern crate alloc;

use alloc::format;

use paperpaw_platform::Storage;

/// The package's id, and its folder under `plugins/`.
pub const HYPHENATION: &str = "hyphenation";

/// Where the firmware notes that it dealt with the English pack.
const MARK: &str = ".paperpaw/seeded/hyphenation";

/// Bytes that must be free before the files are written: the package
/// (about 60 KB) and a card's clusters.
pub const ROOM: u64 = 256 * 1024;

/// The files of a package: path inside its folder, and content.
pub type Files = &'static [(&'static str, &'static [u8])];

/// The English package, as `tools/hyphenation-packs` builds it.
static ENGLISH: Files = include!(concat!(env!("OUT_DIR"), "/english.rs"));

/// The files of the English package.
pub fn english() -> Files {
    ENGLISH
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

/// Removes what a failed start wrote, as far as the card lets it.
fn take_back(storage: &dyn Storage, folder: &str) {
    for &(path, _) in ENGLISH {
        let _ = storage.remove(&format!("{folder}/{path}"));
    }
    let _ = storage.remove_dir_all(folder);
}

/// Puts the English hyphenation package on `storage` if this device never
/// had it dealt with (see the [crate documentation](self)).
pub fn seed_hyphenation(storage: &dyn Storage) -> Seeded {
    if storage.exists(MARK) {
        return Seeded::Kept;
    }
    let folder = format!("plugins/{HYPHENATION}");
    if storage.exists(&format!("{folder}/plugin.ini")) {
        // Found, not made: only note it.
        let _ = storage.write(MARK, b"");
        return Seeded::Kept;
    }
    let tried = storage.size(TRIED).unwrap_or(0) as usize;
    if tried >= TRIES {
        return Seeded::GaveUp;
    }
    if storage.free_space().is_some_and(|free| free < ROOM) {
        return Seeded::NoRoom;
    }
    // `plugin.ini` last: without it the folder is no plugin. Everything
    // else is on the card (`sync`) before it is written, so a power cut
    // cannot leave a manifest over files that were not written back.
    let manifest = |path: &str| path == "plugin.ini";
    for pass in [false, true] {
        for &(path, bytes) in ENGLISH.iter().filter(|(p, _)| manifest(p) == pass) {
            if storage.write(&format!("{folder}/{path}"), bytes).is_err() {
                take_back(storage, &folder);
                let _ = storage.append(TRIED, &[1]);
                return Seeded::Failed;
            }
        }
        if !pass && storage.sync().is_err() {
            take_back(storage, &folder);
            let _ = storage.append(TRIED, &[1]);
            return Seeded::Failed;
        }
    }
    let _ = storage.sync();
    let _ = storage.write(MARK, b"");
    Seeded::Installed
}

#[cfg(test)]
mod tests;
