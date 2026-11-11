//! Settings: the user's configuration read and saved, a book's own
//! settings (the scope), and what the kernel does when one changes.

#[allow(
    unused_imports,
    reason = "a part of the kernel, which sees all of it; not every part uses every name"
)]
use super::*;

/// Largest `paperpaw.ini` read.
const MAX_CONFIG: usize = 64 * 1024;
/// Largest settings file of a book read (see [`Command::SetScope`]).
const MAX_SCOPE: usize = 16 * 1024;

/// Settings that moved to the kernel, by the ids they had (the reader's
/// type and the library folder, October 2026; smooth text for beta.8):
/// read under those where the new ones are not set, so nothing chosen is
/// lost.
const MOVED: [(&str, &str); 13] = [
    ("reader.font-weight", "core.font-weight"),
    ("reader.smooth-text", "core.smooth-text"),
    ("reader.word-spacing", "core.word-spacing"),
    ("reader.line-spacing", "core.line-spacing"),
    ("reader.margin", "core.margin"),
    ("reader.margin-ends", "core.margin-ends"),
    ("reader.justify", "core.justify"),
    ("reader.hyphenate", "core.hyphenate"),
    ("reader.indent", "core.indent"),
    ("reader.paragraph-spacing", "core.paragraph-spacing"),
    ("reader.publisher-styles", "core.publisher-styles"),
    ("reader.footnotes", "core.footnotes"),
    ("files.root", "core.library-folder"),
];

/// Of setting `id` with `value` as kept in a file, the id and value it
/// has now where it moved (a moved choice's values too); `None` where it
/// did not, or where `entries` (the ids the file sets) has the new one.
fn moved(id: &str, value: &str, entries: &[String]) -> Option<(&'static str, String)> {
    let (_, now) = MOVED.iter().find(|(old, _)| *old == id)?;
    if entries.iter().any(|entry| entry == now) {
        return None;
    }
    let value = match value.strip_prefix("reader.footnotes.") {
        Some(choice) => format!("core.footnotes.{choice}"),
        None => value.to_string(),
    };
    Some((now, value))
}

/// Values that were renamed, by setting: the UI font's family where it is
/// drawn from bitmap packs, which may not keep its font's Reserved Font
/// Name (Source Sans 3 is PaperPaw Sans there, `fonts-default`, October
/// 2026), so a reading or status font chosen by the old name is kept.
const RENAMED: [(&str, &str, &str); 2] = [
    ("core.reading-font", "Source Sans 3", "PaperPaw Sans"),
    ("core.status-font", "Source Sans 3", "PaperPaw Sans"),
];

/// `value` of setting `id` as kept in a file, under its new name where it
/// was renamed and only the new one is a value it takes now.
fn renamed(settings: &paperpaw_sdk::Settings, id: &str, value: String) -> String {
    let Some((_, _, now)) = RENAMED
        .iter()
        .find(|(setting, old, _)| *setting == id && *old == value)
    else {
        return value;
    };
    let takes = |v: &str| {
        settings
            .spec(id)
            .is_some_and(|spec| spec.parse(v).is_some())
    };
    if !takes(&value) && takes(now) {
        String::from(*now)
    } else {
        value
    }
}

/// The ids of the settings `source` (a settings file) sets.
fn entries(source: &str) -> Vec<String> {
    paperpaw_config::parse(source)
        .filter_map(Result::ok)
        .map(|entry| format!("{}.{}", entry.section, entry.key))
        .collect()
}

/// The active settings scope: a book's own settings.
#[derive(Debug)]
pub(crate) struct Scope {
    /// Its file.
    pub(crate) path: String,
    /// The screen that set it (see `ActiveScreen::serial`).
    pub(crate) owner: Option<u32>,
    /// Its file is there but could not be read: it is not written over.
    pub(crate) unreadable: bool,
}

