//! Interface-7 event records (tags 32-37): host-side encode, decode and admission.
//!
//! The host builds one event record into a caller-owned buffer (a stack array is
//! enough: the largest record is under 1.1 KB) and hands the bytes to the plugin's
//! `dispatch_event` for the duration of the call. Nothing here allocates, keeps
//! the buffer or calls plugin code. Every failure is an `ERR_*` status and a
//! failed encode leaves the output unchanged.
//!
//! The wire form is a batch of exactly one record (the form tick uses), so the
//! plugin decodes any event with the same framing. Dispatch rules that this module
//! only states, never enforces: see [`bypasses_queue`](crate::native_host::events::bypasses_queue).

use super::registrations::{
    EVENT_BATTERY_MEASURED_TAG, EVENT_PAGE_TURNED_CHAPTER_MAX_BYTES,
    EVENT_PAGE_TURNED_PATH_MAX_BYTES, EVENT_PAGE_TURNED_TAG, EVENT_PAGE_TURNED_TITLE_MAX_BYTES,
    EVENT_RESUME_TAG, EVENT_STARTED_TAG, EVENT_STOPPING_TAG, EVENT_SUSPEND_TAG, EVENT_TICK_TAG,
    EventBatteryMeasured, EventPageTurned, EventResume, EventStarted, EventStopping, EventSuspend,
    Record, records_decode, records_encode,
};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_UNSUPPORTED};

/// Whether `tag` names an event a plugin can register for: tick (3) and 32-37.
#[must_use]
pub const fn is_event_tag(tag: u32) -> bool {
    matches!(
        tag,
        EVENT_TICK_TAG
            | EVENT_STARTED_TAG
            | EVENT_RESUME_TAG
            | EVENT_SUSPEND_TAG
            | EVENT_STOPPING_TAG
            | EVENT_BATTERY_MEASURED_TAG
            | EVENT_PAGE_TURNED_TAG
    )
}

/// Whether the event with this tag must skip the per-plugin event queue limit and
/// is never deferred or dropped (amendment S3): `event.suspend` and `event.stopping`.
/// The dispatcher, not the codec, applies this.
#[must_use]
pub const fn bypasses_queue(tag: u32) -> bool {
    matches!(tag, EVENT_SUSPEND_TAG | EVENT_STOPPING_TAG)
}

/// Encode one event record as a one-record batch; returns the bytes written.
///
/// # Errors
/// `ERR_INVALID` when `record` is not an event record (tags 32-37),
/// `ERR_NO_SPACE` when a string is over its cap or `out` is too small.
pub fn encode(record: &Record<'_>, out: &mut [u8]) -> Result<usize, i32> {
    match record {
        Record::EventStarted(_)
        | Record::EventResume(_)
        | Record::EventSuspend(_)
        | Record::EventStopping(_)
        | Record::EventBatteryMeasured(_)
        | Record::EventPageTurned(_) => records_encode(&[*record], out),
        _ => Err(ERR_INVALID),
    }
}

/// Encode `event.started`.
///
/// # Errors
/// `ERR_NO_SPACE` when `out` is too small.
pub fn encode_started(now_ms: u64, out: &mut [u8]) -> Result<usize, i32> {
    encode(&Record::EventStarted(EventStarted { now_ms }), out)
}

/// Encode `event.resume`.
///
/// # Errors
/// `ERR_NO_SPACE` when `out` is too small.
pub fn encode_resume(now_ms: u64, out: &mut [u8]) -> Result<usize, i32> {
    encode(&Record::EventResume(EventResume { now_ms }), out)
}

/// Encode `event.suspend`.
///
/// # Errors
/// `ERR_NO_SPACE` when `out` is too small.
pub fn encode_suspend(now_ms: u64, out: &mut [u8]) -> Result<usize, i32> {
    encode(&Record::EventSuspend(EventSuspend { now_ms }), out)
}

/// Encode `event.stopping`.
///
/// # Errors
/// `ERR_NO_SPACE` when `out` is too small.
pub fn encode_stopping(now_ms: u64, out: &mut [u8]) -> Result<usize, i32> {
    encode(&Record::EventStopping(EventStopping { now_ms }), out)
}

