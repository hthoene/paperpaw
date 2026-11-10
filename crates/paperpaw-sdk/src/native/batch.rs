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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixtures must fail the test")]
    use super::*;
    use alloc::{vec, vec::Vec};

    #[derive(Clone, Copy)]
    struct Row<'a>(wire::EventActionRun<'a>);
    impl AsWire for Row<'_> {
        fn wire(&self) -> &dyn WireEncode {
            &self.0
        }
    }

    fn rows(count: usize) -> Vec<Row<'static>> {
        const ACTIONS: [&str; 4] = ["", "a", "an.action.of.some.length", "é"];
        (0..count)
            .map(|i| {
                Row(wire::EventActionRun {
                    now_ms: i as u64 * 1_000_003,
                    action: ACTIONS[i % ACTIONS.len()],
                    flags: 0,
                })
            })
            .collect()
    }

    /// This copy of the counted-batch encoder gives the bytes and the statuses of
    /// `records_encode_counted_of`, for every count, cap and room around the limits.
    #[test]
    fn it_encodes_as_the_generated_counted_encoder_does() {
        for count in 0..6 {
            let rows = rows(count);
            let exact = {
                let mut out = vec![0; 512];
                wire::records_encode_counted_of(rows.iter().map(|r| r.0), 8, 512, &mut out).unwrap()
            };
            for max_records in [0, count.saturating_sub(1), count, count + 1] {
                for max_bytes in [0, 1, exact.saturating_sub(1), exact, exact + 1, 512] {
                    for room in [0, 1, exact.saturating_sub(1), exact, exact + 1, 512] {
                        let mut ours = vec![0xA5; room];
                        let mut theirs = vec![0xA5; room];
                        let a =
                            encode_counted(rows.iter().copied(), max_records, max_bytes, &mut ours);
                        let b = wire::records_encode_counted_of(
                            rows.iter().map(|r| r.0),
                            max_records,
                            max_bytes,
                            &mut theirs,
                        );
                        let case = (count, max_records, max_bytes, room);
                        assert_eq!(a, b, "status of {case:?}");
                        assert_eq!(ours, theirs, "bytes of {case:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn counts_over_one_varint_byte_are_framed_alike() {
        let rows = rows(130);
        let mut ours = vec![0; 4096];
        let mut theirs = vec![0; 4096];
        let a = encode_counted(rows.iter().copied(), 200, 4096, &mut ours);
        let b = wire::records_encode_counted_of(rows.iter().map(|r| r.0), 200, 4096, &mut theirs);
        assert_eq!(a, b);
        assert_eq!(ours, theirs);
        assert_eq!(ours[..2], [0x82, 0x01]);
    }
}
