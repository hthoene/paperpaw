//! The text catalogs (`i18n/*.ini`) of a plugin folder, read for the kernel.
//!
//! A catalog given as text stays in memory whole: its text, an index and its
//! unescaped messages, and every language of it, used or not. A code plugin
//! of the store has one per language (stats: 24 KB of text for two), which
//! on the X4's 256 KiB heap is some 27 KB that stays for good, and where
//! the first big allocations of a start spill into the second heap region
//! it splits the largest free block in two (#248). So a code plugin's
//! larger text catalogs are compiled once (`Catalog::compile`, ADR 0033)
//! into a cache file on the card and read from there as they are needed:
//! about a kilobyte of memory a language, the messages of the screen shown.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_platform::Storage;

use super::{Budget, MAX_FILE, MAX_TRANSLATIONS, read};

/// Where the compiled catalogs are kept.
const DIR: &str = ".paperpaw/cache/catalogs";
/// Text catalogs smaller than this stay in memory as text: compiling does
/// not pay.
const MIN_TEXT: usize = 2048;
/// What a compiled catalog's file begins with.
const MAGIC: &[u8; 4] = b"PPCT";

/// The catalogs in the folder `catalogs` of a plugin: the names of those
/// given as text, the paths of the compiled ones.
pub(super) fn names(storage: &dyn Storage, catalogs: &str) -> (Vec<String>, Vec<String>) {
    let mut files: Vec<String> = Vec::new();
    let mut compiled: Vec<String> = Vec::new();
    let _ = storage.list(catalogs, &mut |entry| {
        // `DE.INI` too: a file copied from a memory card may have it so.
        if !entry.is_dir && paperpaw_sdk::files::has_extension(entry.name, "ini") {
            files.push(entry.name.to_string());
        }
        // Read as needed, so not counted (ADR 0033).
        if !entry.is_dir && paperpaw_sdk::files::has_extension(entry.name, "ppcat") {
            compiled.push(format!("{catalogs}/{}", entry.name));
        }
    });
    (files, compiled)
}

/// Reads the text catalogs `files` of the folder `folder` (`catalogs`: its
/// `i18n` folder): the text of each, or, for a code plugin's larger ones,
/// the path of its compiled form in `compiled`. What is left out is told in
/// `budget`.
pub(super) fn read_all(
    storage: &dyn Storage,
    folder: &str,
    catalogs: &str,
    files: Vec<String>,
    code: bool,
    compiled: &mut Vec<String>,
    budget: &mut Budget,
) -> Vec<String> {
    let mut texts = Vec::new();
    let mut left = MAX_TRANSLATIONS;
    for file in files {
        let path = format!("{catalogs}/{file}");
        let size = storage
            .size(&path)
            .map_or(usize::MAX, |s| usize::try_from(s).unwrap_or(usize::MAX));
        if size > left.min(MAX_FILE) {
            budget
                .problems
                .push(format!("{path}: left out, more than the plugin may take"));
            continue;
        }
        if code
            && size >= MIN_TEXT
            && let Some(kept) = compile(storage, folder, &file, &path, size)
        {
            compiled.push(kept);
            continue;
        }
        if let Some(text) = read(storage, &path, MAX_FILE) {
            left = left.saturating_sub(text.len());
            texts.push(text);
        }
    }
    texts
}

/// The compiled form of the text catalog `file` at `path` (`size` bytes) of
/// the plugin in `folder`: kept in the cache, named by the plugin, the file
/// and a digest of the text, so that a changed text makes a new one (and
/// removes the old). `None` where it cannot be had (the text is then read
/// as before).
fn compile(
    storage: &dyn Storage,
    folder: &str,
    file: &str,
    path: &str,
    size: usize,
) -> Option<String> {
    let plugin = folder.rsplit('/').next().unwrap_or(folder);
    let stem = file.rsplit_once('.').map_or(file, |(stem, _)| stem);
    let prefix = format!("{plugin}-{}-", stem.to_ascii_lowercase());
    let kept = format!(
        "{DIR}/{prefix}{size:x}-{:08x}.ppcat",
        digest(storage, path)?
    );
    if storage.size(&kept).is_ok_and(|n| n > MAGIC.len() as u64) && has_magic(storage, &kept) {
        return Some(kept);
    }
    let text = read(storage, path, MAX_FILE)?;
    let bytes = paperpaw_i18n::Catalog::compile(&text).ok()?;
    drop(text);
    for dir in [".paperpaw", ".paperpaw/cache", DIR] {
        let _ = storage.create_dir(dir);
    }
    // Older ones of this plugin's file.
    let mut old: Vec<String> = Vec::new();
    let _ = storage.list(DIR, &mut |entry| {
        if !entry.is_dir && entry.name.starts_with(&prefix) {
            old.push(format!("{DIR}/{}", entry.name));
        }
    });
    for stale in old {
        let _ = storage.remove(&stale);
    }
    // Whole or not at all: a file that was cut short would not open.
    let part = format!("{kept}.new");
    let done = storage
        .write(&part, &bytes)
        .and_then(|()| storage.rename(&part, &kept));
    if done.is_err() {
        let _ = storage.remove(&part);
        return None;
    }
    Some(kept)
}