/// Encode `event.battery_measured`.
///
/// # Errors
/// `ERR_NO_SPACE` when `out` is too small.
pub fn encode_battery_measured(now_ms: u64, out: &mut [u8]) -> Result<usize, i32> {
    encode(
        &Record::EventBatteryMeasured(EventBatteryMeasured { now_ms }),
        out,
    )
}

/// Where a book is being read, as the host knows it. Strings may be over the wire
/// caps: [`encode_page_turned`] cuts them at a UTF-8 boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageTurn<'a> {
    /// Host clock in milliseconds.
    pub now_ms: u64,
    /// Start of the page: section.
    pub section: u32,
    /// Start of the page: offset in the section.
    pub offset: u32,
    /// Progress through the book in millionths (0 to 1,000,000).
    pub progress_ppm: u32,
    /// The book's path.
    pub path: &'a str,
    /// The book's title.
    pub title: &'a str,
    /// The chapter's title, empty where unknown.
    pub chapter: &'a str,
    /// The page number once the reader counted pages; 1-based, so never `Some(0)`.
    pub page: Option<u32>,
    /// The page count once counted; never `Some(0)`.
    pub pages: Option<u32>,
}

/// The longest prefix of `text` that fits `cap` bytes and ends on a char boundary.
#[must_use]
pub fn cut(text: &str, cap: usize) -> &str {
    let mut end = cap.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(..end).unwrap_or("")
}

/// Whether a page turn says what can be: progress at most 1,000,000 ppm, a page
/// and a count from 1 (absent where not counted, never 0), the page not past
/// the count.
fn sound(progress_ppm: u32, page: Option<u32>, pages: Option<u32>) -> bool {
    progress_ppm <= 1_000_000
        && page != Some(0)
        && pages != Some(0)
        && !matches!((page, pages), (Some(page), Some(pages)) if page > pages)
}

/// Encode `event.page_turned`, cutting each string to its cap at a char boundary.
///
/// # Errors
/// `ERR_INVALID` for `progress_ppm` over 1,000,000, a `Some(0)` page or count, or a
/// page past the count; `ERR_NO_SPACE` when `out` is too small.
pub fn encode_page_turned(turn: &PageTurn<'_>, out: &mut [u8]) -> Result<usize, i32> {
    if !sound(turn.progress_ppm, turn.page, turn.pages) {
        return Err(ERR_INVALID);
    }
    let record = EventPageTurned {
        now_ms: turn.now_ms,
        section: turn.section,
        offset: turn.offset,
        progress_ppm: turn.progress_ppm,
        path: cut(turn.path, EVENT_PAGE_TURNED_PATH_MAX_BYTES),
        title: cut(turn.title, EVENT_PAGE_TURNED_TITLE_MAX_BYTES),
        chapter: cut(turn.chapter, EVENT_PAGE_TURNED_CHAPTER_MAX_BYTES),
        page: turn.page,
        pages: turn.pages,
    };
    encode(&Record::EventPageTurned(record), out)
}

/// Decode bytes that hold exactly one event record (tags 32-37).
///
/// # Errors
/// The codec's status for malformed framing, an over-cap field or bad UTF-8;
/// `ERR_INVALID` when the batch does not hold exactly one record, or a page
/// turn that [`encode_page_turned`] refuses; `ERR_UNSUPPORTED` when that record
/// is not an event.
pub fn decode(bytes: &[u8]) -> Result<Record<'_>, i32> {
    let batch = records_decode(bytes)?;
    let mut records = batch.iter();
    let (Some(record), None) = (records.next(), records.next()) else {
        return Err(ERR_INVALID);
    };
    match record {
        Record::EventStarted(_)
        | Record::EventResume(_)
        | Record::EventSuspend(_)
        | Record::EventStopping(_)
        | Record::EventBatteryMeasured(_) => Ok(record),
        Record::EventPageTurned(turn) if sound(turn.progress_ppm, turn.page, turn.pages) => {
            Ok(record)
        }
        Record::EventPageTurned(_) => Err(ERR_INVALID),
        _ => Err(ERR_UNSUPPORTED),
    }
}
