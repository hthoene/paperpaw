//! Typed, allocation-free reads from `core.registry` major 1.
//! Pages borrow caller-owned output. An entire page is checked before rows are
//! exposed. A stale generation returns State; restart explicitly at zero.
use super::wire::services_wire::{core_registry_v1 as schema, service_schema};
use super::{
    Error, Host,
    declare::records_wire::{self as wire, Record},
    services::Service,
};
use core::{fmt, marker::PhantomData};

mod rows;
pub use rows::{
    RegistryBinding, RegistryBindingOrigin, RegistryChoice, RegistryGroup, RegistryItem,
    RegistryMenuKind, RegistryMenuRow, RegistryNeed, RegistryNeedKind, RegistryPlugin,
    RegistryPluginKind, RegistryPluginState, RegistrySetting, RegistrySettingExtra,
    RegistrySettingKind, RegistrySetupPageKind, RegistrySetupStep, RegistrySlot, RegistryValue,
    RegistryValueSource, RegistryWidget,
};

// `docs/abi/registry.md` section 2: a whole page contains at most 32 rows.
const PAGE_MAX_ROWS: usize = 32;

/// A registry position. Only a page's next cursor may continue a generation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PageCursor {
    offset: u32,
    generation: u32,
}
impl PageCursor {
    /// Begin a fresh enumeration against the host's current generation.
    #[must_use]
    pub const fn start() -> Self {
        Self {
            offset: 0,
            generation: 0,
        }
    }
    /// Number of rows preceding this cursor.
    #[must_use]
    pub const fn offset(self) -> u32 {
        self.offset
    }
    /// Registry generation, or zero for a new enumeration.
    #[must_use]
    pub const fn generation(self) -> u32 {
        self.generation
    }
}

/// Which menu rows to read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum MenuQuery {
    /// Evaluated rows in the user's current menu.
    Current,
    /// Every possible entry, with hidden and removed flags.
    All,
    /// Declared default rows in declared order.
    Declared,
}
impl MenuQuery {
    const fn flags(self) -> u32 {
        match self {
            Self::Current => 0,
            Self::All => 1,
            Self::Declared => 2,
        }
    }
}

/// Source of a values query.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ValuesQuery<'a> {
    /// Complete values of this preset.
    Preset(&'a str),
    /// Complete values set by this widget placement.
    Placement(&'a str),
}

/// Cursor metadata checked against every row in its page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegistryPageEnd {
    total: u32,
    generation: u32,
    next: u32,
    last: bool,
}
impl RegistryPageEnd {
    /// Total rows in this generation.
    #[must_use]
    pub const fn total(self) -> u32 {
        self.total
    }
    /// Nonzero registry generation.
    #[must_use]
    pub const fn generation(self) -> u32 {
        self.generation
    }
    /// Whether this is the final page.
    #[must_use]
    pub const fn is_last(self) -> bool {
        self.last
    }
    /// Continue this exact generation, or None once every row has been read.
    #[must_use]
    pub const fn next_cursor(self) -> Option<PageCursor> {
        if self.last {
            None
        } else {
            Some(PageCursor {
                offset: self.next,
                generation: self.generation,
            })
        }
    }
}

type Decode<'a, T> = fn(Record<'a>) -> Result<T, Error>;

/// A validated page borrowing the output buffer. Rows contain no owned strings.
pub struct RegistryPage<'a, T> {
    bytes: &'a [u8],
    count: usize,
    end: RegistryPageEnd,
    decode: Decode<'a, T>,
}
impl<T> fmt::Debug for RegistryPage<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RegistryPage")
            .field("count", &self.count)
            .field("end", &self.end)
            .finish_non_exhaustive()
    }
}
impl<'a, T> RegistryPage<'a, T> {
    /// Number of whole rows in this page.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.count
    }
    /// Whether the page contains no rows.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }
    /// Validated total, generation and continuation.
    #[must_use]
    pub const fn end(&self) -> RegistryPageEnd {
        self.end
    }
    /// Iterate in host order. Page validation already checked every row.
    #[must_use]
    pub fn rows(&self) -> RegistryRows<'a, T> {
        RegistryRows {
            bytes: self.bytes,
            count: self.count,
            decode: self.decode,
            marker: PhantomData,
        }
    }
}

/// An iterator over an immutable validated registry page.
pub struct RegistryRows<'a, T> {
    bytes: &'a [u8],
    count: usize,
    decode: Decode<'a, T>,
    marker: PhantomData<T>,
}
impl<T> fmt::Debug for RegistryRows<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RegistryRows")
            .field("remaining", &self.count)
            .finish_non_exhaustive()
    }
}
impl<'a, T> Iterator for RegistryRows<'a, T> {
    type Item = T;
    fn next(&mut self) -> Option<T> {
        if self.count == 0 {
            return None;
        }
        // Construction validated these immutable bytes with the same decoder.
        let (record, length) = wire::record_decode_prefix(self.bytes).ok()?;
        let row = (self.decode)(record).ok()?;
        self.bytes = self.bytes.get(length..)?;
        self.count -= 1;
        Some(row)
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.count, Some(self.count))
    }
}
impl<T> ExactSizeIterator for RegistryRows<'_, T> {}
impl<T> core::iter::FusedIterator for RegistryRows<'_, T> {}

