//! Borrowed, schema-checked reads from `core.device` major 1.
//! The caller owns input and reply buffers. Complete replies are validated before
//! any row is exposed; no callback host or heap allocation is retained.
use super::wire::services_wire::{ServiceOp, core_device_v1 as schema};
use super::{
    Error, Host,
    declare::records_wire::{self as wire, Record},
    services::Service,
};
use core::{fmt, marker::PhantomData};

mod rows;
pub use rows::*;

/// A nonzero DEV request number chosen by the requesting plugin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceRequest(core::num::NonZeroU32);
impl DeviceRequest {
    /// Refuse zero, which never identifies a pending request.
    pub const fn new(value: u32) -> Option<Self> {
        match core::num::NonZeroU32::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }
    /// The plugin's original request number.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0.get()
    }
}

/// A cursor returned by a DEV list page. Start again after a State refusal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DeviceCursor {
    offset: u32,
    generation: u32,
}
impl DeviceCursor {
    /// Start an enumeration of the current table.
    #[must_use]
    pub const fn start() -> Self {
        Self {
            offset: 0,
            generation: 0,
        }
    }
}

/// End of a validated DEV list. Unpaged lists have no continuation cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DevicePageEnd {
    total: u32,
    next: Option<DeviceCursor>,
}
impl DevicePageEnd {
    /// Total rows in this table or complete unpaged reply.
    #[must_use]
    pub const fn total(self) -> u32 {
        self.total
    }
    /// The exact cursor to continue, or None after the last page.
    #[must_use]
    pub const fn next_cursor(self) -> Option<DeviceCursor> {
        self.next
    }
}

type Decode<'a, T> = fn(Record<'a>) -> Result<T, Error>;
/// A completely validated list borrowing caller-owned reply bytes.
pub struct DevicePage<'a, T> {
    bytes: &'a [u8],
    len: usize,
    end: DevicePageEnd,
    decode: Decode<'a, T>,
}
impl<T> fmt::Debug for DevicePage<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DevicePage")
            .field("len", &self.len)
            .field("end", &self.end)
            .finish_non_exhaustive()
    }
}
impl<'a, T> DevicePage<'a, T> {
    /// Number of rows on this page.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }
    /// Whether this page contains no rows.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }
    /// Validated continuation state.
    #[must_use]
    pub const fn end(&self) -> DevicePageEnd {
        self.end
    }
    /// Visit the rows in device order.
    pub fn rows(&self) -> DeviceRows<'a, T> {
        DeviceRows {
            bytes: self.bytes,
            remaining: self.len,
            decode: self.decode,
            _row: PhantomData,
        }
    }
}
/// Iterator over already validated DEV rows.
pub struct DeviceRows<'a, T> {
    bytes: &'a [u8],
    remaining: usize,
    decode: Decode<'a, T>,
    _row: PhantomData<T>,
}
impl<T> fmt::Debug for DeviceRows<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeviceRows")
            .field("remaining", &self.remaining)
            .finish_non_exhaustive()
    }
}
impl<'a, T> Iterator for DeviceRows<'a, T> {
    type Item = T;
    fn next(&mut self) -> Option<T> {
        if self.remaining == 0 {
            return None;
        }
        let (record, len) = wire::record_decode_prefix(self.bytes).ok()?;
        self.bytes = self.bytes.get(len..)?;
        self.remaining -= 1;
        (self.decode)(record).ok()
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}
impl<T> ExactSizeIterator for DeviceRows<'_, T> {}

/// A device service handle reusable across callbacks until explicitly released.
#[derive(Debug)]
#[must_use = "release the device service when done"]
pub struct Device {
    service: Service,
}
impl Device {
    /// Open the device service declared by this plugin's metadata.
    ///
    /// # Errors
    /// Returns schema, permission and resource refusals from the host.
    pub fn open(host: &Host<'_>) -> Result<Self, Error> {
        Ok(Self {
            service: Service::open(host, schema::KEY, schema::MAJOR, 0)?,
        })
    }
    /// Release the handle using this callback's host.
    ///
    /// # Errors
    /// Returns the host's refusal for stale or foreign handles.
    pub fn release(self, host: &Host<'_>) -> Result<(), Error> {
        self.service.release(host)
    }
    fn call<'a>(
        &self,
        host: &Host<'_>,
        op: u32,
        records: &[Record<'_>],
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<(&'a [u8], &ServiceOp), Error> {
        let operation = self.service.operation(op).ok_or(Error::Unsupported)?;
        let cap = input.len().min(operation.max_input);
        let len = wire::records_encode_counted(
            records,
            usize::from(operation.input_rows),
            operation.max_input,
            &mut input[..cap],
        )
        .map_err(Error::from_status)?;
        let len = self.service.call(host, op, &input[..len], out)?;
        Ok((out.get(..len).ok_or(Error::Invalid)?, operation))
    }
    fn scalar<'a, T>(
        &self,
        host: &Host<'_>,
        op: u32,
        records: &[Record<'_>],
        input: &mut [u8],
        out: &'a mut [u8],
        decode: Decode<'a, T>,
    ) -> Result<T, Error> {
        let (bytes, _) = self.call(host, op, records, input, out)?;
        let (record, len) = wire::record_decode_prefix(bytes).map_err(|_| Error::Invalid)?;
        if len != bytes.len() {
            return Err(Error::Invalid);
        }
        decode(record)
    }
    fn list<'a, T>(
        &self,
        host: &Host<'_>,
        op: u32,
        cursor: DeviceCursor,
        input: &mut [u8],
        out: &'a mut [u8],
        decode: Decode<'a, T>,
    ) -> Result<DevicePage<'a, T>, Error> {
        if cursor.generation == 0 && cursor.offset != 0 {
            return Err(Error::State);
        }
        let query = Record::ServiceRegQuery(wire::ServiceRegQuery {
            scope: "",
            cursor: cursor.offset,
            generation: cursor.generation,
            flags: 0,
        });
        let (bytes, operation) = if self
            .service
            .operation(op)
            .ok_or(Error::Unsupported)?
            .input
            .is_empty()
        {
            self.call(host, op, &[], input, out)?
        } else {
            self.call(host, op, &[query], input, out)?
        };
        validate_page(bytes, operation, cursor, decode)
    }
}

