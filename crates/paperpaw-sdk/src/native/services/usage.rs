//! Bounded readers for the four usage/clock data services.
use super::{Blob, Error, Host, query};
use crate::native::declare::records_wire::{self as wire, Record};
use core::fmt::{self, Write};

/// Global counters since this start of PaperPaw. All counters saturate; they may
/// restart after boot or power-cutting sleep.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct ServiceUsage {
    /// Fast refresh requests.
    pub fast: u32,
    /// Partial refresh requests.
    pub partial: u32,
    /// Full refresh requests.
    pub full: u32,
    /// Refreshed pixels, including repeated refreshes.
    pub pixels: u64,
    /// Handled inputs.
    pub inputs: u32,
    /// Sleeps.
    pub sleeps: u32,
    /// Reported panel busy time, in milliseconds.
    pub panel_ms: u64,
    /// Bluetooth radio time, in milliseconds.
    pub bluetooth_ms: u64,
}

/// One borrowed plugin row. Wider wire counters preserve future hosts; hosts
/// that count in u32 saturate at u32::MAX, meaning at least that much.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct ServicePluginUsage<'a> {
    /// Plugin identity, never truncated.
    pub plugin: &'a str,
    /// Time in plugin code, in microseconds.
    pub busy_us: u64,
    /// Calls into plugin code.
    pub calls: u32,
    /// Time its screen was shown, in milliseconds.
    pub shown_ms: u64,
    /// Fast refresh requests.
    pub fast: u32,
    /// Partial refresh requests.
    pub partial: u32,
    /// Full refresh requests.
    pub full: u32,
    /// Refreshed pixels.
    pub pixels: u64,
    /// Storage bytes read.
    pub read: u64,
    /// Storage bytes written.
    pub written: u64,
    /// Storage sync attempts, including failures.
    pub syncs: u32,
    /// Network requests.
    pub requests: u32,
    /// Refused network requests.
    pub refused: u32,
    /// Time holds prevented sleeping, in milliseconds.
    pub awake_ms: u64,
    /// Last hold reason, empty if none.
    pub awake_reason: &'a str,
}

impl<'a> ServicePluginUsage<'a> {
    fn from_wire(r: wire::ServicePluginUsage<'a>) -> Self {
        Self {
            plugin: r.plugin,
            busy_us: r.busy_us,
            calls: r.calls,
            shown_ms: r.shown_ms,
            fast: r.fast,
            partial: r.partial,
            full: r.full,
            pixels: r.pixels,
            read: r.read,
            written: r.written,
            syncs: r.syncs,
            requests: r.requests,
            refused: r.refused,
            awake_ms: r.awake_ms,
            awake_reason: r.awake_reason,
        }
    }
}

/// End-of-page cursor. Counters from different pages are not a single snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct PageEnd {
    /// Rows in the table, including left-out rows.
    pub total: u32,
    /// Changes when rows are added, removed or reordered, not when counters change.
    pub generation: u32,
    /// Next row offset, counting rows left out for overlong identities.
    pub next: u32,
    /// More rows remain.
    pub truncated: bool,
    /// At least one overlong identity was left out.
    pub left_out: bool,
}

// The X4 payload floor holds all scalar services, including the 270-byte note.
const READ_BYTES: usize = 384;
struct Key {
    bytes: [u8; 128],
    len: usize,
}
impl Key {
    fn new(service: &str) -> Result<Self, Error> {
        let mut key = Self {
            bytes: [0; 128],
            len: 0,
        };
        key.write_str(service).map_err(|_| Error::Invalid)?;
        Ok(key)
    }
    fn text(&self) -> Result<&str, Error> {
        core::str::from_utf8(&self.bytes[..self.len]).map_err(|_| Error::Invalid)
    }
}
impl Write for Key {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        let end = self.len.checked_add(value.len()).ok_or(fmt::Error)?;
        self.bytes
            .get_mut(self.len..end)
            .ok_or(fmt::Error)?
            .copy_from_slice(value.as_bytes());
        self.len = end;
        Ok(())
    }
}

fn fill(blob: &Blob<'_>, offset: &mut u64, out: &mut [u8]) -> Result<usize, Error> {
    let mut len = 0;
    while len < out.len() {
        let n = blob.read(*offset, &mut out[len..])?;
        if n == 0 {
            break;
        }
        len += n;
        *offset = offset
            .checked_add(n as u64)
            .filter(|n| *n <= 4096)
            .ok_or(Error::Invalid)?;
    }
    Ok(len)
}

fn decode_error(code: i32) -> Error {
    match Error::from_status(code) {
        Error::Unsupported => Error::Invalid,
        other => other,
    }
}