/// A plugin-owned registry service handle, usable with each current callback host.
#[derive(Debug)]
#[must_use = "release the registry handle when it is no longer needed"]
pub struct Registry {
    service: Service,
}
impl Registry {
    /// Open the registry declared in this plugin's signed metadata.
    ///
    /// # Errors
    /// Returns schema, metadata, permission or resource errors from the host.
    pub fn open(host: &Host<'_>) -> Result<Self, Error> {
        Service::open(host, schema::KEY, schema::MAJOR, schema::REVISION)
            .map(|service| Self { service })
    }
    /// Release this handle through the current callback's host.
    ///
    /// # Errors
    /// Returns the host refusal, including State for an expired or foreign handle.
    pub fn release(self, host: &Host<'_>) -> Result<(), Error> {
        self.service.release(host)
    }

    fn call<'a>(
        &self,
        host: &Host<'_>,
        op: u32,
        record: Record<'_>,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<&'a [u8], Error> {
        let operation = service_schema(schema::KEY, schema::MAJOR)
            .and_then(|s| s.op(op))
            .ok_or(Error::Unsupported)?;
        let capacity = input.len().min(operation.max_input);
        let length =
            wire::records_encode(&[record], &mut input[..capacity]).map_err(Error::from_status)?;
        let length = self.service.call(host, op, &input[..length], out)?;
        out.get(..length).ok_or(Error::Invalid)
    }

    fn page<'a, T>(
        &self,
        host: &Host<'_>,
        query: ListQuery<'_>,
        input: &mut [u8],
        out: &'a mut [u8],
        decode: Decode<'a, T>,
    ) -> Result<RegistryPage<'a, T>, Error> {
        let record = Record::ServiceRegQuery(wire::ServiceRegQuery {
            scope: query.scope,
            cursor: query.cursor.offset,
            generation: query.cursor.generation,
            flags: query.flags,
        });
        let bytes = self.call(host, query.op, record, input, out)?;
        validate_page(bytes, query.cursor, decode)
    }
}

struct ListQuery<'a> {
    op: u32,
    scope: &'a str,
    cursor: PageCursor,
    flags: u32,
}

mod details;
mod methods;
pub use details::{RegistryPluginDetail, RegistryPresetDetail, RegistrySettingDetail};

#[cfg(test)]
mod tests;

/// Bare generated records followed by one terminal paging record. Inputs alone
/// use the counted registration encoding; replies are not limited to eight rows.
struct ReplyRecords<'a> {
    bytes: &'a [u8],
    end: Option<wire::ServiceEnd>,
}
impl<'a> ReplyRecords<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, end: None }
    }
    fn next(&mut self) -> Result<Option<Record<'a>>, Error> {
        if self.end.is_some() {
            return Ok(None);
        }
        if self.bytes.is_empty() {
            return Err(Error::Invalid);
        }
        let (record, length) =
            wire::record_decode_prefix(self.bytes).map_err(|_| Error::Invalid)?;
        self.bytes = self.bytes.get(length..).ok_or(Error::Invalid)?;
        if let Record::ServiceEnd(end) = record {
            if !self.bytes.is_empty() {
                return Err(Error::Invalid);
            }
            self.end = Some(end);
            Ok(None)
        } else {
            Ok(Some(record))
        }
    }
    fn finish(&mut self) -> Result<wire::ServiceEnd, Error> {
        if self.next()?.is_some() {
            return Err(Error::Invalid);
        }
        self.end.ok_or(Error::Invalid)
    }
}

fn validate_page<'a, T>(
    bytes: &'a [u8],
    cursor: PageCursor,
    decode: Decode<'a, T>,
) -> Result<RegistryPage<'a, T>, Error> {
    let mut records = ReplyRecords::new(bytes);
    let mut count = 0;
    while let Some(record) = records.next()? {
        count += 1;
        if count > PAGE_MAX_ROWS {
            return Err(Error::Invalid);
        }
        decode(record)?;
    }
    let end = validate_end(records.finish()?, cursor, count)?;
    Ok(RegistryPage {
        bytes,
        count,
        end,
        decode,
    })
}

fn validate_end(
    end: wire::ServiceEnd,
    cursor: PageCursor,
    count: usize,
) -> Result<RegistryPageEnd, Error> {
    if end.generation == 0 {
        return Err(Error::Invalid);
    }
    if cursor.generation != 0 && cursor.generation != end.generation {
        return Err(Error::State);
    }
    let next = cursor
        .offset
        .checked_add(u32::try_from(count).map_err(|_| Error::Invalid)?)
        .ok_or(Error::Invalid)?;
    let last = end.flags & 1 != 0;
    if end.next != next
        || end.next > end.total
        || last != (end.next == end.total)
        || (!last && count == 0)
    {
        return Err(Error::Invalid);
    }
    Ok(RegistryPageEnd {
        total: end.total,
        generation: end.generation,
        next: end.next,
        last,
    })
}
