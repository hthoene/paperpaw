//! Op 13 `position` and op 14 `set_position`: where a book was left, from the
//! position file the readers keep (`paperpaw_sdk::positions`), and the writes
//! of it (`docs/abi/library.md` 2.2).
//!
//! The file is the SDK's own (`[position]`: `section`, `offset`, `percent` in
//! tenths of a percent, `conversion`, `xpointer`, `updated`, `book`), so an
//! interface 6 reader reads what is written here. The host adds two keys the
//! SDK's `Position` has no field for and that older readers skip: `insert`
//! (kept only when not 0) and `sync = 1` (a position written by a sync without
//! the user's confirmation; any other write removes it).
use super::{CatalogReads, fail};
use crate::native_host::lib_service::{BookRef, One, StoredPosition};
use crate::native_host::registrations::{self as wire, Record};
use alloc::string::String;
use core::fmt::Write as _;
use paperpaw_native::host_glue::{ERR_INVALID, ERR_IO, ERR_NO_MEMORY, ERR_NOT_FOUND};
use paperpaw_platform::{Storage, StorageError};
use paperpaw_sdk::{config, library, positions};

/// The most of a position file read (`positions` reads 4 KiB).
const LIMIT: usize = 4096;

/// `lib_position.flags`: has an XPointer, has a conversion, from sync.
const HAS_XPOINTER: u32 = 1;
const HAS_CONVERSION: u32 = 1 << 1;
const FROM_SYNC: u32 = 1 << 2;
/// Bytes of an XPointer the wire carries.
const XPOINTER: usize = 1024;

/// The position of the book at `path`, or `ERR_NOT_FOUND` for none kept.
///
/// `insert` and `from_sync` are the file's own keys (0 and clear where it has
/// none).
pub(super) fn read(storage: &dyn Storage, path: &str, visit: One<'_>) -> Result<(), i32> {
    let position = positions::try_load(storage, path)
        .map_err(fail)?
        .ok_or(ERR_NOT_FOUND)?;
    let marks = marks(storage, path)?;
    // A place with an empty XPointer has none; one that does not fit (the
    // SDK already leaves out one over 1,024 bytes as written) is as if it had
    // none, never cut: the rest of the place still answers.
    let xpointer = position
        .xpointer
        .as_deref()
        .filter(|x| !x.is_empty() && x.len() <= XPOINTER)
        .unwrap_or("");
    let mut flags = 0;
    if marks.from_sync {
        flags |= FROM_SYNC;
    }
    if !xpointer.is_empty() {
        flags |= HAS_XPOINTER;
    }
    if position.conversion.is_some() {
        flags |= HAS_CONVERSION;
    }
    // The file keeps a tenth of a percent: 41.5 % is 415,000 parts per million.
    let percent = if position.percent.is_nan() {
        0.0
    } else {
        position.percent
    };
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to 0..=1000"
    )]
    let tenths = (percent.clamp(0.0, 100.0) * 10.0 + 0.5) as u32;
    let progress_ppm = tenths * 1_000;
    visit(Record::ServiceLibPosition(wire::ServiceLibPosition {
        section: position.at.section,
        offset: position.at.offset,
        insert: marks.insert,
        progress_ppm,
        conversion: position.conversion.unwrap_or(0),
        updated: u64::try_from(position.updated).unwrap_or(0),
        xpointer,
        flags,
    }));
    Ok(())
}

/// What the file keeps that the SDK's `Position` has no field for.
#[derive(Default)]
struct Marks {
    insert: u32,
    from_sync: bool,
    updated: u64,
}

/// The name of the position file of `path`, had fallibly.
fn file(path: &str) -> Result<String, i32> {
    let mut digits = [0_u8; library::KEY_LEN];
    let key = library::key_into(path, &mut digits);
    let mut name = String::new();
    name.try_reserve_exact(positions::DIR.len() + 1 + key.len() + ".txt".len())
        .map_err(|_| ERR_NO_MEMORY)?;
    for part in [positions::DIR, "/", key, ".txt"] {
        name.push_str(part);
    }
    Ok(name)
}

/// The file's text, `None` where the book has no position.
fn text(storage: &dyn Storage, path: &str) -> Result<Option<alloc::vec::Vec<u8>>, i32> {
    match paperpaw_sdk::read_file(storage, &file(path)?, LIMIT) {
        Ok(data) => Ok(Some(data)),
        Err(StorageError::NotFound) => Ok(None),
        Err(StorageError::OutOfMemory) => Err(ERR_NO_MEMORY),
        Err(_) => Err(ERR_IO),
    }
}

