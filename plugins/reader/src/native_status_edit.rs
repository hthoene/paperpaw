//! The editors of the status lines' settings, as Beta 3 had them (the kernel's
//! status lines and status items screens then): the screen `status-edit`, one
//! per setting, on the kit's status items view. For the lines of a place
//! (`status.top-lines`, `status.bottom-lines`) it lists the status lines, for a
//! part of a line the status items. Each change is set at once; *Back* leaves.
use alloc::string::String;
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host,
        callbacks::{DrawFrame, Input},
        registry::StatusScope,
        settings::{self, SettingValue},
        ui::{HandoverId, Instance, Ui},
    },
};
use paperpaw_ui_kit::{
    Messages, Screens,
    mem::{SERVICE_BUFFER, buffer},
    status_items::{StatusItemsView, Texts, view::read_entries},
    text::copy,
};

/// The settings the screen is declared as the editor of; each is also the key
/// of its title.
pub const EDITED: [&str; 8] = [
    "reader.status.top-lines",
    "reader.status.bottom-lines",
    "reader.status.top-left",
    "reader.status.top-center",
    "reader.status.top-right",
    "reader.status.bottom-left",
    "reader.status.bottom-center",
    "reader.status.bottom-right",
];

/// One open editor: the setting it changes and its view.
#[derive(Debug)]
struct Editor {
    setting: &'static str,
    view: StatusItemsView,
}

/// The open editors.
#[derive(Debug, Default)]
pub struct StatusEditors {
    screens: Screens<Editor>,
}

impl StatusEditors {
    /// The editor of the setting the handover names (`id`, one of [`EDITED`])
    /// opened as `instance`: the lines or the items there are, and what the
    /// setting holds now. `ui` is opened where it is not yet.
    ///
    /// # Errors
    /// `Invalid` for no setting or another one; registry, settings, catalog and
    /// allocation errors.
    #[inline(never)]
    pub fn open(
        &mut self,
        host: &Host<'_>,
        ui: &mut Option<Ui>,
        instance: Instance,
        handover: Option<HandoverId>,
    ) -> Result<(), Error> {
        let handover = handover.ok_or(Error::Invalid)?;
        let ui = match ui {
            Some(ui) => ui,
            None => ui.insert(Ui::open(host)?),
        };
        let mut input = buffer(SERVICE_BUFFER)?;
        let mut output = buffer(SERVICE_BUFFER)?;
        let mut setting = None;
        for param in ui.params(host, handover, &mut input, &mut output)? {
            let param = param?;
            if param.key() == "id" {
                setting = EDITED.iter().copied().find(|id| *id == param.value());
            }
        }
        drop((input, output));
        let setting = setting.ok_or(Error::Invalid)?;
        let mut scratch = buffer(SERVICE_BUFFER)?;
        let held = match settings::read_setting(host, setting, &mut scratch)? {
            SettingValue::Text(text) => copy(text)?,
            _ => String::new(),
        };
        drop(scratch);
        let scope = if setting.ends_with("-lines") {
            StatusScope::Lines
        } else {
            StatusScope::Items
        };
        let entries = read_entries(host, scope)?;
        let texts = Texts::read(host)?;
        let title = Messages::new()?.copy(host, setting)?;
        let view = StatusItemsView::new(entries, &held, title, texts)?;
        self.screens.open(instance, Editor { setting, view })
    }

    /// The editor `instance` came to the top.
    ///
    /// # Errors
    /// As [`Screens::shown`].
    pub fn shown(&mut self, token: u32, instance: Instance) -> Result<(), Error> {
        self.screens.shown(token, instance)
    }

    /// The editor `instance` closed.
    pub fn close(&mut self, instance: Instance) {
        self.screens.close(instance);
    }

    /// Draws the editor on top.
    ///
    /// # Errors
    /// `State` where none is open; drawing errors.
    #[inline(never)]
    pub fn draw(
        &mut self,
        host: &Host<'_>,
        token: u32,
        frame: &mut DrawFrame<'_>,
    ) -> Result<(), Error> {
        let editor = self.screens.top(token).ok_or(Error::State)?;
        editor.view.draw(host, frame)
    }

    /// Input to the editor on top: its setting changes at once.
    ///
    /// # Errors
    /// Allocation, command and host errors.
    #[inline(never)]
    pub fn input(
        &mut self,
        host: &Host<'_>,
        token: u32,
        input: &Input<'_>,
    ) -> Result<Handled, Error> {
        match self.screens.top(token) {
            Some(editor) => editor.view.input(host, editor.setting, input),
            None => Ok(Handled::No),
        }
    }
}
