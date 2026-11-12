//! The picker of what goes over the page (`sleep.pick-overlay`), without the
//! host: its rows, which one is chosen now, what a choice sets, and why the
//! picture over the page did not show at the last sleep (kept for the picker).
//! The same rules as the interface-6 `overlay::Picker`: the pictures folder
//! first, then the presets of kind `sleep.overlay` in the host's order, then every
//! widget (*Widget: name*); the chosen row says *On*; choosing sets the
//! settings and closes; why the last sleep showed no picture is noted below.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::{Error, image::ImageStatus};
use paperpaw_ui_kit::{items::Item, mem::push, text::copy};

/// The picture over the page: a file, or empty for the pictures folder.
pub const PICTURE: &str = "sleep.overlay-picture";
/// A widget shown in place of the picture, if one is set.
pub const WIDGET: &str = "sleep.overlay-widget";
/// The kind of the presets that offer pictures.
pub const KIND: &str = "sleep.overlay";
/// The file of the data folder that says why the picture did not show at the
/// last sleep (interface 6's `overlay-failed.txt`).
pub const FAILED_FILE: &str = "overlay-failed.txt";
/// The most of it that is kept and read, as interface 6 read it.
pub const FAILED_MAX: usize = 1024;

/// A row of the picker.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Source {
    /// The pictures folder, one after the other (both settings empty).
    Folder,
    /// A preset of kind [`KIND`], and whether the settings hold its values now.
    Preset { id: String, applied: bool },
    /// A widget, by its id.
    Widget(String),
}

/// What the two settings hold now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Now<'a> {
    /// `sleep.overlay-picture`.
    pub picture: &'a str,
    /// `sleep.overlay-widget`.
    pub widget: &'a str,
}

/// What a choice sets, in this order: the widget, then the picture (the
/// folder's row) or the preset applied (a preset's row).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Choice<'a> {
    /// The value of [`WIDGET`].
    pub widget: &'a str,
    /// The value of [`PICTURE`], where it is set.
    pub picture: Option<&'a str>,
    /// The preset applied, if any.
    pub preset: Option<&'a str>,
}

/// The picker's rows: what each is and what the list shows.
#[derive(Debug, Default)]
pub struct Rows {
    sources: Vec<Source>,
    items: Vec<Item>,
    /// The widget chosen now (trimmed).
    widget: String,
    /// The pictures folder is what is chosen now.
    folder_chosen: bool,
    /// What the chosen row says at its right (*On*).
    mark: String,
}

impl Rows {
    /// The first row, the pictures folder (`folder`, its title), for the
    /// settings as they are `now`; `mark` is what the chosen row says.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn new(folder: &str, now: Now<'_>, mark: &str) -> Result<Self, Error> {
        let widget = now.widget.trim();
        let mut rows = Self {
            sources: Vec::new(),
            items: Vec::new(),
            widget: copy(widget)?,
            folder_chosen: widget.is_empty() && now.picture.trim().is_empty(),
            mark: copy(mark)?,
        };
        rows.add(Source::Folder, folder)?;
        Ok(rows)
    }

    /// A preset of kind [`KIND`]: its id, its name and whether the settings
    /// hold all it names now.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn preset(&mut self, id: &str, title: &str, applied: bool) -> Result<(), Error> {
        let id = copy(id)?;
        self.add(Source::Preset { id, applied }, title)
    }

    /// A widget: its id and its row's title (*Widget: name*).
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn widget(&mut self, id: &str, title: &str) -> Result<(), Error> {
        let id = copy(id)?;
        self.add(Source::Widget(id), title)
    }

    fn add(&mut self, source: Source, title: &str) -> Result<(), Error> {
        let value = if self.chosen(&source) {
            self.mark.as_str()
        } else {
            ""
        };
        let item = Item::row(title, value)?;
        self.sources.try_reserve(1).map_err(|_| Error::NoMemory)?;
        push(&mut self.items, item)?;
        self.sources.push(source);
        Ok(())
    }

    /// Whether `source` is what is chosen now: a widget set is chosen whatever
    /// the picture; else the folder where no picture is set, a preset whose
    /// values the settings hold.
    fn chosen(&self, source: &Source) -> bool {
        match source {
            Source::Widget(id) => *id == self.widget,
            _ if !self.widget.is_empty() => false,
            Source::Folder => self.folder_chosen,
            Source::Preset { applied, .. } => *applied,
        }
    }

    /// The rows, in order.
    #[must_use]
    pub fn sources(&self) -> &[Source] {
        &self.sources
    }

    /// The list's items, one per row; taken for the list, the rows keep only
    /// what they are.
    #[must_use]
    pub fn take_items(&mut self) -> Vec<Item> {
        core::mem::take(&mut self.items)
    }

    /// What choosing row `index` sets; `None` for a row there is not.
    #[must_use]
    pub fn choice(&self, index: usize) -> Option<Choice<'_>> {
        Some(match self.sources.get(index)? {
            Source::Folder => Choice {
                widget: "",
                picture: Some(""),
                preset: None,
            },
            Source::Preset { id, .. } => Choice {
                widget: "",
                picture: None,
                preset: Some(id),
            },
            Source::Widget(id) => Choice {
                widget: id,
                picture: None,
                preset: None,
            },
        })
    }
}

