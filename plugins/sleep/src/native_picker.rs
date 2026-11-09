//! The screen `pick-overlay`: the picture or the widget over the page, chosen
//! from a list (the pictures folder, its pictures, the presets that offer a
//! picture, the widgets). A choice sets the settings and closes the screen in
//! one callback. The rows are [`crate::native_pick`]'s; this reads what they
//! need from the host (the settings, the folder through `core.files`, the
//! registry's widgets and presets) and draws them.
use crate::{
    native_folder::list_folder,
    native_pick::{self as pick, Choice, Layout, MOST_PRESETS, MOST_WIDGETS, Preset, Widget},
    native_screen::read_text,
};
use alloc::{string::String, vec::Vec};
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
    mem::{SERVICE_BUFFER, buffer, push},
    text::copy,
};

/// The settings a choice sets.
const WIDGET: &str = "sleep.overlay-widget";
const PICTURE: &str = "sleep.overlay-picture";
/// The kind of the presets that offer pictures.
const PRESET_KIND: &str = "sleep.overlay";
/// Why the picture did not show at the last sleep (`native_screen::images`).
pub const FAILED_FILE: &str = "overlay-failed.txt";
/// The longest reason kept.
pub const MOST_REASON: usize = 1024;
/// Pages of the registry read at most for one list.
const MOST_PAGES: usize = 16;
/// Pages of the values of one preset read at most.
const MOST_VALUE_PAGES: usize = 2;
/// Room for the commands a choice stages: a path of [`pick::MOST_PATH`] bytes
/// and its frame.
const COMMANDS: usize = pick::MOST_PATH + 256;

/// The picker: its rows and what a row stands for.
#[derive(Debug)]
pub struct Picker {
    folder: String,
    layout: Layout,
    list: ItemList,
    /// Why nothing over the page shows, where the reader can mend it.
    note: Option<String>,
}

impl Picker {
    /// Reads what the rows need and builds them; the focus starts on the
    /// chosen one.
    ///
    /// # Errors
    /// [`Error::NoMemory`], and the host's refusal of what the screen cannot
    /// do without (the settings).
    pub fn read(host: &Host<'_>, registry: Option<&Registry>) -> Result<Self, Error> {
        let mut messages = Messages::new()?;
        let folder = copy(pick::folder(&read_text(host, "sleep.folder")?))?;
        let widget = read_text(host, WIDGET)?;
        let picture = read_text(host, PICTURE)?;
        let pictures = list_folder(host, &folder)?;
        let (mut widgets, mut presets) = (Vec::new(), Vec::new());
        if let Some(registry) = registry {
            widgets = read_widgets(host, registry, &mut messages)?;
            presets = read_presets(host, registry)?;
        }
        let folder_title = messages.copy(host, "sleep.overlay.folder")?;
        let built = pick::build(
            &folder_title,
            &folder,
            pictures.names(),
            &presets,
            &widgets,
            &widget,
            &picture,
        )?;
        let note = note(host, &mut messages, &widget, &widgets)?;
        let mut list = ItemList::new(built.items);
        list.set_focus(built.focus);
        Ok(Self {
            folder,
            layout: built.layout,
            list,
            note,
        })
    }

    /// Draws the list under the title of the setting.
    ///
    /// # Errors
    /// The host's and the allocator's.
    pub fn draw(&mut self, host: &Host<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        let title = Messages::new()?.copy(host, "sleep.setting.overlay-picture")?;
        let footer = self.note.as_deref().map_or(Footer::None, Footer::Note);
        self.list.draw_with(host, frame, &title, true, footer)
    }

    /// Moves the focus; a choice sets the settings and closes the screen.
    ///
    /// # Errors
    /// The host's and the allocator's; an error drops the whole choice.
    pub fn input(&mut self, host: &Host<'_>, input: &Input<'_>) -> Result<Handled, Error> {
        let step = self.list.input(host, input)?;
        if let ItemStep::Chosen(index) = step {
            let choice = self.layout.choice(self.list.items(), &self.folder, index)?;
            if let Some(choice) = choice {
                commit(host, &choice)?;
            }
            return Ok(Handled::Yes);
        }
        Ok(ItemList::handled(step))
    }
}

/// The settings set and the screen closed, in one callback.
fn commit(host: &Host<'_>, choice: &Choice) -> Result<(), Error> {
    let mut scratch = buffer(COMMANDS)?;
    let mut commands = CommandSender::new(host, &mut scratch);
    let sets = choice.sets();
    if let Some(widget) = sets.widget {
        commands.set_setting(WIDGET, SettingValue::Text(widget))?;
    }
    if let Some(picture) = sets.picture {
        commands.set_setting(PICTURE, SettingValue::Text(picture))?;
    }
    if let Some(preset) = sets.preset {
        commands.apply_preset(preset, PresetMode::Apply)?;
    }
    commands.close_screen()
}