impl Kernel {
    /// Setting `id`'s value now, as written in `paperpaw.ini`, where a
    /// plugin hears [`events::SETTING_CHANGED`] (it is not made for
    /// nobody).
    pub(crate) fn setting_text_if_heard(&self, id: &Id) -> Option<String> {
        let heard = !self.event_observers.is_empty()
            || self
                .subscriptions
                .iter()
                .any(|(event, ..)| *event == events::SETTING_CHANGED);
        let value = self.shared.settings.get(id.as_str()).filter(|_| heard)?;
        Some(value.to_config_text())
    }

    /// Forgets a setting's value (see [`Command::ResetSetting`]): from the
    /// active scope, or from the configuration file.
    pub(crate) fn reset_setting(&mut self, id: &Id) {
        // A secret setting is kept with the secrets: reset, it goes.
        if self
            .shared
            .settings
            .spec(id.as_str())
            .is_some_and(|spec| spec.extra().secret)
        {
            self.set_secret(paperpaw_sdk::secrets::SecretChange::remove(id.as_str()));
            self.secret_setting_changed(id);
            return;
        }
        let scoped = self.shared.settings.goes_to_scope(id.as_str());
        let before = scoped.then(|| self.scoped_values());
        let native_old = self.native_setting_before(id.as_str());
        let old = self.setting_text_if_heard(id);
        if let Err(error) = self.shared.settings.reset(id.as_str()) {
            return self.warn_runtime(format!("`{id}`: {error}"));
        }
        self.native_setting_after(id.as_str(), native_old.as_ref());
        if let Some(before) = before {
            self.save_scope();
            self.announce_scope_changes(before);
            return;
        }
        if let Some((section, key)) = id.as_str().split_once('.') {
            self.update_config(&format_args!("`{id}`"), |source| {
                let mut source = paperpaw_config::remove(source, section, key);
                // Kept under the id it had before it moved: that goes too,
                // or it would be read again at the next start.
                for (old, _) in MOVED.iter().filter(|(_, now)| *now == id.as_str()) {
                    if let Some((section, key)) = old.split_once('.') {
                        source = paperpaw_config::remove(&source, section, key);
                    }
                }
                source
            });
        }
        self.setting_changed(id);
        let changed = events::SettingChanged::new(id.clone()).old(old);
        self.output
            .commands
            .push(events::SETTING_CHANGED.emit(changed));
    }

    /// Writes a changed setting to the configuration file on the device,
    /// keeping the rest of the file as it is.
    pub(crate) fn persist_setting(&mut self, id: &Id) {
        let Some(value) = self.shared.settings.global(id.as_str()) else {
            return;
        };
        let Some((section, key)) = id.as_str().split_once('.') else {
            return;
        };
        let value = value.to_config_text();
        self.save_config(section, key, &value);
    }

    /// The effective values of the scoped settings, to see what a scope
    /// change changes.
    pub(crate) fn scoped_values(&self) -> Vec<(Id, Option<paperpaw_sdk::SettingValue>)> {
        let settings = &self.shared.settings;
        settings
            .specs()
            .filter(|spec| spec.scoped)
            .map(|spec| (spec.id.clone(), settings.get(spec.id.as_str()).cloned()))
            .collect()
    }

    /// Announces the scoped settings whose value differs from `before`.
    pub(crate) fn announce_scope_changes(
        &mut self,
        before: Vec<(Id, Option<paperpaw_sdk::SettingValue>)>,
    ) {
        for (id, old) in before {
            if self.shared.settings.get(id.as_str()) != old.as_ref() {
                self.setting_changed(&id);
                let old = old.map(|value| value.to_config_text());
                let changed = events::SettingChanged::new(id.clone()).old(old);
                self.output
                    .commands
                    .push(events::SETTING_CHANGED.emit(changed));
            }
        }
    }

