//! One marker type per event kind, to ask [`Incoming`](super::Incoming) for a kind.
//!
//! A plugin names the kinds it handles and links the decoders of those only:
//! `event.decode_as::<kinds::Tick>()?`. The markers are named like the [`Event`]
//! variants they give; the three book edges, which are one variant, have one marker
//! each (`BookOpened`, `BookClosed`, `BookFinished`).
use super::{Error, Event, core_events, host_events, uix_events, wire};

pub(super) mod sealed {
    use super::{Error, Event, wire};

    /// What only this crate's markers implement: reading the record of their kind.
    pub trait Sealed {
        /// Read `raw`, a record of this kind, as an event.
        fn decode_event<'a>(raw: wire::RawRecord<'a>, bytes: &'a [u8]) -> Result<Event<'a>, Error>;
    }
}

/// A kind of event: a marker of this module. Plugins cannot implement it.
pub trait Kind: sealed::Sealed {
    /// The wire tag of the record of this kind.
    const TAG: u32;
}

/// `Incoming::decode_as::<K>`, as a plain function.
#[cfg(test)]
pub(super) type ProbeFn = for<'a> fn(&super::Incoming<'a>) -> Result<Option<Event<'a>>, Error>;
#[cfg(test)]
fn probe<'a, K: Kind>(incoming: &super::Incoming<'a>) -> Result<Option<Event<'a>>, Error> {
    incoming.decode_as::<K>()
}

macro_rules! kinds {
    ($($marker:ident => $tag:ident, $module:ident::$decode:ident, $variant:ident;)*) => {
        $(
            #[doc = concat!("The kind of [`Event::", stringify!($variant), "`].")]
            #[derive(Clone, Copy, Debug, PartialEq, Eq)]
            pub struct $marker;
            impl sealed::Sealed for $marker {
                fn decode_event<'a>(
                    raw: wire::RawRecord<'a>,
                    bytes: &'a [u8],
                ) -> Result<Event<'a>, Error> {
                    $module::$decode::alone(raw, bytes)
                }
            }
            impl Kind for $marker {
                const TAG: u32 = wire::$tag;
            }
        )*

        /// Every kind's tag and its `decode_as`, for the test that they agree with `decode`.
        #[cfg(test)]
        pub(super) const ALL: &[(u32, ProbeFn)] = &[$((wire::$tag, probe::<$marker>)),*];

        /// Read `raw` as the event its tag names. The one table of tags and decoders
        /// serves both [`Incoming::decode`](super::Incoming::decode) and the markers.
        #[inline(never)]
        pub(super) fn decode_any<'a>(
            raw: wire::RawRecord<'a>,
            bytes: &'a [u8],
        ) -> Result<Event<'a>, Error> {
            match raw.tag() {
                $(wire::$tag => $module::$decode::inline(raw, bytes),)*
                _ => Err(Error::Unsupported),
            }
        }
    };
}

