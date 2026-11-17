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
use alloc::string::String;
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
    let kept = format!("{DIR}/{prefix}{size:x}-{:08x}.ppcat", digest(storage, path)?);
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

/// FNV-1a of the file's bytes, read in small pieces.
fn digest(storage: &dyn Storage, path: &str) -> Option<u32> {
    let mut hash = 0x811c_9dc5_u32;
    let mut piece = [0u8; 512];
    let mut at = 0u64;
    loop {
        let n = storage.read_at(path, at, &mut piece).ok()?;
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
