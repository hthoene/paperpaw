//! The flat picture home screen plugin, first slice: what needs no dashboard.
//!
//! Here: the picture chooser (the screen `pick`: the pictures of the folder
//! on the card, read through `core.files`, and what a choice sets), the
//! editor of the bar's three item settings (the screen `status-items`, the
//! kit's status items view), and the declarations the image carries. The
//! dashboard and *Add* are declared and answer `Unsupported` when opened until
//! their slices (`PORTING-7.md`); nothing opens them while the interface-6
//! home screen of this plugin is the one the app has.
use crate::{
    declarations::{PICK, STATUS_ITEMS},
    native_pick,
    native_picture::Pictures,
};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, DrawPurpose, Input},
        commands::CommandSender,
        events::{Event, Incoming, kinds},
        files::Files,
        formatting::Arg,
        registrations,
        registry::StatusScope,
        reply::Reply,
        settings::{self, SettingValue},
        storage::Root,
        ui::{HandoverId, Instance, Ui},
    },
};
use paperpaw_ui_kit::read_event;
use paperpaw_ui_kit::{
    Footer, ItemList, ItemStep, Messages, Screens,
    mem::{SERVICE_BUFFER, buffer},
    status_items::{StatusItemsView, Texts, view::read_entries},
    text::copy,
};

mod image {
    include!(concat!(env!("OUT_DIR"), "/picture_home_declarations.rs"));
}

/// The setting of the folder the pictures are in.
const FOLDER: &str = "picture-home.folder";
/// The setting the chooser sets.
const PICTURE: &str = "picture-home.picture";
/// The screens, by the ids the host gives them.
const PICK_SCREEN: &str = "picture-home.pick";
const ITEMS_SCREEN: &str = "picture-home.status-items";
/// The screens whose slices come later.
const LATER_SCREENS: [&str; 2] = ["picture-home.dashboard", "picture-home.add"];
/// The settings of the bar's items, each with the key of its title: the only
/// ones the screen `status-items` is declared as the editor of.
const BAR_ITEMS: [(&str, &str); 3] = [
    (
        "picture-home.bar.left-items",
        "picture-home.setting.bar-left",
    ),
    (
        "picture-home.bar.center-items",
        "picture-home.setting.bar-center",
    ),
    (
        "picture-home.bar.right-items",
        "picture-home.setting.bar-right",
    ),
];
/// Space for the commands a choice stages.
const COMMANDS: usize = 1024;
/// Pages of a folder read at most (32 entries each at most): a folder that
/// long is cut, not slowed down. Interface 6 read all of it.
pub const MOST_PAGES: usize = 1024;

/// One bar items editor: the setting it changes and its view.
#[derive(Debug)]
struct Editor {
    setting: String,
    view: StatusItemsView,
}

/// The chooser: the pictures and the list of rows.
#[derive(Debug)]
struct Chooser {
    /// The pictures of the folder, sorted; row 0 of the list is *No picture*.
    names: Vec<String>,
    folder: String,
    list: ItemList,
}

/// The interface-7 picture home screen plugin.
#[derive(Debug, Default)]
pub struct NativePictureHome {
    ui: Option<Ui>,
    editors: Screens<Editor>,
    choosers: Screens<Chooser>,
}

impl Plugin for NativePictureHome {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS
            .iter()
            .chain(image::SCREENS)
            .chain(image::NAVIGATION)
        {
            registrations::submit(host, batch)?;
        }
        // `core.ui` is opened by the first callback: opening needs a live frame.
        Ok(Self::default())
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        _token: u32,
        incoming: Incoming<'_>,
        _: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let Some(event) =
            read_event!(incoming; kinds::ScreenOpened, kinds::ScreenShown, kinds::ScreenClosed)
        else {
            return Ok(());
        };
        match event {
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
            } => self.editors.shown(STATUS_ITEMS, instance),
            Event::ScreenShown {
                screen: PICK_SCREEN,
                instance: Some(instance),
                ..
            } => self.choosers.shown(PICK, instance),
            Event::ScreenClosed { instance, .. } => {
                self.editors.close(instance);
                self.choosers.close(instance);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        if !matches!(frame.purpose, DrawPurpose::Screen) {
            return Ok(());
        }
        match token {
            STATUS_ITEMS => {
                let editor = self.editors.top(token).ok_or(Error::State)?;
                editor.view.draw(host, &mut frame)
            }
            PICK => {
                let chooser = self.choosers.top(token).ok_or(Error::State)?;
                chooser.draw(host, &mut frame)
            }
            _ => Err(Error::State),
        }
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        match token {
            STATUS_ITEMS => match self.editors.top(token) {
                Some(editor) => editor.view.input(host, &editor.setting, &input),
                None => Ok(Handled::No),
            },
            PICK => match self.choosers.top(token) {
                Some(chooser) => chooser.input(host, &input),
                None => Ok(Handled::No),
            },
            _ => Ok(Handled::No),
        }
    }
}

impl NativePictureHome {
    /// Opens `core.ui` in the first live callback; the handle serves later ones.
    fn ensure_ui(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.ui.is_none() {
            self.ui = Some(Ui::open(host)?);
        }
        Ok(())
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
        match screen {
            ITEMS_SCREEN => {
                self.ensure_ui(host)?;
                let editor = self.items_editor(host, handover)?;
                self.editors.open(instance, editor)
            }
            PICK_SCREEN => {
                let chooser = Chooser::read(host)?;
                self.choosers.open(instance, chooser)
            }
            _ => Err(Error::Invalid),
        }
    }

