//! The picker of what goes over the page on interface 7 (`sleep.pick-overlay`,
//! the editor of `sleep.overlay-picture` and `sleep.overlay-widget`): the rows
//! of [`crate::native_pick`], read when the screen opens from the settings and
//! `core.registry` (the presets of kind `sleep.overlay` and the widgets, one
//! page at a time), shown with the kit's `ItemList`, why the last sleep showed
//! no picture noted below. Choosing sets the settings and closes; Back closes
//! (the host's, the list leaves it). Nothing but the rows is held while it is
//! open, and it all goes when it closes.
//!
//! Here too: [`remember`], which keeps why the picture over the page did not
//! show for the picker (the sleep screen calls it).
use crate::{
    native_pick::{self as model, Failure, Now, Rows},
    native_screen::read_text,
};
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host,
        callbacks::{DrawFrame, Input},
        commands::{CommandSender, PresetMode},
        formatting::Arg,
        registry::{PageCursor, Registry, ValuesQuery},
        settings::{self, SettingValue},
        storage::{self, Root},
    },
};
use paperpaw_ui_kit::{
    Footer, ItemList, ItemStep, Messages,
    mem::{SERVICE_BUFFER, buffer},
    text::copy,
};

/// The picker's title (interface 6's).
const TITLE: &str = "sleep.setting.overlay-picture";
/// Pages of presets, of a preset's values and of widgets read at most.
const MOST_PAGES: usize = 8;
/// Room for the longest text setting and its frame (`settings.read`'s most).
const VALUE_BUFFER: usize = 4096;
/// Room for the commands of a choice: two settings and a preset.
const COMMANDS: usize = 1024;

/// One open picker.
#[derive(Debug)]
pub struct Picker {
    rows: Rows,
    list: ItemList,
    /// Why the last sleep showed no picture, said below the rows.
    note: Option<alloc::string::String>,
}

impl Picker {
    /// Reads the settings, the presets and the widgets. A registry that
    /// refuses a list leaves its rows out: the pictures folder is always
    /// there.
    ///
    /// # Errors
    /// [`Error::NoMemory`] and the host's refusals of the settings.
    #[inline(never)]
    pub fn read(host: &Host<'_>, registry: Option<&Registry>) -> Result<Self, Error> {
        let picture = read_text(host, model::PICTURE)?;
        let widget = read_text(host, model::WIDGET)?;
        let now = Now {
            picture: &picture,
            widget: &widget,
        };
        let mut messages = Messages::new()?;
        let folder = messages.copy(host, "sleep.overlay.folder")?;
        // The host's *On*; a catalog without it, a dot.
        let on = match messages.copy(host, "core.state.on") {
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => copy("•")?,
            Ok(on) => on,
        };
        let mut rows = Rows::new(&folder, now, &on)?;
        if let Some(registry) = registry {
            only_memory(presets(host, registry, &mut rows))?;
            only_memory(widgets(host, registry, &mut messages, &mut rows))?;
        }
        // A note the host cannot say is left out.
        let note = match note(host, &mut messages) {
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => None,
            Ok(note) => note,
        };
        let list = ItemList::new(rows.take_items());
        Ok(Self { rows, list, note })
    }

    /// Draws the rows under the title, the note below them.
    ///
    /// # Errors
    /// Allocation, font, catalog and drawing errors.
    pub fn draw(&mut self, host: &Host<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        let footer = self.note.as_deref().map_or(Footer::None, Footer::Note);
        self.list.draw_with(host, frame, TITLE, true, footer)
    }

    /// Moves the focus, or sets what a row chooses and closes. Back is the
    /// host's: it closes.
    ///
    /// # Errors
    /// Allocation and the host's refusals of the commands.
    #[inline(never)]
    pub fn input(&mut self, host: &Host<'_>, input: &Input<'_>) -> Result<Handled, Error> {
        let step = self.list.input(host, input)?;
        let ItemStep::Chosen(index) = step else {
            return Ok(ItemList::handled(step));
        };
        let Some(choice) = self.rows.choice(index) else {
            return Ok(Handled::Yes);
        };
        // The settings are set and the screen closes in one callback.
        let mut scratch = buffer(COMMANDS)?;
        let mut commands = CommandSender::new(host, &mut scratch);
        commands.set_setting(model::WIDGET, SettingValue::Text(choice.widget))?;
        if let Some(picture) = choice.picture {
            commands.set_setting(model::PICTURE, SettingValue::Text(picture))?;
        }
        if let Some(preset) = choice.preset {
            // A preset gone since the picker opened is left, as interface 6
            // left one it no longer found.
            match commands.apply_preset(preset, PresetMode::Apply) {
                Ok(()) | Err(Error::NotFound) => {}
                Err(error) => return Err(error),
            }
        }
        commands.close_screen()?;
        Ok(Handled::Yes)
    }
}

/// A refusal of the registry but a memory shortage leaves its rows out.
fn only_memory(result: Result<(), Error>) -> Result<(), Error> {
    match result {
        Err(Error::NoMemory) => Err(Error::NoMemory),
        Ok(()) | Err(_) => Ok(()),
    }
}