    /// Activates the scope stored at `path` for the top screen, or ends
    /// the scope (see [`Command::SetScope`]).
    pub(crate) fn set_scope(&mut self, path: Option<String>) {
        if path.as_deref() == self.scope.as_ref().map(|scope| scope.path.as_str()) {
            // Same scope (e.g. the reader shown again): keep it.
            let top = self.stack.last().map(|top| top.serial);
            if let Some(scope) = &mut self.scope {
                scope.owner = top;
            }
            return;
        }
        let before = self.scoped_values();
        let had_values = self
            .shared
            .settings
            .scope()
            .is_some_and(|scope| !scope.is_empty());
        let read = path.as_deref().map(|path| {
            let storage = self.shared.storage.as_ref();
            paperpaw_sdk::files::load_text_for_update(storage, path, MAX_SCOPE)
        });
        let unreadable = match (&path, &read) {
            (Some(path), Some(Err(error))) => {
                self.warn_runtime(format!(
                    "{path} cannot be read ({error}): its settings are not used or saved"
                ));
                true
            }
            _ => false,
        };
        let values = read.map(|text| {
            let text = text.ok().flatten().unwrap_or_default();
            let mut values = paperpaw_sdk::collections::VecMap::new();
            let ids = entries(&text);
            for entry in paperpaw_config::parse(&text).filter_map(Result::ok) {
                let mut id = format!("{}.{}", entry.section, entry.key);
                let mut value = entry.value.as_str().into_owned();
                if let Some((now, was)) = moved(&id, &value, &ids) {
                    (id, value) = (now.to_string(), was);
                }
                let value = renamed(&self.shared.settings, &id, value);
                let parsed = self
                    .shared
                    .settings
                    .specs()
                    .find(|spec| spec.id.as_str() == id)
                    .and_then(|spec| Some((spec.id.clone(), spec.parse(&value)?)));
                if let Some((id, value)) = parsed {
                    values.insert(id, value);
                }
            }
            values
        });
        self.shared.settings.set_scope(values);
        if had_values
            || self
                .shared
                .settings
                .scope()
                .is_some_and(|scope| !scope.is_empty())
        {
            self.native_setting_typed("", None);
        }
        let owner = self.stack.last().map(|top| top.serial);
        self.scope = path.map(|path| Scope {
            path,
            owner,
            unreadable,
        });
        self.announce_scope_changes(before);
    }

    /// Changes a setting in the active scope and saves the scope.
    pub(crate) fn set_in_scope(&mut self, id: &Id, value: paperpaw_sdk::SettingValue) {
        // This explicit operation always writes to the active scope, even
        // before the scope-only switch has been turned on.
        let native_old = self.native_event_sink().map(|_| {
            crate::native_host::event_producers::OldValue::capture(
                self.shared
                    .settings
                    .scope()
                    .and_then(|scope| scope.get(id.as_str())),
            )
        });
        let before = self.scoped_values();
        if let Err(error) = self.shared.settings.set_in_scope(id.as_str(), value) {
            return self.warn_runtime(format!("`{id}`: {error}"));
        }
        self.native_setting_after(id.as_str(), native_old.as_ref());
        self.save_scope();
        self.announce_scope_changes(before);
        // The switch itself is not a scoped value others see change.
        if *id == core_settings::SCOPE_ONLY {
            self.output
                .commands
                .push(events::SETTING_CHANGED.emit(events::SettingChanged::new(id.clone())));
        }
    }

