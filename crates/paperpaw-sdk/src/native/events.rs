//! Borrowed interface-7 host events and stage subscriber calls, decoded without allocations.
//! All strings borrow the
//! callback input; decoding allocates nothing and copies no strings.
use super::{Error, declare::records_wire as wire};

/// One function per event: the record's body read as its type and made an [`Event`].
/// `$r` is the decoded record, `$body` the event (it may use `?`).
macro_rules! event_fn {
    ($name:ident, $ty:ty, |$r:ident| $body:expr) => {
        // Two entries to one decoder. `inline` is what `decode_any` (every kind) uses,
        // folded into it. `alone` is what `Incoming::decode_as` uses: a function of its
        // own, so that a plugin's link holds the decoders of the kinds it reads and
        // `tools/plugin-symbols.sh --events` can tell which.
        pub(super) mod $name {
            #[allow(unused_imports, reason = "what the decoder of one kind happens to use")]
            use super::*;
            #[inline(always)]
            pub(in crate::native::events) fn inline<'a>(
                raw: wire::RawRecord<'a>,
                _bytes: &'a [u8],
            ) -> Result<Event<'a>, Error> {
                let $r: $ty = raw.decode().map_err(Error::from_status)?;
                Ok($body)
            }
            #[inline(never)]
            pub(in crate::native::events) fn alone<'a>(
                raw: wire::RawRecord<'a>,
                bytes: &'a [u8],
            ) -> Result<Event<'a>, Error> {
                inline(raw, bytes)
            }
        }
    };
}

mod core_events;
mod host_events;
pub mod kinds;
mod states;
pub use states::{
    BluetoothChange, BluetoothDeviceKind, BluetoothKind, BluetoothReason, BookKind, ChargeFlags,
    InstallDone, LibraryChanges, PowerChange, WifiDone, WifiFailure, WifiLinkState,
};

/// A host-issued plugin installation request, obtained from a decoded event.
///
/// ```compile_fail
/// use paperpaw_sdk::native::events::PluginRequestId;
/// let request = PluginRequestId(7);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PluginRequestId(u32);
impl PluginRequestId {
    /// The host-issued number, unchanged.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

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
    /// Pages after this one in its chapter, if the reader knows the chapter's end.
    chapter_left: Option<u32>,
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
    /// The pages after this one up to the end of its chapter, 0 on the chapter's last page;
    /// `None` where the reader has no chapter end (no table of contents, a page before its
    /// first entry, pages not counted yet). Present only with a page and a page count, and
    /// at most the pages after this one.
    #[must_use]
    pub fn chapter_left(&self) -> Option<u32> {
        self.chapter_left
    }
    fn validate(self) -> Result<Self, Error> {
        if self.progress_ppm > 1_000_000
            || self.page == Some(0)
            || self.pages == Some(0)
            || matches!((self.page, self.pages), (Some(p), Some(n)) if p > n)
        {
            return Err(Error::Invalid);
        }
        // Chapter pages left need the counted pages and fit in what follows this page.
        match (self.chapter_left, self.page, self.pages) {
            (None, _, _) => Ok(self),
            (Some(left), Some(page), Some(pages)) if left <= pages - page => Ok(self),
            _ => Err(Error::Invalid),
        }
    }
}

/// Text typed from outside (`event.text_typed`). It may be a password, so `Debug`
/// shows only its length; read it with [`TypedText::as_str`].
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct TypedText<'a>(&'a str);

impl<'a> TypedText<'a> {
    pub(super) const fn new(text: &'a str) -> Self {
        Self(text)
    }

    /// The typed text, borrowed from the callback input.
    #[must_use]
    pub const fn as_str(&self) -> &'a str {
        self.0
    }
}

impl core::fmt::Debug for TypedText<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TypedText")
            .field("len", &self.0.len())
            .finish_non_exhaustive()
    }
}

