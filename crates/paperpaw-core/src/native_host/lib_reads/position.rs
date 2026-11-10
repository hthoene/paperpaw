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
use super::{CatalogReads, fail, kept_file};
use crate::native_host::lib_service::{BookRef, One, StoredPosition};
use crate::native_host::registrations::{self as wire, Record};
use alloc::string::String;
use core::fmt::Write as _;
use paperpaw_native::host_glue::{ERR_BUSY, ERR_INVALID, ERR_IO, ERR_NO_MEMORY, ERR_NOT_FOUND};
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
pub(super) fn file(path: &str) -> Result<String, i32> {
    kept_file(positions::DIR, path)
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

/// The fingerprint a reader keeps in the file (`book = <size> <md5>`), as
/// written; `None` where the file has none or is no text (it is replaced).
fn kept_fingerprint(data: &[u8]) -> Option<&str> {
    let source = core::str::from_utf8(paperpaw_sdk::files::without_bom(data)).ok()?;
    config::parse(source)
        .filter_map(Result::ok)
        .find(|entry| entry.section == "position" && entry.key == "book")
        .map(|entry| entry.value.raw().trim())
        .filter(|raw| library::Fingerprint::parse(raw).is_some())
}

/// Op 14's `write_position`: the whole file written atomically (the card's
/// `write`); the book's fingerprint, which a reader keeps in it, stays. `busy`
/// says a book is in use (a library move carries it): `ERR_BUSY`, nothing
/// written.
///
/// A file this cannot read as text (a hand-edited or damaged one) is
/// replaced by the new place: the reader is at a new place anyway. A read
/// that fails (`ERR_IO`, memory) writes nothing, so a card that stumbles does
/// not cost the fingerprint.
pub(super) fn write(
    reads: &CatalogReads,
    book: BookRef<'_>,
    at: &wire::ServiceLibPosition<'_>,
    from_sync: bool,
    busy: &dyn Fn(&str) -> bool,
) -> Result<(), i32> {
    reads.with_path(book, |path| {
        if busy(path) {
            return Err(ERR_BUSY);
        }
        let old = text(&*reads.storage, path)?;
        let text = render(at, from_sync, old.as_deref().and_then(kept_fingerprint))?;
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

/// How `config::quote` writes `value` and how long: whether it is quoted, and
/// the bytes of what it writes between the quotes (or the value itself).
fn quoted_size(value: &str) -> (bool, usize) {
    let plain = !value.is_empty()
        && value.trim() == value
        && !value.starts_with('"')
        && !value.contains(['\n', '\r']);
    if plain {
        return (false, value.len());
    }
    let body = value
        .chars()
        .map(|c| match c {
            '"' | '\\' | '\n' | '\t' => 2,
            '\r' => 0,
            c => c.len_utf8(),
        })
        .sum();
    (true, body)
}

/// `value` as `config::quote` writes it, into room already reserved.
fn push_quoted(text: &mut String, value: &str, quoted: bool) {
    if !quoted {
        text.push_str(value);
        return;
    }
    text.push('"');
    for c in value.chars() {
        match c {
            '"' => text.push_str("\\\""),
            '\\' => text.push_str("\\\\"),
            '\n' => text.push_str("\\n"),
            '\t' => text.push_str("\\t"),
            '\r' => {}
            c => text.push(c),
        }
    }
    text.push('"');
}

/// The file's text for a position, built fallibly with integers only (the
/// percent is the tenth the file keeps) in one reservation. The XPointer is
/// as long as the SDK reads back (`positions::XPOINTER_MAX`) as it is
/// written, escapes included: a longer one is `ERR_INVALID`, never stored to
/// be read back as none.
fn render(
    at: &wire::ServiceLibPosition<'_>,
    from_sync: bool,
    fingerprint: Option<&str>,
) -> Result<String, i32> {
    let xpointer = (at.flags & HAS_XPOINTER != 0).then_some(at.xpointer);
    let (quoted, written) = xpointer.map_or((false, 0), quoted_size);
    if xpointer.is_some_and(|x| x.is_empty() || written > positions::XPOINTER_MAX) {
        return Err(ERR_INVALID);
    }
    let mut text = String::new();
    // The numbers, the keys and the sync mark are under 256 bytes; the
    // XPointer adds its quotes.
    text.try_reserve_exact(256 + written + 2 + fingerprint.map_or(0, str::len))
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
    if let Some(xpointer) = xpointer {
        text.push_str("xpointer = ");
        push_quoted(&mut text, xpointer, quoted);
        text.push('\n');
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
        text.push_str("book = ");
        text.push_str(fingerprint);
        text.push('\n');
    }
    Ok(text)
}

#[cfg(test)]
#[path = "position_tests.rs"]
mod tests;
