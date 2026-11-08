//! Choosing the fonts and what is offered to choose from, while running:
//! after a plugin was loaded, installed or removed, and at start-up. The
//! heap may be short on a small device, so nothing here allocates without
//! checking: where it cannot, what was chosen and offered before stays and
//! a line in the warnings says so (the next refresh tries again).

use alloc::borrow::Cow;
use alloc::string::String;
use alloc::vec::Vec;

use crate::plugins::FONT_SIZES;
use paperpaw_sdk::Command;
use paperpaw_sdk::FontError;
use paperpaw_sdk::builtin::settings as core_settings;

use super::*;

/// The `parts` one after another, with fallible allocation.
pub(crate) fn fallible_string(parts: &[&str]) -> Option<String> {
    let mut text = String::new();
    text.try_reserve_exact(parts.iter().map(|part| part.len()).sum())
        .ok()?;
    for part in parts {
        text.push_str(part);
    }
    Some(text)
}

/// The choices `fixed` and then `rest`, or `None` where the heap has no
/// room for them.
pub(crate) fn choice_list<'a>(
    fixed: &[&'static str],
    rest: impl Iterator<Item = &'a str>,
) -> Option<Vec<Cow<'static, str>>> {
    let mut list: Vec<Cow<'static, str>> = Vec::new();
    list.try_reserve_exact(fixed.len() + rest.size_hint().0)
        .ok()?;
    list.extend(fixed.iter().map(|value| Cow::Borrowed(*value)));
    for value in rest {
        let owned = fallible_string(&[value])?;
        list.try_reserve(1).ok()?;
        list.push(Cow::Owned(owned));
    }
    Some(list)
}

impl Kernel {
    /// A warning of the runtime made of `parts`; none where the heap has no
    /// room for it.
    pub(crate) fn try_warn_runtime(&mut self, parts: &[&str]) {
        if let Some(message) = fallible_string(parts)
            && self.warnings.try_reserve(1).is_ok()
        {
            self.warn(WarningSource::Runtime, None, message);
        }
    }

    /// `list` is what setting `id` offers from now on, or, where it could not
    /// be made, what it offered stays.
    pub(crate) fn offer_list(&mut self, id: &str, list: Option<Vec<Cow<'static, str>>>) {
        match list {
            Some(list) => {
                self.shared.settings.set_choices(id, list);
            }
            None => self.try_warn_runtime(&[
                "not enough memory to update the choices of ",
                id,
                "; they stay as they were",
            ]),
        }
    }

    /// The reading font and status font choices: the registered families.
    pub(crate) fn offer_font_choices(&mut self) {
        let Ok(families) = self.shared.fonts.try_families() else {
            return self.try_warn_runtime(&[
                "not enough memory to update the font choices; they stay as they were",
            ]);
        };
        let reading = choice_list(&[core_settings::DEFAULT_FONT], families.iter().copied());
        let status = choice_list(
            &[
                core_settings::STATUS_FONT_UI,
                core_settings::STATUS_FONT_BOOK,
            ],
            families.iter().copied(),
        );
        drop(families);
        self.offer_list(core_settings::READING_FONT.as_str(), reading);
        self.offer_list(core_settings::STATUS_FONT.as_str(), status);
    }

    /// Chooses the reading font family from `core.reading-font` and loads
    /// its files. Where the heap is short the family in use stays.
    pub(crate) fn apply_reading_font(&mut self) {
        if self.try_apply_reading_font().is_err() {
            self.try_warn_runtime(&[
                "not enough memory to change the reading font; the one in use stays",
            ]);
        }
    }

    fn try_apply_reading_font(&mut self) -> Result<(), FontError> {
        let chosen = self
            .shared
            .settings
            .text(core_settings::READING_FONT.as_str());
        let family = (chosen != core_settings::DEFAULT_FONT
            && self.shared.fonts.has_family(chosen))
        .then_some(chosen);
        let previous = self.shared.fonts.try_set_family(family)?;
        match self.shared.fonts.try_load(&self.shared.storage) {
            Ok(failed) => self.warn_fonts_failed(failed),
            Err(error) => {
                self.shared.fonts.restore_family(previous);
                return Err(error);
            }
        }
        self.fit_font_size();
        Ok(())
    }

    /// Chooses what the status lines are in from `core.status-font` and
    /// loads its files. Where the heap is short the choice in use stays.
    pub(crate) fn apply_status_font(&mut self) {
        if self.try_apply_status_font().is_err() {
            self.try_warn_runtime(&[
                "not enough memory to change the status font; the one in use stays",
            ]);
        }
    }

    fn try_apply_status_font(&mut self) -> Result<(), FontError> {
        let chosen = self
            .shared
            .settings
            .text(core_settings::STATUS_FONT.as_str());
        let previous = self.shared.fonts.try_set_status(chosen)?;
        match self.shared.fonts.try_load(&self.shared.storage) {
            Ok(failed) => self.warn_fonts_failed(failed),
            Err(error) => {
                self.shared.fonts.restore_status(previous);
                return Err(error);
            }
        }
        Ok(())
    }

    fn warn_fonts_failed(&mut self, failed: Vec<String>) {
        for path in &failed {
            self.try_warn_runtime(&[path, ": font could not be loaded"]);
        }
    }

    /// The sizes of book text offered as far as the reading font has them:
    /// a font pack's largest (built into the firmware, kept small) bounds
    /// them, a scalable font has every size. A size chosen past it gives
    /// way to the largest.
    pub(crate) fn fit_font_size(&mut self) {
        let (min, max) = FONT_SIZES;
        let dpi = f32::from(self.shared.device.display.dpi.max(1));
        let largest = self
            .shared
            .fonts
            .role(paperpaw_sdk::builtin::fonts::READING.as_str())
            .and_then(|font| font.largest_size());
        // No `round` without std.
        let most = largest.map_or(max, |px| ((px * 72.0 / dpi + 0.5) as i64).clamp(min, max));
        let id = core_settings::FONT_SIZE;
        self.shared.settings.set_range(id.as_str(), min, most);
        self.font_size_changed();
    }

    /// A font size past the largest the reading font has (set before, or
    /// a book's own) gives way to the largest; at the largest of a font
    /// pack, the help says where larger ones are found. The setting is
    /// changed by a command, which the next round of commands runs.
    pub(crate) fn font_size_changed(&mut self) {
        let id = core_settings::FONT_SIZE.static_str();
        let settings = &mut self.shared.settings;
        let most = match settings.spec(id).map(|spec| &spec.kind) {
            Some(paperpaw_sdk::SettingKind::Int { max, .. }) => *max,
            _ => return,
        };
        let size = settings.int(id);
        let largest = most < FONT_SIZES.1 && size >= most;
        let key = if largest {
            "core.setting.font-size.largest"
        } else {
            "core.setting.font-size.help"
        };
        settings.set_description(id, key);
        if size > most {
            let value = paperpaw_sdk::SettingValue::Int(most);
            let id = core_settings::FONT_SIZE.id().try_clone();
            match id {
                Some(id) if self.output.commands.try_reserve(1).is_ok() => {
                    self.output.commands.push(Command::SetSetting(id, value));
                }
                _ => self.try_warn_runtime(&[
                    "not enough memory to fit the font size; it is fitted at the next refresh",
                ]),
            }
        }
    }
}
