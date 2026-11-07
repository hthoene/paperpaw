//! Typed screen navigation and messages.
use super::{CommandSender, Error, wire};
use crate::native::ui::{Instance, Notify, Open, ScreenPolicy, SpeechRequest, navigation::full_id};

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