/// The marks of the file at `path` (none where there is no file).
fn marks(storage: &dyn Storage, path: &str) -> Result<Marks, i32> {
    let Some(data) = text(storage, path)? else {
        return Ok(Marks::default());
    };
    let source =
        core::str::from_utf8(paperpaw_sdk::files::without_bom(&data)).map_err(|_| ERR_IO)?;
    let mut marks = Marks::default();
    for entry in config::parse(source).filter_map(Result::ok) {
        if entry.section != "position" {
            continue;
        }
        match entry.key {
            "insert" => marks.insert = entry.value.as_u32().unwrap_or(0),
            "sync" => marks.from_sync = entry.value.as_u32() == Some(1),
            "updated" => {
                marks.updated = u64::try_from(entry.value.as_i64().unwrap_or(0)).unwrap_or(0)
            }
            _ => {}
        }
    }
    Ok(marks)
}

/// Op 14's `stored_position`: how the book's position was written and when,
/// `None` where it has none. A file that is no position is `ERR_IO`, as for
/// the read.
pub(super) fn stored(
    reads: &CatalogReads,
    book: BookRef<'_>,
) -> Result<Option<StoredPosition>, i32> {
    reads.with_path(book, |path| {
        if positions::try_load(&*reads.storage, path)
            .map_err(fail)?
            .is_none()
        {
            return Ok(None);
        }
        let marks = marks(&*reads.storage, path)?;
        Ok(Some(StoredPosition {
            from_sync: marks.from_sync,
            updated: marks.updated,
        }))
    })
}

/// Op 14's `write_position`: the whole file written atomically (the card's
/// `write`); the book's fingerprint, which a reader keeps in it, stays.
pub(super) fn write(
    reads: &CatalogReads,
    book: BookRef<'_>,
    at: &wire::ServiceLibPosition<'_>,
    from_sync: bool,
) -> Result<(), i32> {
    reads.with_path(book, |path| {
        let fingerprint = positions::book(&*reads.storage, path);
        let text = render(
            at,
            from_sync,
            fingerprint.as_ref().map(library::Fingerprint::text),
        )?;
        let _ = reads.storage.create_dir(positions::DIR);
        reads
            .storage
            .write(&file(path)?, text.as_bytes())
            .map_err(|e| match e {
                StorageError::OutOfMemory => ERR_NO_MEMORY,
                StorageError::Full | StorageError::TooLarge => {
                    paperpaw_native::host_glue::ERR_NO_SPACE
                }
                _ => ERR_IO,
            })
    })
}

/// The file's text for a position, built fallibly with integers only (the
/// percent is the tenth the file keeps).
fn render(
    at: &wire::ServiceLibPosition<'_>,
    from_sync: bool,
    fingerprint: Option<String>,
) -> Result<String, i32> {
    let xpointer = (at.flags & HAS_XPOINTER != 0).then_some(at.xpointer);
    if xpointer.is_some_and(|x| x.is_empty() || x.len() > positions::XPOINTER_MAX) {
        return Err(ERR_INVALID);
    }
    let quoted = xpointer.map(config::quote);
    let fingerprint_len = fingerprint.as_ref().map_or(0, String::len);
    let mut text = String::new();
    // The numbers, the keys and the sync mark are under 256 bytes.
    text.try_reserve_exact(256 + quoted.as_ref().map_or(0, |q| q.len()) + fingerprint_len)
        .map_err(|_| ERR_NO_MEMORY)?;
    let tenths = (at.progress_ppm.min(1_000_000) + 500) / 1_000;
    let _ = write!(
        text,
        "[position]\nsection = {}\noffset = {}\npercent = {}.{}\n",
        at.section,
        at.offset,
        tenths / 10,
        tenths % 10
    );
    if at.flags & HAS_CONVERSION != 0 {
        let _ = writeln!(text, "conversion = {:016x}", at.conversion);
    }
    if let Some(quoted) = quoted {
        let _ = writeln!(text, "xpointer = {quoted}");
    }
    if at.updated != 0 {
        let _ = writeln!(text, "updated = {}", at.updated);
    }
    if at.insert != 0 {
        let _ = writeln!(text, "insert = {}", at.insert);
    }
    if from_sync {
        text.push_str("sync = 1\n");
    }
    if let Some(fingerprint) = fingerprint {
        let _ = writeln!(text, "book = {fingerprint}");
    }
    Ok(text)
}

#[cfg(test)]
#[path = "position_tests.rs"]
mod tests;
