//! Choosing the fonts and what is offered to choose from, while running:
//! after a plugin was loaded, installed or removed, and at start-up. The
//! heap may be short on a small device, so nothing here allocates without
//! checking: where it cannot, what was chosen and offered before stays and
//! a line in the warnings says so (the next refresh tries again).

use alloc::borrow::Cow;
use alloc::string::String;
use alloc::vec::Vec;

use crate::plugins::FONT_SIZES;
use core::mem;

use paperpaw_sdk::builtin::settings as core_settings;
use paperpaw_sdk::{Command, FontError, IdError};

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
        self.try_warn(WarningSource::Runtime, parts);
    }

    /// A warning from `source` made of `parts`; none where the heap has no
    /// room for it.
    pub(crate) fn try_warn(&mut self, source: WarningSource, parts: &[&str]) {
        if let Some(message) = fallible_string(parts) {
            self.try_warn_made(source, message);
        }
    }

    /// A warning from `source` whose `message` is made already (fallibly,
    /// while what it names was borrowed); none where the heap has no room for
    /// it.
    pub(crate) fn try_warn_made(&mut self, source: WarningSource, message: String) {
        if self.warnings.try_reserve(1).is_ok() {
            self.warn(source, None, message);
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

    /// The values plugins offer among other plugins' choices
    /// ([`paperpaw_sdk::Registrar::add_choice`]), after the setting's own.
    /// A value that cannot be added for lack of memory is kept, with those
    /// after it, for the next refresh. While native plugins register (until
    /// the start), one for a setting nobody declared yet waits too: a native
    /// plugin declares its settings after the kernel is built.
    pub(crate) fn add_plugin_choices(&mut self) {
        let mut pending = mem::take(&mut self.plugin_choices);
        let mut no_room = false;
        pending.retain(|(plugin, id, value)| {
            if no_room {
                return true;
            }
            match self.add_plugin_choice(plugin, id, value) {
                Offered::Taken => false,
                Offered::Later => true,
                Offered::NoRoom => {
                    no_room = true;
                    true
                }
            }
        });
        if no_room {
            self.try_warn_runtime(&[
                "not enough memory to add the choices plugins offer; they are added at the next refresh",
            ]);
        }
        // Nothing was added to the list meanwhile.
        pending.append(&mut self.plugin_choices);
        self.plugin_choices = pending;
    }

    /// Adds one value, or refuses it with a log line.
    fn add_plugin_choice(&mut self, plugin: &Id, id: &Id, value: &str) -> Offered {
        let refused = match self.shared.settings.spec(id.as_str()) {
            None if self.native_registering => return Offered::Later,
            None => Some("no such setting"),
            Some(spec) if spec.guarded => Some("it keeps the device safe"),
            Some(spec) => match &spec.kind {
                paperpaw_sdk::SettingKind::Choice(choices) if choices.contains(value) => {
                    Some("it has that choice already")
                }
                paperpaw_sdk::SettingKind::Choice(_) => match Id::try_parse(value) {
                    Ok(v) if v.namespace() == plugin.as_str() => None,
                    Ok(_) | Err(IdError::Invalid) => {
                        Some("the value is not in the plugin's namespace")
                    }
                    Err(_) => return Offered::NoRoom,
                },
                _ => Some("it is no choice"),
            },
        };
        if let Some(why) = refused {
            // Told where there is room to tell it.
            if let Some(source) = plugin.try_clone() {
                self.try_warn(
                    WarningSource::Plugin(source),
                    &[
                        "choice `",
                        value,
                        "` for `",
                        id.as_str(),
                        "`: ",
                        why,
                        "; not added",
                    ],
                );
            }
            return Offered::Taken;
        }
        let Some(paperpaw_sdk::SettingKind::Choice(choices)) =
            self.shared.settings.spec(id.as_str()).map(|s| &s.kind)
        else {
            return Offered::Taken;
        };
        match choice_list(&[], choices.iter().chain(core::iter::once(value))) {
            Some(values) => {
                self.shared.settings.set_choices(id.as_str(), values);
                Offered::Taken
            }
            None => Offered::NoRoom,
        }
    }

    /// The reading apps to choose: the default one, and one for each
    /// format (a setting each, in the reading settings' part for them),
    /// each offering the readers that read its books. Where only one
    /// does, there is nothing to choose and the setting is not listed.
    /// Where the heap has no room what was offered stays.
    pub(crate) fn offer_reading_apps(&mut self) {
        if self.try_offer_reading_apps().is_err() {
            self.try_warn_runtime(&[
                "not enough memory to update the reading apps; they stay as they were",
            ]);
        }
    }

    fn try_offer_reading_apps(&mut self) -> Result<(), ()> {
        let Shared {
            readers,
            formats,
            settings,
            ..
        } = &mut self.shared;
        let default = [core_settings::READING_APP_DEFAULT];
        let several = readers.len() > 1;
        let all = choice_list(
            &default,
            readers.iter().map(|r| r.id.as_str()).filter(|_| several),
        )
        .ok_or(())?;
        settings.set_choices(core_settings::READING_APP.as_str(), all);
        // The native formats' stand-in (`native_book_formats`) is no format
        // a reader is chosen for: native books open through `core.document`.
        let stand_in = crate::native_host::native_book_formats::STAND_IN_ID;
        for format in formats.iter().filter(|f| f.id.as_str() != stand_in) {
            let several = readers.reading(Some(&format.id)).nth(1).is_some();
            let choices = choice_list(
                &default,
                readers
                    .reading(Some(&format.id))
                    .map(|r| r.id.as_str())
                    .filter(|_| several),
            )
            .ok_or(())?;
            let id =
                fallible_string(&[core_settings::READING_APP.as_str(), ".", format.id.as_str()])
                    .ok_or(())?;
            if settings.spec(&id).is_some() {
                settings.set_choices(&id, choices);
                continue;
            }
            // Nothing to choose where one reader reads it: no setting
            // (memory on small devices), until another comes.
            if !several {
                continue;
            }
            let setting = match Id::try_parse(&id) {
                Ok(setting) => setting,
                Err(IdError::Invalid) => continue,
                Err(_) => return Err(()),
            };
            // The room first, so that defining it cannot fail for lack of it.
            settings.try_reserve(1, 0).map_err(|_| ())?;
            /// The reading apps' part, keeping apps not installed now.
            static READING_APPS: paperpaw_sdk::SettingExtra = paperpaw_sdk::SettingExtra::new()
                .section("core.section.reading-apps")
                .keeps_unknown();
            let title = match &format.title {
                Cow::Borrowed(title) => Cow::Borrowed(*title),
                Cow::Owned(title) => Cow::Owned(fallible_string(&[title]).ok_or(())?),
            };
            let mut spec = paperpaw_sdk::SettingSpec::new(
                setting,
                "",
                paperpaw_sdk::SettingKind::Choice(choices.into()),
                paperpaw_sdk::SettingValue::Text(Cow::Borrowed(core_settings::READING_APP_DEFAULT)),
                paperpaw_sdk::builtin::groups::READING_BEHAVIOUR,
            )
            .description("core.setting.reading-app.format.help")
            .with_extra(&READING_APPS);
            spec.title = paperpaw_sdk::Str::new(title);
            let _ = settings.define(spec);
        }
        Ok(())
    }
}

/// What became of a value a plugin offers among another's choices.
enum Offered {
    /// Added, or refused with a log line: gone from the list.
    Taken,
    /// Its setting is not declared yet, while native plugins register.
    Later,
    /// No room: kept, with those after it, for the next refresh.
    NoRoom,
}
