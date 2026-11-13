//! The flat home screen plugin, first slice: what needs no dashboard.
//!
//! Here: the editor of the footer's items and of the status line's parts (the
//! kit's status items view on the screen `status-items`; for the line, the
//! status lines other plugins draw), the two actions (*Edit home screen*, *All tools*),
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
        ui::{HandoverId, Instance, Open, Param, Ui, UiCursor},
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

use crate::declarations::{
    ADD, ALL_TOOLS, DASHBOARD, DRAWER, EDIT_PAGES, SETTING_CHANGED, STACK_CHANGED, STATUS_ITEMS,
    STATUS_PARTS, TICK, WIDGET_CHANGED,
};
use crate::native_add_screen::{self as add_screen, AddScreen};
use crate::native_dashboard::{Dashboard, redraw};
use crate::native_drawer::{self as drawer, Drawer};
use crate::native_lines;
use crate::native_reflow::fit;
use crate::native_source::is_grid_setting;
use crate::native_widgets::Widgets;

/// The setting the footer's editor changes.
const FOOTER_ITEMS: &str = "home.footer.items";
/// The status line widget, whose settings (`home.status.<part>`, or of one
/// placement `home.status@<placement>.<part>`) end in the parts
/// `STATUS_PARTS` names.
const STATUS_WIDGET: &str = "home.status";
/// The part that lists whole lines, not items.
const LINE_PART: &str = "line";
/// The screen the editor is declared as.
const ITEMS_SCREEN: &str = "home.status-items";
const DASHBOARD_SCREEN: &str = "home.dashboard";
/// Space for the two commands that open a screen.
const COMMANDS: usize = 512;

/// An items (or lines) editor: the setting it changes and its view.
#[derive(Debug)]
struct Editor {
    setting: String,
    view: StatusItemsView,
}

/// A setting the screen `status-items` edits: its full id, its title, and
/// whether it lists status lines (else status items).
#[derive(Debug, PartialEq, Eq)]
struct Edited {
    setting: String,
    title: &'static str,
    lines: bool,
}

/// The interface-7 home screen plugin.
#[derive(Debug, Default)]
pub struct NativeHome {
    ui: Option<Ui>,
    screens: Screens<Slot>,
    /// The instances of the home screen's own widgets.
    widgets: Widgets,
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
    Add(AddScreen),
    Drawer(Drawer),
}