kinds! {
    Tick => EVENT_TICK_TAG, host_events::tick, Tick;
    StageCall => STAGE_CALL_TAG, host_events::stage_call, StageCall;
    Started => EVENT_STARTED_TAG, host_events::started, Started;
    Resume => EVENT_RESUME_TAG, host_events::resume, Resume;
    Suspend => EVENT_SUSPEND_TAG, host_events::suspend, Suspend;
    Stopping => EVENT_STOPPING_TAG, host_events::stopping, Stopping;
    BatteryMeasured => EVENT_BATTERY_MEASURED_TAG, host_events::battery_measured, BatteryMeasured;
    PageTurned => EVENT_PAGE_TURNED_TAG, host_events::page_turned, PageTurned;
    ScreenShown => EVENT_SCREEN_SHOWN_TAG, host_events::screen_shown, ScreenShown;
    ActionRun => EVENT_ACTION_RUN_TAG, host_events::action_run, ActionRun;
    TextTyped => EVENT_TEXT_TYPED_TAG, host_events::text_typed, TextTyped;
    RegistryChanged => EVENT_REGISTRY_CHANGED_TAG, host_events::registry_changed, RegistryChanged;
    PreviewChanged => EVENT_PREVIEW_CHANGED_TAG, host_events::preview_changed, PreviewChanged;
    PluginsRequested => EVENT_PLUGINS_REQUESTED_TAG, host_events::plugins_requested, PluginsRequested;
    PluginAnswered => EVENT_PLUGIN_ANSWERED_TAG, host_events::plugin_answered, PluginAnswered;
    SettingChanged => EVENT_SETTING_CHANGED_TAG, core_events::setting_changed, SettingChanged;
    BookOpened => EVENT_BOOK_OPENED_TAG, core_events::book_opened, Book;
    BookClosed => EVENT_BOOK_CLOSED_TAG, core_events::book_closed, Book;
    BookFinished => EVENT_BOOK_FINISHED_TAG, core_events::book_finished, Book;
    BookMoved => EVENT_BOOK_MOVED_TAG, core_events::book_moved, BookMoved;
    BookRemoved => EVENT_BOOK_REMOVED_TAG, core_events::book_removed, BookRemoved;
    LibraryChanged => EVENT_LIBRARY_CHANGED_TAG, core_events::library_changed, LibraryChanged;
    PluginsChanged => EVENT_PLUGINS_CHANGED_TAG, core_events::plugins_changed, PluginsChanged;
    WifiChanged => EVENT_WIFI_CHANGED_TAG, core_events::wifi_changed, WifiChanged;
    WifiScanned => EVENT_WIFI_SCANNED_TAG, core_events::wifi_scanned, WifiScanned;
    WifiDone => EVENT_WIFI_DONE_TAG, core_events::wifi_done, WifiDone;
    NetworkWanted => EVENT_NETWORK_WANTED_TAG, core_events::network_wanted, NetworkWanted;
    NetworkIdle => EVENT_NETWORK_IDLE_TAG, core_events::network_idle, NetworkIdle;
    FirmwareChanged => EVENT_FIRMWARE_CHANGED_TAG, core_events::firmware_changed, FirmwareChanged;
    InstallDone => EVENT_INSTALL_DONE_TAG, core_events::install_done, InstallDone;
    PowerChanged => EVENT_POWER_CHANGED_TAG, core_events::power_changed, PowerChanged;
    StatusChanged => EVENT_STATUS_CHANGED_TAG, core_events::status_changed, StatusChanged;
    ChargeChanged => EVENT_CHARGE_CHANGED_TAG, core_events::charge_changed, ChargeChanged;
    Bluetooth => EVENT_BLUETOOTH_TAG, core_events::bluetooth, Bluetooth;
    HttpDone => EVENT_HTTP_DONE_TAG, core_events::http_done, HttpDone;
    HttpProgress => EVENT_HTTP_PROGRESS_TAG, core_events::http_progress, HttpProgress;
    Server => EVENT_SERVER_TAG, core_events::server, Server;
    ServedRequest => EVENT_SERVED_REQUEST_TAG, core_events::served_request, ServedRequest;
    ImageReady => EVENT_IMAGE_READY_TAG, core_events::image_ready, ImageReady;
    Socket => EVENT_SOCKET_TAG, core_events::socket, Socket;
    Broadcast => EVENT_BROADCAST_TAG, core_events::broadcast, Broadcast;
    WorkDue => EVENT_WORK_DUE_TAG, core_events::work_due, WorkDue;
    WorkRefused => EVENT_WORK_REFUSED_TAG, core_events::work_refused, WorkRefused;
    WorkAllowed => EVENT_WORK_ALLOWED_TAG, core_events::work_allowed, WorkAllowed;
    LookupReady => EVENT_LOOKUP_READY_TAG, core_events::lookup_ready, LookupReady;
    DocumentChanged => EVENT_DOCUMENT_CHANGED_TAG, core_events::document_changed, DocumentChanged;
    ScreenOpened => EVENT_SCREEN_OPENED_TAG, uix_events::screen_opened, ScreenOpened;
    ScreenClosed => EVENT_SCREEN_CLOSED_TAG, uix_events::screen_closed, ScreenClosed;
    StackChanged => EVENT_STACK_CHANGED_TAG, uix_events::stack_changed, StackChanged;
    Answered => EVENT_ANSWERED_TAG, uix_events::answered, Answered;
    WidgetMade => EVENT_WIDGET_MADE_TAG, uix_events::widget_made, WidgetMade;
    WidgetDropped => EVENT_WIDGET_DROPPED_TAG, uix_events::widget_dropped, WidgetDropped;
    WidgetChanged => EVENT_WIDGET_CHANGED_TAG, uix_events::widget_changed, WidgetChanged;
    ScreenReady => EVENT_SCREEN_READY_TAG, uix_events::screen_ready, ScreenReady;
    Spoken => EVENT_SPOKEN_TAG, uix_events::spoken, Spoken;
}
