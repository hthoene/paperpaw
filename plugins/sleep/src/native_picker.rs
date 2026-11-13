//! The picker of what goes over the page (`sleep.pick-overlay`, the editor of
//! the settings `sleep.overlay-picture` and `sleep.overlay-widget`): the
//! pictures folder first, then the presets of the kind `sleep.overlay` that
//! plugins offer (a preset sets the picture and its place and size), then the
//! widgets. What is chosen shows *On*; why the last sleep showed no picture, if
//! it did not, is said under the list (`FAILED`, written by the screen).
//!
//! The rows are made when the screen opens and the list is the only thing it
//! keeps; every text is read into a buffer of its own that is gone before the
//! next.
use crate::native_screen::read_text;
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host,
        callbacks::{DrawFrame, Input},
        commands::{CommandSender, PresetMode},
        formatting::Arg,
        registry::{Registry, ValuesQuery},
        settings::{self, SettingValue},
        storage::{self, Root},
    },
};
use paperpaw_ui_kit::{
    Footer, Item, ItemList, ItemStep, Messages,
    mem::{SERVICE_BUFFER, buffer, push},
    rows,
    text::copy,
};

/// The setting of the picture: a file, or empty for the pictures folder.
const PICTURE: &str = "sleep.overlay-picture";
/// The setting of the widget shown instead of a picture.
const WIDGET: &str = "sleep.overlay-widget";
/// The kind of the presets that offer pictures.
const KIND: &str = "sleep.overlay";
/// The file of the data folder that says why no picture showed.
pub const FAILED: &str = "overlay-failed.txt";
/// The most rows: the folder, the presets and the widgets (a device has dozens).
const MOST: usize = 128;
/// The room of the commands a choice sends (the longest identifier is 128 bytes).
const COMMANDS: usize = 1024;
/// The longest reason read from `FAILED`.
const REASON: usize = 512;
/// The longest value of a setting compared with a preset's.
const VALUE: usize = 1024;

/// What a row stands for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Source {
    /// The pictures folder, one after the other.
    Folder,
    /// A preset of the kind `sleep.overlay`, by its identifier.
    Preset(String),
    /// A widget, by its identifier.
    Widget(String),
}

/// Whether the row of `source` is what is chosen now. A widget chosen, only it
/// is; else the folder where no file is named, and a preset whose values the
/// settings have.
pub(crate) fn marked(source: &Source, widget: &str, picture: &str, applied: bool) -> bool {
    match source {
        Source::Widget(id) => widget == id,
        _ if !widget.is_empty() => false,
        Source::Folder => picture.is_empty(),
        Source::Preset(_) => applied,
    }
}

/// Whether a setting that is `now` has what a preset gives it: a value the
/// preset leaves out (a reset, one too long to read) counts as had.
pub(crate) fn same(preset: Option<SettingValue<'_>>, now: SettingValue<'_>) -> bool {
    preset.is_none_or(|value| value == now)
}

/// The screen: its rows and the note.
#[derive(Debug)]
pub struct Picker {
    sources: Vec<Source>,
    list: ItemList,
    /// Why the last sleep showed no picture, said under the list.
    note: Option<String>,
}

