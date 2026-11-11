//! Typed callback subscriptions encoded by the generated registration codec.
use super::{Declarations, DeclareError, Item, Latched, records_wire};
use records_wire::Registration;

/// A host event supported by the callback-registration contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum EventKind {
    /// Periodic callback tick.
    Tick,
    /// The host has started.
    Started,
    /// The device resumed from sleep.
    Resume,
    /// The device is about to sleep.
    Suspend,
    /// The host is stopping.
    Stopping,
    /// A battery reading is available.
    BatteryMeasured,
    /// A page was turned.
    PageTurned,
    /// A screen became visible.
    ScreenShown,
    /// An action was run.
    ActionRun,
    /// Subscribe to the setting changed host event.
    SettingChanged,
    /// Subscribe to the book opened host event.
    BookOpened,
    /// Subscribe to the book closed host event.
    BookClosed,
    /// Subscribe to the book finished host event.
    BookFinished,
    /// Subscribe to the book moved host event.
    BookMoved,
    /// Subscribe to the book removed host event.
    BookRemoved,
    /// Subscribe to the library changed host event (a coalesced hint).
    LibraryChanged,
    /// Subscribe to the plugins changed host event.
    PluginsChanged,
    /// Subscribe to the wifi changed host event.
    WifiChanged,
    /// A Wi-Fi scan finished (a coalesced hint).
    WifiScanned,
    /// A `join` or `host` request of this plugin ended.
    WifiDone,
    /// Network work waits for a network (network manager only).
    NetworkWanted,
    /// The network is no longer needed (network manager only).
    NetworkIdle,
    /// Firmware installation progressed (a hint).
    FirmwareChanged,
    /// An install, removal, firmware or check request of this plugin ended.
    InstallDone,
    /// Subscribe to the power changed host event.
    PowerChanged,
    /// Subscribe to the status changed host event.
    StatusChanged,
    /// Subscribe to the charge changed host event.
    ChargeChanged,
    /// Registry sections or pushed item state changed.
    RegistryChanged,
    /// The settings preview changed or ended.
    PreviewChanged,
    /// A plugin asks subscribers to offer installation.
    PluginsRequested,
    /// One requested plugin received an outcome.
    PluginAnswered,
    /// The visible screen stack changed.
    StackChanged,
    /// A screen drew its first page and is ready for help.
    ScreenReady,
    /// A request of this plugin ended (`core.net`). Needed before the first request.
    HttpDone,
    /// Bytes of a request's body came.
    HttpProgress,
    /// A server of this plugin started, failed or stopped (`core.net`).
    Server,
    /// A server of this plugin got a request.
    ServedRequest,
    /// A picture the plugin asked the host to prepare is ready or failed.
    ImageReady,
    /// Something happened to a socket of this plugin (`core.sockets`). Needed
    /// before the first `connect`.
    Socket,
    /// Something happened to a broadcast of this plugin. Needed before the
    /// first `broadcast`.
    Broadcast,
    /// Work of this plugin came due (`core.work`). Needed before `Work::declare`
    /// (without it the call answers `State`); the event goes only to the plugin
    /// that declared the work.
    WorkDue,
    /// Work of this plugin was taken out for good. Optional: without it a refused
    /// work just vanishes from the work list.
    WorkRefused,
    /// What held work back no longer does (a hint, coalesced). Optional: without
    /// it the plugin tries again at its next work or event.
    WorkAllowed,
    /// A lookup provider's pending answer is kept (`event.lookup_ready`). Needed
    /// for the host to remember an ask that comes back `pending`.
    LookupReady,
    /// A widget instance of this plugin changed (`event.widget_changed`, ui.md 5.2): a
    /// widget a screen shows says its content changed, so the plugin refreshes its view.
    WidgetChanged,
    /// The host's answer to text handed to `command.speak` (`event.spoken`, ui.md 7):
    /// it goes only to plugins that subscribed.
    Spoken,
}

