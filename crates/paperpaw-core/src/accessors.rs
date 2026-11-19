//! Plain accessors of the kernel, and the secrets it keeps for shells.

use crate::{
    Ask, Effects, Fonts, Formats, Id, Instant, Kernel, MAX_SECRETS, Params, Plugins, Settings,
    Storage, Translator, Warning, config, events, meter, secrets_i6, slots,
};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::mem;
use paperpaw_platform::DeviceInfo;
#[cfg(test)]
use paperpaw_platform::FrameView;
use paperpaw_sdk::event::EventData;

impl Kernel {
    /// The framebuffer, for tests of a kernel that holds the whole screen.
    #[cfg(test)]
    pub(crate) fn frame(&self) -> FrameView<'_> {
        self.framebuffer.view()
    }

    pub fn device(&self) -> &DeviceInfo {
        &self.shared.device
    }

    /// The panel took `millis` for a refresh: shells that know say so
    /// ([`Usage::panel_ms`](paperpaw_sdk::usage::Usage::panel_ms)).
    pub fn panel_busy(&mut self, millis: u64) {
        self.shared.usage.panel_ms = self.shared.usage.panel_ms.saturating_add(millis);
    }

    /// The device's storage, e.g. for a shell that copies files onto it.
    pub fn storage(&self) -> &dyn Storage {
        self.shared.storage.as_ref()
    }

    /// The power level and mode worn (see [`paperpaw_sdk::power`]), e.g.
    /// for a shell that slows its processor down while saving.
    pub fn power(&self) -> &paperpaw_sdk::power::Power {
        &self.shared.power
    }

    pub fn settings(&self) -> &Settings {
        &self.shared.settings
    }

    /// The device's name as others see it (`Place::device_name`), e.g. for
    /// a shell that gives it to the router as its host name.
    pub fn device_name(&self) -> String {
        let value = self
            .shared
            .settings
            .text(paperpaw_sdk::device_name::SETTING.as_str());
        paperpaw_sdk::device_name::chosen(value, &self.shared.device)
    }

    /// The passwords and keys (see [`paperpaw_sdk::secrets`]), e.g. for a
    /// shell that joins networks itself.
    pub fn secrets(&self) -> &paperpaw_sdk::secrets::Secrets {
        &self.shared.secrets
    }

    /// Keeps or removes a password or key, as [`Ask::SetSecret`] of a
    /// plugin does (e.g. a network's password a shell was given).
    ///
    /// The file is read again first: where it is there but cannot be
    /// read, it is left as it is (written, the others in it would be
    /// lost), the change applies until the end, and the user is told; one
    /// readable again takes the place of what could not be read at start.
    /// A change not written is not [`saved`](paperpaw_sdk::secrets::Secrets::saved).
    pub fn set_secret(&mut self, change: paperpaw_sdk::secrets::SecretChange) {
        use paperpaw_sdk::secrets;
        let storage = self.shared.storage.clone();
        let sealer = self.secrets_host();
        let read =
            paperpaw_sdk::files::load_for_update(storage.as_ref(), secrets::FILE, MAX_SECRETS);
        if let Ok(data) = &read
            && (self.secrets_unreadable || sealer.is_some())
        {
            // Damaged text keeps what can be read: values are in hex. Where
            // values are sealed the file is read again each time: native
            // plugins write it too.
            let text = String::from_utf8_lossy(data.as_deref().unwrap_or_default());
            self.shared.secrets = Self::decode_secrets(&text, sealer.as_ref());
            self.secrets_unreadable = false;
        }
        let id = change.id.clone();
        if sealer.is_some() && change.value.is_some() && !secrets_i6::sealable_id(&id) {
            // No owner to bind a sealed value to: it is never applied, so it
            // is never written veiled (nor does it stop later saves).
            self.shared.secrets.mark_unsaved(&id);
            self.warn_runtime(format!("{id}: cannot be sealed (no owner): not saved"));
            return;
        }
        self.shared.secrets.apply(change);
        if let Err(error) = read {
            self.shared.secrets.mark_unsaved(&id);
            self.secrets_unreadable = true;
            let file = secrets::FILE;
            self.warn_runtime(format!(
                "{file} cannot be read ({error}): not saved, left as it is"
            ));
            let file = paperpaw_sdk::Arg::Str(file);
            let text = self
                .shared
                .translator
                .format("core.secrets-unreadable", &[("file", file)]);
            self.output.commands.push(Ask::Notify(text));
            return;
        }
        let previous = read
            .as_ref()
            .ok()
            .and_then(|d| d.as_deref())
            .unwrap_or_default();
        let previous = String::from_utf8_lossy(previous);
        let text =
            match Self::sealed_secrets(self.shared.secrets.encode(), &previous, sealer.as_ref()) {
                Ok(text) => text,
                Err(status) => {
                    // No value is written veiled where values are sealed.
                    self.shared.secrets.mark_unsaved(&id);
                    self.warn_runtime(format!(
                        "{}: not sealed ({status}): not saved",
                        secrets::FILE
                    ));
                    return;
                }
            };
        let storage = self.shared.storage.as_ref();
        let _ = storage.create_dir(paperpaw_sdk::data::DIR);
        let saved = if self.shared.secrets.is_empty() {
            storage.remove(secrets::FILE).or(Ok(()))
        } else {
            storage.write(secrets::FILE, text.as_bytes())
        };
        match saved {
            Ok(()) => self.shared.secrets.mark_saved(),
            Err(error) => {
                self.shared.secrets.mark_unsaved(&id);
                let message = format!("{}: {error}", secrets::FILE);
                self.warn_runtime(message);
            }
        }
    }

    pub fn translator(&self) -> &Translator {
        &self.shared.translator
    }

    pub fn fonts(&self) -> &Fonts {
        &self.shared.fonts
    }

    pub fn formats(&self) -> &Formats {
        &self.shared.formats
    }

    /// The widgets every plugin added (e.g. for a test that draws each).
    pub fn widgets(&self) -> &crate::kit::widgets::Widgets {
        &self.shared.widgets
    }

    /// The menus every plugin added to, with their icons (e.g. for a tool
    /// that checks a plugin's).
    pub fn menus(&self) -> crate::kit::menus::MenuView<'_> {
        crate::kit::menus::MenuView::new(
            &self.shared.menus,
            &self.shared.actions,
            &self.shared.settings,
        )
    }

    /// Every plugin with its state, `core` first.
    pub fn plugins(&self) -> &Plugins {
        &self.shared.plugins
    }

    /// The id of the screen currently shown.
    pub fn active_screen(&self) -> Option<&Id> {
        self.stack.last().map(|s| &s.id)
    }

    /// The ids of the screens open, the one shown last.
    pub fn screens(&self) -> impl Iterator<Item = &Id> {
        self.stack.iter().map(|s| &s.id)
    }

    /// The ids of every screen plugins added, whether open or not.
    pub fn known_screens(&self) -> impl Iterator<Item = &Id> {
        self.shared.registry.screens.list().iter().map(|(id, _)| id)
    }

    /// What the device did since start (see [`paperpaw_sdk::usage`]).
    pub fn usage(&self) -> &paperpaw_sdk::usage::Usage {
        &self.shared.usage
    }

    /// What each plugin took since start, `core` first.
    pub fn plugin_usage(&self) -> Vec<paperpaw_sdk::usage::PluginUsage> {
        self.shared.meter.table.borrow().clone()
    }

    /// Every action plugins added.
    pub fn actions(&self) -> &crate::kit::action::Actions {
        &self.shared.actions
    }

    /// Sends `event` with `params` as a plugin would, to every plugin and
    /// screen that hears it, e.g. asked by a shell's remote control or a
    /// test.
    pub fn send_event(&mut self, now: Instant, event: Id, params: Params) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        // Asked from outside (a remote control, a test): as the reader.
        self.shared.reader = true;
        self.shared.now = now;
        self.output.commands.push(Ask::Emit(event, params));
        self.run_commands();
        self.finish()
    }

    /// The screen bound to [`slots::KEYBOARD`], if one is: what a tool
    /// opens with [`Self::stage_text_question`], not with a bare
    /// [`Self::open_screen`] (a native keyboard needs the question).
    #[must_use]
    pub fn keyboard_screen(&self) -> Option<&Id> {
        self.shared.registry.resolve_slot(slots::KEYBOARD.as_str())
    }

    /// Types `text` into the text field shown, as if on its keys, and
    /// confirms it with `done` ([`events::TYPE_TEXT`]): for a shell's
    /// remote control or keys of a keyboard. `None` where no text field
    /// shows (the screen bound to [`slots::KEYBOARD`] is not on top).
    pub fn type_text(&mut self, now: Instant, text: &str, done: bool) -> Option<Effects> {
        let keyboard = self
            .shared
            .registry
            .resolve_slot(slots::KEYBOARD.as_str())?;
        if self.active_screen() != Some(keyboard) {
            return None;
        }
        let typed = events::TypeText::new(text.into()).done(done);
        Some(self.send_event(now, events::TYPE_TEXT.id().clone(), typed.to_params()))
    }

    /// The update on trial is kept, its mark written for good
    /// ([`crate::KernelBuilder::native_after_trial`]): plugins now see what the
    /// firmware takes, and the store, opened or opened again, asks for the
    /// builds that fit. Nothing is told where the update is not (the mark
    /// could not be written: it may still go back).
    pub fn trial_kept(&mut self) {
        if let Some(native) = self.shared.after_trial.take() {
            self.shared.native = Some(native);
        }
    }

    /// Returns and clears the collected warnings.
    pub fn take_warnings(&mut self) -> Vec<Warning> {
        self.report_event_failures();
        mem::take(&mut self.warnings)
    }

    /// Sets setting `id` from its text as configuration files write it
    /// (`on`, `12`, a choice's value), as the settings screen would: saved,
    /// what follows from it done, plugins told. E.g. for a shell's remote
    /// control or a test; why not, where the setting is unknown or the
    /// text no value of it.
    pub fn set_setting(
        &mut self,
        now: Instant,
        id: &str,
        text: &str,
    ) -> Result<Effects, &'static str> {
        // A placement's too (`widgets.clock@2.face`).
        let spec = config::spec_to_set(&self.shared.settings, id)?;
        let value = spec.parse(text).ok_or("not a value of this setting")?;
        let id = Id::parse(id).map_err(|_| "no such setting")?;
        let _running = self.shared.meter.enter(meter::CORE);
        self.shared.now = now;
        self.last_activity = now;
        self.output.commands.push(Ask::SetSetting(id, value));
        // The shell's, on the reader's behalf: also while a screen guards
        // the device.
        self.trusted.push(self.output.commands.len() - 1);
        self.run_commands();
        Ok(self.finish())
    }
}
