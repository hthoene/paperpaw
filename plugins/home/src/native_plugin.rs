//! The flat home screen plugin, first slice: what needs no dashboard.
//!
//! Here: the editor of the footer's items (the kit's status items view on the
//! screen `status-items`), the two actions (*Edit home screen*, *All tools*),
//! and the declarations the image carries. The dashboard, *Add* and the tools
//! drawer are declared and answer `Unsupported` when opened until their
//! slices (`PORTING-7.md`); nothing opens them while the interface-6 home
//! screen is the one the app has.
use alloc::string::String;
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, DrawPurpose, Input},
        commands::CommandSender,
        events::{Event, Incoming, kinds},
        registrations,
        registry::StatusScope,
        reply::Reply,
        settings::{self, SettingValue},
        ui::{HandoverId, Instance, Open, Param, Ui},
    },
};
use paperpaw_ui_kit::read_event;
use paperpaw_ui_kit::{
    Messages, Screens,
    mem::{SERVICE_BUFFER, buffer},
    status_items::{StatusItemsView, Texts, view::read_entries},
    text::copy,
};

mod image {
    include!(concat!(env!("OUT_DIR"), "/home_declarations.rs"));
}

use crate::declarations::{ALL_TOOLS, EDIT_PAGES, STATUS_ITEMS};

/// The setting the footer's editor changes.
const FOOTER_ITEMS: &str = "home.footer.items";
/// The screen the editor is declared as.
const ITEMS_SCREEN: &str = "home.status-items";
/// The screens whose slices come later.
const LATER_SCREENS: [&str; 3] = ["home.dashboard", "home.add-widget", "home.drawer"];
/// Space for the two commands that open a screen.
const COMMANDS: usize = 512;

/// A footer items editor: the setting it changes and its view.
#[derive(Debug)]
struct Editor {
    setting: String,
    view: StatusItemsView,
}

/// The interface-7 home screen plugin.
#[derive(Debug, Default)]
pub struct NativeHome {
    ui: Option<Ui>,
    screens: Screens<Editor>,
}

impl Plugin for NativeHome {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS
            .iter()
            .chain(image::SCREENS)
            .chain(image::NAVIGATION)
            .chain(image::EVENTS)
        {
            registrations::submit(host, batch)?;
        }
        // `core.ui` is opened by the first callback: opening needs a live frame.
        Ok(Self::default())
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        incoming: Incoming<'_>,
        _: &mut Reply<'_>,
    ) -> Result<(), Error> {
        self.ensure_ui(host)?;
        let Some(event) = read_event!(incoming; kinds::ActionRun, kinds::ScreenOpened, kinds::ScreenShown, kinds::ScreenClosed, kinds::SettingChanged)
        else {
            return Ok(());
        };
        match event {
            Event::ActionRun { .. } if token == ALL_TOOLS => {
                let mut scratch = [0; COMMANDS];
                CommandSender::new(host, &mut scratch).open(Open::screen("home.drawer")?)
            }
            Event::ActionRun { .. } if token == EDIT_PAGES => self.edit_pages(host),
            Event::ScreenOpened {
                screen,
                instance,
                handover,
                ..
            } => self.opened(host, screen, instance, handover),
            Event::ScreenShown {
                screen: ITEMS_SCREEN,
                instance: Some(instance),
                ..
            } => self.screens.shown(STATUS_ITEMS, instance),
            Event::ScreenClosed { instance, .. } => {
                self.screens.close(instance);
                Ok(())
            }
            // The pages are fitted to a new grid by the dashboard's slice, which
            // measures widgets (`PORTING-7.md`).
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        if !matches!(frame.purpose, DrawPurpose::Screen) {
            return Ok(());
        }
        let editor = self.screens.top(token).ok_or(Error::State)?;
        editor.view.draw(host, &mut frame)
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        match self.screens.top(token) {
            Some(editor) => editor.view.input(host, &editor.setting, &input),
            None => Ok(Handled::No),
        }
    }
}

impl NativeHome {
    /// Opens `core.ui` in the first live callback; the handle serves later ones.
    fn ensure_ui(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.ui.is_none() {
            self.ui = Some(Ui::open(host)?);
        }
        Ok(())
    }

    fn ui(&self) -> Result<&Ui, Error> {
        self.ui.as_ref().ok_or(Error::State)
    }

    /// *Edit home screen*: the home slot opens (or the dashboard turns to
    /// editing, once it exists) with `mode` = `edit`.
    fn edit_pages(&self, host: &Host<'_>) -> Result<(), Error> {
        let mut scratch = [0; COMMANDS];
        let mut output = buffer(SERVICE_BUFFER)?;
        let mode = Param::new("mode", "edit")?;
        let handover = self
            .ui()?
            .handover(host, &[mode], &mut scratch, &mut output)?;
        CommandSender::new(host, &mut scratch).open(Open::slot("core.home")?.handover(handover))
    }

    fn opened(
        &mut self,
        host: &Host<'_>,
        screen: &str,
        instance: Instance,
        handover: Option<HandoverId>,
    ) -> Result<(), Error> {
        if LATER_SCREENS.contains(&screen) {
            return Err(Error::Unsupported);
        }
        if screen != ITEMS_SCREEN {
            return Err(Error::Invalid);
        }
        let editor = self.items_editor(host, handover)?;
        self.screens.open(instance, editor)
    }

    /// The editor for the setting the handover names (`id`): the items there
    /// are, and what the setting holds now.
    fn items_editor(&self, host: &Host<'_>, handover: Option<HandoverId>) -> Result<Editor, Error> {
        let mut setting = None;
        if let Some(handover) = handover {
            let mut input = buffer(SERVICE_BUFFER)?;
            let mut output = buffer(SERVICE_BUFFER)?;
            for param in self.ui()?.params(host, handover, &mut input, &mut output)? {
                let param = param?;
                if param.key() == "id" {
                    setting = Some(copy(param.value())?);
                }
            }
        }
        // The only setting this screen is declared as the editor of.
        let setting = setting
            .filter(|id| id == FOOTER_ITEMS)
            .ok_or(Error::Invalid)?;
        let mut scratch = buffer(SERVICE_BUFFER)?;
        let held = match settings::read_setting(host, &setting, &mut scratch)? {
            SettingValue::Text(text) => copy(text)?,
            _ => String::new(),
        };
        let entries = read_entries(host, StatusScope::Items)?;
        let texts = Texts::read(host)?;
        let title = Messages::new()?.copy(host, "home.setting.footer-items")?;
        let view = StatusItemsView::new(entries, &held, title, texts)?;
        Ok(Editor { setting, view })
    }
}
