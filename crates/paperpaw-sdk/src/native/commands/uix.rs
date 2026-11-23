//! Typed screen navigation and messages.
use super::{CommandSender, Error, wire};
use crate::native::ui::{
    ButtonHint, Instance, Notify, Open, ScreenPolicy, SpeechRequest, navigation::full_id,
};

/// The least `after_ms` of [`CommandSender::show_again`] (one minute).
pub const SHOW_AGAIN_MIN_MS: u32 = 60_000;

impl CommandSender<'_, '_> {
    /// Set runtime policies for a decoded own instance, or None for own top.
    ///
    /// # Errors
    /// Returns encoding limits or the host's ownership/top/guard refusal.
    pub fn screen_policy(
        &mut self,
        instance: Option<Instance>,
        policy: ScreenPolicy,
    ) -> Result<(), Error> {
        self.submit(wire::Command::ScreenPolicy {
            instance: instance.map_or(0, Instance::get),
            flags: policy.flags(),
        })
    }
    /// Ask for the sleep screen to be shown again while the device sleeps
    /// (`command.show_again`): `after_ms` from now (at least
    /// [`SHOW_AGAIN_MIN_MS`]), then every `every_ms` (at least the same; 0:
    /// once); a new request replaces the one waiting. For the own top
    /// instance, and only while the device is asleep (send it from the
    /// `core.suspend` event).
    ///
    /// # Errors
    /// `Unsupported` where the device cannot refresh while asleep
    /// ([`DeviceCapability::RefreshAsleep`](crate::native::device::DeviceCapability)),
    /// `State` when it is not asleep or the plugin has no screen on top,
    /// `Invalid` for a time under the least; buffer limits.
    pub fn show_again(&mut self, after_ms: u32, every_ms: u32) -> Result<(), Error> {
        // Checked here, not left to the host: it reads `after_ms` 0 as a
        // cancel, which would drop the request that waits instead of refusing.
        if after_ms < SHOW_AGAIN_MIN_MS || (every_ms != 0 && every_ms < SHOW_AGAIN_MIN_MS) {
            return Err(Error::Invalid);
        }
        self.submit(wire::Command::ShowAgain {
            instance: 0,
            after_ms,
            every_ms,
            flags: 0,
        })
    }
    /// Cancel what [`show_again`](Self::show_again) asked for; nothing if
    /// nothing waits.
    ///
    /// # Errors
    /// Returns buffer limits or the host's refusal.
    pub fn cancel_show_again(&mut self) -> Result<(), Error> {
        self.submit(wire::Command::ShowAgain {
            instance: 0,
            after_ms: 0,
            every_ms: 0,
            flags: 0,
        })
    }
    /// Queue typed speech with an unchanged caller-chosen cookie.
    ///
    /// # Errors
    /// Returns buffer limits or the host's refusal; completion is event.spoken.
    pub fn speak(&mut self, request: SpeechRequest<'_>) -> Result<(), Error> {
        self.submit(request.command())
    }
    /// Stop only the submitting plugin's speech.
    ///
    /// # Errors
    /// Returns buffer limits or the host's refusal.
    pub fn stop_speaking(&mut self) -> Result<(), Error> {
        self.submit(wire::Command::StopSpeaking {})
    }

    /// Stage full-id navigation, optionally carrying a host-owned handover.
    ///
    /// # Errors
    /// Returns invalid syntax, buffer limits or the host's refusal.
    pub fn open(&mut self, open: Open<'_>) -> Result<(), Error> {
        self.submit(open.command()?)
    }
    /// Close the submitter's own top instance.
    ///
    /// # Errors
    /// Returns buffer limits or the host's ownership/guard refusal.
    pub fn back(&mut self) -> Result<(), Error> {
        self.submit(wire::Command::Close { instance: 0 })
    }
    /// Close a specific instance, subject to host ownership and guard checks.
    ///
    /// # Errors
    /// Returns buffer limits or the host's refusal.
    pub fn close(&mut self, instance: Instance) -> Result<(), Error> {
        self.submit(wire::Command::Close {
            instance: instance.get(),
        })
    }
    /// Return to an already open screen or slot by its full id.
    ///
    /// # Errors
    /// Returns invalid syntax, buffer limits or the host's guard refusal.
    pub fn back_to(&mut self, target: impl AsRef<str>) -> Result<(), Error> {
        let target = target.as_ref();
        full_id(target)?;
        self.submit(wire::Command::BackTo { target })
    }
    /// Set an own top instance's key layer. Empty removes the extra layer.
    ///
    /// # Errors
    /// Returns invalid syntax, buffer limits or the host's refusal.
    pub fn set_key_layer(&mut self, layer: impl AsRef<str>) -> Result<(), Error> {
        let layer = layer.as_ref();
        if !layer.is_empty() {
            full_id(layer)?;
        }
        self.submit(wire::Command::SetKeyLayer { layer })
    }
    /// Show the device's button hints, where it has a hint row.
    ///
    /// # Errors
    /// Returns buffer limits or the host's refusal.
    pub fn show_hints(&mut self) -> Result<(), Error> {
        self.submit(wire::Command::ShowHints {})
    }
    /// Label a button in the row of button hints while the submitter's own top
    /// screen shows (a key that says *Try again* here). On a device without a
    /// hint row the host accepts it and does nothing.
    ///
    /// # Errors
    /// Returns buffer limits or the host's refusal: State outside the own top
    /// screen, NotFound for an unknown icon, Denied for hiding a way out,
    /// NoSpace for a ninth action of one instance.
    pub fn set_hint(&mut self, hint: ButtonHint<'_>) -> Result<(), Error> {
        self.submit(hint.command())
    }
    /// Raise a device input or an input owned by the submitting plugin.
    ///
    /// # Errors
    /// Returns syntax, buffer limits or host restrictions, including Denied
    /// for Power, escape chords or another plugin's declared input.
    pub fn raise_input(&mut self, input: impl AsRef<str>) -> Result<(), Error> {
        let input = input.as_ref();
        full_id(input)?;
        self.submit(wire::Command::RaiseInput { input })
    }
    /// Announce that the submitter's waiting screen drew its first page.
    ///
    /// # Errors
    /// Returns buffer limits or the host's ownership refusal.
    pub fn screen_ready(&mut self) -> Result<(), Error> {
        self.submit(wire::Command::ScreenReady {})
    }
    /// Show formatted display text while this plugin has a screen open.
    ///
    /// # Errors
    /// Returns buffer limits or the host's refusal.
    pub fn notify(&mut self, message: Notify<'_>) -> Result<(), Error> {
        self.submit(message.command())
    }
    /// Notify the maker that a widget instance needs new layout or drawing.
    ///
    /// # Errors
    /// Returns buffer limits or the host's ownership refusal.
    pub fn widget_changed(&mut self, instance: Instance) -> Result<(), Error> {
        self.submit(wire::Command::WidgetChanged {
            instance: instance.get(),
        })
    }
}
