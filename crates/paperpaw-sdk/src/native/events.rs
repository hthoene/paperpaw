//! Borrowed interface-7 host events, decoded without allocations.
//! All strings borrow the
//! callback input; decoding allocates nothing and copies no strings.
use super::{
    Error,
    declare::records_wire::{self as wire, Record},
};

mod core_events;
mod states;
pub use states::{BookKind, ChargeFlags, PowerChange, WifiLinkState};

/// Registry sections that changed. Unknown future bits remain available through [`Self::bits`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegistryChanges(u32);
impl RegistryChanges {
    /// Keep every host-stamped bit, including future sections.
    #[must_use]
    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }
    /// The complete host-stamped mask.
    #[must_use]
    pub const fn bits(self) -> u32 {
        self.0
    }
    /// Menus or live item state changed.
    #[must_use]
    pub const fn menus(self) -> bool {
        self.0 & 1 != 0
    }
    /// Key bindings changed.
    #[must_use]
    pub const fn bindings(self) -> bool {
        self.0 & (1 << 1) != 0
    }
    /// Slot bindings or offers changed.
    #[must_use]
    pub const fn slots(self) -> bool {
        self.0 & (1 << 2) != 0
    }
    /// Actions or screens changed.
    #[must_use]
    pub const fn actions_or_screens(self) -> bool {
        self.0 & (1 << 3) != 0
    }
    /// Setup steps changed.
    #[must_use]
    pub const fn setup(self) -> bool {
        self.0 & (1 << 4) != 0
    }
    /// Settings groups changed.
    #[must_use]
    pub const fn groups(self) -> bool {
        self.0 & (1 << 5) != 0
    }
    /// Presets changed.
    #[must_use]
    pub const fn presets(self) -> bool {
        self.0 & (1 << 6) != 0
    }
    /// Widget discovery changed.
    #[must_use]
    pub const fn widgets(self) -> bool {
        self.0 & (1 << 7) != 0
    }
    /// One of this plugin's staged registry batches was dropped when applied.
    #[must_use]
    pub const fn dropped(self) -> bool {
        self.0 & (1 << 31) != 0
    }
}

/// Validated requested plugin identities, borrowed from one event callback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RequestedPlugins<'a>(&'a str);
impl<'a> RequestedPlugins<'a> {
    /// Iterate complete plugin identities in their original order without allocating.
    pub fn iter(self) -> impl Iterator<Item = &'a str> + Clone {
        self.0
            .split(wire::EVENT_PLUGINS_REQUESTED_PLUGINS_SEPARATOR)
    }
    /// Number of requested identities, bounded by the generated event-list limit.
    #[must_use]
    pub fn len(self) -> usize {
        self.iter().count()
    }
    /// Whether no identities were requested. A decoded request is always nonempty.
    #[must_use]
    pub fn is_empty(self) -> bool {
        self.0.is_empty()
    }
    fn decode(text: &'a str) -> Result<Self, Error> {
        let plugins = Self(text);
        if plugins.len() > wire::EVENT_PLUGINS_REQUESTED_PLUGINS_MAX_ITEMS
            || plugins.iter().any(|id| {
                id.is_empty() || id.len() > wire::EVENT_PLUGINS_REQUESTED_PLUGINS_ITEM_MAX_BYTES
            })
        {
            return Err(Error::Invalid);
        }
        Ok(plugins)
    }
}

/// A reading position borrowed from a page turn or book-session event.
///
/// The view cannot outlive the callback bytes:
///
/// ```compile_fail
/// use paperpaw_sdk::native::events::{Event, ReadingView};
/// fn escape(bytes: &[u8]) -> Option<ReadingView<'static>> {
///     match Event::decode(bytes).ok()? {
///         Event::Book { reading, .. } => Some(reading),
///         _ => None,
///     }
/// }
/// ```
///
/// Counters are absent until
/// counted; a present counter is 1-based. Progress is in millionths.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct ReadingView<'a> {
    /// Emission time on the snapshot's monotonic millisecond clock.
    now_ms: u64,
    /// Start section.
    section: u32,
    /// Offset within the section.
    offset: u32,
    /// Insert ordinal at this text position; zero is source text.
    insert: u32,
    /// Progress from 0 to 1,000,000.
    progress_ppm: u32,
    /// Book path.
    path: &'a str,
    /// Book title.
    title: &'a str,
    /// Chapter title, empty when unknown.
    chapter: &'a str,
    /// Page number, if counted.
    page: Option<u32>,
    /// Page count, if counted.
    pages: Option<u32>,
}