    /// The editor for the setting the handover names (`id`): the items there
    /// are, and what the setting holds now.
    fn items_editor(&self, host: &Host<'_>, handover: Option<HandoverId>) -> Result<Editor, Error> {
        let ui = self.ui.as_ref().ok_or(Error::State)?;
        let mut setting = None;
        if let Some(handover) = handover {
            let mut input = buffer(SERVICE_BUFFER)?;
            let mut output = buffer(SERVICE_BUFFER)?;
            for param in ui.params(host, handover, &mut input, &mut output)? {
                let param = param?;
                if param.key() == "id" {
                    setting = Some(copy(param.value())?);
                }
            }
        }
        // Only the three settings this screen is declared as the editor of.
        let setting = setting.ok_or(Error::Invalid)?;
        let (_, title) = BAR_ITEMS
            .iter()
            .find(|(id, _)| *id == setting)
            .ok_or(Error::Invalid)?;
        let mut scratch = buffer(SERVICE_BUFFER)?;
        let held = match settings::read_setting(host, &setting, &mut scratch)? {
            SettingValue::Text(text) => copy(text)?,
            _ => String::new(),
        };
        let entries = read_entries(host, StatusScope::Items)?;
        let texts = Texts::read(host)?;
        let title = Messages::new()?.copy(host, title)?;
        let view = StatusItemsView::new(entries, &held, title, texts)?;
        Ok(Editor { setting, view })
    }
}

impl Chooser {
    /// Reads the folder and the picture from the settings and the pictures
    /// from the card; the focus starts on the chosen one.
    fn read(host: &Host<'_>) -> Result<Self, Error> {
        let mut scratch = buffer(SERVICE_BUFFER)?;
        let folder = match settings::read_setting(host, FOLDER, &mut scratch)? {
            SettingValue::Text(text) => copy(native_pick::folder(text))?,
            _ => String::new(),
        };
        let chosen = match settings::read_setting(host, PICTURE, &mut scratch)? {
            SettingValue::Text(text) => copy(text)?,
            _ => String::new(),
        };
        let names = pictures(host, &folder)?;
        let none = Messages::new()?.copy(host, "picture-home.no-picture")?;
        let mut list = ItemList::new(native_pick::rows(&none, &names, &chosen)?);
        list.set_focus(native_pick::start(&names, &chosen));
        Ok(Self {
            names,
            folder,
            list,
        })
    }

    fn draw(&mut self, host: &Host<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        let mut messages = Messages::new()?;
        let title = messages.copy(host, "picture-home.setting.picture")?;
        if !self.names.is_empty() {
            return self.list.draw_with(host, frame, &title, true, Footer::None);
        }
        let empty = messages.copy(host, "picture-home.no-pictures.title")?;
        let note = messages.format(
            host,
            "picture-home.no-pictures",
            &[Arg::Text {
                name: "folder",
                value: &self.folder,
            }],
        )?;
        self.list.draw_with(
            host,
            frame,
            &title,
            true,
            Footer::Empty {
                title: &empty,
                text: &note,
            },
        )
    }

    fn input(&mut self, host: &Host<'_>, input: &Input<'_>) -> Result<Handled, Error> {
        let step = self.list.input(host, input)?;
        if let ItemStep::Chosen(index) = step {
            // The name is set and the screen closes in one callback: an error
            // drops both.
            if let Some(name) = native_pick::choice(&self.names, index)? {
                let mut scratch = buffer(COMMANDS)?;
                let mut commands = CommandSender::new(host, &mut scratch);
                commands.set_setting(PICTURE, SettingValue::Text(&name))?;
                commands.close_screen()?;
            }
            return Ok(Handled::Yes);
        }
        Ok(ItemList::handled(step))
    }
}

/// The pictures of `folder` on the card, sorted, as many as are kept
/// ([`native_picture::pictures`]). A folder that is not there, is a file or
/// cannot be read by this plugin has none, as on interface 6.
fn pictures(host: &Host<'_>, folder: &str) -> Result<Vec<String>, Error> {
    let files = Files::open(host)?;
    let found = read_folder(host, &files, folder);
    // The service is released whatever the listing did.
    let released = files.release(host);
    let found = found?;
    released?;
    Ok(found)
}

fn read_folder(host: &Host<'_>, files: &Files, folder: &str) -> Result<Vec<String>, Error> {
    let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
    let mut kept = Pictures::default();
    let mut cursor = 0;
    for _ in 0..MOST_PAGES {
        let page = match files.list(host, Root::Files, folder, cursor, &mut input, &mut out) {
            Ok(page) => page,
            Err(
                Error::NotFound
                | Error::Invalid
                | Error::Denied
                | Error::Unsupported
                | Error::NoSpace,
            ) => break,
            Err(error) => return Err(error),
        };
        for entry in page.entries() {
            kept.add(entry.name(), entry.is_folder())?;
        }
        match page.next() {
            Some(next) => cursor = next,
            None => break,
        }
    }
    Ok(kept.finish())
}