fn validate_page<'a, T>(
    bytes: &'a [u8],
    op: &ServiceOp,
    cursor: DeviceCursor,
    decode: Decode<'a, T>,
) -> Result<DevicePage<'a, T>, Error> {
    let mut rest = bytes;
    let mut count = 0usize;
    let cap = if op.paged {
        schema::PAGE_MAX_ROWS
    } else {
        usize::from(op.reply_rows)
    };
    loop {
        let (record, len) = wire::record_decode_prefix(rest).map_err(|_| Error::Invalid)?;
        rest = rest.get(len..).ok_or(Error::Invalid)?;
        if let Record::ServiceEnd(end) = record {
            if !rest.is_empty() {
                return Err(Error::Invalid);
            }
            let rows = u32::try_from(count).map_err(|_| Error::Invalid)?;
            let next = if op.paged {
                if end.generation == 0 || end.flags & !1 != 0 {
                    return Err(Error::Invalid);
                }
                if cursor.generation != 0 && end.generation != cursor.generation {
                    return Err(Error::State);
                }
                let expected = cursor.offset.checked_add(rows).ok_or(Error::Invalid)?;
                let last = end.flags & 1 != 0;
                if end.next != expected
                    || end.next > end.total
                    || last != (end.next == end.total)
                    || (!last && count == 0)
                {
                    return Err(Error::Invalid);
                }
                if last {
                    None
                } else {
                    Some(DeviceCursor {
                        offset: end.next,
                        generation: end.generation,
                    })
                }
            } else {
                if end.flags != 0 || end.next != 0 || end.generation != 0 || end.total != rows {
                    return Err(Error::Invalid);
                }
                None
            };
            return Ok(DevicePage {
                bytes,
                len: count,
                end: DevicePageEnd {
                    total: end.total,
                    next,
                },
                decode,
            });
        }
        count = count.checked_add(1).ok_or(Error::Invalid)?;
        if count > cap {
            return Err(Error::Invalid);
        }
        decode(record)?;
    }
}

mod methods;
#[cfg(test)]
mod tests;
