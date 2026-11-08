//! Counted batches of records made on the fly, encoded through the typed codec: only the
//! encoders of the record types a facade builds are linked, never the whole `Record` enum.
//!
//! The framing, the limits and the statuses are those of
//! `records_wire::records_encode_counted_of`; this walks rows of several record types
//! (a local enum, one variant each) that a `&dyn WireEncode` iterator cannot hold.
use super::declare::records_wire::{self as wire, WireEncode};

/// A row that shows the record it holds as a typed encoder.
pub(crate) trait AsWire {
    fn wire(&self) -> &dyn WireEncode;
}

fn varint_len(mut value: u32) -> usize {
    let mut len = 1;
    while value >= 0x80 {
        len += 1;
        value >>= 7;
    }
    len
}

fn write_varint(mut value: u32, out: &mut [u8]) -> usize {
    let mut at = 0;
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        out[at] = byte | if value == 0 { 0 } else { 0x80 };
        at += 1;
        if value == 0 {
            return at;
        }
    }
}

/// A counted batch of at most `max_records` records and `max_bytes` bytes. The iterator is
/// cloned and walked three times (count, size, write) and must yield the same rows each
/// time. Every cap and the room are checked before any write; a failure is ERR_NO_SPACE
/// and nothing is ever cut. A walk that differs on the writing pass is ERR_INVALID.
pub(crate) fn encode_counted<R, I>(
    rows: I,
    max_records: usize,
    max_bytes: usize,
    out: &mut [u8],
) -> Result<usize, i32>
where
    R: AsWire,
    I: Iterator<Item = R> + Clone,
{
    let count = rows.clone().take(max_records.saturating_add(1)).count();
    let Ok(count32) = u32::try_from(count) else {
        return Err(-3);
    };
    if count > max_records {
        return Err(-3);
    }
    let mut required = varint_len(count32);
    for row in rows.clone() {
        required += wire::record_framed_len_of(row.wire())?;
        if required > max_bytes {
            return Err(-3);
        }
    }
    if out.len() < required {
        return Err(-3);
    }
    let mut offset = write_varint(count32, out);
    let mut written = 0;
    for row in rows {
        written += 1;
        let rest = out.get_mut(offset..required).ok_or(-2)?;
        offset += wire::record_encode_of(row.wire(), rest).map_err(|_| -2)?;
    }
    if written != count || offset != required {
        return Err(-2);
    }
    Ok(offset)
}
