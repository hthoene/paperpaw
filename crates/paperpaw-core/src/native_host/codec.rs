//! The host's records by type: each op decodes the record types it expects and
//! encodes the ones it answers with, so the X4 image links neither the
//! enum-wide decoder (`Record`, ~500 records) nor its encoder (todo 129).
//!
//! Statuses are those of the enum-wide calls these replace: a frame error is
//! the framing's, a record of another tag than expected is `ERR_INVALID`.
use super::registrations::{
    RawRecord, Record, WireEncode, WireRecord, as_wire, decode_record, record_encode_of,
    record_framed_len_of, records_encode_of,
};
use paperpaw_native::host_glue::ERR_INVALID;
use paperpaw_native::host_glue::ERR_NO_SPACE;

/// `raw` as the record type `R`; another tag is `ERR_INVALID`.
pub(crate) fn decode<'a, R: WireRecord<'a>>(raw: RawRecord<'a>) -> Result<R, i32> {
    if raw.tag() != R::TAG {
        return Err(ERR_INVALID);
    }
    raw.decode()
}

/// A row of a reply: a record type, or a host-local enum of the types a reply
/// may hold. Another tag is `ERR_INVALID`.
pub(crate) trait Row<'a>: Sized {
    fn from_raw(raw: RawRecord<'a>) -> Result<Self, i32>;
}
impl<'a, R: WireRecord<'a>> Row<'a> for R {
    fn from_raw(raw: RawRecord<'a>) -> Result<Self, i32> {
        decode(raw)
    }
}

/// A batch of exactly one record of type `R` (count 1, the record, nothing after).
pub(crate) fn single<'a, R: WireRecord<'a>>(bytes: &'a [u8]) -> Result<R, i32> {
    decode(RawRecord::single(bytes)?)
}

// ---- `Record` values of the host's subset (`registrations::subset`) ----

/// A batch of exactly one record, as the `Record` its tag names.
pub(crate) fn decode_single(bytes: &[u8]) -> Result<Record<'_>, i32> {
    decode_record(RawRecord::single(bytes)?)
}

/// One bare record, nothing after it.
pub(crate) fn decode_one(bytes: &[u8]) -> Result<Record<'_>, i32> {
    decode_record(RawRecord::one(bytes)?)
}

/// The bare record at the front of `bytes`, and the frame length.
pub(crate) fn decode_prefix(bytes: &[u8]) -> Result<(Record<'_>, usize), i32> {
    let (raw, used) = RawRecord::prefix(bytes)?;
    Ok((decode_record(raw)?, used))
}

/// One bare `Record` at the front of `out`; the length written.
pub(crate) fn encode_one(record: &Record<'_>, out: &mut [u8]) -> Result<usize, i32> {
    record_encode_of(as_wire(record)?, out)
}

/// The framed length of one bare `Record`.
pub(crate) fn framed_len(record: &Record<'_>) -> Result<usize, i32> {
    record_framed_len_of(as_wire(record)?)
}

/// A counted batch of `Record`s (at most `REGISTRATION_MAX_RECORDS`).
pub(crate) fn encode_batch(records: &[Record<'_>], out: &mut [u8]) -> Result<usize, i32> {
    const MAX: usize = 8;
    let Some(first) = records.first() else {
        return records_encode_of(&[], out);
    };
    if records.len() > MAX {
        return Err(ERR_NO_SPACE);
    }
    let filler = as_wire(first)?;
    let mut refs: [&dyn WireEncode; MAX] = [filler; MAX];
    for (place, record) in refs.iter_mut().zip(records) {
        *place = as_wire(record)?;
    }
    records_encode_of(&refs[..records.len()], out)
}