    /// Writes the active scope's values to its file (removed when nothing
    /// is left in it), keeping what else it holds (e.g. settings of a
    /// plugin switched off); not over a file that could not be read.
    pub(crate) fn save_scope(&mut self) {
        let (Some(scope), Some(values)) = (&self.scope, self.shared.settings.scope()) else {
            return;
        };
        let storage = self.shared.storage.as_ref();
        let path = scope.path.clone();
        let source = if scope.unreadable {
            Err(StorageError::InvalidData)
        } else {
            paperpaw_sdk::files::load_text_for_update(storage, &path, MAX_SCOPE)
        };
        let source = match source {
            Ok(source) => source.unwrap_or_default(),
            Err(error) => {
                return self.warn_runtime(format!(
                    "{path} cannot be read ({error}): settings not saved to it"
                ));
            }
        };
        let set = values.iter().filter_map(|(id, value)| {
            let (section, key) = id.as_str().split_once('.')?;
            Some((section, key, value.to_config_text()))
        });
        let mut text = paperpaw_config::set_all(&source, set);
        // Scoped settings no longer set in it: their own values are gone.
        let settings = &self.shared.settings;
        let gone: Vec<(String, String)> = paperpaw_config::parse(&source)
            .filter_map(Result::ok)
            .filter(|entry| {
                let id = format!("{}.{}", entry.section, entry.key);
                settings.spec(&id).is_some_and(|spec| spec.scoped)
                    && values.get(id.as_str()).is_none()
            })
            .map(|entry| (entry.section.to_string(), entry.key.to_string()))
            .collect();
        for (section, key) in gone {
            text = paperpaw_config::remove(&text, &section, &key);
        }
        if text == source {
            return;
        }
        let empty = paperpaw_config::parse(&text).next().is_none();
        let saved = if empty {
            storage.remove(&path)
        } else {
            storage.write(&path, text.as_bytes())
        };
        if saved.is_err() {
            self.warn_runtime(format!("could not save settings to {path}"));
        }
    }

    /// Ends the scope when the screen that set it is gone.
    pub(crate) fn check_scope_owner(&mut self) {
        let Some(scope) = &self.scope else {
            return;
        };
        let alive = self.stack.iter().any(|s| Some(s.serial) == scope.owner);
        if !alive {
            self.set_scope(None);
        }
    }

    /// Shows settings without keeping them, for the screen on top (see
    /// [`Command::PreviewSettings`]); empty, ends that.
    pub(crate) fn preview_settings(
        &mut self,
        values: Vec<(Id, Option<paperpaw_sdk::SettingValue>)>,
    ) {
        let ending = values.is_empty();
        if ending && self.preview_owner.is_none() && !self.shared.settings.previewing() {
            return;
        }
        self.shared.settings.set_preview(values);
        self.preview_owner = if ending {
            None
        } else {
            self.stack.last().map(|top| top.serial)
        };
        self.output.commands.push(events::PREVIEW_CHANGED.emit(()));
        self.native_hint(crate::native_host::registrations::EVENT_PREVIEW_CHANGED_TAG);
    }

    /// Ends the values shown without being kept when the screen that
    /// asked for them is gone.
    pub(crate) fn check_preview_owner(&mut self) {
        let Some(owner) = self.preview_owner else {
            return;
        };
        if !self.stack.iter().any(|s| s.serial == owner) {
            self.preview_settings(Vec::new());
        }
    }

    /// Sets `key` in `section` of the configuration file on the device.
    pub(crate) fn save_config(&mut self, section: &str, key: &str, value: &str) {
        self.update_config(&format_args!("`{section}.{key}`"), |source| {
            paperpaw_config::set(source, section, key, value)
        });
    }

    /// Changes the configuration file on the device with `change`, from
    /// its text (empty where there is none) to the text written; `what`
    /// names the change where it could not be saved. A file that is there
    /// but could not be read is left as it is: written from nothing, all
    /// else in it would be lost. The user is told then.
    pub(crate) fn update_config(
        &mut self,
        what: &dyn fmt::Display,
        change: impl FnOnce(&str) -> String,
    ) {
        let storage = self.shared.storage.as_ref();
        let source = match read_config(storage) {
            Ok(source) => source.unwrap_or_default(),
            Err(error) => return self.config_unreadable(what, error),
        };
        let updated = change(&source);
        if updated != source && storage.write(CONFIG_FILE, updated.as_bytes()).is_err() {
            self.warn_runtime(format!("could not save {what} to {CONFIG_FILE}"));
        }
    }