/// Bytes of a text read at once for its digest: a card answers each read
/// after a wait, so 4 KiB in one read take a fraction of the time of eight
/// reads of a block (a text of 12 KB was 24 reads at every start, #305).
const DIGEST_PIECE: usize = 4096;

/// FNV-1a of the file's bytes, read in pieces of [`DIGEST_PIECE`] (of a
/// block where there is no memory for one).
fn digest(storage: &dyn Storage, path: &str) -> Option<u32> {
    let mut hash = 0x811c_9dc5_u32;
    let mut small = [0u8; 512];
    let mut large: Vec<u8> = Vec::new();
    let piece: &mut [u8] = match large.try_reserve_exact(DIGEST_PIECE) {
        Ok(()) => {
            large.resize(DIGEST_PIECE, 0);
            &mut large
        }
        Err(_) => &mut small,
    };
    let mut at = 0u64;
    loop {
        let n = storage.read_at(path, at, piece).ok()?;
        if n == 0 {
            return Some(hash);
        }
        for byte in piece.get(..n)? {
            hash = (hash ^ u32::from(*byte)).wrapping_mul(0x0100_0193);
        }
        at += n as u64;
    }
}

fn has_magic(storage: &dyn Storage, path: &str) -> bool {
    let mut head = [0u8; 4];
    storage.read_at(path, 0, &mut head).ok() == Some(head.len()) && &head == MAGIC
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::MemoryStorage;

    /// A storage that counts its reads.
    struct Reads(MemoryStorage, core::cell::Cell<u32>);

    impl Storage for Reads {
        fn read_at(
            &self,
            path: &str,
            at: u64,
            buf: &mut [u8],
        ) -> Result<usize, paperpaw_platform::StorageError> {
            self.1.set(self.1.get() + 1);
            self.0.read_at(path, at, buf)
        }
        fn size(&self, path: &str) -> Result<u64, paperpaw_platform::StorageError> {
            self.0.size(path)
        }
        fn write(&self, path: &str, data: &[u8]) -> Result<(), paperpaw_platform::StorageError> {
            self.0.write(path, data)
        }
        fn append(&self, path: &str, data: &[u8]) -> Result<(), paperpaw_platform::StorageError> {
            self.0.append(path, data)
        }
        fn list(
            &self,
            dir: &str,
            visit: &mut dyn FnMut(paperpaw_platform::DirEntry<'_>),
        ) -> Result<(), paperpaw_platform::StorageError> {
            self.0.list(dir, visit)
        }
        fn remove(&self, path: &str) -> Result<(), paperpaw_platform::StorageError> {
            self.0.remove(path)
        }
    }

    /// A text's digest is read in pieces of 4 KiB, not of a block (#305):
    /// 12 KB were 24 reads at every start, and are 4. The digest is the
    /// same, so the compiled catalogs kept stay.
    #[test]
    fn a_text_is_digested_in_few_reads() {
        let text: Vec<u8> = (0..12_000u32).map(|i| (i * 7 % 251) as u8).collect();
        let hash = text.iter().fold(0x811c_9dc5_u32, |hash, byte| {
            (hash ^ u32::from(*byte)).wrapping_mul(0x0100_0193)
        });
        let storage = Reads(
            MemoryStorage::new().with("plugins/stats/i18n/en.ini", text),
            core::cell::Cell::new(0),
        );
        assert_eq!(digest(&storage, "plugins/stats/i18n/en.ini"), Some(hash));
        assert!(storage.1.get() <= 4, "{} reads", storage.1.get());
        assert_eq!(digest(&storage, "plugins/stats/none.ini"), None);
    }
}
