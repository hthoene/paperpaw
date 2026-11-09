//! One `stage.reply` framed in the caller's own `out`, its payload written
//! there by the subscriber (`docs/abi/stages-services.md` 1.2 and 1.4): no
//! second reply buffer on the X4.
//!
//! The payload is the record's last field, so the host lends the subscriber
//! `out[room..room + cap]`, where `room` is the longest header this reply can
//! have (the subscriber's id, the fixed fields and the widest length prefixes
//! for `cap` payload bytes). Once the subscriber has answered,
//! [`record_encode_tail`] writes the real header at the front and moves the
//! payload down to meet it. The bytes are those of `record_encode_one` for the
//! same record; the tests compare them for every prefix width.

use super::registrations::{
    STAGE_REPLY_PAYLOAD_MAX_BYTES, STAGE_REPLY_SUBSCRIBER_MAX_BYTES, STAGE_REPLY_TAG,
};
use paperpaw_native::host_glue::ERR_NO_SPACE;

/// The fields of a `stage.reply` before its payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ReplyHead<'a> {
    pub(crate) subscriber: &'a str,
    pub(crate) index: u32,
    pub(crate) status: i32,
    pub(crate) flags: u32,
    pub(crate) signature: u64,
}

/// Bytes of `index`, `status`, `flags` and `signature`.
const FIXED: usize = 4 + 4 + 4 + 8;

fn varint_len(mut value: usize) -> usize {
    let mut len = 1;
    while value >= 0x80 {
        len += 1;
        value >>= 7;
    }
    len
}

fn write_varint(mut value: usize, out: &mut [u8], at: &mut usize) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        out[*at] = byte | if value == 0 { 0 } else { 0x80 };
        *at += 1;
        if value == 0 {
            return;
        }
    }
}

fn write_bytes(bytes: &[u8], out: &mut [u8], at: &mut usize) {
    out[*at..*at + bytes.len()].copy_from_slice(bytes);
    *at += bytes.len();
}

/// The body length (bitmap, fields, payload) for `subscriber` bytes of id
/// and `payload` bytes of payload.
fn body_len(subscriber: usize, payload: usize) -> usize {
    1 + varint_len(subscriber) + subscriber + FIXED + varint_len(payload) + payload
}

/// Everything before the payload's bytes.
fn header_len(subscriber: usize, payload: usize) -> usize {
    let body = body_len(subscriber, payload);
    varint_len(STAGE_REPLY_TAG as usize) + varint_len(body) + body - payload
}

fn check_caps(subscriber: usize, cap: usize) -> Result<(), i32> {
    if subscriber > STAGE_REPLY_SUBSCRIBER_MAX_BYTES || cap > STAGE_REPLY_PAYLOAD_MAX_BYTES {
        Err(ERR_NO_SPACE)
    } else {
        Ok(())
    }
}

/// Where the lent payload starts in `out`: the longest header a reply from
/// a subscriber with this id can have for at most `cap` payload bytes.
/// `out` must hold `room + cap` bytes; `ERR_NO_SPACE` over the record's
/// caps.
pub(crate) fn reply_room(subscriber: &str, cap: usize) -> Result<usize, i32> {
    check_caps(subscriber.len(), cap)?;
    // The header only grows with the payload's length (its prefixes).
    Ok(header_len(subscriber.len(), cap))
}

/// Frames the reply whose `payload` bytes the subscriber wrote at
/// `out[room..]` ([`reply_room`] for at most `cap` bytes): the header goes
/// to the front and the payload follows it. Returns the record's length.
///
/// A negative `status` is framed with an empty payload, and the lent region
/// is zeroed, so nothing the subscriber wrote stays behind it. Every check
/// comes before any write: on `ERR_NO_SPACE` (`room` not this reply's,
/// `payload` over `cap`, `out` too small, a cap over the record's) `out` is
/// unchanged.
pub(crate) fn record_encode_tail(
    head: &ReplyHead<'_>,
    room: usize,
    cap: usize,
    payload: usize,
    out: &mut [u8],
) -> Result<usize, i32> {
    if reply_room(head.subscriber, cap)? != room || payload > cap {
        return Err(ERR_NO_SPACE);
    }
    let lent_end = room.checked_add(cap).ok_or(ERR_NO_SPACE)?;
    if out.len() < lent_end {
        return Err(ERR_NO_SPACE);
    }
    let payload = if head.status < 0 {
        out[room..lent_end].fill(0);
        0
    } else {
        payload
    };
    let id = head.subscriber.as_bytes();
    let mut at = 0;
    write_varint(STAGE_REPLY_TAG as usize, out, &mut at);
    write_varint(body_len(id.len(), payload), out, &mut at);
    write_varint(0, out, &mut at); // The bitmap is empty.
    write_varint(id.len(), out, &mut at);
    write_bytes(id, out, &mut at);
    write_bytes(&head.index.to_le_bytes(), out, &mut at);
    write_bytes(&head.status.to_le_bytes(), out, &mut at);
    write_bytes(&head.flags.to_le_bytes(), out, &mut at);
    write_bytes(&head.signature.to_le_bytes(), out, &mut at);
    write_varint(payload, out, &mut at);
    // The header is never longer than the room left for it.
    debug_assert!(at <= room);
    out.copy_within(room..room + payload, at);
    Ok(at + payload)
}

#[cfg(test)]
#[path = "stage_reply_tail_tests.rs"]
mod tests;