    /// Says that the configuration file could not be read, so `what` was
    /// not saved to it: in a warning, and to the user.
    fn config_unreadable(&mut self, what: &dyn fmt::Display, error: StorageError) {
        self.warn_runtime(format!(
            "{CONFIG_FILE} cannot be read ({error}): {what} not saved, the file left as it is"
        ));
        let text = self.config_notice();
        self.output.commands.push(Command::Notify(text));
    }

    /// What tells the user that the configuration file could not be read.
    pub(crate) fn config_notice(&self) -> String {
        let file = paperpaw_sdk::Arg::Str(CONFIG_FILE);
        self.shared
            .translator
            .format("core.config-unreadable", &[("file", file)])
    }

    /// Switches a plugin for the next start (see [`Command::SetPluginEnabled`]).
    pub(crate) fn set_plugin_enabled(&mut self, id: &Id, enabled: bool) {
        let Some(info) = self.shared.plugins.get(id.as_str()) else {
            return self.warn_runtime(format!("unknown plugin `{id}`"));
        };
        if !enabled && !info.can_switch_off() {
            return self.warn_runtime(format!("plugin `{id}` cannot be switched off"));
        }
        if info.enabled == enabled {
            return;
        }
        self.save_config("plugins", id.as_str(), if enabled { "on" } else { "off" });
        if let Some(info) = self.shared.plugin_mut(id.as_str()) {
            info.enabled = enabled;
        }
        self.native_hint(crate::native_host::registrations::EVENT_PLUGINS_CHANGED_TAG);
        self.native_registry_changed(crate::native_host::REGISTRY_ALL);
    }

    pub(crate) fn setting_changed(&mut self, id: &Id) {
        if *id == core_settings::LANGUAGE {
            self.shared
                .translator
                .set_language(self.shared.settings.text(id.as_str()));
        } else if *id == core_settings::NIGHT_MODE {
            self.framebuffer
                .set_inverted(self.shared.settings.bool(id.as_str()));
        } else if *id == core_settings::NIGHT_PICTURES {
            self.framebuffer
                .set_pictures_kept(!self.shared.settings.bool(id.as_str()));
        } else if *id == core_settings::MONO_TEXT {
            self.framebuffer
                .set_mono_coverage(mono_coverage(&self.shared.settings));
        }
        if *id == core_settings::KEYMAP_PRESET
            || self.keys.when.iter().any(|(s, _)| s == id)
            || self.native_key_when(id.as_str())
        {
            self.rebuild_keymap();
        }
        if *id == core_settings::STATUS_FONT {
            self.apply_status_font();
        }
        if *id == core_settings::UI_TEXT_CURVE || *id == core_settings::LANGUAGE {
            self.apply_ui_text_curve();
        }
        if *id == core_settings::READING_FONT {
            self.apply_reading_font();
        }
        if *id == core_settings::FONT_SIZE {
            self.font_size_changed();
        }
        // Its size follows the interface's. A shell that turns the screen
        // resizes the kernel for it (see `Kernel::rotation`), which makes
        // the screens anew once, for the row too.
        if *id == core_settings::BUTTON_HINTS
            || *id == core_settings::UI_SIZE
            || (*id == core_settings::ROTATION && !self.shared.caps.rotation)
        {
            let shown = self.shown_as_laid_out();
            self.apply_hint_height();
            // A detail beside its list where the screen is wide at the
            // new size, alone where it no longer is.
            self.sync_panes(shown);
        }
        if *id == core_settings::LANGUAGE
            || *id == core_settings::NIGHT_MODE
            || *id == core_settings::NIGHT_PICTURES
            || *id == core_settings::MONO_TEXT
            || *id == core_settings::TEXT_COLOR
            || *id == core_settings::PAPER_COLOR
            || *id == core_settings::TEXT_COLOR_OWN
            || *id == core_settings::PAPER_COLOR_OWN
        {
            let bounds = self.framebuffer.bounds();
            self.invalidate(bounds);
            self.output.request_refresh(RefreshMode::Full);
        }
    }

