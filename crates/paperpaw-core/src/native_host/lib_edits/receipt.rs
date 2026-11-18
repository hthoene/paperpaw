//! The receipt of a committed transaction (`library.md` 2.2, op 26): a file
//! `<transaction>.receipt` of a 24-byte header (the plugin, the clock at the
//! commit, the generation the commit answered, the number of edits) and one
//! row of 8 bytes (`id`, `action`) per edit, in the order of the edits. It
//! is read a page at a time and never held. It ends at the plugin's next
//! `edit_begin`, 10 minutes after the commit (where the host has a clock) or
//! when the plugin stops; the start removes the ones a power cut left.
use super::{LibEdits, stage};
use crate::abi7::context::RowsEnd;
use crate::native_host::registrations::{self as wire, Record};
use alloc::{string::String, vec::Vec};
use core::ops::ControlFlow;
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_MEMORY, ERR_NOT_FOUND};

const HEADER: usize = 24;
const ROW: usize = 8;
const EXTENSION: &str = "receipt";
/// How long a receipt is kept.
const LIFE_MS: u64 = 600_000;
/// Receipt files looked at when a plugin's end of receipts is asked.
const LISTED: usize = 16;

/// Writes the receipt of the committed `transaction`: `rows` are the `(id,
/// action)` of each edit.
pub(super) fn write(
    host: &LibEdits,
    transaction: u32,
    (owner, generation): (u64, u32),
    rows: &[(u32, u32)],
) -> Result<(), i32> {
    let edits = u32::try_from(rows.len()).map_err(|_| ERR_INVALID)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(HEADER + rows.len() * ROW)
        .map_err(|_| ERR_NO_MEMORY)?;
    bytes.extend_from_slice(&owner.to_le_bytes());
    bytes.extend_from_slice(&host.clock_ms().to_le_bytes());
    bytes.extend_from_slice(&generation.to_le_bytes());
    bytes.extend_from_slice(&edits.to_le_bytes());
    for (id, action) in rows {
        bytes.extend_from_slice(&id.to_le_bytes());
        bytes.extend_from_slice(&action.to_le_bytes());
    }
    let file = stage::path(transaction, EXTENSION)?;
    host.stage
        .storage()
        .write(&file, &bytes)
        .map_err(stage::status)
}

pub(super) fn remove(host: &LibEdits, transaction: u32) {
    if let Ok(file) = stage::path(transaction, EXTENSION) {
        let _ = host.stage.storage().remove(&file);
    }
}

struct Header {
    owner: u64,
    committed: u64,
    generation: u32,
    edits: u32,
}

fn header(host: &LibEdits, file: &str) -> Option<Header> {
    let mut bytes = [0_u8; HEADER];
    let read = stage::read_full(host.stage.storage(), file, 0, &mut bytes).ok()?;
    if read < HEADER {
        return None;
    }
    let word = |at: usize| -> u32 {
        u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
    };
    let long = |at: usize| u64::from(word(at)) | (u64::from(word(at + 4)) << 32);
    Some(Header {
        owner: long(0),
        committed: long(8),
        generation: word(16),
        edits: word(20),
    })
}

/// Whether the receipt made at `committed` is out of time.
fn expired(host: &LibEdits, committed: u64) -> bool {
    let now = host.clock_ms();
    now != 0 && now.saturating_sub(committed) > LIFE_MS
}

/// The rows from `cursor` on, while `visit` takes them.
///
/// # Errors
/// `ERR_NOT_FOUND`: there is none for the plugin, or it ended.
pub(super) fn read(
    host: &LibEdits,
    owner: u64,
    transaction: u32,
    cursor: u32,
    visit: &mut dyn FnMut(Record<'_>) -> ControlFlow<()>,
) -> Result<RowsEnd, i32> {
    let file = stage::path(transaction, EXTENSION)?;
    let head = header(host, &file).ok_or(ERR_NOT_FOUND)?;
    if head.owner != owner {
        return Err(ERR_NOT_FOUND);
    }
    if expired(host, head.committed) {
        remove(host, transaction);
        return Err(ERR_NOT_FOUND);
    }
    if cursor > head.edits {
        return Err(ERR_INVALID);
    }
    let mut next = cursor;
    let mut row = [0_u8; ROW];
    while next < head.edits {
        let at = (HEADER + next as usize * ROW) as u64;
        let read = stage::read_full(host.stage.storage(), &file, at, &mut row)?;
        if read < ROW {
            return Err(ERR_INVALID);
        }
        let record = Record::ServiceLibReceipt(wire::ServiceLibReceipt {
            edit: next,
            id: u32::from_le_bytes([row[0], row[1], row[2], row[3]]),
            action: u32::from_le_bytes([row[4], row[5], row[6], row[7]]),
        });
        if visit(record).is_break() {
            break;
        }
        next += 1;
    }
    Ok(RowsEnd {
        total: head.edits,
        generation: head.generation,
        next,
    })
}

/// Removes the receipts of `owner`, and the ones out of time.
pub(super) fn forget(host: &LibEdits, owner: u64) {
    let mut names: Vec<String> = Vec::new();
    if names.try_reserve(LISTED).is_err() {
        return;
    }
    let _ = host.stage.storage().list(stage::DIR, &mut |entry| {
        if names.len() < LISTED
            && entry.name.ends_with(EXTENSION)
            && let mut name = String::new()
            && name
                .try_reserve_exact(stage::DIR.len() + 1 + entry.name.len())
                .is_ok()
        {
            name.push_str(stage::DIR);
            name.push('/');
            name.push_str(entry.name);
            names.push(name);
        }
    });
    for file in &names {
        if let Some(head) = header(host, file)
            && (head.owner == owner || expired(host, head.committed))
        {
            let _ = host.stage.storage().remove(file);
        }
    }
}
