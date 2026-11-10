//! Interface-7 event records (tags 32-39): host-side encode, decode and admission.
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
    EVENT_ACTION_RUN_TAG, EVENT_BATTERY_MEASURED_TAG, EVENT_BOOK_CLOSED_TAG,
    EVENT_BOOK_FINISHED_TAG, EVENT_BOOK_OPENED_TAG, EVENT_BROADCAST_TAG, EVENT_CHARGE_CHANGED_TAG,
    EVENT_DOCUMENT_CLOSED_TAG, EVENT_FIRMWARE_CHANGED_TAG, EVENT_HTTP_DONE_TAG,
    EVENT_HTTP_PROGRESS_TAG, EVENT_IMAGE_READY_TAG, EVENT_INSTALL_DONE_TAG, EVENT_LOOKUP_READY_TAG,
    EVENT_PAGE_TURNED_CHAPTER_MAX_BYTES, EVENT_PAGE_TURNED_PATH_MAX_BYTES, EVENT_PAGE_TURNED_TAG,
    EVENT_PAGE_TURNED_TITLE_MAX_BYTES, EVENT_PLUGINS_CHANGED_TAG, EVENT_POWER_CHANGED_TAG,
    EVENT_PREVIEW_CHANGED_TAG, EVENT_REGISTRY_CHANGED_TAG, EVENT_RESUME_TAG,
    EVENT_SCREEN_READY_TAG, EVENT_SCREEN_SHOWN_TAG, EVENT_SERVED_REQUEST_TAG, EVENT_SERVER_TAG,
    EVENT_SETTING_CHANGED_TAG, EVENT_SOCKET_TAG, EVENT_STARTED_TAG, EVENT_STATUS_CHANGED_TAG,
    EVENT_STOPPING_TAG, EVENT_SUSPEND_TAG, EVENT_TICK_TAG, EVENT_WIDGET_CHANGED_TAG,
    EVENT_WIFI_CHANGED_TAG, EVENT_WIFI_DONE_TAG, EVENT_WORK_ALLOWED_TAG, EVENT_WORK_DUE_TAG,
    EVENT_WORK_REFUSED_TAG, EventBatteryMeasured, EventPageTurned, EventResume, EventStarted,
    EventStopping, EventSuspend, REGISTRATION_MAX_RECORDS, Record,
};
use super::registrations::{EVENT_BOOK_MOVED_TAG, EVENT_BOOK_REMOVED_TAG, EVENT_STACK_CHANGED_TAG};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_SPACE, ERR_UNSUPPORTED};

/// Whether `tag` names an event a plugin can register for: tick (3), 32-37,
/// `event.setting_changed` (64), the hints 70-74 and the registry's hints
/// `event.registry_changed` (75) and `event.preview_changed` (76)
/// (`setting_events`), `event.widget_changed` (86, `widget_events`), the
/// firmware events 96 and 97 (`firmware_events`), the events of the services
/// the kernel pumps: `core.net` 99-102 and `core.sockets` 106-107 and
/// `core.document` 108 (`service_pump`), `event.image_ready` 90
/// (`image_pump`) and `event.lookup_ready` 110 (`core.lookup`; both check the
/// subscription before they deliver), and `event.book_opened`,
/// `event.book_closed`, `event.book_finished`, `event.book_moved` and
/// `event.book_removed` (65-69): registered now,
/// delivered when the reader port produces them (`core-events.md` 3.3: the
/// host never fails a registration for an event a device does not raise, and
/// until the reading session reports, nothing raises these). The same holds
/// for `event.stack_changed` (81, a hint, `ui.md` 5.1): a plugin that closes
/// what a covering screen hides registers for it.
///
/// The answers of a plugin's own requests register as well: `event.wifi_done` (93,
/// ended `join` and `host`; `wifi_events`) and the work events 103-105
/// (`work_pump`).
///
/// Never 38 or 39: those go to the token of the screen or action declaration
/// they name (`docs/abi/screens-input.md` section 2).
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
            | EVENT_HTTP_DONE_TAG
            | EVENT_HTTP_PROGRESS_TAG
            | EVENT_SERVER_TAG
            | EVENT_SERVED_REQUEST_TAG
            | EVENT_SOCKET_TAG
            | EVENT_BROADCAST_TAG
            | EVENT_BOOK_OPENED_TAG
            | EVENT_DOCUMENT_CLOSED_TAG
            | EVENT_IMAGE_READY_TAG
            | EVENT_LOOKUP_READY_TAG
            | EVENT_BOOK_CLOSED_TAG
            | EVENT_BOOK_FINISHED_TAG
            | EVENT_BOOK_MOVED_TAG
            | EVENT_BOOK_REMOVED_TAG
            | EVENT_STACK_CHANGED_TAG
            | EVENT_SETTING_CHANGED_TAG
            | EVENT_PLUGINS_CHANGED_TAG
            | EVENT_WIFI_CHANGED_TAG
            | EVENT_FIRMWARE_CHANGED_TAG
            | EVENT_INSTALL_DONE_TAG
            | EVENT_POWER_CHANGED_TAG
            | EVENT_STATUS_CHANGED_TAG
            | EVENT_CHARGE_CHANGED_TAG
            | EVENT_WIDGET_CHANGED_TAG
            | EVENT_REGISTRY_CHANGED_TAG
            | EVENT_PREVIEW_CHANGED_TAG
            | EVENT_SCREEN_READY_TAG
            | EVENT_WIFI_DONE_TAG
            | EVENT_WORK_DUE_TAG
            | EVENT_WORK_REFUSED_TAG
            | EVENT_WORK_ALLOWED_TAG
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
/// `ERR_INVALID` when `record` is not an event record (tags 32-39),
/// `ERR_NO_SPACE` when a string is over its cap or `out` is too small.
pub fn encode(record: &Record<'_>, out: &mut [u8]) -> Result<usize, i32> {
    match record {
        Record::EventStarted(_)
        | Record::EventResume(_)
        | Record::EventSuspend(_)
        | Record::EventStopping(_)
        | Record::EventBatteryMeasured(_)
        | Record::EventPageTurned(_)
        | Record::EventScreenShown(_)
        | Record::EventActionRun(_) => super::codec::encode_batch(&[*record], out),
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

/// The `event.page_turned` record for `turn`, each string cut to its cap at a
/// char boundary. Nothing is encoded or allocated.
///
/// # Errors
/// `ERR_INVALID` for `progress_ppm` over 1,000,000, a `Some(0)` page or count, or a
/// page past the count.
pub(crate) fn page_turned_record<'a>(turn: &PageTurn<'a>) -> Result<EventPageTurned<'a>, i32> {
    if !sound(turn.progress_ppm, turn.page, turn.pages) {
        return Err(ERR_INVALID);
    }
    Ok(EventPageTurned {
        now_ms: turn.now_ms,
        section: turn.section,
        offset: turn.offset,
        progress_ppm: turn.progress_ppm,
        path: cut(turn.path, EVENT_PAGE_TURNED_PATH_MAX_BYTES),
        title: cut(turn.title, EVENT_PAGE_TURNED_TITLE_MAX_BYTES),
        chapter: cut(turn.chapter, EVENT_PAGE_TURNED_CHAPTER_MAX_BYTES),
        page: turn.page,
        pages: turn.pages,
        // The kernel reader inserts no content: every page starts in text (N5).
        insert: None,
        chapter_left: None,
    })
}

