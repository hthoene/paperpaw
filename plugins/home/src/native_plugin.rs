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

use crate::declarations::{ALL_TOOLS, DASHBOARD, EDIT_PAGES, STATUS_ITEMS};
use crate::native_dashboard::Dashboard;

/// The setting the footer's editor changes.
const FOOTER_ITEMS: &str = "home.footer.items";
/// The screen the editor is declared as.
const ITEMS_SCREEN: &str = "home.status-items";
/// The screens whose slices come later.
const LATER_SCREENS: [&str; 2] = ["home.add-widget", "home.drawer"];
const DASHBOARD_SCREEN: &str = "home.dashboard";
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
    screens: Screens<Slot>,
}

/// What an open screen of this plugin is.
#[derive(Debug)]
#[allow(
    clippy::large_enum_variant,
    reason = "kept unboxed: a box would allocate infallibly in a callback; the list grows fallibly"
)]
enum Slot {
    Items(Editor),
    Dashboard(Dashboard),
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
                instance: Some(instance),
                ..
            } if token == STATUS_ITEMS => self.screens.shown(STATUS_ITEMS, instance),
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } if token == DASHBOARD => {
                self.screens.shown(DASHBOARD, instance)?;
                if let Some(Slot::Dashboard(dashboard)) = self.screens.get(instance) {
                    dashboard.shown();
                }
                Ok(())
            }
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
        let ui = self.ui.as_ref().ok_or(Error::State)?;
        match self.screens.top(token).ok_or(Error::State)? {
            Slot::Items(editor) => editor.view.draw(host, &mut frame),
            Slot::Dashboard(dashboard) => dashboard.draw(host, ui, &mut frame),
        }
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        self.ensure_ui(host)?;
        let ui = self.ui.as_ref().ok_or(Error::State)?;
        match self.screens.top(token) {
            Some(Slot::Items(editor)) => editor.view.input(host, &editor.setting, &input),
            Some(Slot::Dashboard(dashboard)) => dashboard.input(host, ui, &input),
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
        // Opened to edit (`mode` = `edit`) shows the page as it is until editing
        // is ported (`PORTING-7.md`).
        if screen == DASHBOARD_SCREEN {
            return self
                .screens
                .open(instance, Slot::Dashboard(Dashboard::new()));
        }
        if screen != ITEMS_SCREEN {
            return Err(Error::Invalid);
        }
        let editor = self.items_editor(host, handover)?;
        self.screens.open(instance, Slot::Items(editor))
    }

    /// The editor for the setting the handover names (`id`): the items there
    /// are, and what the setting holds now.
    fn items_editor(&self, host: &Host<'_>, handover: Option<HandoverId>) -> Result<Editor, Error> {
        let mut input = buffer(SERVICE_BUFFER)?;
        let mut output = buffer(SERVICE_BUFFER)?;
        let setting = match handover {
            Some(handover) => editor_setting(
                self.ui()?
                    .params(host, handover, &mut input, &mut output)?
                    .map(|param| param.map(|param| (param.key(), param.value()))),
            )?,
            None => return Err(Error::Invalid),
        };
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

/// The setting a handover names (`id`): the only one this screen is declared as
/// the editor of. No `id`, another one or one repeated differently is
/// `Invalid`.
fn editor_setting<'a>(
    params: impl Iterator<Item = Result<(&'a str, &'a str), Error>>,
) -> Result<String, Error> {
    let mut setting = None;
    for param in params {
        let (key, value) = param?;
        if key == "id" {
            if setting.is_some_and(|seen: &str| seen != value) {
                return Err(Error::Invalid);
            }
            setting = Some(value);
        }
    }
    match setting {
        Some(FOOTER_ITEMS) => copy(FOOTER_ITEMS),
        _ => Err(Error::Invalid),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;
    use crate::fail_alloc;

    fn pairs<'a>(
        list: &'a [(&'a str, &'a str)],
    ) -> impl Iterator<Item = Result<(&'a str, &'a str), Error>> {
        list.iter().map(|pair| Ok(*pair))
    }

    #[test]
    fn the_footer_items_setting_is_the_one_named() {
        let named = [("mode", "edit"), ("id", FOOTER_ITEMS)];
        assert_eq!(editor_setting(pairs(&named)).unwrap(), FOOTER_ITEMS);
    }

    #[test]
    fn another_setting_is_invalid() {
        let other = [("id", "home.columns")];
        assert_eq!(editor_setting(pairs(&other)), Err(Error::Invalid));
        let empty = [("id", "")];
        assert_eq!(editor_setting(pairs(&empty)), Err(Error::Invalid));
    }

    #[test]
    fn a_handover_without_an_id_is_invalid() {
        assert_eq!(editor_setting(pairs(&[])), Err(Error::Invalid));
        let none = [("mode", "edit")];
        assert_eq!(editor_setting(pairs(&none)), Err(Error::Invalid));
    }

    #[test]
    fn two_ids_that_differ_are_invalid() {
        let two = [("id", FOOTER_ITEMS), ("id", "home.columns")];
        assert_eq!(editor_setting(pairs(&two)), Err(Error::Invalid));
    }

    #[test]
    fn a_host_refusal_while_reading_the_params_is_passed_on() {
        let refused = [Ok(("id", FOOTER_ITEMS)), Err(Error::Invalid)];
        assert_eq!(editor_setting(refused.into_iter()), Err(Error::Invalid));
        let busy = [Err(Error::Busy)];
        assert_eq!(editor_setting(busy.into_iter()), Err(Error::Busy));
    }

    #[test]
    fn every_allocation_of_the_setting_may_be_refused() {
        fail_alloc::sweep(|| {
            let named = [("mode", "edit"), ("id", FOOTER_ITEMS)];
            editor_setting(pairs(&named)).map(drop)
        });
    }
}
