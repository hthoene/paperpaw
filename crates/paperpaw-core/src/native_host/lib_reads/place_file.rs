//! The position file as two slots written in place (#305).
//!
//! A place was saved by writing the whole file anew (a temporary file, two
//! renames: 13 to 25 blocks written on a card for 200 bytes). Now the file
//! the host writes is 1,024 bytes, two slots of a block each, and a save
//! writes one slot where it is ([`Storage::write_at`]): one block, no
//! cluster taken, no folder changed.
//!
//! A slot is the place's text as before (`[position]` and its keys), then
//! the line `slot = <generation> <hash>` and blanks up to a last line end.
//! The hash is of the slot's bytes up to and with the generation (`fnv`,
//! folded to 32 bits). A save writes the slot that does not hold the newest
//! place, with the next generation. Cut off, that block holds what it held
//! or the new place; a block that is neither (a card that does not write a
//! block at once) fails its hash, and the other slot, the place saved
//! before, is read.
//!
//! Both slots are text the readers of the file take (`slot` is a key they
//! skip), so a reader that knows no slots (a plugin built with an SDK of
//! before) reads a place all the same: the second slot's where it has one,
//! at most one save behind. The SDK's own readers pick the newest
//! (`paperpaw_sdk::positions`). A file without slots (written by a reader
//! itself, or by an earlier version) is read as it is and gets its slots
//! with the next save; a place too long for a slot (one with an XPointer,
//! kept when a book is closed) is written as the whole file, as before.

use alloc::vec::Vec;
use core::fmt::Write as _;

use paperpaw_native::host_glue::ERR_NO_MEMORY;
use paperpaw_platform::{Storage, StorageError};
use paperpaw_sdk::digest::fnv1a64;

/// Bytes of a slot: a block of a card.
pub(super) const SLOT: usize = 512;
/// What starts a slot's own line.
const MARK: &[u8] = b"slot = ";

/// The hash a slot keeps of `bytes`.
fn fold(bytes: &[u8]) -> u32 {
    let hash = fnv1a64(bytes);
    u32::try_from((hash ^ (hash >> 32)) & 0xffff_ffff).unwrap_or(0)
}

/// The generation and the place's text of the slot `bytes`, where it is one
/// written whole.
fn slot(bytes: &[u8]) -> Option<(u32, &[u8])> {
    let at = bytes
        .windows(MARK.len())
        .rposition(|window| window == MARK)
        .filter(|&at| at == 0 || bytes[at - 1] == b'\n')?;
    let line = &bytes[at + MARK.len()..];
    let end = line.iter().position(|&b| b == b'\n')?;
    if !line[end..].iter().all(|b| matches!(b, b' ' | b'\n')) {
        return None;
    }
    let (generation, hash) = core::str::from_utf8(&line[..end]).ok()?.split_once(' ')?;
    let hashed = &bytes[..at + MARK.len() + generation.len()];
    if u32::from_str_radix(hash, 16).ok()? != fold(hashed) {
        return None;
    }
    Some((generation.parse().ok()?, &bytes[..at]))
}

/// The slot of the file `data` that holds the newest place: which of the
/// two, its generation and the place's text. `None` for a file without
/// slots.
pub(super) fn live(data: &[u8]) -> Option<(usize, u32, &[u8])> {
    if data.len() != 2 * SLOT {
        return None;
    }
    let first = slot(&data[..SLOT]).map(|(generation, text)| (0, generation, text));
    let second = slot(&data[SLOT..]).map(|(generation, text)| (1, generation, text));
    match (first, second) {
        // The later generation, also once the count has gone round.
        (Some(a), Some(b)) if matches!(a.1.wrapping_sub(b.1), 1..=0x7fff_ffff) => Some(a),
        (a, b) => b.or(a),
    }
}

