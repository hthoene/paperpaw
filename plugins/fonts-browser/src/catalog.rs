//! The catalog of fonts: a JSON file from the font library (by default
//! paperpaw.org's), read as it streams. Each font comes ready to install
//! as a data plugin: its files for devices that draw scalable fonts
//! (`outline`, the font files as they are) and for those that draw font
//! packs (`packs`, made for PaperPaw), each with its size and SHA-1. Of
//! the two lists only the one this device installs is kept.
//!
//! ```json
//! {"format": 1, "fonts": [{
//!   "id": "fredoka", "name": "Fredoka", "category": "sans",
//!   "author": "…", "licence": "OFL-1.1", "version": "2.1.0",
//!   "styles": 2, "sample": "https://…/fredoka/sample.ppfp",
//!   "packs": [{"path": "fonts/regular.ppfp", "url": "https://…",
//!              "size": 201234, "sha1": "…"}, …],
//!   "outline": […]}]}
//! ```

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::json::stream::{Event, read_file};
use paperpaw_sdk::web::join_url;
use paperpaw_sdk::{Env, MemoryClass, Storage, Version};

use crate::PLUGIN;

/// The catalog's formats this plugin reads.
const FORMAT: u32 = 1;
/// Fonts read at most, and files of one font.
const MAX_FONTS: usize = 200;
const MAX_FILES: usize = 16;

/// One file of a font's plugin.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FontFile {
    /// Where it goes in the plugin's folder, e.g. `fonts/regular.ppfp`.
    pub path: String,
    pub url: String,
    pub size: u64,
    pub sha1: String,
}

/// A font the catalog offers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Font {
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
    /// The files for this device: the font files where scalable fonts
    /// are drawn and there is the memory to hold them (a font file is
    /// loaded whole), else the packs, read a glyph at a time.
    pub files: Vec<FontFile>,
}

impl Font {
    /// Bytes to download.
    pub(crate) fn size(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }

    /// The id of the plugin it becomes.
    pub(crate) fn plugin(&self) -> String {
        format!("font-{}", self.id)
    }

    /// Whether it is installed, and in which version.
    pub(crate) fn installed(&self, env: &Env<'_>) -> Option<Version> {
        env.plugins()
            .get(&self.plugin())
            .map(|info| info.manifest.version)
    }

    /// Whether a newer version than the one installed is offered.
    pub(crate) fn newer(&self, env: &Env<'_>) -> bool {
        match (self.installed(env), Version::parse(&self.version)) {
            (Some(installed), Some(offered)) => offered > installed,
            _ => false,
        }
    }

    /// Whether it is the font chosen for `setting` (its family is named
    /// as the font).
    pub(crate) fn chosen_for(&self, env: &Env<'_>, setting: &str) -> bool {
        self.installed(env).is_some() && env.settings().text(setting) == self.name
    }

    /// Fit for this device: something to download, nothing that leaves
    /// its folder.
    fn usable(&self) -> bool {
        is_name(&self.id) && !self.name.is_empty() && !self.files.is_empty()
    }

    /// The font as a screen's parameter, for its page to have it without
    /// reading the catalog again: its fields on the first line, then a
    /// line for each file, tabs between.
    pub(crate) fn to_param(&self) -> String {
        let mut text = [
            self.id.as_str(),
            self.name.as_str(),
            self.category.as_str(),
            self.author.as_str(),
            self.licence.as_str(),
            self.version.as_str(),
            self.styles.to_string().as_str(),
            self.sample.as_str(),
        ]
        .map(plain)
        .join("\t");
        for file in &self.files {
            text.push('\n');
            text.push_str(
                &[
                    file.path.as_str(),
                    file.size.to_string().as_str(),
                    file.sha1.as_str(),
                    file.url.as_str(),
                ]
                .map(plain)
                .join("\t"),
            );
        }
        text
    }

    /// The font [`to_param`](Self::to_param) made `text` of; `None` if it
    /// is none, or not one fit for this device.
    pub(crate) fn from_param(text: &str) -> Option<Font> {
        let mut lines = text.split('\n');
        let mut fields = lines.next()?.split('\t');
        let mut field = || fields.next().map(String::from);
        let mut font = Font {
            id: field()?,
            name: field()?,
            category: field()?,
            author: field()?,
            licence: field()?,
            version: field()?,
            styles: field()?.parse().ok()?,
            sample: field()?,
            files: Vec::new(),
        };
        for line in lines.take(MAX_FILES) {
            let mut parts = line.splitn(4, '\t');
            font.files.push(FontFile {
                path: parts.next()?.to_string(),
                size: parts.next()?.parse().ok()?,
                sha1: parts.next()?.to_string(),
                url: parts.next()?.to_string(),
            });
        }
        font.usable().then_some(font)
    }
}

