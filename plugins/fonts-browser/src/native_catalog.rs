//! The catalog of fonts without the host: what the library's JSON file says
//! of each font, read as it streams, and the files of one font to download.
//! The same rules as the interface-6 reader (`catalog.rs`), with fallible
//! allocation: a refused allocation ends the reading in
//! [`Error::NoMemory`] and nothing is returned in part.
//!
//! The JSON text itself is read by `paperpaw_json::stream::Parser` with
//! `try_feed`/`try_finish`, whose allocations are bounded (64 levels,
//! 4,096 bytes a string) and fallible: a refusal ends the read in
//! [`Error::NoMemory`]. [`Catalog::event`] takes what it reports.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::json::stream::{Event, Parser};
use paperpaw_sdk::{Storage, StorageError, Version, native::Error};
use paperpaw_ui_kit::{
    mem::{push, stable_sort_by},
    text::{cmp_lowercase, concat, copy},
};

mod url;
pub use url::join_url;

/// The catalog's formats this plugin reads.
pub const FORMAT: u32 = 1;
/// Fonts read at most, and files of one font.
pub const MAX_FONTS: usize = 200;
pub const MAX_FILES: usize = 16;

/// One file of a font's plugin.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FontFile {
    /// Where it goes in the plugin's folder, e.g. `fonts/regular.ppfp`.
    pub path: String,
    pub url: String,
    pub size: u64,
    pub sha1: String,
}

impl FontFile {
    fn try_clone(&self) -> Result<Self, Error> {
        Ok(Self {
            path: copy(&self.path)?,
            url: copy(&self.url)?,
            size: self.size,
            sha1: copy(&self.sha1)?,
        })
    }
}

/// A font the catalog offers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Font {
    /// E.g. `fredoka`; its plugin is `font-fredoka`.
    pub id: String,
    pub name: String,
    /// `serif`, `sans`, `slab`, `mono`, `display` or `handwriting`.
    pub category: String,
    pub author: String,
    pub licence: String,
    pub version: String,
    /// How many styles it has (regular, bold, italic, bold italic).
    pub styles: u32,
    pub sample: String,
    /// The files for this device: the font files where scalable fonts are
    /// drawn and there is the memory to hold them, else the packs.
    pub files: Vec<FontFile>,
}

impl Font {
    /// Bytes to download.
    #[must_use]
    pub fn size(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }

    /// The id of the plugin it becomes.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn plugin(&self) -> Result<String, Error> {
        concat(&["font-", &self.id])
    }

    /// Whether a newer version than `installed` is offered.
    #[must_use]
    pub fn newer(&self, installed: Option<Version>) -> bool {
        match (installed, Version::parse(&self.version)) {
            (Some(installed), Some(offered)) => offered > installed,
            _ => false,
        }
    }

    /// Fit for this device: something to download, nothing that leaves its
    /// folder.
    fn usable(&self) -> bool {
        is_name(&self.id) && !self.name.is_empty() && !self.files.is_empty()
    }

    /// A copy made with fallible allocation.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn try_clone(&self) -> Result<Self, Error> {
        let mut files = Vec::new();
        files
            .try_reserve_exact(self.files.len())
            .map_err(|_| Error::NoMemory)?;
        for file in &self.files {
            files.push(file.try_clone()?);
        }
        Ok(Self {
            id: copy(&self.id)?,
            name: copy(&self.name)?,
            category: copy(&self.category)?,
            author: copy(&self.author)?,
            licence: copy(&self.licence)?,
            version: copy(&self.version)?,
            styles: self.styles,
            sample: copy(&self.sample)?,
            files,
        })
    }

    /// The font as comparable text (the interface-6 reader's `to_param`):
    /// its fields on the first line, then a line for each file, tabs between.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    #[cfg(test)]
    pub fn to_param(&self) -> Result<String, Error> {
        let mut text = String::new();
        let mut size = [0u8; 20];
        let styles = paperpaw_ui_kit::text::decimal(self.styles as usize, &mut size);
        let first = [
            self.id.as_str(),
            self.name.as_str(),
            self.category.as_str(),
            self.author.as_str(),
            self.licence.as_str(),
            self.version.as_str(),
            styles,
            self.sample.as_str(),
        ];
        let mut len = first.len();
        for part in first {
            len = len.saturating_add(part.len());
        }
        for file in &self.files {
            len = len
                .saturating_add(file.path.len() + file.url.len() + file.sha1.len() + 24)
                .saturating_add(4);
        }
        text.try_reserve_exact(len).map_err(|_| Error::NoMemory)?;
        line(&mut text, &first);
        for file in &self.files {
            let mut digits = [0u8; 20];
            let size = paperpaw_ui_kit::text::decimal_u64(file.size, &mut digits);
            text.push('\n');
            line(&mut text, &[&file.path, size, &file.sha1, &file.url]);
        }
        Ok(text)
    }

    /// What a font's page is handed over (`core.ui` hand-over, at most 1,024
    /// bytes of keys and values on any device): its id. The page finds the
    /// font in the kept catalog by it ([`Self::find`]); the whole entry would
    /// not fit where the catalog lists a font's packs (the X4's).
    #[must_use]
    pub fn page_param(&self) -> &str {
        &self.id
    }

    /// The font with the id `param` ([`Self::page_param`]) of `fonts`.
    #[must_use]
    pub fn find(fonts: Vec<Font>, param: &str) -> Option<Font> {
        fonts.into_iter().find(|font| font.id == param)
    }
}