/// The place's text in the file `data`: its newest slot's, or all of a file
/// without slots.
pub(super) fn record(data: &[u8]) -> &[u8] {
    live(data).map_or(data, |(_, _, text)| text)
}

/// A slot that holds the place `text` as generation `generation`; `None`
/// where the text is too long for one.
fn filled(text: &str, generation: u32) -> Result<Option<Vec<u8>>, i32> {
    let mut line = mark_line(generation);
    // The text, the line with its hash (a blank, 8 digits, the line's end)
    // and the slot's last line end.
    let fits = text.len() + line.len() + 10 < SLOT;
    if !fits || !text.ends_with('\n') {
        return Ok(None);
    }
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(SLOT).map_err(|_| ERR_NO_MEMORY)?;
    bytes.extend_from_slice(text.as_bytes());
    bytes.extend_from_slice(line.as_bytes());
    let hash = fold(&bytes);
    line.clear();
    let _ = write!(line, " {hash:08x}\n");
    bytes.extend_from_slice(line.as_bytes());
    bytes.resize(SLOT - 1, b' ');
    bytes.push(b'\n');
    Ok(Some(bytes))
}

/// `slot = <generation>`, in a text of its own that takes no memory.
fn mark_line(generation: u32) -> Line {
    let mut line = Line::default();
    let _ = write!(line, "slot = {generation}");
    line
}

/// A short text on the stack.
#[derive(Default)]
struct Line {
    bytes: [u8; 24],
    len: usize,
}

impl Line {
    fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }

    fn len(&self) -> usize {
        self.len
    }

    fn clear(&mut self) {
        self.len = 0;
    }
}

impl core::fmt::Write for Line {
    fn write_str(&mut self, text: &str) -> core::fmt::Result {
        let room = self
            .bytes
            .get_mut(self.len..self.len + text.len())
            .ok_or(core::fmt::Error)?;
        room.copy_from_slice(text.as_bytes());
        self.len += text.len();
        Ok(())
    }
}

/// Saves the place `text` in the file `name`, which holds `old` now: into
/// the slot that does not hold the newest place, in place, where the file
/// has slots; as a new file of two slots where it has none; as the whole
/// file, as before, where the text is too long for a slot. On a storage
/// that writes nothing in place, the file is written anew with its slots.
pub(super) fn save(
    storage: &dyn Storage,
    name: &str,
    old: Option<&[u8]>,
    text: &str,
) -> Result<(), i32> {
    let newest = old.and_then(live);
    let generation = newest.map_or(1, |(_, generation, _)| generation.wrapping_add(1));
    let Some(slot) = filled(text, generation)? else {
        return storage.write(name, text.as_bytes()).map_err(status);
    };
    let mut file = Vec::new();
    if let (Some(old), Some((newest, ..))) = (old, newest) {
        let other = 1 - newest;
        let at = if other == 0 { 0 } else { SLOT as u64 };
        match storage.write_at(name, at, &slot) {
            Err(StorageError::Unsupported) => {}
            result => return result.map_err(status),
        }
        file.try_reserve_exact(2 * SLOT)
            .map_err(|_| ERR_NO_MEMORY)?;
        file.extend_from_slice(old);
        file[other * SLOT..(other + 1) * SLOT].copy_from_slice(&slot);
    } else {
        // The first slot, and a second one of blanks.
        file.try_reserve_exact(2 * SLOT)
            .map_err(|_| ERR_NO_MEMORY)?;
        file.extend_from_slice(&slot);
        file.resize(2 * SLOT - 1, b' ');
        file.push(b'\n');
    }
    storage.write(name, &file).map_err(status)
}

/// The status of a write that failed.
fn status(error: StorageError) -> i32 {
    match error {
        StorageError::OutOfMemory => ERR_NO_MEMORY,
        StorageError::Full | StorageError::TooLarge => paperpaw_native::host_glue::ERR_NO_SPACE,
        _ => paperpaw_native::host_glue::ERR_IO,
    }
}
