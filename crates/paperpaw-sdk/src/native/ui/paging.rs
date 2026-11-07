//! Validated bare UI reply streams. Rows borrow the output buffer.
use super::{Error, wire};
use crate::native::wire::services_wire::ServiceOp;

/// A UI list continuation, obtainable only from the previous page's end.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UiCursor {
    pub(super) offset: u32,
    pub(super) generation: u32,
}
impl UiCursor {
    /// Begin a new enumeration.
    #[must_use]
    pub const fn start() -> Self {
        Self {
            offset: 0,
            generation: 0,
        }
    }
}

/// Metadata for a checked UI page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UiPageEnd {
    total: u32,
    generation: u32,
    next: u32,
    flags: u32,
    paged: bool,
}
impl UiPageEnd {
    /// Total rows visible in this generation.
    #[must_use]
    pub const fn total(self) -> u32 {
        self.total
    }
    /// Current list generation.
    #[must_use]
    pub const fn generation(self) -> u32 {
        self.generation
    }
    /// Some guarded stack entries were left out.
    #[must_use]
    pub const fn has_hidden(self) -> bool {
        self.flags & 4 != 0
    }
    /// Continue this enumeration, or None after its final page.
    #[must_use]
    pub const fn next(self) -> Option<UiCursor> {
        if !self.paged || self.flags & 1 != 0 {
            None
        } else {
            Some(UiCursor {
                offset: self.next,
                generation: self.generation,
            })
        }
    }
}

/// A fully checked UI page. Iterating allocates nothing and borrows only its
/// reply buffer; the service and current callback binding are not retained.
#[derive(Clone, Debug)]
pub struct UiPage<'a, T> {
    bytes: &'a [u8],
    end: UiPageEnd,
    decode: fn(wire::Record<'a>) -> Result<T, Error>,
}
impl<'a, T> UiPage<'a, T> {
    pub(super) fn checked(
        bytes: &'a [u8],
        cursor: UiCursor,
        op: &ServiceOp,
        decode: fn(wire::Record<'a>) -> Result<T, Error>,
    ) -> Result<Self, Error> {
        Self::check(bytes, Some(cursor), op, decode)
    }
    pub(super) fn terminal(
        bytes: &'a [u8],
        op: &ServiceOp,
        decode: fn(wire::Record<'a>) -> Result<T, Error>,
    ) -> Result<Self, Error> {
        Self::check(bytes, None, op, decode)
    }
    fn check(
        bytes: &'a [u8],
        cursor: Option<UiCursor>,
        op: &ServiceOp,
        decode: fn(wire::Record<'a>) -> Result<T, Error>,
    ) -> Result<Self, Error> {
        let mut rest = bytes;
        let mut rows = 0u32;
        loop {
            let (record, length) = wire::record_decode_prefix(rest).map_err(Error::from_status)?;
            rest = rest.get(length..).ok_or(Error::Invalid)?;
            if let wire::Record::ServiceEnd(end) = record {
                if !rest.is_empty() {
                    return Err(Error::Invalid);
                }
                if op.paged {
                    let cursor = cursor.ok_or(Error::State)?;
                    if end.generation == 0
                        || end.next != cursor.offset.checked_add(rows).ok_or(Error::Invalid)?
                        || end.next > end.total
                        || end.flags & 1 != 0 && end.next != end.total
                        || end.flags & 1 == 0 && (end.next == end.total || rows == 0)
                    {
                        return Err(Error::Invalid);
                    }
                    if cursor.generation != 0 && end.generation != cursor.generation {
                        return Err(Error::State);
                    }
                } else if cursor.is_some()
                    || end.total != rows
                    || end.next != 0
                    || end.generation != 0
                    || end.flags != 0
                {
                    return Err(Error::Invalid);
                }
                return Ok(Self {
                    bytes,
                    end: UiPageEnd {
                        total: end.total,
                        generation: end.generation,
                        next: end.next,
                        flags: end.flags,
                        paged: op.paged,
                    },
                    decode,
                });
            }
            decode(record)?;
            rows += 1;
            if rows > u32::from(op.reply_rows) {
                return Err(Error::Invalid);
            }
        }
    }
    /// The validated continuation and visibility metadata.
    #[must_use]
    pub const fn end(&self) -> UiPageEnd {
        self.end
    }
}
impl<'a, T> Iterator for UiPage<'a, T> {
    type Item = Result<T, Error>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.bytes.is_empty() {
            return None;
        }
        match wire::record_decode_prefix(self.bytes) {
            Ok((wire::Record::ServiceEnd(_), _)) => {
                self.bytes = &[];
                None
            }
            Ok((row, len)) => {
                self.bytes = &self.bytes[len..];
                Some((self.decode)(row))
            }
            Err(error) => {
                self.bytes = &[];
                Some(Err(Error::from_status(error)))
            }
        }
    }
}