impl EventKind {
    const fn registration_event_tag(self) -> u32 {
        match self {
            Self::Tick => records_wire::EVENT_TICK_TAG,
            Self::Started => records_wire::EVENT_STARTED_TAG,
            Self::Resume => records_wire::EVENT_RESUME_TAG,
            Self::Suspend => records_wire::EVENT_SUSPEND_TAG,
            Self::Stopping => records_wire::EVENT_STOPPING_TAG,
            Self::BatteryMeasured => records_wire::EVENT_BATTERY_MEASURED_TAG,
            Self::PageTurned => records_wire::EVENT_PAGE_TURNED_TAG,
            Self::ScreenShown => records_wire::EVENT_SCREEN_SHOWN_TAG,
            Self::ActionRun => records_wire::EVENT_ACTION_RUN_TAG,
            Self::SettingChanged => records_wire::EVENT_SETTING_CHANGED_TAG,
            Self::BookOpened => records_wire::EVENT_BOOK_OPENED_TAG,
            Self::BookClosed => records_wire::EVENT_BOOK_CLOSED_TAG,
            Self::BookFinished => records_wire::EVENT_BOOK_FINISHED_TAG,
            Self::BookMoved => records_wire::EVENT_BOOK_MOVED_TAG,
            Self::BookRemoved => records_wire::EVENT_BOOK_REMOVED_TAG,
            Self::LibraryChanged => records_wire::EVENT_LIBRARY_CHANGED_TAG,
            Self::PluginsChanged => records_wire::EVENT_PLUGINS_CHANGED_TAG,
            Self::WifiChanged => records_wire::EVENT_WIFI_CHANGED_TAG,
            Self::WifiScanned => records_wire::EVENT_WIFI_SCANNED_TAG,
            Self::WifiDone => records_wire::EVENT_WIFI_DONE_TAG,
            Self::NetworkWanted => records_wire::EVENT_NETWORK_WANTED_TAG,
            Self::NetworkIdle => records_wire::EVENT_NETWORK_IDLE_TAG,
            Self::FirmwareChanged => records_wire::EVENT_FIRMWARE_CHANGED_TAG,
            Self::InstallDone => records_wire::EVENT_INSTALL_DONE_TAG,
            Self::PowerChanged => records_wire::EVENT_POWER_CHANGED_TAG,
            Self::StatusChanged => records_wire::EVENT_STATUS_CHANGED_TAG,
            Self::ChargeChanged => records_wire::EVENT_CHARGE_CHANGED_TAG,
            Self::RegistryChanged => records_wire::EVENT_REGISTRY_CHANGED_TAG,
            Self::PreviewChanged => records_wire::EVENT_PREVIEW_CHANGED_TAG,
            Self::PluginsRequested => records_wire::EVENT_PLUGINS_REQUESTED_TAG,
            Self::PluginAnswered => records_wire::EVENT_PLUGIN_ANSWERED_TAG,
            Self::StackChanged => records_wire::EVENT_STACK_CHANGED_TAG,
            Self::ScreenReady => records_wire::EVENT_SCREEN_READY_TAG,
            Self::HttpDone => records_wire::EVENT_HTTP_DONE_TAG,
            Self::HttpProgress => records_wire::EVENT_HTTP_PROGRESS_TAG,
            Self::Server => records_wire::EVENT_SERVER_TAG,
            Self::ServedRequest => records_wire::EVENT_SERVED_REQUEST_TAG,
            Self::ImageReady => records_wire::EVENT_IMAGE_READY_TAG,
            Self::Socket => records_wire::EVENT_SOCKET_TAG,
            Self::Broadcast => records_wire::EVENT_BROADCAST_TAG,
            Self::WorkDue => records_wire::EVENT_WORK_DUE_TAG,
            Self::WorkRefused => records_wire::EVENT_WORK_REFUSED_TAG,
            Self::WorkAllowed => records_wire::EVENT_WORK_ALLOWED_TAG,
            Self::LookupReady => records_wire::EVENT_LOOKUP_READY_TAG,
            Self::WidgetChanged => records_wire::EVENT_WIDGET_CHANGED_TAG,
            Self::Spoken => records_wire::EVENT_SPOKEN_TAG,
        }
    }
}

impl<F: FnMut(&[u8]) -> Result<(), i32>> Declarations<F> {
    /// Subscribe a callback token to one host event during plugin construction.
    /// The host validates event support and callback-token ownership. Records
    /// retain declaration order and share the builder's bounded batches.
    ///
    /// # Errors
    /// Returns a latched builder failure, allocation failure or sink refusal.
    /// A failure prevents subsequent declarations from being accepted.
    pub fn subscribe_event(
        &mut self,
        kind: EventKind,
        token: u32,
    ) -> Result<&mut Self, DeclareError> {
        self.push_callback_registration(Registration::Event {
            event_tag: kind.registration_event_tag(),
            token,
        })
    }

    /// Register a draw callback token during plugin construction.
    /// This callback record shares the batching and order of other declarations;
    /// use a screen declaration when registering a named screen.
    ///
    /// # Errors
    /// Returns a latched builder failure, allocation failure or sink refusal.
    /// A failure prevents subsequent declarations from being accepted.
    pub fn register_draw(&mut self, token: u32) -> Result<&mut Self, DeclareError> {
        self.push_callback_registration(Registration::Draw { token })
    }

    fn push_callback_registration(
        &mut self,
        registration: Registration,
    ) -> Result<&mut Self, DeclareError> {
        self.check_sink()?;
        if let Err(error) = self.push(Item::Registration(registration)) {
            if self.failed.is_none() {
                self.failed = Some(Latched::Refused(error));
            }
            return Err(error);
        }
        Ok(self)
    }
}

#[cfg(test)]
mod tests;