impl Picker {
    /// Reads what there is to choose from and what is chosen now.
    ///
    /// # Errors
    /// Allocation and the registry's refusals.
    pub fn open(host: &Host<'_>, registry: &Registry) -> Result<Self, Error> {
        let mut messages = Messages::new()?;
        let widget = read_text(host, WIDGET)?;
        let picture = read_text(host, PICTURE)?;
        let (widget, picture) = (widget.trim(), picture.trim());
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let mut sources = Vec::new();
        let mut items = Vec::new();
        let folder = messages.copy(host, "sleep.overlay.folder")?;
        push(&mut sources, Source::Folder)?;
        push(&mut items, Item::row(&folder, "")?)?;
        rows!(cursor => registry.presets(host, KIND, cursor, &mut input, &mut out), |row| {
            if sources.len() < MOST {
                push(&mut sources, Source::Preset(copy(row.id())?))?;
                push(&mut items, Item::row(row.title(), "")?)
            } else {
                Ok(())
            }
        })?;
        rows!(cursor => registry.widgets(host, "", cursor, &mut input, &mut out), |row| {
            if sources.len() < MOST {
                let title = messages.format(
                    host,
                    "sleep.overlay.widget",
                    &[Arg::Text {
                        name: "name",
                        value: row.title(),
                    }],
                )?;
                push(&mut sources, Source::Widget(copy(row.id())?))?;
                push(&mut items, Item::row(&title, "")?)
            } else {
                Ok(())
            }
        })?;
        let mut list = ItemList::new(items);
        let on = messages.copy(host, "core.state.on")?;
        for (index, source) in sources.iter().enumerate() {
            let applied = match source {
                Source::Preset(id) if widget.is_empty() => {
                    applied(host, registry, id, &mut input, &mut out)?
                }
                _ => false,
            };
            if marked(source, widget, picture, applied) {
                list.set_value(index, copy(&on)?);
            }
        }
        let note = note(host, &mut messages)?;
        Ok(Self {
            sources,
            list,
            note,
        })
    }

    /// Draws the list under its title, the note under it.
    ///
    /// # Errors
    /// Allocation, font and drawing errors.
    pub fn draw(&mut self, host: &Host<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        let title = Messages::new()?.copy(host, "sleep.setting.overlay-picture")?;
        let footer = match &self.note {
            Some(text) => Footer::Note(text),
            None => Footer::None,
        };
        self.list.draw_with(host, frame, &title, true, footer)
    }

    /// A choice sets the settings and closes the screen, in one callback: an
    /// error drops both.
    ///
    /// # Errors
    /// Allocation and the host's refusals.
    pub fn input(&mut self, host: &Host<'_>, input: &Input<'_>) -> Result<Handled, Error> {
        let step = self.list.input(host, input)?;
        let ItemStep::Chosen(index) = step else {
            return Ok(ItemList::handled(step));
        };
        let Some(source) = self.sources.get(index) else {
            return Ok(Handled::Yes);
        };
        let mut scratch = buffer(COMMANDS)?;
        let mut commands = CommandSender::new(host, &mut scratch);
        match source {
            Source::Folder => {
                commands.set_setting(WIDGET, SettingValue::Text(""))?;
                commands.set_setting(PICTURE, SettingValue::Text(""))?;
            }
            Source::Widget(id) => commands.set_setting(WIDGET, SettingValue::Text(id))?,
            Source::Preset(id) => {
                commands.set_setting(WIDGET, SettingValue::Text(""))?;
                commands.apply_preset(id, PresetMode::Apply)?;
            }
        }
        commands.close_screen()?;
        Ok(Handled::Yes)
    }
}

/// Whether the settings have every value the preset `id` gives them. A setting
/// this device lacks or cannot compare (a value too long) counts as had.
fn applied(
    host: &Host<'_>,
    registry: &Registry,
    id: &str,
    input: &mut [u8],
    out: &mut [u8],
) -> Result<bool, Error> {
    let mut scratch = buffer(VALUE)?;
    let mut all = true;
    rows!(cursor => registry.values(host, ValuesQuery::Preset(id), cursor, input, out), |row| {
        if all {
            all = match settings::read_setting(host, row.key(), &mut scratch) {
                Ok(now) => same(row.value(), now),
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                Err(Error::NoSpace) => false,
                Err(_) => true,
            };
        }
        Ok(())
    })?;
    Ok(all)
}

/// Why the last sleep showed no picture, as a sentence; none where it did.
fn note(host: &Host<'_>, messages: &mut Messages) -> Result<Option<String>, Error> {
    let mut bytes = [0; REASON];
    let Ok(read) = storage::read_at(host, Root::Data, FAILED, 0, &mut bytes) else {
        return Ok(None);
    };
    let Some(reason) = bytes
        .get(..read)
        .and_then(|bytes| core::str::from_utf8(bytes).ok())
        .filter(|reason| !reason.is_empty())
    else {
        return Ok(None);
    };
    messages
        .format(
            host,
            "sleep.overlay.failed",
            &[Arg::Text {
                name: "reason",
                value: reason,
            }],
        )
        .map(Some)
}

#[cfg(test)]
mod tests;