/// Encode `event.page_turned`, cutting each string to its cap at a char boundary.
///
/// # Errors
/// `ERR_INVALID` for `progress_ppm` over 1,000,000, a `Some(0)` page or count, or a
/// page past the count; `ERR_NO_SPACE` when `out` is too small.
pub fn encode_page_turned(turn: &PageTurn<'_>, out: &mut [u8]) -> Result<usize, i32> {
    encode(&Record::EventPageTurned(page_turned_record(turn)?), out)
}

/// Whether the batch's leading count is a whole, canonical varint above
/// `REGISTRATION_MAX_RECORDS`: `ERR_NO_SPACE`, before any record is read (as
/// the counted batch decoder answered). A malformed count is left to the
/// framing's own status.
fn count_over_max(bytes: &[u8]) -> bool {
    let mut count = 0_u32;
    for (index, byte) in bytes.iter().take(5).enumerate() {
        if index == 4 && *byte > 15 {
            return false;
        }
        count |= u32::from(byte & 0x7f) << (7 * index as u32);
        if byte & 0x80 == 0 {
            return (index == 0 || *byte != 0) && count as usize > REGISTRATION_MAX_RECORDS;
        }
    }
    false
}

/// The tags `decode` reads.
const EVENT_TAGS: [u32; 8] = [
    EVENT_STARTED_TAG,
    EVENT_RESUME_TAG,
    EVENT_SUSPEND_TAG,
    EVENT_STOPPING_TAG,
    EVENT_BATTERY_MEASURED_TAG,
    EVENT_SCREEN_SHOWN_TAG,
    EVENT_ACTION_RUN_TAG,
    EVENT_PAGE_TURNED_TAG,
];

/// Decode bytes that hold exactly one event record (tags 32-39).
///
/// # Errors
/// The codec's status for malformed framing, an over-cap field or bad UTF-8;
/// `ERR_INVALID` when the batch does not hold exactly one record, or a page
/// turn that [`encode_page_turned`] refuses; `ERR_UNSUPPORTED` when that record
/// is not an event.
pub fn decode(bytes: &[u8]) -> Result<Record<'_>, i32> {
    // Only the events are read; a record of any other tag, known or not, is
    // `ERR_UNSUPPORTED` before its body is looked at.
    if count_over_max(bytes) {
        return Err(ERR_NO_SPACE);
    }
    let raw = super::registrations::RawRecord::single(bytes)?;
    if !EVENT_TAGS.contains(&raw.tag()) {
        return Err(ERR_UNSUPPORTED);
    }
    let record = super::codec::decode_single(bytes)?;
    match record {
        Record::EventStarted(_)
        | Record::EventResume(_)
        | Record::EventSuspend(_)
        | Record::EventStopping(_)
        | Record::EventBatteryMeasured(_)
        | Record::EventScreenShown(_)
        | Record::EventActionRun(_) => Ok(record),
        Record::EventPageTurned(turn) if sound(turn.progress_ppm, turn.page, turn.pages) => {
            Ok(record)
        }
        Record::EventPageTurned(_) => Err(ERR_INVALID),
        _ => Err(ERR_UNSUPPORTED),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_count_above_the_batch_cap_is_no_space_and_other_counts_keep_their_status() {
        // Count 9, then nothing: no record is read.
        assert_eq!(decode(&[9]).unwrap_err(), ERR_NO_SPACE);
        assert_eq!(decode(&[0x80, 0x01]).unwrap_err(), ERR_NO_SPACE);
        // Counts 2..=8 are not one record: ERR_INVALID.
        for count in 2..=8_u8 {
            assert_eq!(decode(&[count]).unwrap_err(), ERR_INVALID);
        }
        // A non-canonical or truncated count is the framing's own status.
        assert_eq!(decode(&[0x89, 0x00]).unwrap_err(), ERR_INVALID);
        assert_eq!(decode(&[0x89]).unwrap_err(), ERR_INVALID);
        assert_eq!(decode(&[]).unwrap_err(), ERR_INVALID);
    }
}