impl Plugin for NativeHome {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS
            .iter()
            .chain(image::WIDGETS)
            .chain(image::SCREENS)
            .chain(image::NAVIGATION)
            .chain(image::EVENTS)
            .chain(image::HELP)
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
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        self.ensure_ui(host)?;
        let Some(event) = read_event!(incoming; kinds::ActionRun, kinds::ScreenOpened, kinds::ScreenShown, kinds::ScreenClosed, kinds::SettingChanged, kinds::Tick, kinds::WidgetChanged, kinds::StackChanged, kinds::WidgetMade, kinds::WidgetDropped, kinds::StageCall)
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
            } if token == ADD => self.screens.shown(ADD, instance),
            Event::ScreenShown {
                instance: Some(instance),
                last_input,
                ..
            } if token == DRAWER => {
                self.screens.shown(DRAWER, instance)?;
                if let Some(Slot::Drawer(drawer)) = self.screens.get(instance) {
                    drawer.shown(last_input);
                }
                Ok(())
            }
            Event::ScreenShown {
                instance: Some(instance),
                last_input,
                ..
            } if token == DASHBOARD => {
                self.screens.shown(DASHBOARD, instance)?;
                if let Some(Slot::Dashboard(dashboard)) = self.screens.get(instance) {
                    dashboard.shown(last_input);
                }
                Ok(())
            }
            Event::ScreenClosed { instance, .. } => {
                self.screens.close(instance);
                Ok(())
            }
            // The minute tick, and a widget that says it shows something new
            // (`event.widget_changed`): what shows is laid out and drawn anew.
            Event::WidgetMade {
                widget, instance, ..
            } => self.widgets.made(widget, instance),
            Event::WidgetDropped { instance, .. } => {
                self.widgets.dropped(instance);
                Ok(())
            }
            Event::StageCall {
                stage: "core.widget",
                payload,
                ..
            } => self.widgets.stage(host, self.ui()?, payload, reply),
            Event::Tick { .. } if token == TICK => {
                self.widgets
                    .tick(host, self.ui.as_ref().ok_or(Error::State)?);
                self.refresh(host)
            }
            Event::WidgetChanged { .. } if token == WIDGET_CHANGED => self.refresh(host),
            Event::StackChanged { .. } if token == STACK_CHANGED => self.covered(host),
            // The grid's columns or rows changed: the pages are fitted to it.
            Event::SettingChanged { key, .. } if token == SETTING_CHANGED => {
                if is_grid_setting(key) {
                    fit(host)?;
                    self.refresh(host)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        if frame.purpose == DrawPurpose::Widget {
            return self
                .widgets
                .draw(host, self.ui.as_ref().ok_or(Error::State)?, frame);
        }
        if !matches!(frame.purpose, DrawPurpose::Screen) {
            return Ok(());
        }
        let ui = self.ui.as_ref().ok_or(Error::State)?;
        match self.screens.top(token).ok_or(Error::State)? {
            Slot::Items(editor) => editor.view.draw(host, &mut frame),
            Slot::Dashboard(dashboard) => dashboard.draw(host, ui, &mut frame),
            Slot::Add(add) => add.draw(host, ui, &mut frame),
            Slot::Drawer(drawer) => drawer.draw(host, &mut frame),
        }
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        self.ensure_ui(host)?;
        let ui = self.ui.as_ref().ok_or(Error::State)?;
        // A tap on a book of the card or the shelf.
        if let Input::Widget {
            instance,
            target,
            kind,
            ..
        } = input
        {
            let opened = self.widgets.input(host, ui, instance, target, kind)?;
            return Ok(if opened { Handled::Yes } else { Handled::No });
        }
        match self.screens.top(token) {
            Some(Slot::Items(editor)) => editor.view.input(host, &editor.setting, &input),
            Some(Slot::Dashboard(dashboard)) => dashboard.input(host, ui, &input),
            Some(Slot::Add(add)) => add.input(host, ui, &input),
            Some(Slot::Drawer(drawer)) => drawer.input(host, &input),
            None => Ok(Handled::No),
        }
    }
}

