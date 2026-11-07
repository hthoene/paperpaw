//! Borrowed interface-7 events, including stage subscriber calls.
//! All strings borrow the
//! callback input; decoding allocates nothing and copies no strings.
use super::{
    Error,
    declare::records_wire::{self as wire, Record},
};

/// A page location borrowed from the event bytes. Counters are absent until
/// counted; a present counter is 1-based. Progress is in millionths.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct PageTurn<'a> {
    /// Emission time on the snapshot's monotonic millisecond clock.
    pub now_ms: u64,
    /// Start section.
    pub section: u32,
    /// Offset within the section.
    pub offset: u32,
    /// Insert ordinal at this text position; zero is source text.
    pub insert: u32,
    /// Progress from 0 to 1,000,000.
    pub progress_ppm: u32,
    /// Book path.
    pub path: &'a str,
    /// Book title.
    pub title: &'a str,
    /// Chapter title, empty when unknown.
    pub chapter: &'a str,
    /// Page number, if counted.
    pub page: Option<u32>,
    /// Page count, if counted.
    pub pages: Option<u32>,
}

/// One event, borrowing its callback's bytes. The scalar variants carry the
/// emission time on the snapshot's monotonic millisecond clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Event<'a> {
    /// A scheduled tick callback.
    Tick { now_ms: u64 },
    /// PaperPaw started.
    Started { now_ms: u64 },
    /// PaperPaw resumed.
    Resume { now_ms: u64 },
    /// PaperPaw is suspending; save synchronously before returning.
    Suspend { now_ms: u64 },
    /// PaperPaw is stopping; save synchronously before returning.
    Stopping { now_ms: u64 },
    /// A fresh battery measurement is available through the snapshot.
    BatteryMeasured { now_ms: u64 },
    /// The displayed page changed.
    PageTurned(PageTurn<'a>),
    /// A screen became visible or its pane changed size.
    ScreenShown {
        now_ms: u64,
        screen: &'a str,
        width: u32,
        height: u32,
    },
    /// An extension stage calls this subscription's token. Answer through
    /// [`super::reply::Reply`] using the stage owner's payload schema.
    ///
    /// ```
    /// use paperpaw_sdk::native::{Error, events::Event, reply::Reply};
    /// fn answer(event: Event<'_>, reply: &mut Reply<'_>, encoded: &[u8])
    ///     -> Result<(), Error>
    /// {
    ///     match event {
    ///         Event::StageCall { stage: "reader.insert", schema: 1, .. } => {
    ///             // Encode with the stage owner's schema before writing.
    ///             reply.write(encoded)
    ///         }
    ///         Event::StageCall { .. } => Err(Error::Unsupported),
    ///         _ => Ok(()),
    ///     }
    /// }
    /// ```
    StageCall {
        stage: &'a str,
        schema: u32,
        payload: &'a [u8],
    },
    /// A registered action was invoked.
    ActionRun { now_ms: u64, action: &'a str },
}

impl<'a> Event<'a> {
    /// Decode one event, borrowing strings from the callback input.
    ///
    /// The decoded event cannot outlive its input:
    ///
    /// ```compile_fail
    /// use paperpaw_sdk::native::events::Event;
    /// fn escape(input: &[u8]) -> Event<'static> {
    ///     Event::decode(input).unwrap()
    /// }
    /// ```
    ///
    /// # Errors
    /// Returns framing, UTF-8, cap or event-field errors. Other exact tags
    /// return [`Error::Unsupported`].
    pub fn decode(bytes: &'a [u8]) -> Result<Self, Error> {
        decode(bytes)
    }
}

/// Decode exactly one event record. Unknown appended optional fields are
/// accepted; strings remain borrowed from `bytes`.
///
/// # Errors
/// Returns framing/UTF-8/cap errors from the generated codec; Invalid for a
/// non-singleton batch or unsound counters/progress; Unsupported for other tags.
fn decode(bytes: &[u8]) -> Result<Event<'_>, Error> {
    let record = match wire::records_decode_single(bytes) {
        Ok(record) => record,
        Err(paperpaw_native::host_glue::ERR_UNSUPPORTED) => {
            return wire::tick_decode(bytes)
                .map(|now_ms| Event::Tick { now_ms })
                .map_err(Error::from_status);
        }
        Err(status) => return Err(Error::from_status(status)),
    };
    Ok(match record {
        Record::StageCall(r) => Event::StageCall {
            stage: r.stage,
            schema: r.schema,
            payload: r.payload,
        },
        Record::EventStarted(r) => Event::Started { now_ms: r.now_ms },
        Record::EventResume(r) => Event::Resume { now_ms: r.now_ms },
        Record::EventSuspend(r) => Event::Suspend { now_ms: r.now_ms },
        Record::EventStopping(r) => Event::Stopping { now_ms: r.now_ms },
        Record::EventBatteryMeasured(r) => Event::BatteryMeasured { now_ms: r.now_ms },
        Record::EventPageTurned(r) => {
            if r.progress_ppm > 1_000_000
                || r.page == Some(0)
                || r.pages == Some(0)
                || matches!((r.page, r.pages), (Some(p), Some(n)) if p > n)
            {
                return Err(Error::Invalid);
            }
            Event::PageTurned(PageTurn {
                now_ms: r.now_ms,
                section: r.section,
                offset: r.offset,
                insert: r.insert.unwrap_or(0),
                progress_ppm: r.progress_ppm,
                path: r.path,
                title: r.title,
                chapter: r.chapter,
                page: r.page,
                pages: r.pages,
            })
        }
        Record::EventScreenShown(r) => Event::ScreenShown {
            now_ms: r.now_ms,
            screen: r.screen,
            width: r.width,
            height: r.height,
        },
        Record::EventActionRun(r) => Event::ActionRun {
            now_ms: r.now_ms,
            action: r.action,
        },
        _ => return Err(Error::Unsupported),
    })
}

#[cfg(test)]
#[path = "events_tests.rs"]
mod tests;