/// Why the picture over the page did not show, as interface 6 said it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Failure {
    /// The widget chosen is not there.
    NoWidget(String),
    /// The pictures folder has no picture (or cannot be read).
    EmptyFolder,
    /// The picture's file is not there.
    Missing(String),
    /// The picture cannot be shown, and why (in English, as interface 6 said).
    Unreadable(String, &'static str),
}

impl Failure {
    /// Why a picture the host was asked to decode, `path`, did not come; none
    /// where it came.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn of(path: &str, status: ImageStatus) -> Result<Option<Self>, Error> {
        let reason = match status {
            ImageStatus::Ready => return Ok(None),
            ImageStatus::NotFound => return Ok(Some(Self::Missing(copy(path)?))),
            ImageStatus::Unsupported => "unsupported image: format",
            ImageStatus::TooLarge => "image too large for this decoder",
            ImageStatus::CardFailure => "image could not be read or written",
            ImageStatus::NoMemory => "not enough memory for the image",
            _ => "invalid image: damaged",
        };
        Ok(Some(Self::Unreadable(copy(path)?, reason)))
    }

    /// The catalog key of the message.
    #[must_use]
    pub fn key(&self) -> &'static str {
        match self {
            Self::NoWidget(_) => "sleep.overlay.no-widget",
            Self::EmptyFolder => "sleep.overlay.empty-folder",
            Self::Missing(_) => "sleep.overlay.missing",
            Self::Unreadable(..) => "sleep.overlay.unreadable",
        }
    }

    /// The message's `{name}` and `{reason}`, where it has them.
    #[must_use]
    pub fn args(&self) -> (Option<&str>, Option<&str>) {
        match self {
            Self::NoWidget(name) | Self::Missing(name) => (Some(name), None),
            Self::EmptyFolder => (None, None),
            Self::Unreadable(name, reason) => (Some(name), Some(reason)),
        }
    }
}

/// The part of `text` that is kept in [`FAILED_FILE`]: at most
/// [`FAILED_MAX`] bytes, cut where a character ends.
#[must_use]
pub fn kept(text: &str) -> &str {
    let mut end = text.len().min(FAILED_MAX);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(..end).unwrap_or("")
}

/// Why the last sleep showed no picture, from what [`FAILED_FILE`] holds;
/// none for no file, an empty one or one that is not text.
#[must_use]
pub fn failed(bytes: Option<&[u8]>) -> Option<&str> {
    core::str::from_utf8(bytes?)
        .ok()
        .map(str::trim)
        .filter(|why| !why.is_empty())
}