/// The widgets the registry lists, as rows ("Widget: {name}"), at most
/// [`MOST_WIDGETS`]. What the registry cannot tell leaves none.
fn read_widgets(
    host: &Host<'_>,
    registry: &Registry,
    messages: &mut Messages,
) -> Result<Vec<Widget>, Error> {
    let (mut input, mut out) = (buffer(512)?, buffer(SERVICE_BUFFER)?);
    let mut found = Vec::new();
    let mut cursor = PageCursor::start();
    for _ in 0..MOST_PAGES {
        let page = match registry.widgets(host, "", cursor, &mut input, &mut out) {
            Ok(page) => page,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => break,
        };
        for row in page.rows() {
            if found.len() >= MOST_WIDGETS {
                break;
            }
            let title = messages.format(
                host,
                "sleep.overlay.widget",
                &[Arg::Text {
                    name: "name",
                    value: row.title(),
                }],
            )?;
            push(
                &mut found,
                Widget {
                    id: copy(row.id())?,
                    title,
                },
            )?;
        }
        match page.end().next_cursor() {
            Some(next) if found.len() < MOST_WIDGETS => cursor = next,
            _ => break,
        }
    }
    Ok(found)
}

/// The presets of kind `sleep.overlay`, and whether each is applied now.
fn read_presets(host: &Host<'_>, registry: &Registry) -> Result<Vec<Preset>, Error> {
    let (mut input, mut out) = (buffer(512)?, buffer(SERVICE_BUFFER)?);
    let mut found = Vec::new();
    let mut cursor = PageCursor::start();
    for _ in 0..MOST_PAGES {
        let page = match registry.presets(host, PRESET_KIND, cursor, &mut input, &mut out) {
            Ok(page) => page,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => break,
        };
        for row in page.rows() {
            if found.len() >= MOST_PRESETS {
                break;
            }
            push(
                &mut found,
                Preset {
                    id: copy(row.id())?,
                    title: copy(row.title())?,
                    applied: false,
                },
            )?;
        }
        match page.end().next_cursor() {
            Some(next) if found.len() < MOST_PRESETS => cursor = next,
            _ => break,
        }
    }
    for preset in &mut found {
        preset.applied = applied(host, registry, &preset.id, &mut input, &mut out)?;
    }
    Ok(found)
}

/// Whether every value of preset `id` is what its setting holds now (a
/// preset without a value is not applied).
fn applied(
    host: &Host<'_>,
    registry: &Registry,
    id: &str,
    input: &mut [u8],
    out: &mut [u8],
) -> Result<bool, Error> {
    let mut scratch = buffer(SERVICE_BUFFER + 64)?;
    let mut any = false;
    let mut cursor = PageCursor::start();
    for _ in 0..MOST_VALUE_PAGES {
        let page = match registry.values(host, ValuesQuery::Preset(id), cursor, input, out) {
            Ok(page) => page,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => return Ok(false),
        };
        for row in page.rows() {
            // A value withheld, too long or a reset cannot be compared.
            let Some(want) = row.value() else { continue };
            match settings::read_setting(host, row.key(), &mut scratch) {
                Ok(have) if have == want => any = true,
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                _ => return Ok(false),
            }
        }
        match page.end().next_cursor() {
            Some(next) => cursor = next,
            None => break,
        }
    }
    Ok(any)
}

/// What the footer says: that the widget chosen is not there any more, else
/// why the picture did not show at the last sleep.
fn note(
    host: &Host<'_>,
    messages: &mut Messages,
    widget: &str,
    widgets: &[Widget],
) -> Result<Option<String>, Error> {
    let widget = widget.trim();
    let reason = if widget.is_empty() {
        let mut kept = buffer(MOST_REASON)?;
        let read = storage::read_at(host, Root::Data, FAILED_FILE, 0, &mut kept).unwrap_or(0);
        let text = kept
            .get(..read)
            .and_then(|bytes| core::str::from_utf8(bytes).ok())
            .unwrap_or("");
        if text.is_empty() {
            return Ok(None);
        }
        copy(text)?
    } else if widgets.iter().any(|entry| entry.id == widget) || widgets.len() >= MOST_WIDGETS {
        return Ok(None);
    } else {
        messages.format(
            host,
            "sleep.overlay.no-widget",
            &[Arg::Text {
                name: "name",
                value: widget,
            }],
        )?
    };
    messages
        .format(
            host,
            "sleep.overlay.failed",
            &[Arg::Text {
                name: "reason",
                value: &reason,
            }],
        )
        .map(Some)
}