fn scalar<T>(
    host: &Host<'_>,
    key: &str,
    visit: impl FnOnce(Record<'_>) -> Result<T, Error>,
) -> Result<T, Error> {
    let blob = query(host, key, 1, 0)?;
    let mut bytes = [0; READ_BYTES];
    let mut offset = 0;
    let len = fill(&blob, &mut offset, &mut bytes)?;
    if len == bytes.len() && blob.read(offset, &mut [0])? != 0 {
        return Err(Error::NoSpace);
    }
    let record = wire::record_decode_one(&bytes[..len]).map_err(decode_error)?;
    blob.release()?;
    visit(record)
}

/// Read global usage counters without allocating.
///
/// # Errors
/// Returns query/read/release errors or Invalid for an unexpected record tag.
pub fn device_usage(host: &Host<'_>) -> Result<ServiceUsage, Error> {
    scalar(host, "core.usage", |record| {
        let Record::ServiceUsage(r) = record else {
            return Err(Error::Invalid);
        };
        Ok(ServiceUsage {
            fast: r.fast,
            partial: r.partial,
            full: r.full,
            pixels: r.pixels,
            inputs: r.inputs,
            sleeps: r.sleeps,
            panel_ms: r.panel_ms,
            bluetooth_ms: r.bluetooth_ms,
        })
    })
}

fn plugin_key(service: &str, id: &str) -> Result<Key, Error> {
    let mut key = Key::new(service)?;
    write!(key, "/{id}").map_err(|_| Error::Invalid)?;
    Ok(key)
}

/// Visit the display name translated in the host's current UI language.
/// The borrowed name is valid only during `visit`.
///
/// # Errors
/// Returns validation, query/read/release errors or Invalid for the wrong tag.
pub fn plugin_name(host: &Host<'_>, id: &str, visit: impl FnOnce(&str)) -> Result<(), Error> {
    let key = plugin_key("core.plugin", id)?;
    scalar(host, key.text()?, |record| {
        let Record::ServicePlugin(r) = record else {
            return Err(Error::Invalid);
        };
        visit(r.name);
        Ok(())
    })
}

/// Visit the last note and its monotonic millisecond timestamp. NotFound means
/// no note; an empty text is a present note. Text is borrowed only during visit.
///
/// # Errors
/// Returns validation, query/read/release errors or Invalid for the wrong tag.
pub fn work_note(host: &Host<'_>, id: &str, visit: impl FnOnce(u64, &str)) -> Result<(), Error> {
    let key = plugin_key("core.work-note", id)?;
    scalar(host, key.text()?, |record| {
        let Record::ServiceWorkNote(r) = record else {
            return Err(Error::Invalid);
        };
        visit(r.at_ms, r.text);
        Ok(())
    })
}

/// Visit one table page, keeping only a 256-byte generated carry buffer plus
/// one X4-sized read buffer. Rows borrow those buffers only during each visit.
/// Visits may occur before a later stream error; commit a page only on success.
///
/// # Errors
/// Returns host/codec errors; Invalid for missing, repeated or nonterminal end
/// records, foreign records or an inconsistent cursor/truncation flag.
pub fn plugin_usage_page(
    host: &Host<'_>,
    from: u32,
    visit: &mut dyn FnMut(&ServicePluginUsage<'_>),
) -> Result<PageEnd, Error> {
    let mut key = Key::new("core.plugin-usage")?;
    write!(key, "/from.{from}").map_err(|_| Error::Invalid)?;
    let blob = query(host, key.text()?, 1, 0)?;
    let mut stream = wire::RecordStream::new();
    let mut bytes = [0; READ_BYTES];
    let mut offset = 0;
    let mut end = None;
    loop {
        let len = fill(&blob, &mut offset, &mut bytes)?;
        if len == 0 {
            break;
        }
        stream
            .feed(&bytes[..len], &mut |r| {
                if end.is_some() {
                    return Err(i32::from(Error::Invalid));
                }
                match r {
                    Record::ServicePluginUsage(row) => visit(&ServicePluginUsage::from_wire(row)),
                    Record::ServiceEnd(r) => {
                        if r.next > r.total
                            || (r.flags & 1 != 0) != (r.next < r.total)
                            || (from < r.total && r.next <= from)
                        {
                            return Err(i32::from(Error::Invalid));
                        }
                        end = Some(PageEnd {
                            total: r.total,
                            generation: r.generation,
                            next: r.next,
                            truncated: r.flags & 1 != 0,
                            left_out: r.flags & 2 != 0,
                        });
                    }
                    _ => return Err(i32::from(Error::Invalid)),
                }
                Ok(())
            })
            .map_err(decode_error)?;
    }
    stream.finish().map_err(decode_error)?;
    let end = end.ok_or(Error::Invalid)?;
    blob.release()?;
    Ok(end)
}

/// Visit all pages without restarting on generation changes. Keep the last row
/// for each id to deduplicate a changing table. Cursor progress and a limit of
/// the first page's total + 1 queries bound the work even if the table changes.
///
/// # Errors
/// Returns page errors or Invalid when the cursor fails to progress or the
/// page limit is reached. Earlier visits may already have occurred on failure.
pub fn for_each_plugin_usage(
    host: &Host<'_>,
    visit: &mut dyn FnMut(&ServicePluginUsage<'_>),
) -> Result<(), Error> {
    let mut from = 0;
    let mut remaining = None;
    loop {
        let end = plugin_usage_page(host, from, visit)?;
        let pages = remaining.get_or_insert(u64::from(end.total) + 1);
        *pages -= 1;
        if end.next >= end.total {
            return Ok(());
        }
        if end.next <= from || *pages == 0 {
            return Err(Error::Invalid);
        }
        from = end.next;
    }
}
