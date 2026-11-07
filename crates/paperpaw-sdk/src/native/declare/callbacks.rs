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
    /// Subscribe to the plugins changed host event.
    PluginsChanged,
    /// Subscribe to the wifi changed host event.
    WifiChanged,
    /// Subscribe to the power changed host event.
    PowerChanged,
    /// Subscribe to the status changed host event.
    StatusChanged,
    /// Subscribe to the charge changed host event.
    ChargeChanged,
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
            Self::PluginsChanged => records_wire::EVENT_PLUGINS_CHANGED_TAG,
            Self::WifiChanged => records_wire::EVENT_WIFI_CHANGED_TAG,
            Self::PowerChanged => records_wire::EVENT_POWER_CHANGED_TAG,
            Self::StatusChanged => records_wire::EVENT_STATUS_CHANGED_TAG,
            Self::ChargeChanged => records_wire::EVENT_CHARGE_CHANGED_TAG,
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