    #[expect(
        clippy::too_many_lines,
        reason = "longer than the limit from before it was set; split it when it is next changed"
    )]
    pub(crate) fn apply_user_config(&mut self, source: &str) {
        let ids = entries(source);
        // Secret settings kept in the file before: moved to the secrets.
        let mut secrets_moved: Vec<(String, String)> = Vec::new();
        for entry in paperpaw_config::parse(source) {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    self.warn(
                        WarningSource::UserConfig,
                        Some(error.line),
                        error.to_string(),
                    );
                    continue;
                }
            };
            let line = Some(entry.line);
            let value = entry.value.as_str();
            match entry.section {
                // `[firmware]` is the firmware's own, read before the kernel
                // (`dual-boot`, see the X4's shell): not a plugin's.
                "plugins" | "firmware" => {}
                section if section.starts_with("keymap.") || section.starts_with("zones.") => {}
                "fonts" => match (Id::parse(entry.key), Id::parse(&value)) {
                    (Ok(role), Ok(font)) if self.shared.fonts.contains(font.as_str()) => {
                        self.shared.fonts.bind_user(role, font);
                    }
                    (Ok(_), Ok(font)) => self.warn(
                        WarningSource::UserConfig,
                        line,
                        format!("unknown font `{font}`"),
                    ),
                    _ => self.warn(
                        WarningSource::UserConfig,
                        line,
                        "invalid font binding".into(),
                    ),
                },
                "menus" | crate::plugins::KNOWN => {
                    let actions: Result<Vec<Id>, _> = value
                        .split(',')
                        .map(str::trim)
                        .filter(|a| !a.is_empty())
                        .map(Id::parse)
                        .collect();
                    self.shared.registry_generation.bump();
                    let known = entry.section == crate::plugins::KNOWN;
                    match (Id::parse(entry.key), actions) {
                        (Ok(menu), Ok(actions)) if known => {
                            self.shared.menus.set_known(menu, actions);
                        }
                        (Ok(menu), Ok(actions)) => self.shared.menus.customize(menu, actions),
                        _ => self.warn(WarningSource::UserConfig, line, "invalid menu".into()),
                    }
                }
                "slots" => match (Id::parse(entry.key), Id::parse(&value)) {
                    (Ok(slot), Ok(screen)) => {
                        self.shared.registry_generation.bump();
                        self.shared.registry.bind_user(slot, screen);
                    }
                    _ => self.warn(
                        WarningSource::UserConfig,
                        line,
                        "invalid slot binding".into(),
                    ),
                },
                "" => self.warn(
                    WarningSource::UserConfig,
                    line,
                    "settings belong in a [<plugin-id>] section".into(),
                ),
                section => {
                    let id = format!("{section}.{}", entry.key);
                    let (id, value) = match moved(&id, &value, &ids) {
                        Some((now, value)) => (now.to_string(), Cow::Owned(value)),
                        // Moved, and set under its new id as well.
                        None if MOVED.iter().any(|(old, _)| *old == id) => continue,
                        None => (id, value),
                    };
                    let value =
                        Cow::<str>::Owned(renamed(&self.shared.settings, &id, value.into_owned()));
                    let secret = self
                        .shared
                        .settings
                        .spec(&id)
                        .is_some_and(|spec| spec.extra().secret);
                    if secret {
                        secrets_moved.push((id, value.to_string()));
                        continue;
                    }
                    match self.shared.settings.set_from_text(&id, &value) {
                        // The reading app of a format with one reader now
                        // (another was removed): nothing to choose, nothing
                        // to tell.
                        Err(paperpaw_sdk::SettingError::Unknown)
                            if id.starts_with(&format!(
                                "{}.",
                                paperpaw_sdk::builtin::settings::READING_APP.as_str()
                            )) => {}
                        Err(error) => {
                            if matches!(error, paperpaw_sdk::SettingError::Unknown) {
                                // A native plugin may declare it later.
                                self.keep_config_value(&id, &value, line);
                            }
                            self.warn(WarningSource::UserConfig, line, format!("`{id}`: {error}"));
                        }
                        Ok(()) => {}
                    }
                }
            }
        }
        self.move_secrets(secrets_moved);
        // Kept apart (see `KeySources::user`).
        for problem in self.shared.keymap.load(source, Origin::User, false) {
            self.warn(
                WarningSource::UserConfig,
                Some(problem.line),
                problem.message.into_owned(),
            );
        }
    }

    /// Secret settings the configuration file kept: moved to the secrets and
    /// taken out of the file once they are saved there.
    pub(crate) fn move_secrets(&mut self, moved: Vec<(String, String)>) {
        for (id, value) in moved {
            let label = self
                .shared
                .settings
                .spec(&id)
                .map(|spec| String::from(self.shared.translator.tr(&spec.title)))
                .unwrap_or_default();
            self.set_secret(paperpaw_sdk::secrets::SecretChange::keep(
                &id, &label, &value,
            ));
            // Left in the file until it is with the secrets on the card.
            if !self.shared.secrets.saved(&id) {
                continue;
            }
            if let Some((section, key)) = id.split_once('.') {
                self.update_config(&format_args!("`{id}`"), |source| {
                    paperpaw_config::remove(source, section, key)
                });
            }
        }
    }

    /// Reports references to contributions that do not exist.
    pub(crate) fn validate(&mut self) {
        let mut messages = Vec::new();
        for (slot, screen) in self.shared.registry.user_bindings() {
            if !self.shared.registry.slots.contains_key(slot) {
                messages.push(format!("unknown slot `{slot}`"));
            } else if !self.shared.registry.screens.contains_key(screen) {
                messages.push(format!("unknown screen `{screen}` for slot `{slot}`"));
            }
        }
        for (setting, _) in &self.keys.when {
            if self.shared.settings.get(setting.as_str()).is_none() {
                messages.push(format!("unknown switch `{setting}` for a key map"));
            }
        }
        let preset_actions = self.preset_actions();
        let mut unknown: Vec<&Id> = self
            .shared
            .keymap
            .actions()
            .chain(self.shared.menus.referenced_items())
            .chain(preset_actions.iter())
            .filter(|id| {
                !self.shared.actions.contains(id.as_str())
                    && self.shared.settings.get(id.as_str()).is_none()
                    && !self.shared.menus.is_section(id.as_str())
            })
            .collect();
        paperpaw_sort::sort(&mut unknown);
        unknown.dedup();
        for action in unknown {
            messages.push(format!("unknown action `{action}` in a key map or menu"));
        }
        for message in messages {
            self.warn_runtime(message);
        }
    }
}