/// `parts` with tabs between, any control character in them a space.
#[cfg(test)]
fn line(text: &mut String, parts: &[&str]) {
    for (n, part) in parts.iter().enumerate() {
        if n > 0 {
            text.push('\t');
        }
        for c in part.chars() {
            text.push(if c.is_control() { ' ' } else { c });
        }
    }
}

/// Whether `id` is usable in a plugin's id and a file's name.
fn is_name(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 40
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Whether `path` stays within a plugin's folder: plain names, `/` between
/// them.
fn is_inside(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 100
        && path.split('/').all(|part| {
            !part.is_empty()
                && !part.starts_with('.')
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
        })
}

/// The catalog as it streams in.
#[derive(Debug)]
pub struct Catalog<'a> {
    /// The catalog's address, which the others are relative to.
    base: &'a str,
    /// Whether this device takes font files rather than packs.
    outlines: bool,
    fonts: Vec<Font>,
    format: Option<u32>,
    /// The packs of the font being read where font files are wanted: taken
    /// if it has none.
    packs: Vec<FontFile>,
    /// Whether a font kept is being read (the last one): the values of a
    /// font past [`MAX_FONTS`] are passed over.
    reading: bool,
    /// Whether a file kept is being read (the last of its list): the values
    /// of a file past [`MAX_FILES`], or of a list this device does not take,
    /// are passed over.
    file: bool,
    failed: Option<Error>,
}

impl<'a> Catalog<'a> {
    /// A reader of the catalog at `base`; `outlines` when this device takes
    /// font files rather than packs.
    #[must_use]
    pub fn new(base: &'a str, outlines: bool) -> Self {
        Self {
            base,
            outlines,
            fonts: Vec::new(),
            format: None,
            packs: Vec::new(),
            reading: false,
            file: false,
            failed: None,
        }
    }

    /// Reads `text` (the whole catalog), in pieces as a file is read.
    ///
    /// # Errors
    /// [`Error::NoMemory`]; `Ok(None)` when it is no catalog this plugin
    /// reads.
    pub fn read(base: &str, outlines: bool, text: &[u8]) -> Result<Option<Vec<Font>>, Error> {
        let mut catalog = Catalog::new(base, outlines);
        let mut parser = Parser::default();
        for piece in text.chunks(2048) {
            parser
                .try_feed(piece, &mut |event| catalog.event(event))
                .map_err(|_| Error::NoMemory)?;
        }
        parser
            .try_finish(&mut |event| catalog.event(event))
            .map_err(|_| Error::NoMemory)?;
        catalog.finish()
    }

    /// Reads the catalog in the file `path` of `storage`, 2,048 bytes at a
    /// time (the file may be 512 KiB: it is never held whole).
    ///
    /// # Errors
    /// [`Error::NoMemory`]; [`Error::Io`] when the storage fails; `Ok(None)`
    /// when there is no file, or it is no catalog this plugin reads.
    pub fn read_file(
        base: &str,
        outlines: bool,
        storage: &dyn Storage,
        path: &str,
    ) -> Result<Option<Vec<Font>>, Error> {
        let mut catalog = Catalog::new(base, outlines);
        let mut parser = Parser::default();
        let mut piece = Vec::new();
        piece.try_reserve_exact(2048).map_err(|_| Error::NoMemory)?;
        piece.resize(2048, 0);
        let mut at = 0u64;
        loop {
            let n = match storage.read_at(path, at, &mut piece) {
                Ok(n) => n,
                Err(StorageError::NotFound) if at == 0 => return Ok(None),
                Err(_) => return Err(Error::Io),
            };
            if n == 0 {
                break;
            }
            let Some(part) = piece.get(..n) else {
                return Err(Error::Invalid);
            };
            parser
                .try_feed(part, &mut |event| catalog.event(event))
                .map_err(|_| Error::NoMemory)?;
            at += n as u64;
        }
        parser
            .try_finish(&mut |event| catalog.event(event))
            .map_err(|_| Error::NoMemory)?;
        catalog.finish()
    }

    /// Takes what the parser reports.
    pub fn event(&mut self, event: Event<'_>) {
        if self.failed.is_some() {
            return;
        }
        if let Err(error) = self.take(event) {
            self.failed = Some(error);
        }
    }