/// The room the stack query's request needs (a cursor).
const STACK_REQUEST: usize = 256;

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

    /// The dashboard shows something new (the minute passed, a widget
    /// changed): laid out and drawn anew, as interface 6 did, where it is on
    /// top; under another screen nothing is drawn (it is laid out when shown).
    fn refresh(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let Some(instance) = self.screens.top_instance(DASHBOARD) else {
            return Ok(());
        };
        if self.stack_top(host)? != Some(instance) {
            return Ok(());
        }
        if let Some(Slot::Dashboard(dashboard)) = self.screens.get(instance) {
            dashboard.mark_stale();
        }
        redraw(host)
    }

    /// The stack changed: a dashboard under another screen lets its page go (a
    /// book is read, the page is not held; it is laid out again when shown).
    fn covered(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let holding = self
            .screens
            .all()
            .any(|slot| matches!(slot, Slot::Dashboard(d) if d.holds_page()));
        if !holding {
            return Ok(());
        }
        let top = self.stack_top(host)?;
        let ui = self.ui.as_ref().ok_or(Error::State)?;
        for (instance, slot) in self.screens.all_with_instance() {
            if let Slot::Dashboard(dashboard) = slot
                && Some(instance) != top
            {
                dashboard.cover(host, ui);
            }
        }
        Ok(())
    }

    /// The instance on top of the screen stack, if there is one.
    fn stack_top(&self, host: &Host<'_>) -> Result<Option<Instance>, Error> {
        // The request is a cursor and no more; the reply is a page of rows.
        let mut input = buffer(STACK_REQUEST)?;
        let mut output = buffer(SERVICE_BUFFER)?;
        let page = self
            .ui()?
            .stack(host, UiCursor::start(), &mut input, &mut output)?;
        Ok(page
            .into_iter()
            .next()
            .transpose()?
            .map(|row| row.instance()))
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
        // Opened to edit (`mode` = `edit`) starts editing.
        if screen == DASHBOARD_SCREEN {
            let edit = self.edits(host, handover)?;
            return open_dashboard(&mut self.screens, instance, edit);
        }
        if screen == drawer::SCREEN {
            let drawer = Drawer::new(host, None)?;
            return self.screens.open(instance, Slot::Drawer(drawer));
        }
        if screen == add_screen::SCREEN {
            let add = self.add_screen(host, handover)?;
            return self.screens.open(instance, Slot::Add(add));
        }
        if screen != ITEMS_SCREEN {
            return Err(Error::Invalid);
        }
        let editor = self.items_editor(host, handover)?;
        self.screens.open(instance, Slot::Items(editor))
    }

    /// Whether the handover asks to edit (`mode` = `edit`).
    fn edits(&self, host: &Host<'_>, handover: Option<HandoverId>) -> Result<bool, Error> {
        let Some(handover) = handover else {
            return Ok(false);
        };
        let mut input = buffer(SERVICE_BUFFER)?;
        let mut output = buffer(SERVICE_BUFFER)?;
        edit_mode(
            self.ui()?
                .params(host, handover, &mut input, &mut output)?
                .map(|param| param.map(|param| (param.key(), param.value()))),
        )
    }

    /// The *Add* screen for the page and kind the handover names; opened with
    /// none it adds to the first page.
    fn add_screen(
        &self,
        host: &Host<'_>,
        handover: Option<HandoverId>,
    ) -> Result<AddScreen, Error> {
        let (page, kind) = match handover {
            Some(handover) => {
                let mut input = buffer(SERVICE_BUFFER)?;
                let mut output = buffer(SERVICE_BUFFER)?;
                add_screen::params(
                    self.ui()?
                        .params(host, handover, &mut input, &mut output)?
                        .map(|param| param.map(|param| (param.key(), param.value()))),
                )?
            }
            None => (0, None),
        };
        AddScreen::new(host, page, kind)
    }

    /// The editor for the setting the handover names (`id`): the items (or,
    /// for the status line's line, the lines) there are, and what the setting
    /// holds now. As interface 6's items screen, it stays open while the
    /// choice changes (each change is set at once); *Back* leaves it.
    fn items_editor(&self, host: &Host<'_>, handover: Option<HandoverId>) -> Result<Editor, Error> {
        let mut input = buffer(SERVICE_BUFFER)?;
        let mut output = buffer(SERVICE_BUFFER)?;
        let edited = match handover {
            Some(handover) => editor_setting(
                self.ui()?
                    .params(host, handover, &mut input, &mut output)?
                    .map(|param| param.map(|param| (param.key(), param.value()))),
            )?,
            None => return Err(Error::Invalid),
        };
        drop((input, output));
        let mut scratch = buffer(SERVICE_BUFFER)?;
        let held = match settings::read_setting(host, &edited.setting, &mut scratch)? {
            SettingValue::Text(text) => copy(text)?,
            _ => String::new(),
        };
        drop(scratch);
        let entries = if edited.lines {
            native_lines::entries(host)?
        } else {
            read_entries(host, StatusScope::Items)?
        };
        let texts = Texts::read(host)?;
        let title = Messages::new()?.copy(host, edited.title)?;
        let view = StatusItemsView::new(entries, &held, title, texts)?;
        Ok(Editor {
            setting: edited.setting,
            view,
        })
    }
}

/// Opens the dashboard of `instance`, starting in editing where `edit`. One
/// opened again keeps its view (its page, its widgets): replacing it would
/// drop the widgets it made without letting them go.
fn open_dashboard(
    screens: &mut Screens<Slot>,
    instance: Instance,
    edit: bool,
) -> Result<(), Error> {
    if let Some(Slot::Dashboard(dashboard)) = screens.get(instance) {
        dashboard.reopened(edit);
        return Ok(());
    }
    let mut dashboard = Dashboard::new();
    if edit {
        dashboard.start_editing();
    }
    screens.open(instance, Slot::Dashboard(dashboard))
}

