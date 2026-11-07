//! The reply builders of the reads: one record, and a page of whole rows then
//! exactly one `service.end` (`registry.md` 2.1), built in the lent scratch.
use super::check::{self, Row};
use crate::abi7::context::RowsEnd;
use crate::native_host::registrations::{self as wire, Record};
use core::ops::ControlFlow;
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_SPACE, ERR_STATE};

/// A record the host returned, framed into `out`. A record the codec refuses
/// (a field over its cap) is the host's malformed answer.
fn encode(record: &Record<'_>, out: &mut [u8]) -> Result<usize, i32> {
    let length = wire::record_framed_len(record).map_err(|_| ERR_INVALID)?;
    if length > out.len() {
        return Err(ERR_NO_SPACE);
    }
    wire::record_encode_one(record, out)
}

/// The one record of a detail answer: the host calls `ask`'s callback exactly
/// once, `check` accepts it, and it becomes the reply. Returns its length.
pub(super) fn one(
    out: &mut [u8],
    ask: impl FnOnce(&mut dyn FnMut(Record<'_>)) -> Result<(), i32>,
    check: impl Fn(&Record<'_>) -> Result<(), i32>,
) -> Result<usize, i32> {
    let mut written = None;
    ask(&mut |record| {
        written = Some(if written.is_some() {
            Err(ERR_INVALID)
        } else {
            check(&record).and_then(|()| encode(&record, out))
        });
    })?;
    written.unwrap_or(Err(ERR_INVALID))
}

/// Where a paged call started: the cursor and the generation it named.
#[derive(Clone, Copy)]
pub(super) struct Request {
    pub(super) cursor: u32,
    pub(super) generation: u32,
}

/// Rows being written whole into the scratch, with room kept for the `end`.
struct Sink<'a> {
    out: &'a mut [u8],
    available: usize,
    used: usize,
    rows: usize,
    cap: usize,
    kind: Row,
    /// A row did not fit, or the page is full.
    stopped: bool,
    failure: Option<i32>,
}
impl<'a> Sink<'a> {
    fn new(out: &'a mut [u8], cap: usize, kind: Row) -> Result<Self, i32> {
        let reserved = wire::record_framed_len(&Record::ServiceEnd(END))?;
        let available = out.len().checked_sub(reserved).ok_or(ERR_NO_SPACE)?;
        Ok(Self {
            out,
            available,
            used: 0,
            rows: 0,
            cap,
            kind,
            stopped: false,
            failure: None,
        })
    }

    /// Take one row of the host, or tell it to stop.
    fn offer(&mut self, record: &Record<'_>) -> ControlFlow<()> {
        if self.stopped || self.failure.is_some() || self.rows == self.cap {
            self.stopped = true;
            return ControlFlow::Break(());
        }
        let before = &self.out[..self.used];
        let length = check::row(self.kind, record, before)
            .and_then(|()| wire::record_framed_len(record).map_err(|_| ERR_INVALID));
        match length {
            Err(error) => self.failure = Some(error),
            Ok(length) if length > self.available - self.used => self.stopped = true,
            Ok(_) => match wire::record_encode_one(record, &mut self.out[self.used..]) {
                Ok(written) => {
                    self.used += written;
                    self.rows += 1;
                    return ControlFlow::Continue(());
                }
                Err(error) => self.failure = Some(error),
            },
        }
        ControlFlow::Break(())
    }

    /// The closing record after the rows; returns the reply length.
    fn finish(self, end: wire::ServiceEnd) -> Result<usize, i32> {
        Ok(self.used
            + wire::record_encode_one(&Record::ServiceEnd(end), &mut self.out[self.used..])?)
    }
}

const END: wire::ServiceEnd = wire::ServiceEnd {
    flags: 0,
    total: 0,
    generation: 0,
    next: 0,
};

/// A page of a catalog or annotation list. The host's position is checked
/// against the request: a generation is never 0 and, once the caller named
/// one, never another (`ERR_STATE`); the rows are exactly those between the
/// cursor and `next`; a page that is not the last has rows and moves forward.
pub(super) fn list(
    out: &mut [u8],
    rows_cap: usize,
    kind: Row,
    request: Request,
    ask: impl FnOnce(&mut dyn FnMut(Record<'_>) -> ControlFlow<()>) -> Result<RowsEnd, i32>,
) -> Result<usize, i32> {
    let mut sink = Sink::new(out, rows_cap, kind)?;
    let position = ask(&mut |record| sink.offer(&record))?;
    if let Some(error) = sink.failure {
        return Err(error);
    }
    if position.generation == 0 {
        return Err(ERR_INVALID);
    }
    if request.generation != 0 && position.generation != request.generation {
        return Err(ERR_STATE);
    }
    let start = request.cursor.min(position.total);
    let rows = u32::try_from(sink.rows).map_err(|_| ERR_INVALID)?;
    if position.next > position.total || position.next.checked_sub(start) != Some(rows) {
        return Err(ERR_INVALID);
    }
    let last = position.next >= position.total;
    if !last && sink.rows == 0 {
        // A row that fits no page would loop its reader for ever.
        return Err(ERR_NO_SPACE);
    }
    sink.finish(wire::ServiceEnd {
        flags: u32::from(last),
        total: position.total,
        generation: position.generation,
        next: position.next,
    })
}

/// A whole list in one reply (op 22 has no cursor): every row or
/// `ERR_NO_SPACE`, never a part. `end` says last page, `total` the rows.
pub(super) fn whole(
    out: &mut [u8],
    rows_cap: usize,
    kind: Row,
    ask: impl FnOnce(&mut dyn FnMut(Record<'_>) -> ControlFlow<()>) -> Result<(), i32>,
) -> Result<usize, i32> {
    let mut sink = Sink::new(out, rows_cap, kind)?;
    ask(&mut |record| sink.offer(&record))?;
    if let Some(error) = sink.failure {
        return Err(error);
    }
    if sink.stopped {
        return Err(ERR_NO_SPACE);
    }
    let total = u32::try_from(sink.rows).map_err(|_| ERR_INVALID)?;
    sink.finish(wire::ServiceEnd {
        flags: 1,
        total,
        generation: 0,
        next: total,
    })
}