    fn take(&mut self, event: Event<'_>) -> Result<(), Error> {
        match event {
            Event::Open(at) if at == ["fonts", "[]"] => {
                self.reading = self.fonts.len() < MAX_FONTS;
                if self.reading {
                    push(&mut self.fonts, Font::default())?;
                    self.packs.clear();
                }
            }
            Event::Close(at) if at == ["fonts", "[]"] => {
                if self.reading {
                    self.close()?;
                }
            }
            Event::Open([fonts, item, list, file])
                if fonts == "fonts" && item == "[]" && file == "[]" =>
            {
                self.file = match self.list(list) {
                    Some(files) if files.len() < MAX_FILES => {
                        push(files, FontFile::default())?;
                        true
                    }
                    _ => false,
                };
            }
            Event::Value([key], value) if key == "format" => {
                self.format = value.parse::<u32>().ok();
            }
            Event::Value([fonts, item, key], value) if fonts == "fonts" && item == "[]" => {
                if !self.reading {
                    return Ok(());
                }
                let Some(font) = self.fonts.last_mut() else {
                    return Ok(());
                };
                match key.as_str() {
                    "id" => font.id = copy(value)?,
                    "name" => font.name = copy(value)?,
                    "category" => font.category = copy(value)?,
                    "author" => font.author = copy(value)?,
                    "licence" => font.licence = copy(value)?,
                    "version" => font.version = copy(value)?,
                    "styles" => font.styles = value.parse().unwrap_or(0),
                    "sample" => font.sample = copy(value)?,
                    _ => {}
                }
            }
            Event::Value([fonts, item, list, file, key], value)
                if fonts == "fonts" && item == "[]" && file == "[]" =>
            {
                if !self.file {
                    return Ok(());
                }
                let Some(entry) = self.list(list).and_then(|files| files.last_mut()) else {
                    return Ok(());
                };
                match key.as_str() {
                    "path" => entry.path = copy(value)?,
                    "url" => entry.url = copy(value)?,
                    "size" => entry.size = value.parse().unwrap_or(0),
                    "sha1" => {
                        entry.sha1 = copy(value)?;
                        entry.sha1.make_ascii_lowercase();
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Where the files of the font being read's list `name` go; `None` for
    /// a list this device does not take, or no font being read.
    fn list(&mut self, name: &str) -> Option<&mut Vec<FontFile>> {
        if !self.reading {
            return None;
        }
        match (name, self.outlines) {
            ("packs", false) | ("outline", true) => self.fonts.last_mut().map(|f| &mut f.files),
            ("packs", true) => Some(&mut self.packs),
            _ => None,
        }
    }

    /// The font being read is all here: its addresses made whole, and its
    /// packs taken where it has no font files fit to install.
    fn close(&mut self) -> Result<(), Error> {
        self.reading = false;
        self.file = false;
        let packs = core::mem::take(&mut self.packs);
        let base = self.base;
        let Some(font) = self.fonts.last_mut() else {
            return Ok(());
        };
        if !font.sample.is_empty() {
            font.sample = join_url(base, &font.sample)?;
        }
        font.files = checked(core::mem::take(&mut font.files), base)?;
        if font.files.is_empty() {
            font.files = checked(packs, base)?;
        }
        Ok(())
    }

    /// The fonts of the catalog fit for this device (font files where
    /// `outlines`, else packs), by name; `None` if it is no catalog this
    /// plugin reads.
    ///
    /// # Errors
    /// [`Error::NoMemory`], also for a refusal while it was read.
    pub fn finish(mut self) -> Result<Option<Vec<Font>>, Error> {
        if let Some(error) = self.failed {
            return Err(error);
        }
        // A catalog cut short: the font it ends in.
        if self.reading {
            self.close()?;
        }
        if self.format != Some(FORMAT) {
            return Ok(None);
        }
        let outlines = self.outlines;
        let mut fonts = self.fonts;
        // Fonts this device cannot install (a file over the caps, none or a
        // zero size) are not offered.
        fonts.retain(|font| font.usable() && crate::native_plan::fits(font, outlines));
        stable_sort_by(&mut fonts, |a, b| cmp_lowercase(&a.name, &b.name));
        Ok(Some(fonts))
    }
}

/// `files` with their addresses relative to `base` made whole; none unless
/// every file stays inside the folder, with what checks it, and a
/// `plugin.ini` is among them.
fn checked(mut files: Vec<FontFile>, base: &str) -> Result<Vec<FontFile>, Error> {
    for file in &mut files {
        file.url = join_url(base, &file.url)?;
    }
    let whole = files.iter().all(|f| {
        is_inside(&f.path)
            && !f.url.is_empty()
            && !f.url.contains(char::is_control)
            && f.sha1.len() == 40
    }) && files.iter().any(|f| f.path == "plugin.ini");
    if !whole {
        files.clear();
    }
    Ok(files)
}

#[cfg(test)]
mod tests;