/// Whether the params ask to edit: `mode` = `edit`.
fn edit_mode<'a>(
    params: impl Iterator<Item = Result<(&'a str, &'a str), Error>>,
) -> Result<bool, Error> {
    let mut edits = false;
    for param in params {
        let (key, value) = param?;
        if key == "mode" {
            edits = value == "edit";
        }
    }
    Ok(edits)
}

/// What the screen `status-items` edits for `id`: the footer's items, and the
/// status line widget's parts it is declared as the editor of (`STATUS_PARTS`),
/// also of one placement; its part `line` lists lines.
fn edited(id: &str) -> Option<(&'static str, bool)> {
    if id == FOOTER_ITEMS {
        return Some(("home.setting.footer-items", false));
    }
    let (placement, name) = id.strip_prefix(STATUS_WIDGET)?.rsplit_once('.')?;
    if !placement.is_empty() && !(placement.len() > 1 && placement.starts_with('@')) {
        return None;
    }
    STATUS_PARTS
        .iter()
        .find(|&&(part, _, _, _, editor)| editor && part == name)
        .map(|&(part, _, title, _, _)| (title, part == LINE_PART))
}

/// The setting a handover names (`id`): one this screen is declared as the
/// editor of. No `id`, another one or one repeated differently is `Invalid`.
fn editor_setting<'a>(
    params: impl Iterator<Item = Result<(&'a str, &'a str), Error>>,
) -> Result<Edited, Error> {
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
    let setting = setting.ok_or(Error::Invalid)?;
    let (title, lines) = edited(setting).ok_or(Error::Invalid)?;
    Ok(Edited {
        setting: copy(setting)?,
        title,
        lines,
    })
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
        let edited = editor_setting(pairs(&named)).unwrap();
        assert_eq!(edited.setting, FOOTER_ITEMS);
        assert_eq!(edited.title, "home.setting.footer-items");
        assert!(!edited.lines);
    }

    #[test]
    fn the_status_lines_line_lists_lines_and_its_parts_items() {
        let line = editor_setting(pairs(&[("id", "home.status.line")])).unwrap();
        assert_eq!(line.setting, "home.status.line");
        assert_eq!(line.title, "home.status.line");
        assert!(line.lines);
        for (id, title) in [
            ("home.status.left", "home.status.left"),
            ("home.status.center", "home.status.center"),
            ("home.status.right", "home.status.right"),
        ] {
            let part = editor_setting(pairs(&[("id", id)])).unwrap();
            assert_eq!((part.setting.as_str(), part.title), (id, title));
            assert!(!part.lines, "{id}");
        }
        // Of one placement, as the settings of a placed widget are named.
        let placed = [("id", "home.status@home.page-1/0.line")];
        let placed = editor_setting(pairs(&placed)).unwrap();
        assert_eq!(placed.setting, "home.status@home.page-1/0.line");
        assert!(placed.lines);
        let placed = editor_setting(pairs(&[("id", "home.status@2.right")])).unwrap();
        assert!(!placed.lines);
        for other in ["home.status@.line", "home.statusx.line", "home.status-bar"] {
            assert_eq!(
                editor_setting(pairs(&[("id", other)])),
                Err(Error::Invalid),
                "{other}"
            );
        }
        // The look of the line is not edited here.
        let look = [("id", "home.status.paper")];
        assert_eq!(editor_setting(pairs(&look)), Err(Error::Invalid));
        assert_eq!(
            editor_setting(pairs(&[("id", "home.status.")])),
            Err(Error::Invalid)
        );
    }

    #[test]
    fn the_dashboard_edits_when_asked_to() {
        let edit = [("mode", "edit")];
        assert!(edit_mode(pairs(&edit)).unwrap());
        let other = [("mode", "view"), ("page", "1")];
        assert!(!edit_mode(pairs(&other)).unwrap());
        assert!(!edit_mode(pairs(&[])).unwrap());
        let refused = [Err(Error::Busy)];
        assert_eq!(edit_mode(refused.into_iter()), Err(Error::Busy));
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