/// The configuration file on the device: `None` where there is none, an
/// error where it is there but cannot be read (not UTF-8, too large, a
/// failing card). A byte order mark at its start is left out.
pub(crate) fn read_config(storage: &dyn Storage) -> Result<Option<String>, StorageError> {
    paperpaw_sdk::files::load_text_for_update(storage, CONFIG_FILE, MAX_CONFIG)
}

/// Plugin ids switched on (`on`) or off in the `[plugins]` section of a
/// user config.
pub(crate) fn plugin_switches(source: &str, on: bool) -> Vec<String> {
    paperpaw_config::parse(source)
        .filter_map(Result::ok)
        .filter(|entry| entry.section == "plugins" && entry.value.as_bool() == Some(on))
        .map(|entry| String::from(entry.key))
        .collect()
}

/// From what coverage an edge pixel is drawn in black and white, by the
/// setting [`core_settings::MONO_TEXT`].
pub(crate) fn mono_coverage(settings: &Settings) -> u8 {
    let level = settings.text(core_settings::MONO_TEXT.as_str());
    match core_settings::MONO_TEXT_LEVELS
        .iter()
        .position(|l| *l == level)
    {
        Some(0) => 128,
        Some(2) => 64,
        _ => paperpaw_graphics::MONO_COVERAGE,
    }
}
