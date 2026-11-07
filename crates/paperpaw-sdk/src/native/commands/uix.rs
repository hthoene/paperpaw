//! Typed screen navigation and messages.
use super::{CommandSender, Error, wire};
use crate::native::ui::{Instance, Open, navigation::full_id};

impl CommandSender<'_, '_> {
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
    pub fn notify(&mut self, text: impl AsRef<str>) -> Result<(), Error> {
        self.submit(wire::Command::Notify {
            text: text.as_ref(),
            flags: 0,
        })
    }
    /// Show a reader-requested or device message also from background work.
    ///
    /// # Errors
    /// Returns buffer limits or the host's refusal.
    pub fn announce(&mut self, text: impl AsRef<str>) -> Result<(), Error> {
        self.submit(wire::Command::Notify {
            text: text.as_ref(),
            flags: 1,
        })
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