/// One event, borrowing its callback's bytes. The scalar variants carry the
/// emission time on the snapshot's monotonic millisecond clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Event<'a> {
    /// A scheduled tick callback.
    #[non_exhaustive]
    Tick { now_ms: u64 },
    /// PaperPaw started.
    #[non_exhaustive]
    Started { now_ms: u64 },
    /// PaperPaw resumed.
    #[non_exhaustive]
    Resume { now_ms: u64 },
    /// PaperPaw is suspending; save synchronously before returning.
    #[non_exhaustive]
    Suspend { now_ms: u64 },
    /// PaperPaw is stopping; save synchronously before returning.
    #[non_exhaustive]
    Stopping { now_ms: u64 },
    /// A fresh battery measurement is available through the snapshot.
    #[non_exhaustive]
    BatteryMeasured { now_ms: u64 },
    /// The displayed page changed.
    PageTurned(ReadingView<'a>),
    /// A screen became visible or its pane changed size. `last_input` is what
    /// the user used last before it was shown, as the host saw it (`None` where
    /// the host does not say): a focus at once for buttons, none for a finger.
    #[non_exhaustive]
    ScreenShown {
        now_ms: u64,
        screen: &'a str,
        width: u32,
        height: u32,
        instance: Option<super::ui::Instance>,
        last_input: Option<crate::keys::InputMode>,
    },
    /// A registered action was invoked. `background`: not the reader's own
    /// doing but a rule's or another plugin's run (interface 6
    /// `!env.reader()`), which may wait for a good moment (declared work);
    /// `false` (the reader pressed, chose or mapped it): act at once.
    #[non_exhaustive]
    ActionRun {
        now_ms: u64,
        action: &'a str,
        background: bool,
    },
    /// Text typed from outside: a hardware keyboard's keys or a shell's remote
    /// control (`event.text_typed`, 109). Sent only to the instance on top when it
    /// is a screen offered on slot `core.keyboard`, through `dispatch_event`, and
    /// needs no subscription (naming 109 in a registration refuses the batch). Text
    /// over 1,024 bytes comes in several events cut between characters; `done` is
    /// set only on the last, and confirms the dialog.
    #[non_exhaustive]
    TextTyped {
        now_ms: u64,
        text: TypedText<'a>,
        done: bool,
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
    #[non_exhaustive]
    StageCall {
        stage: &'a str,
        schema: u32,
        payload: &'a [u8],
    },
    /// A setting changed. An empty key asks the plugin to reread retained settings.
    /// Empty old bytes mean no previous readable value; the new value is read from the host.
    #[non_exhaustive]
    SettingChanged {
        now_ms: u64,
        key: &'a str,
        old: Option<super::settings::SettingValue<'a>>,
    },
    /// A book session edge with the host-stamped reading position.
    /// Reason 0 is an ordinary open/close or a finished book. Reason 1 is a
    /// restored open or a sleep/shutdown close. Reserved reasons become 0.
    #[non_exhaustive]
    Book {
        kind: BookKind,
        reason: u32,
        reading: ReadingView<'a>,
    },
    /// A root-1 book moved; paths are complete and never truncated.
    #[non_exhaustive]
    BookMoved {
        now_ms: u64,
        from: &'a str,
        path: &'a str,
    },
    /// A root-1 book was removed.
    #[non_exhaustive]
    BookRemoved { now_ms: u64, path: &'a str },
    /// The library changed (`event.library_changed`): a hint, coalesced per
    /// plugin, so reread what you show. `generation` is the catalog generation;
    /// `path` is the one book the change was about, empty for several. The
    /// plugin's own changes arrive too.
    #[non_exhaustive]
    LibraryChanged {
        now_ms: u64,
        what: LibraryChanges,
        generation: u32,
        path: &'a str,
    },
    /// Installed plugin state changed; read inventory through its owning service.
    #[non_exhaustive]
    PluginsChanged { now_ms: u64 },
    /// The radio connection state changed.
    #[non_exhaustive]
    WifiChanged { now_ms: u64, state: WifiLinkState },
    /// A Wi-Fi scan finished (`event.wifi_scanned`), a hint coalesced per kind:
    /// read the list again. `networks` is the number of networks found.
    #[non_exhaustive]
    WifiScanned { now_ms: u64, networks: u32 },
    /// A `join` or `host` request of this plugin ended, exactly once.
    WifiDone(WifiDone<'a>),
    /// Network work waits for a network (`event.network_wanted`). Only the
    /// network manager (`core.network-manager`) gets it.
    #[non_exhaustive]
    NetworkWanted { now_ms: u64 },
    /// The network is no longer needed (`event.network_idle`); the network
    /// manager may switch the radio off. Only the network manager gets it.
    #[non_exhaustive]
    NetworkIdle { now_ms: u64 },
    /// Firmware installation progressed (`event.firmware_changed`), at most every
    /// 250 ms: a hint, read the details with the device's firmware read.
    #[non_exhaustive]
    FirmwareChanged {
        now_ms: u64,
        request: Option<super::device::DeviceRequest>,
        state: super::device::FirmwareState,
    },
    /// An install, removal, firmware or file-check request of this plugin ended,
    /// exactly once.
    InstallDone(InstallDone<'a>),
    /// A Bluetooth device or scan changed; sent to the plugin bound to
    /// `core.bluetooth` only, so it is declared as a slot offer, not as a
    /// subscription.
    Bluetooth(BluetoothChange<'a>),
    /// The interface-6 power mode or level changed.
    PowerChanged(PowerChange<'a>),
    /// Status items changed; read them through the owning stage.
    #[non_exhaustive]
    StatusChanged { now_ms: u64 },
    /// Charging, external power or low-battery state changed.
    #[non_exhaustive]
    ChargeChanged { now_ms: u64, flags: ChargeFlags },
    /// Registry sections changed; reread the sections used by this plugin.
    #[non_exhaustive]
    RegistryChanged { now_ms: u64, what: RegistryChanges },
    /// The active settings preview changed or ended.
    #[non_exhaustive]
    PreviewChanged { now_ms: u64 },
    /// Another plugin asks subscribers to offer installation. This installs nothing itself.
    #[non_exhaustive]
    PluginsRequested {
        now_ms: u64,
        request: PluginRequestId,
        by: &'a str,
        plugins: RequestedPlugins<'a>,
    },
    /// One requested plugin received an installation outcome. Delivered only to the requester.
    /// Reserved outcome values become Failed.
    #[non_exhaustive]
    PluginAnswered {
        now_ms: u64,
        request: PluginRequestId,
        plugin: &'a str,
        outcome: super::commands::PluginOutcome,
    },
    /// An instance opened or received a new handover.
    #[non_exhaustive]
    ScreenOpened {
        now_ms: u64,
        screen: &'a str,
        instance: super::ui::Instance,
        slot: &'a str,
        handover: Option<super::ui::HandoverId>,
        params: u32,
        text_bytes: u32,
        again: bool,
        restored: bool,
    },
    /// An instance closed after its final draw.
    #[non_exhaustive]
    ScreenClosed {
        now_ms: u64,
        screen: &'a str,
        instance: super::ui::Instance,
    },
    /// Read the current stack through core.ui.
    #[non_exhaustive]
    StackChanged { now_ms: u64 },
    /// A request of this plugin ended (`core.net`); read the answer in this callback.
    HttpDone(super::net::HttpDone),
    /// Bytes of a request's body came.
    HttpProgress(super::net::HttpProgress),
    /// A server of this plugin started, failed or stopped (`core.net`).
    Server(super::net::ServerEvent<'a>),
    /// A server of this plugin got a request; its body, if any, is readable
    /// only in this callback.
    ServedRequest(super::net::ServedRequest<'a>),
    /// A picture the plugin asked the host to prepare is ready or failed
    /// (`core.document`).
    ImageReady(super::image::ImageReady),
    /// Something happened to a socket of this plugin (`core.sockets`); read
    /// arrived bytes in this callback.
    Socket(super::sockets::SocketEvent),
    /// Something happened to a broadcast of this plugin.
    Broadcast(super::sockets::BroadcastEvent<'a>),
    /// Work of this plugin came due (`core.work`): opens a run. Requests and sockets
    /// started in this callback belong to the run (background, `work.md` 2.3); the
    /// run ends when this callback has returned and its last request and socket
    /// have ended, so its sockets must have an idle time. It lasts at most 10
    /// minutes. The run is quiet: UI calls that would ask the user answer
    /// `Error::State` in this callback and in the events of the run's requests and
    /// sockets, and notices go to the plugin's work note.
    WorkDue(super::work::WorkDue<'a>),
    /// Work was taken out for good (the user turned the network off).
    WorkRefused(super::work::WorkRefused<'a>),
    /// What held work back no longer does: try again what was held.
    #[non_exhaustive]
    WorkAllowed { now_ms: u64 },
    /// A provider kept the answer it had marked `pending`: ask again with the
    /// same text and language (`event.lookup_ready`). Only an asker subscribed to
    /// it is told.
    #[non_exhaustive]
    LookupReady { now_ms: u64, provider: &'a str },
    /// The state or the progress of a document this plugin opened moved while it
    /// was being prepared (`event.document_changed`, 88, `document.md` 2.1): a
    /// coalesced hint with the latest values; read the state again
    /// (`Documents::state`). Only an opener subscribed to it is told.
    #[non_exhaustive]
    DocumentChanged {
        now_ms: u64,
        document: super::document::DocId,
        state: super::document::Preparation,
        progress_ppm: u32,
    },
    /// The host ended a document this plugin opened (`core.document`): every later
    /// call on it is `Error::Cancelled`. Close it
    /// ([`Documents::close`](super::document::Documents::close)) to forget its id.
    #[non_exhaustive]
    DocumentClosed {
        now_ms: u64,
        document: super::document::DocId,
        reason: super::document::DocClosedReason,
    },
    /// A final answer or committed live update, readable in this callback.
    #[non_exhaustive]
    Answered { now_ms: u64, answer: Answered },
    /// The host made a widget instance for its maker.
    #[non_exhaustive]
    WidgetMade {
        now_ms: u64,
        widget: &'a str,
        instance: super::ui::Instance,
        place: &'a str,
    },
    /// The host dropped a widget instance.
    #[non_exhaustive]
    WidgetDropped {
        now_ms: u64,
        instance: super::ui::Instance,
    },
    /// A widget's content or layout changed.
    #[non_exhaustive]
    WidgetChanged {
        now_ms: u64,
        instance: super::ui::Instance,
    },
    /// A screen drew its first page and is ready for help.
    #[non_exhaustive]
    ScreenReady {
        now_ms: u64,
        screen: &'a str,
        instance: super::ui::Instance,
    },
    /// Speech ended or was refused; the caller's cookie is unchanged.
    #[non_exhaustive]
    Spoken {
        now_ms: u64,
        cookie: u32,
        status: SpeechStatus,
    },
}

impl<'a> Event<'a> {
    /// Decode one event, borrowing strings from the callback input. This decodes
    /// whichever event the record holds, so it links the decoder of every event; a
    /// plugin that reads a few kinds uses [`Incoming::decode_as`].
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
        Incoming::new(bytes)?.decode()
    }
}

/// One event as the host delivered it: its record framed and its body not read yet.
/// The plugin reads the kinds it handles with [`Self::decode_as`] and links only
/// their decoders; [`Self::decode`] reads any kind.
///
/// ```
/// use paperpaw_sdk::native::{Error, events::{Event, Incoming, kinds}};
/// fn on_tick(event: Incoming<'_>) -> Result<Option<u64>, Error> {
///     Ok(match event.decode_as::<kinds::Tick>()? {
///         Some(Event::Tick { now_ms, .. }) => Some(now_ms),
///         _ => None,
///     })
/// }
/// ```
///
/// It cannot outlive the callback bytes:
///
/// ```compile_fail
/// use paperpaw_sdk::native::events::Incoming;
/// fn escape(bytes: &[u8]) -> Incoming<'static> {
///     Incoming::new(bytes).unwrap()
/// }
/// ```
#[derive(Clone, Copy)]
pub struct Incoming<'a> {
    raw: wire::RawRecord<'a>,
    bytes: &'a [u8],
}

/// Its tag only: a body may hold text a plugin keeps private.
impl core::fmt::Debug for Incoming<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Incoming")
            .field("tag", &self.raw.tag())
            .finish()
    }
}

impl<'a> Incoming<'a> {
    /// Frame one event record: exactly one record in a batch of one, nothing after.
    /// The body is not read.
    ///
    /// # Errors
    /// Returns the framing errors [`Event::decode`] returns before it reads a body.
    pub fn new(bytes: &'a [u8]) -> Result<Self, Error> {
        let raw = wire::RawRecord::single(bytes).map_err(Error::from_status)?;
        Ok(Self { raw, bytes })
    }

    /// Whether this is an event of kind `K`; the body is not read.
    #[must_use]
    pub fn is<K: kinds::Kind>(&self) -> bool {
        self.raw.tag() == K::TAG
    }

    /// Read the event if it is of kind `K`: `Ok(None)` for any other, without reading
    /// its body. A plugin links the decoders of the kinds it names only.
    ///
    /// # Errors
    /// Returns UTF-8, cap or event-field errors of an event of kind `K`.
    pub fn decode_as<K: kinds::Kind>(&self) -> Result<Option<Event<'a>>, Error> {
        if self.raw.tag() != K::TAG {
            return Ok(None);
        }
        <K as kinds::sealed::Sealed>::decode_event(self.raw, self.bytes).map(Some)
    }

    /// Read the event, whichever kind it is, as [`Event::decode`] does. This links the
    /// decoder of every event: use [`Self::decode_as`] where a plugin handles a few.
    ///
    /// # Errors
    /// Returns UTF-8, cap or event-field errors; Unsupported for a tag no event has.
    pub fn decode(&self) -> Result<Event<'a>, Error> {
        kinds::decode_any(self.raw, self.bytes)
    }
}

/// What the tests of the decoders read an event with.
#[cfg(test)]
fn decode(bytes: &[u8]) -> Result<Event<'_>, Error> {
    Event::decode(bytes)
}

#[cfg(test)]
#[path = "events/kinds_tests.rs"]
mod kinds_tests;

#[cfg(test)]
#[path = "events_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "events/core_events_tests.rs"]
mod core_tests;

mod uix_events;
pub use uix_events::{AnswerOutcome, Answered, SpeechStatus};

#[cfg(test)]
#[path = "events/uix_tests.rs"]
mod uix_tests;
