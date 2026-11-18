//! What the kernel does once it is registered: the display settings applied,
//! the registry settled.

use super::*;

impl Kernel {
    /// The language, night mode and colours the settings say.
    pub(super) fn apply_display_settings(&mut self) {
        let shared = &mut self.shared;
        shared
            .translator
            .set_language(shared.settings.read(&core_settings::LANGUAGE));
        let settings = &self.shared.settings;
        self.framebuffer
            .set_inverted(settings.bool(core_settings::NIGHT_MODE.as_str()));
        self.framebuffer
            .set_pictures_kept(!settings.bool(core_settings::NIGHT_PICTURES.as_str()));
        self.framebuffer.set_mono_coverage(mono_coverage(settings));
        self.apply_ui_text_curve();
        if let Some(panel) = self.shared.device.display.color {
            self.framebuffer.set_saturation(panel.boost_percent());
        }
    }

    /// Done registering: memory given back, the slots and screens known,
    /// the row of hints as tall as the first screen is made for.
    pub(super) fn settle(&mut self) {
        self.shrink_to_fit();
        self.shared.slots = self.shared.registry.bound_slots();
        self.hint.height = self.wanted_hint_height();
    }
}