/// `text` without tabs and line breaks, for a parameter's field.
fn plain(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

/// Whether this device takes font files rather than packs.
fn outlines(env: &Env<'_>) -> bool {
    paperpaw_sdk::text::OUTLINES && env.device().memory == MemoryClass::Large
}

/// Whether `id` is usable in a plugin's id and a file's name.
fn is_name(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 40
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Whether `path` stays within a plugin's folder: plain names, `/`
/// between them.
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

/// Where the catalog is kept between visits.
pub(crate) fn path() -> String {
    format!("{}/catalog.json", Env::plugin_dir(PLUGIN.as_str()))
}

/// The catalog kept from the last visit.
pub(crate) fn kept(env: &Env<'_>) -> Vec<Font> {
    let base = env.settings().read(&crate::CATALOG);
    read(env.storage(), &path(), base.trim(), outlines(env)).unwrap_or_default()
}

/// The catalog fetched just now, kept at `path`.
pub(crate) fn fetched(env: &Env<'_>, path: &str, base: &str) -> Option<Vec<Font>> {
    read(env.storage(), path, base, outlines(env))
}

/// The fonts of the catalog file at `path` fit for this device (font
/// files where `outlines`, else packs), by name, their addresses relative
/// to `base` (the catalog's); `None` if it is no catalog this plugin
/// reads.
pub(crate) fn read(
    storage: &dyn Storage,
    path: &str,
    base: &str,
    outlines: bool,
) -> Option<Vec<Font>> {
    let mut reader = Reader {
        base,
        outlines,
        fonts: Vec::new(),
        format: None,
        packs: Vec::new(),
        reading: false,
        file: false,
    };
    read_file(storage, path, &mut |event| reader.event(event));
    // A catalog cut short: the font it ends in.
    if reader.reading {
        reader.finish();
    }
    if reader.format != Some(FORMAT) {
        return None;
    }
    let mut fonts = reader.fonts;
    fonts.retain(Font::usable);
    paperpaw_sdk::sort::sort_by_cached_key(&mut fonts, |f| f.name.to_lowercase());
    fonts.shrink_to_fit();
    Some(fonts)
}

/// The catalog as it streams in.
struct Reader<'a> {
    /// The catalog's address, which the others are relative to.
    base: &'a str,
    /// Whether this device takes font files rather than packs.
    outlines: bool,
    fonts: Vec<Font>,
    format: Option<u32>,
    /// The packs of the font being read where font files are wanted:
    /// taken if it has none.
    packs: Vec<FontFile>,
    /// Whether a font kept is being read (the last one): the values of a
    /// font past [`MAX_FONTS`] are passed over.
    reading: bool,
    /// Whether a file kept is being read (the last of its list): the
    /// values of a file past [`MAX_FILES`], or of a list this device does
    /// not take, are passed over.
    file: bool,
}

impl Reader<'_> {
    fn event(&mut self, event: Event<'_>) {
        match event {
            Event::Open(at) if at == ["fonts", "[]"] => {
                self.reading = self.fonts.len() < MAX_FONTS;
                if self.reading {
                    self.fonts.push(Font::default());
                    self.packs.clear();
                }
            }
            Event::Close(at) if at == ["fonts", "[]"] => {
                if self.reading {
                    self.finish();
                }
            }
            Event::Open([fonts, item, list, file])
                if fonts == "fonts" && item == "[]" && file == "[]" =>
            {
                self.file = match self.list(list) {
                    Some(files) if files.len() < MAX_FILES => {
                        files.push(FontFile::default());
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
                    return;
                }
                let Some(font) = self.fonts.last_mut() else {
                    return;
                };
                let value = value.to_string();
                match key.as_str() {
                    "id" => font.id = value,
                    "name" => font.name = value,
                    "category" => font.category = value,
                    "author" => font.author = value,
                    "licence" => font.licence = value,
                    "version" => font.version = value,
                    "styles" => font.styles = value.parse().unwrap_or(0),
                    "sample" => font.sample = value,
                    _ => {}
                }
            }
            Event::Value([fonts, item, list, file, key], value)
                if fonts == "fonts" && item == "[]" && file == "[]" =>
            {
                if !self.file {
                    return;
                }
                let Some(entry) = self.list(list).and_then(|files| files.last_mut()) else {
                    return;
                };
                match key.as_str() {
                    "path" => entry.path = value.to_string(),
                    "url" => entry.url = value.to_string(),
                    "size" => entry.size = value.parse().unwrap_or(0),
                    "sha1" => entry.sha1 = value.to_ascii_lowercase(),
                    _ => {}
                }
            }
            _ => {}
        }
    }

    /// Where the files of the font being read's list `name` go; `None`
    /// for a list this device does not take, or no font being read.
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

    /// The font being read is all here: its addresses made whole, and
    /// its packs taken where it has no font files fit to install.
    fn finish(&mut self) {
        self.reading = false;
        self.file = false;
        let packs = core::mem::take(&mut self.packs);
        let base = self.base;
        let Some(font) = self.fonts.last_mut() else {
            return;
        };
        if !font.sample.is_empty() {
            font.sample = join_url(base, &font.sample);
        }
        font.files = checked(core::mem::take(&mut font.files), base);
        if font.files.is_empty() {
            font.files = checked(packs, base);
        }
    }
}

/// `files` with their addresses relative to `base` made whole; none
/// unless every file stays inside the folder, with what checks it, and a
/// `plugin.ini` is among them.
fn checked(mut files: Vec<FontFile>, base: &str) -> Vec<FontFile> {
    for file in &mut files {
        file.url = join_url(base, &file.url);
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
    files.shrink_to_fit();
    files
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use paperpaw_sdk::MemoryStorage;

    const BASE: &str = "https://fonts.example/fonts.json";
    const SHA1: &str = "0123456789abcdef0123456789abcdef01234567";

    /// A file of font `id`, `n` its number.
    fn file(id: &str, path: &str, n: usize) -> String {
        format!(r#"{{"path": "{path}", "url": "/{id}/{n}", "size": {n}, "sha1": "{SHA1}"}}"#)
    }

    /// Font `id` with `files` packs, and font files if `outline`.
    fn font(id: &str, files: usize, outline: bool) -> String {
        let list = |kind: &str| {
            (0..files)
                .map(|n| match n {
                    0 => file(id, "plugin.ini", n),
                    _ => file(id, &format!("fonts/{kind}{n}"), n),
                })
                .collect::<Vec<_>>()
                .join(", ")
        };
        let outline = if outline {
            format!(r#", "outline": [{}]"#, list("outline"))
        } else {
            String::new()
        };
        format!(
            r#"{{"id": "{id}", "name": "Font {id}", "version": "1.0", "styles": 1,
  "sample": "/{id}/sample", "packs": [{}]{outline}}}"#,
            list("pack")
        )
    }

    fn catalog(fonts: &[String]) -> Vec<u8> {
        format!(r#"{{"format": 1, "fonts": [{}]}}"#, fonts.join(",\n")).into_bytes()
    }

    fn read_all(json: Vec<u8>, outlines: bool) -> Vec<Font> {
        let storage = MemoryStorage::new().with("catalog.json", json);
        read(&storage, "catalog.json", BASE, outlines).unwrap()
    }

    #[test]
    fn fonts_and_files_past_the_limits_leave_those_kept_whole() {
        // The first font with more files than are read, then more fonts
        // than are read.
        let mut fonts = vec![font("a000", MAX_FILES + 3, false)];
        fonts.extend((1..MAX_FONTS + 5).map(|n| font(&format!("a{n:03}"), 3, false)));
        let read = read_all(catalog(&fonts), false);
        assert_eq!(read.len(), MAX_FONTS);
        for (n, font) in read.iter().enumerate() {
            let id = format!("a{n:03}");
            assert_eq!(font.id, id);
            assert_eq!(font.name, format!("Font {id}"));
            assert_eq!(font.sample, format!("https://fonts.example/{id}/sample"));
            for (k, file) in font.files.iter().enumerate() {
                assert_eq!(file.url, format!("https://fonts.example/{id}/{k}"));
                assert_eq!(file.size, k as u64);
            }
        }
        assert_eq!(read[0].files.len(), MAX_FILES);
        assert_eq!(read[0].files[MAX_FILES - 1].path, "fonts/pack15");
        assert_eq!(read[MAX_FONTS - 1].files.len(), 3);
    }

    #[test]
    fn only_the_files_this_device_takes_are_kept() {
        // Font files and packs; packs only; font files without a
        // `plugin.ini`, which are not fit to install.
        let broken = font("c", 2, true).replace(
            r#""outline": [{"path": "plugin.ini""#,
            r#""outline": [{"path": "theme.ini""#,
        );
        let json = catalog(&[font("a", 2, true), font("b", 2, false), broken]);
        let paths =
            |font: &Font| -> Vec<String> { font.files.iter().map(|f| f.path.clone()).collect() };
        for font in read_all(json.clone(), false) {
            assert_eq!(paths(&font), ["plugin.ini", "fonts/pack1"], "{}", font.id);
        }
        let read = read_all(json, true);
        assert_eq!(paths(&read[0]), ["plugin.ini", "fonts/outline1"]);
        assert_eq!(paths(&read[1]), ["plugin.ini", "fonts/pack1"]);
        assert_eq!(paths(&read[2]), ["plugin.ini", "fonts/pack1"]);
    }

    #[test]
    fn a_font_goes_to_its_page_whole() {
        let mut font = read_all(catalog(&[font("a", 3, false)]), false).remove(0);
        assert_eq!(Font::from_param(&font.to_param()).as_ref(), Some(&font));
        // What would break the lines is left out.
        font.name = String::from("Two\tWords\n");
        let back = Font::from_param(&font.to_param()).unwrap();
        assert_eq!(back.name, "Two Words ");
        assert_eq!(back.files, font.files);
        assert_eq!(Font::from_param(""), None);
        assert_eq!(Font::from_param("../a\tA\t\t\t\t1.0\t1\t"), None);
    }
}