/// The presets of kind `sleep.overlay`, in the host's order, each with
/// whether the settings hold its values now.
fn presets(host: &Host<'_>, registry: &Registry, rows: &mut Rows) -> Result<(), Error> {
    let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
    let mut found = alloc::vec::Vec::new();
    let mut cursor = PageCursor::start();
    for _ in 0..MOST_PAGES {
        let page = registry.presets(host, model::KIND, cursor, &mut input, &mut out)?;
        for row in page.rows() {
            paperpaw_ui_kit::mem::push(&mut found, (copy(row.id())?, copy(row.title())?))?;
        }
        match page.end().next_cursor() {
            Some(next) => cursor = next,
            None => break,
        }
    }
    drop((input, out));
    for (id, title) in &found {
        // A preset whose values cannot be read is not marked.
        let applied = match applied(host, registry, id) {
            Ok(applied) => applied,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => false,
        };
        rows.preset(id, title, applied)?;
    }
    Ok(())
}

/// Whether the settings hold every value preset `id` names (interface 6's
/// `presets::applied` of a kind that covers only what it names). A value
/// the registry does not give (withheld, too long, a reset) is not compared.
#[inline(never)]
fn applied(host: &Host<'_>, registry: &Registry, id: &str) -> Result<bool, Error> {
    let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
    let mut now = buffer(VALUE_BUFFER)?;
    let mut cursor = PageCursor::start();
    for _ in 0..MOST_PAGES {
        let page = registry.values(host, ValuesQuery::Preset(id), cursor, &mut input, &mut out)?;
        for row in page.rows() {
            let Some(value) = row.value() else {
                continue;
            };
            // A setting this plugin may not read is not the preset's.
            match settings::read_setting(host, row.key(), &mut now) {
                Ok(held) if held == value => {}
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                Ok(_) | Err(_) => return Ok(false),
            }
        }
        match page.end().next_cursor() {
            Some(next) => cursor = next,
            None => break,
        }
    }
    Ok(true)
}

/// Every widget, *Widget: name*, in the host's order.
fn widgets(
    host: &Host<'_>,
    registry: &Registry,
    messages: &mut Messages,
    rows: &mut Rows,
) -> Result<(), Error> {
    let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
    let mut cursor = PageCursor::start();
    for _ in 0..MOST_PAGES {
        let page = registry.widgets(host, "", cursor, &mut input, &mut out)?;
        for row in page.rows() {
            let name = Arg::Text {
                name: "name",
                value: row.title(),
            };
            let title = messages.format(host, "sleep.overlay.widget", &[name])?;
            rows.widget(row.id(), &title)?;
        }
        match page.end().next_cursor() {
            Some(next) => cursor = next,
            None => break,
        }
    }
    Ok(())
}

/// The note of why the last sleep showed no picture (*Not shown at the last
/// sleep: …*); none where it showed or nothing is known.
fn note(host: &Host<'_>, messages: &mut Messages) -> Result<Option<alloc::string::String>, Error> {
    let mut bytes = buffer(model::FAILED_MAX)?;
    let read = storage::read_at(host, Root::Data, model::FAILED_FILE, 0, &mut bytes).ok();
    let Some(why) = model::failed(read.and_then(|n| bytes.get(..n))) else {
        return Ok(None);
    };
    let reason = Arg::Text {
        name: "reason",
        value: why,
    };
    messages
        .format(host, "sleep.overlay.failed", &[reason])
        .map(Some)
}

/// Keeps why the picture over the page did not show (`Some`), or that it
/// showed (`None`: the file goes), in the data folder for the picker;
/// written only when it changed. A file that cannot be written, or a message
/// the host cannot format, is not told: the sleep screen shows what it can
/// whatever the picker will say.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn remember(host: &Host<'_>, why: Option<&Failure>) -> Result<(), Error> {
    only_memory(try_remember(host, why))
}

#[inline(never)]
fn try_remember(host: &Host<'_>, why: Option<&Failure>) -> Result<(), Error> {
    let mut before = buffer(model::FAILED_MAX)?;
    let read = storage::read_at(host, Root::Data, model::FAILED_FILE, 0, &mut before).ok();
    let Some(why) = why else {
        if read.is_some() {
            storage::remove(host, Root::Data, model::FAILED_FILE).ok();
        }
        return Ok(());
    };
    let (name, reason) = why.args();
    let mut args = [
        Arg::Text {
            name: "name",
            value: "",
        },
        Arg::Text {
            name: "reason",
            value: "",
        },
    ];
    let mut count = 0;
    for (key, value) in [("name", name), ("reason", reason)] {
        if let (Some(value), Some(arg)) = (value, args.get_mut(count)) {
            *arg = Arg::Text { name: key, value };
            count += 1;
        }
    }
    let text = Messages::new()?.format(host, why.key(), args.get(..count).unwrap_or(&[]))?;
    let text = model::kept(&text);
    if read.and_then(|n| before.get(..n)) == Some(text.as_bytes()) {
        return Ok(());
    }
    storage::write(host, Root::Data, model::FAILED_FILE, text.as_bytes()).ok();
    Ok(())
}
