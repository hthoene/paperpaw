//! The host's records by type: each op decodes the record types it expects and
//! encodes the ones it answers with, so the X4 image links neither the
//! enum-wide decoder (`Record`, ~500 records) nor its encoder (todo 129).
//!
//! Statuses are those of the enum-wide calls these replace: a frame error is
//! the framing's, a record of another tag than expected is `ERR_INVALID`.
use super::registrations::{
    RawRecord, Record, WireEncode, WireRecord, as_wire, decode_record, record_encode_of,
    record_framed_len_of, records_encode_of, unexpected,
};
use paperpaw_native::host_glue::ERR_NO_SPACE;

/// `raw` as the record type `R`. A tag the codec does not know is
/// `ERR_UNSUPPORTED`; a known tag the op does not expect is `ERR_INVALID`
/// ([`unexpected`]). An expected tag with a bad body has the decoder's own
/// status (`ERR_INVALID` for a body that ends early, `ERR_NO_SPACE` for an
/// over-cap field, and so on), as the enum path had.
pub(crate) fn decode<'a, R: WireRecord<'a>>(raw: RawRecord<'a>) -> Result<R, i32> {
    if raw.tag() != R::TAG {
        return Err(unexpected(raw.tag()));
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

/// The bare record of type `R` at the front of `bytes`, and the frame length.
pub(crate) fn prefix<'a, R: WireRecord<'a>>(bytes: &'a [u8]) -> Result<(R, usize), i32> {
    let (raw, used) = RawRecord::prefix(bytes)?;
    Ok((decode(raw)?, used))
}

// ---- `Record` values of the host's subset (`registrations::subset`) ----

/// A batch of exactly one record, as the `Record` its tag names (the few the
/// host reads through the enum: `registrations::subset::decode_record`).
pub(crate) fn decode_single(bytes: &[u8]) -> Result<Record<'_>, i32> {
    decode_record(RawRecord::single(bytes)?)
}

/// The bare record at the front of `bytes`, and the frame length.
pub(crate) fn decode_prefix(bytes: &[u8]) -> Result<(Record<'_>, usize), i32> {
    let (raw, used) = RawRecord::prefix(bytes)?;
    Ok((decode_record(raw)?, used))
}

/// Any record of the subset, in tests that read the reply of any op.
#[cfg(test)]
pub(crate) fn decode_any_single(bytes: &[u8]) -> Result<Record<'_>, i32> {
    super::registrations::decode_any(RawRecord::single(bytes)?)
}

/// The bare record at the front of `bytes`, any of the subset, and the frame length.
#[cfg(test)]
pub(crate) fn decode_any_prefix(bytes: &[u8]) -> Result<(Record<'_>, usize), i32> {
    let (raw, used) = RawRecord::prefix(bytes)?;
    Ok((super::registrations::decode_any(raw)?, used))
}

/// One bare `Record` at the front of `out`; the length written.
pub(crate) fn encode_one(record: &Record<'_>, out: &mut [u8]) -> Result<usize, i32> {
    record_encode_of(as_wire(record)?, out)
}

/// One event as the one-record batch (count 1, the record) the kernel hands
/// to a plugin's callback; the length written.
pub(crate) fn encode_event(record: &Record<'_>, out: &mut [u8]) -> Result<usize, i32> {
    encode_batch(core::slice::from_ref(record), out)
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