impl<'a> ReadingView<'a> {
    /// Emission time in monotonic milliseconds.
    #[must_use]
    pub fn now_ms(&self) -> u64 {
        self.now_ms
    }
    /// Start section.
    #[must_use]
    pub fn section(&self) -> u32 {
        self.section
    }
    /// Offset within the section.
    #[must_use]
    pub fn offset(&self) -> u32 {
        self.offset
    }
    /// Insert ordinal; zero is source text.
    #[must_use]
    pub fn insert(&self) -> u32 {
        self.insert
    }
    /// Reading progress in millionths.
    #[must_use]
    pub fn progress_ppm(&self) -> u32 {
        self.progress_ppm
    }
    /// Complete root-1 book path.
    #[must_use]
    pub fn path(&self) -> &'a str {
        self.path
    }
    /// Book title.
    #[must_use]
    pub fn title(&self) -> &'a str {
        self.title
    }
    /// Chapter title, empty when unknown.
    #[must_use]
    pub fn chapter(&self) -> &'a str {
        self.chapter
    }
    /// One-based page number, if counted.
    #[must_use]
    pub fn page(&self) -> Option<u32> {
        self.page
    }
    /// Page count, if counted.
    #[must_use]
    pub fn pages(&self) -> Option<u32> {
        self.pages
    }
    fn validate(self) -> Result<Self, Error> {
        if self.progress_ppm > 1_000_000
            || self.page == Some(0)
            || self.pages == Some(0)
            || matches!((self.page, self.pages), (Some(p), Some(n)) if p > n)
        {
            Err(Error::Invalid)
        } else {
            Ok(self)
        }
    }
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
    PageTurned(ReadingView<'a>),
    /// A screen became visible or its pane changed size.
    ScreenShown {
        now_ms: u64,
        screen: &'a str,
        width: u32,
        height: u32,
    },
    /// A registered action was invoked.
    ActionRun { now_ms: u64, action: &'a str },
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
    /// A setting changed. An empty key asks the plugin to reread retained settings.
    /// Empty old bytes mean no previous readable value; the new value is read from the host.
    SettingChanged {
        now_ms: u64,
        key: &'a str,
        old: Option<super::settings::SettingValue<'a>>,
    },
    /// A book session edge with the host-stamped reading position.
    /// Reason 0 is an ordinary open/close or a finished book. Reason 1 is a
    /// restored open or a sleep/shutdown close. Reserved reasons become 0.
    Book {
        kind: BookKind,
        reason: u32,
        reading: ReadingView<'a>,
    },
    /// A root-1 book moved; paths are complete and never truncated.
    BookMoved {
        now_ms: u64,
        from: &'a str,
        path: &'a str,
    },
    /// A root-1 book was removed.
    BookRemoved { now_ms: u64, path: &'a str },
    /// Installed plugin state changed; read inventory through its owning service.
    PluginsChanged { now_ms: u64 },
    /// The radio connection state changed.
    WifiChanged { now_ms: u64, state: WifiLinkState },
    /// The interface-6 power mode or level changed.
    PowerChanged(PowerChange<'a>),
    /// Status items changed; read them through the owning stage.
    StatusChanged { now_ms: u64 },
    /// Charging, external power or low-battery state changed.
    ChargeChanged { now_ms: u64, flags: ChargeFlags },
    /// Registry sections changed; reread the sections used by this plugin.
    RegistryChanged { now_ms: u64, what: RegistryChanges },
    /// The active settings preview changed or ended.
    PreviewChanged { now_ms: u64 },
    /// Another plugin asks subscribers to offer installation. This installs nothing itself.
    PluginsRequested {
        now_ms: u64,
        request: u32,
        by: &'a str,
        plugins: RequestedPlugins<'a>,
    },
    /// One requested plugin received an installation outcome. Delivered only to the requester.
    /// Reserved outcome values become Failed.
    PluginAnswered {
        now_ms: u64,
        request: u32,
        plugin: &'a str,
        outcome: super::commands::PluginOutcome,
    },
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
        Record::EventPageTurned(r) => Event::PageTurned(
            ReadingView {
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
            }
            .validate()?,
        ),
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
        other => decode_registry(other)?,
    })
}

fn decode_registry(record: Record<'_>) -> Result<Event<'_>, Error> {
    Ok(match record {
        Record::EventRegistryChanged(r) => Event::RegistryChanged {
            now_ms: r.now_ms,
            what: RegistryChanges::from_bits(r.what),
        },
        Record::EventPreviewChanged(r) => Event::PreviewChanged { now_ms: r.now_ms },
        Record::EventPluginsRequested(r) => Event::PluginsRequested {
            now_ms: r.now_ms,
            request: r.request,
            by: r.by,
            plugins: RequestedPlugins::decode(r.plugins)?,
        },
        Record::EventPluginAnswered(r) => {
            use super::commands::PluginOutcome;
            let outcome = match r.outcome {
                0 => PluginOutcome::Installed,
                1 => PluginOutcome::Declined,
                3 => PluginOutcome::NotFound,
                _ => PluginOutcome::Failed,
            };
            Event::PluginAnswered {
                now_ms: r.now_ms,
                request: r.request,
                plugin: r.plugin,
                outcome,
            }
        }
        other => core_events::decode(other)?,
    })
}

#[cfg(test)]
#[path = "events_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "events/core_events_tests.rs"]
mod core_tests;
