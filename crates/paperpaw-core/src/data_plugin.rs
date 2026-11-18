//! Data plugins: folders on the storage with a `plugin.ini` and data
//! files, no code. They are installed by copying a folder into a plugin
//! folder (e.g. `plugins/` on an SD card) and work on every device.
//!
//! ```text
//! plugins/large-print/
//!   plugin.ini     [plugin] id, name, description, version, formats
//!   i18n/*.ini     translation catalogs (also whole new languages)
//!   i18n/*.ppcat   the same compiled, read as needed (formats 4, ADR 0033)
//!   keymap.ini     key bindings
//!   settings.ini   new defaults for any plugin's settings ([core] font-size = 14)
//!   fonts/*.ttf    fonts to read with (also .otf, .ttc), by family
//!   fonts/*.ppfp   packs with characters other fonts lack, named in
//!                  plugin.ini: [extends] cyrillic.ppfp = fonts-default.literata
//!   fonts/…        a family's faces, named in plugin.ini: [family] name =
//!                  Fredoka, regular = fredoka.ppfp, bold, italic,
//!                  bold-italic (packs, or .ttf/.otf where those are drawn)
//!   styles.css     a stylesheet for books, after a book's own
//!   presets.ini    ready-made sets of settings, e.g. power modes, a
//!                  section each: [night] kind = core.power-mode, name,
//!                  description, then the values (power.level = …)
//!   marks.ini      words marked on book pages by patterns, a section each:
//!                  [names] title, pattern (one or more), the look
//!                  (background = yellow, line = wavy …); formats 2
//!   menus.ini      entries of menus: another plugin's as
//!                  [menu core.reader-menu] add = my.night after core.toc,
//!                  hide = stats.open; a menu of one's own: [tools] title,
//!                  entries = a.x (a line each, see `formats`)
//!   actions.ini    actions made of steps, a section each: [night] title,
//!                  icon, run = core.night-mode=on, run = notify:my.key,
//!                  run = open:core.library, run = emit:my.event,
//!                  run = another.action (a step a line, at most 16)
//!   icons.ini      icons of actions and settings: item = icon-name
//!   points/*.txt   lines for a point (`<point>.txt`, see
//!                  `Staging::contribute`): keyboard layouts, About lines
//!   setup.ini      steps of the first-run setup, a section each: [size]
//!                  setting = core.font-size (or screen = …), order, title,
//!                  help
//!   plugin.ini     also [fonts]: a role = a font, e.g.
//!                  core.font-ui-strong = my.semibold; formats 3
//!   hyphenation/*.pphp  hyphenation tables, named in plugin.ini:
//!                  [hyphenation] de = hyphenation/de.pphp, a language a
//!                  line (see `hyphenation`); formats 5
//! ```
//!
//! The kernel lists them like every other plugin, so they can be switched
//! off in the plugin manager. Files are read once at start-up, at most
//! [`MAX_PLUGIN_DATA`] of a plugin in all; the files that are parsed
//! (settings, presets, marks, menus, actions, icons, setup steps, points)
//! one at a time as the plugin registers, so that only what it
//! contributes stays in memory.

use alloc::borrow::Cow;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::RefCell;

use crate::Staging;
use paperpaw_platform::Storage;
use paperpaw_sdk::FontFace;
use paperpaw_sdk::presets::Preset;
use paperpaw_sdk::{Id, Manifest, RegisterError, Version};

mod catalogs;
pub(crate) mod formats;
pub(crate) mod hyphenation;
mod names;
use formats::{actions, menus, setup_steps};

/// Largest data file read (a translation catalog, a key map).
const MAX_FILE: usize = 64 * 1024;
/// Largest `plugin.ini`.
const MAX_MANIFEST: usize = 4 * 1024;
/// Most files in `points/`.
const MAX_POINTS: usize = 16;
/// Most lines a plugin contributes to points, all files together.
const MAX_POINT_LINES: usize = 64;
/// What the files of a data plugin may take together, in bytes: its key
/// map, styles and the files it registers.
pub(crate) const MAX_PLUGIN_DATA: usize = 96 * 1024;
/// What its translations may take together, in bytes: they stay in
/// memory as catalogs anyway, and a language of its own (all plugins'
/// texts) is large.
const MAX_TRANSLATIONS: usize = 512 * 1024;
/// What the files of formats 3 (`menus.ini`, `actions.ini`, `icons.ini`,
/// `setup.ini`, `points/`) may take of it.
const MAX_FORMAT3_DATA: usize = 24 * 1024;
/// The version of the formats that brought those files.
const FORMATS_3: u32 = 3;
/// The version that brought compiled catalogs (`i18n/*.ppcat`).
const FORMATS_4: u32 = 4;
/// The files read as the plugin registers, and whether each is of
/// formats 3.
const LATER: [(&str, bool); 7] = [
    ("settings.ini", false),
    ("presets.ini", false),
    ("marks.ini", false),
    ("menus.ini", true),
    ("actions.ini", true),
    ("icons.ini", true),
    ("setup.ini", true),
];
/// The version of the formats of data plugins this PaperPaw reads.
pub(crate) const FORMATS: u32 = paperpaw_sdk::DATA_FORMATS;

/// The styles of a family's faces in `[family]`: (key, bold, italic).
const FACES: [(&str, bool, bool); 4] = [
    ("regular", false, false),
    ("bold", true, false),
    ("italic", false, true),
    ("bold-italic", true, true),
];

/// A plugin made of data files.
pub(crate) struct DataPlugin {
    manifest: Manifest,
    /// Its folder, e.g. `plugins/large-print`.
    folder: String,
    /// Taken by `register`, which runs once.
    translations: RefCell<Vec<String>>,
    /// Compiled catalogs (`i18n/*.ppcat`): their paths, read as needed.
    catalog_files: RefCell<Vec<String>>,
    keymap: RefCell<Option<String>>,
    styles: RefCell<Option<String>>,
    /// Where the files read as it registers are.
    storage: Option<Rc<dyn Storage>>,
    /// The files read as it registers (within the budget): their paths.
    later: RefCell<Vec<String>>,
    /// Fonts for roles, from `[fonts]` of `plugin.ini`.
    roles: RefCell<Vec<(String, String)>>,
    /// What was left out as it was read, to tell (see
    /// [`DataPlugin::take_problems`]).
    problems: RefCell<Vec<String>>,
    /// Paths of font files.
    fonts: RefCell<Vec<String>>,
    /// Packs that extend other fonts: their paths and those fonts' ids.
    extensions: RefCell<Vec<(String, String)>>,
    /// A family's faces: their key in `[family]` and file, the family.
    faces: RefCell<Vec<(&'static str, String)>>,
    family: Option<String>,
    /// Packs loose in a font folder, faces of families their file names
    /// give (see [`pack_face`]): family, key in [`FACES`], path.
    loose_faces: RefCell<Vec<(String, &'static str, String)>>,
    /// Hyphenation tables `[hyphenation]` names, by language.
    hyphenation: RefCell<Vec<hyphenation::Table>>,
    /// Its folder holds native code (see `code_plugin`).
    code: bool,
}

impl DataPlugin {
    /// Its folder on the storage.
    pub(crate) fn folder(&self) -> &str {
        &self.folder
    }

    /// Keeps of its translations only what lists it (its name and
    /// description, in every language), for a plugin switched off: the
    /// rest would take memory for nothing.
    pub(crate) fn keep_listing_only(&self) {
        let prefix = format!("{}.plugin.", self.manifest.id);
        for source in self.translations.borrow_mut().iter_mut() {
            *source = listing(source, &prefix);
        }
    }

    /// File `name` of its folder, read now where it was let through as it
    /// loaded.
    fn read_later(&self, name: &str) -> Option<String> {
        let path = format!("{}/{name}", self.folder);
        self.take_later(&path)
    }

    /// File `path`, read now where it was let through as it loaded.
    fn take_later(&self, path: &str) -> Option<String> {
        let mut later = self.later.borrow_mut();
        let at = later.iter().position(|p| p == path)?;
        later.remove(at);
        drop(later);
        read(self.storage.as_deref()?, path, MAX_FILE)
    }

    /// What was left out of it as it was read (files of formats it does
    /// not say it is made for, files too large), to tell once.
    pub(crate) fn take_problems(&self) -> Vec<String> {
        self.problems.take()
    }

    /// The hyphenation tables it names, to add once it is registered.
    pub(crate) fn take_hyphenation(&self) -> Vec<hyphenation::Table> {
        self.hyphenation.take()
    }

    /// Whether its folder holds native code: it is a code plugin.
    pub(crate) fn has_code(&self) -> bool {
        self.code
    }
}

impl DataPlugin {
    pub(crate) fn manifest(&self) -> Manifest {
        self.manifest.clone()
    }

    /// Adds what its folder declares (`Kernel::add_data`).
    pub(crate) fn register(&self, r: &mut Staging) -> Result<(), RegisterError> {
        for source in self.translations.take() {
            r.add_translations(Cow::Owned(source))?;
        }
        for path in self.catalog_files.take() {
            r.add_translation_file(path)?;
        }
        if let Some(css) = self.styles.take() {
            r.add_stylesheet(Cow::Owned(css))?;
        }
        if let Some(source) = self.keymap.take() {
            r.add_keymap(Cow::Owned(source))?;
        }
        self.register_fonts(r)?;
        self.register_files(r)?;
        self.register_points(r)?;
        for (role, font) in self.roles.take() {
            let (role, font) = (parsed_id(&role, 0)?, parsed_id(&font, 0)?);
            r.bind_font(role, font, paperpaw_sdk::Priority::DEFAULT)?;
        }
        if let Some(source) = self.read_later("marks.ini") {
            let marks = crate::kit::decorations::MarkRules::parse(&self.manifest.id, &source)?;
            for (role, title, look) in marks.roles {
                crate::kit::decorations::add_role(&mut |s| r.add_setting(s), &role, title, look)?;
            }
            if !marks.decorator.is_empty() {
                let id = Id::parse(&format!("{}.marks", self.manifest.id)).map_err(|_| {
                    RegisterError::Invalid {
                        message: Cow::Borrowed("no decorator id"),
                    }
                })?;
                r.add_decorator(marks.decorator.into_spec(id, self.manifest.name.clone()))?;
            }
        }
        Ok(())
    }

    /// Adds its font files, extension packs and faces.
    fn register_fonts(&self, r: &mut Staging) -> Result<(), RegisterError> {
        let mut used: Vec<String> = Vec::new();
        for path in self.fonts.take() {
            let mut name = slug(path.rsplit('/').next().unwrap_or(&path));
            // The same file name in two folders.
            while used.contains(&name) {
                name.push('-');
                name.push('2');
            }
            used.push(name.clone());
            let id = Id::parse(&format!("{}.{name}", self.manifest.id)).map_err(|_| {
                RegisterError::Invalid {
                    message: Cow::Owned(format!("{path}: unusable file name")),
                }
            })?;
            r.add_font_file(id, path)?;
        }
        let mut named: Vec<String> = Vec::new();
        for (path, extends) in self.extensions.take() {
            let mut name = slug(path.rsplit('/').next().unwrap_or(&path));
            // A pack that extends several fonts: an id for each.
            let before = named.iter().filter(|n| **n == name).count();
            named.push(name.clone());
            if before > 0 {
                name = format!("{name}-{}", before + 1);
            }
            let id = Id::parse(&format!("{}.{name}", self.manifest.id));
            let extends = Id::parse(&extends);
            let (Ok(id), Ok(extends)) = (id, extends) else {
                return Err(RegisterError::Invalid {
                    message: Cow::Owned(format!("{path}: unusable name or font")),
                });
            };
            r.add_font_extension(id, path, extends)?;
        }
        if let Some(family) = &self.family {
            for (key, path) in self.faces.take() {
                let (bold, italic) = FACES
                    .iter()
                    .find(|(k, ..)| *k == key)
                    .map_or((false, false), |&(_, b, i)| (b, i));
                let id = Id::parse(&format!("{}.{key}", self.manifest.id)).map_err(|_| {
                    RegisterError::Invalid {
                        message: Cow::Owned(format!("{path}: unusable name")),
                    }
                })?;
                r.add_font_face(FontFace::new(id, path, family.clone(), bold, italic))?;
            }
        }
        for (family, key, path) in self.loose_faces.take() {
            let (bold, italic) = FACES
                .iter()
                .find(|(k, ..)| *k == key)
                .map_or((false, false), |&(_, b, i)| (b, i));
            let mut name = format!("{}-{key}", slug(&family));
            // The same family and face twice (two folders).
            while used.contains(&name) {
                name.push_str("-2");
            }
            used.push(name.clone());
            let id = Id::parse(&format!("{}.{name}", self.manifest.id)).map_err(|_| {
                RegisterError::Invalid {
                    message: Cow::Owned(format!("{path}: unusable file name")),
                }
            })?;
            r.add_font_face(FontFace::new(id, path, family, bold, italic))?;
        }
        Ok(())
    }

    /// Adds what its settings, presets, actions, menus, icons and setup
    /// files declare.
    fn register_files(&self, r: &mut Staging) -> Result<(), RegisterError> {
        let id = &self.manifest.id;
        if let Some(source) = self.read_later("settings.ini") {
            for entry in paperpaw_config::parse(&source) {
                let entry = entry.map_err(|error| RegisterError::Parse {
                    line: error.line,
                    message: Cow::Owned(error.to_string()),
                })?;
                let id = Id::parse(&format!("{}.{}", entry.section, entry.key)).map_err(|_| {
                    RegisterError::Parse {
                        line: entry.line,
                        message: Cow::Owned(format!(
                            "invalid setting `{}.{}`",
                            entry.section, entry.key
                        )),
                    }
                })?;
                r.set_setting_default(id, Cow::Owned(entry.value.as_str().into_owned()))?;
            }
        }
        if let Some(source) = self.read_later("presets.ini") {
            for preset in presets(&self.manifest.id, &source)? {
                r.add_preset(preset)?;
            }
        }
        if let Some(source) = self.read_later("actions.ini") {
            for action in actions(id, &source)? {
                r.add_data_action(action.spec, action.steps)?;
                if let Some(icon) = action.icon {
                    r.add_icon(action.id, Cow::Owned(icon))?;
                }
            }
        }
        if let Some(source) = self.read_later("menus.ini") {
            menus(r, id, &source)?;
        }
        if let Some(source) = self.read_later("icons.ini") {
            for entry in paperpaw_config::parse(&source) {
                let entry = entry.map_err(parse_error)?;
                let item = parsed_id(entry.key, entry.line)?;
                r.add_icon(item, Cow::Owned(entry.value.as_str().trim().to_string()))?;
            }
        }
        if let Some(source) = self.read_later("setup.ini") {
            for step in setup_steps(id, &source)? {
                r.add_setup_step(step)?;
            }
        }
        Ok(())
    }

    /// Adds the lines of its points' files, one file at a time and at
    /// most `MAX_POINT_LINES` lines in all.
    fn register_points(&self, r: &mut Staging) -> Result<(), RegisterError> {
        let prefix = format!("{}/points/", self.folder);
        let files: Vec<String> = self
            .later
            .borrow()
            .iter()
            .filter(|p| p.starts_with(&prefix))
            .cloned()
            .collect();
        let mut lines_left = MAX_POINT_LINES;
        for file in files {
            let Some(text) = self.take_later(&file) else {
                continue;
            };
            let name = file.rsplit('/').next().unwrap_or(&file);
            let point = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
            let point = parsed_id(point, 0)?;
            for line in text.lines().map(str::trim) {
                if line.is_empty() || line.starts_with('#') || lines_left == 0 {
                    continue;
                }
                lines_left -= 1;
                r.contribute(point.clone(), Cow::Owned(line.to_string()))?;
            }
        }
        Ok(())
    }
}

fn parse_error(error: paperpaw_config::ParseError) -> RegisterError {
    RegisterError::Parse {
        line: error.line,
        message: Cow::Owned(error.to_string()),
    }
}

/// `text` as an id, or what is wrong at `line`.
fn parsed_id(text: &str, line: u32) -> Result<Id, RegisterError> {
    Id::parse(text.trim()).map_err(|_| RegisterError::Parse {
        line,
        message: Cow::Owned(format!("invalid id `{}`", text.trim())),
    })
}

/// The id a section `name` of a data plugin's file stands for: `name`
/// where it is of the plugin's namespace, else `<plugin>.<name>`.
fn own_id(plugin: &Id, name: &str, line: u32) -> Result<Id, RegisterError> {
    let id = parsed_id(name, line)
        .ok()
        .filter(|id| id.namespace() == plugin.as_str() && id.as_str() != plugin.as_str());
    match id {
        Some(id) => Ok(id),
        None => parsed_id(&format!("{plugin}.{name}"), line),
    }
}

/// The fonts `[fonts]` in a `plugin.ini` binds to roles, where the plugin
/// is made for formats 3 (else left out, told).
fn fonts_for_roles(source: &str, path: &str, budget: &mut Budget) -> Vec<(String, String)> {
    let found = roles(source);
    if budget.formats < FORMATS_3 && !found.is_empty() {
        budget.problems.push(format!(
            "{path}/plugin.ini: [fonts] left out, the plugin says it is made for formats {}",
            budget.formats
        ));
        return Vec::new();
    }
    found
}

/// The fonts `[fonts]` in a `plugin.ini` binds to roles.
fn roles(source: &str) -> Vec<(String, String)> {
    paperpaw_config::parse(source)
        .filter_map(Result::ok)
        .filter(|entry| entry.section == "fonts")
        .map(|entry| {
            (
                entry.key.to_string(),
                entry.value.as_str().trim().to_string(),
            )
        })
        .collect()
}

/// What the files of one plugin may still take (all, and those of
/// formats 3), and what was left out.
struct Budget {
    /// The formats the plugin says it is made for.
    formats: u32,
    left: usize,
    left3: usize,
    problems: Vec<String>,
}

impl Budget {
    /// Whether file `path` (`format3`: of formats 3) is read, within what
    /// is left; `false` where it is not there, or left out (told).
    fn allow(&mut self, storage: &dyn Storage, path: &str, format3: bool) -> bool {
        let Ok(size) = storage.size(path) else {
            return false;
        };
        if format3 && self.formats < FORMATS_3 {
            self.problems.push(format!(
                "{path}: left out, the plugin says it is made for formats {}",
                self.formats
            ));
            return false;
        }
        let size = usize::try_from(size).unwrap_or(usize::MAX);
        let room = if format3 {
            self.left.min(self.left3)
        } else {
            self.left
        };
        if size > room.min(MAX_FILE) {
            self.problems
                .push(format!("{path}: left out, more than the plugin may take"));
            return false;
        }
        self.left -= size;
        if format3 {
            self.left3 -= size;
        }
        true
    }

    /// File `path`, read now within what is left.
    fn read_now(&mut self, storage: &dyn Storage, path: &str) -> Option<String> {
        self.allow(storage, path, false)
            .then(|| read(storage, path, MAX_FILE))
            .flatten()
    }
}

/// The files of a plugin's `points` folder that may be read, by path.
fn point_files(storage: &dyn Storage, path: &str, budget: &mut Budget) -> Vec<String> {
    let dir = format!("{path}/points");
    let mut names: Vec<String> = Vec::new();
    let _ = storage.list(&dir, &mut |entry| {
        if !entry.is_dir && paperpaw_sdk::files::has_extension(entry.name, "txt") {
            names.push(entry.name.to_string());
        }
    });
    paperpaw_sort::sort(&mut names);
    names
        .into_iter()
        .take(MAX_POINTS)
        .map(|name| format!("{dir}/{name}"))
        .filter(|file| budget.allow(storage, file, true))
        .collect()
}

/// Of a translation catalog `source`, its `[meta]` section and the
/// messages whose keys start with `prefix`.
fn listing(source: &str, prefix: &str) -> String {
    let mut out = String::new();
    let mut meta = false;
    for line in source.lines() {
        let text = line.trim();
        if text.starts_with('[') {
            meta = text == "[meta]";
            out.push_str(text);
        } else if meta || text.starts_with(prefix) {
            out.push_str(line);
        } else {
            continue;
        }
        out.push('\n');
    }
    out
}

/// The presets of a `presets.ini`, a section each: its `kind`, `name`
/// and `description`, then its values (keys with a dot: settings).
fn presets(plugin: &Id, source: &str) -> Result<Vec<Preset>, RegisterError> {
    let mut out: Vec<Preset> = Vec::new();
    let mut section = "";
    for entry in paperpaw_config::parse(source) {
        let entry = entry.map_err(|error| RegisterError::Parse {
            line: error.line,
            message: Cow::Owned(error.to_string()),
        })?;
        if entry.section.is_empty() {
            continue;
        }
        if entry.section != section {
            section = entry.section;
            let id =
                Id::parse(&format!("{plugin}.{section}")).map_err(|_| RegisterError::Parse {
                    line: entry.line,
                    message: Cow::Owned(format!("invalid preset `{section}`")),
                })?;
            out.push(Preset::new(id, plugin.clone(), "", "", String::new()));
        }
        let Some(preset) = out.last_mut() else {
            continue;
        };
        let value = entry.value.as_str();
        match entry.key {
            "kind" => {
                preset.kind = Id::parse(value.trim()).map_err(|_| RegisterError::Parse {
                    line: entry.line,
                    message: Cow::Owned(format!("invalid kind `{value}`")),
                })?;
            }
            "name" => preset.title = Cow::Owned(value.into_owned()),
            "description" => preset.description = Cow::Owned(value.into_owned()),
            key if key.contains('.') => {
                let values = preset.values.to_mut();
                values.push_str(key);
                values.push_str(" = ");
                values.push_str(&value);
                values.push('\n');
            }
            _ => {}
        }
    }
    Ok(out)
}

/// The folders of the data plugins in `dir` (not the hidden ones), in
/// name order, to load one at a time ([`load_folder`]): read all first,
/// their files together might not fit in memory.
///
/// In the same listing, a plugin set aside by an install cut short (the
/// power went) is put back: `.<id>.old` takes the place of a missing
/// `<id>`, and goes where `<id>` is there (the new version got there);
/// what cannot be put back is reported through `warn`.
pub(crate) fn folders(
    storage: &dyn Storage,
    dir: &str,
    warn: &mut dyn FnMut(String),
) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let mut aside: Vec<String> = Vec::new();
    let _ = storage.list(dir, &mut |entry| {
        if !entry.is_dir {
            return;
        }
        match entry.name.strip_prefix('.') {
            None => names.push(entry.name.to_string()),
            Some(hidden) => {
                if let Some(id) = hidden.strip_suffix(".old").filter(|id| !id.is_empty()) {
                    aside.push(id.to_string());
                }
            }
        }
    });
    for id in aside {
        let old = format!("{dir}/.{id}.old");
        if names.contains(&id) {
            let _ = storage.remove_dir_all(&old);
        } else {
            match storage.rename(&old, &format!("{dir}/{id}")) {
                Ok(()) => names.push(id),
                Err(error) => warn(format!("{old}: not put back ({error})")),
            }
        }
    }
    paperpaw_sort::sort(&mut names);
    names
        .into_iter()
        .map(|name| format!("{dir}/{name}"))
        .collect()
}

/// The plugin in folder `path`, e.g. `plugins/font-fredoka`.
pub(crate) fn load_folder(storage: &Rc<dyn Storage>, path: &str) -> Result<DataPlugin, String> {
    let path = path.trim_end_matches('/');
    let name = path.rsplit('/').next().unwrap_or(path);
    load(storage, path, name)
}

/// The plugin in folder `path`, its id `name` unless its `plugin.ini`
/// names one (for a folder named otherwise, e.g. one downloaded into).
pub(crate) fn load_folder_as(
    storage: &Rc<dyn Storage>,
    path: &str,
    name: &str,
) -> Result<DataPlugin, String> {
    load(storage, path.trim_end_matches('/'), name)
}

/// The id of the plugin in folder `path` as its `plugin.ini` says it
/// (`name` where it says none), whether or not it could be loaded.
pub(crate) fn folder_plugin_id(storage: &dyn Storage, path: &str, name: &str) -> Option<String> {
    let source = read(storage, &format!("{path}/plugin.ini"), MAX_MANIFEST)?;
    let named = paperpaw_config::parse(&source)
        .filter_map(Result::ok)
        .find(|entry| entry.section == "plugin" && entry.key == "id")
        .map(|entry| entry.value.as_str().trim().to_string());
    Some(named.unwrap_or_else(|| name.to_string()))
}

/// The folders in `dir` holding the plugin `id` (by their `plugin.ini`, or
/// by their name where it names no id), loadable or not.
pub(crate) fn folders_of(storage: &dyn Storage, dir: &str, id: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let _ = storage.list(dir, &mut |entry| {
        if entry.is_dir && !entry.name.starts_with('.') {
            names.push(entry.name.to_string());
        }
    });
    names
        .into_iter()
        .filter(|name| {
            let path = format!("{dir}/{name}");
            folder_plugin_id(storage, &path, name).as_deref() == Some(id)
        })
        .map(|name| format!("{dir}/{name}"))
        .collect()
}

/// The plugin in folder `path`.
fn load(shared: &Rc<dyn Storage>, path: &str, folder: &str) -> Result<DataPlugin, String> {
    let storage = shared.as_ref();
    let names = names::Names::of(storage, path);
    let manifest = read(storage, &format!("{path}/plugin.ini"), MAX_MANIFEST)
        .ok_or_else(|| format!("{path}: no plugin.ini"))?;
    let source = manifest;
    let (manifest, formats) =
        parse_manifest(&source, folder).map_err(|e| format!("{path}/plugin.ini: {e}"))?;
    let mut budget = Budget {
        formats,
        left: MAX_PLUGIN_DATA,
        left3: MAX_FORMAT3_DATA,
        problems: Vec::new(),
    };
    let catalogs = format!("{path}/i18n");
    let (mut files, mut compiled) = match names.has("i18n") {
        true => catalogs::names(storage, &catalogs),
        false => (Vec::new(), Vec::new()),
    };
    paperpaw_sort::sort(&mut files);
    paperpaw_sort::sort(&mut compiled);
    if budget.formats < FORMATS_4 {
        if !compiled.is_empty() {
            budget.problems.push(format!(
                "{catalogs}: compiled catalogs left out, plugin.ini does not say formats = {FORMATS_4}"
            ));
        }
        compiled.clear();
    }
    // A language compiled is read from it: its text beside it is for an
    // older PaperPaw.
    let stem = |name: &str| {
        name.rsplit_once('.')
            .map_or(name, |(stem, _)| stem)
            .to_ascii_lowercase()
    };
    files.retain(|file| {
        !compiled
            .iter()
            .any(|path| stem(path.rsplit('/').next().unwrap_or(path)) == stem(file))
    });
    let code = paperpaw_sdk::native::CODE_FILES
        .iter()
        .any(|file| names.has(file) && storage.size(&format!("{path}/{file}")).is_ok());
    let translations = catalogs::read_all(
        storage,
        path,
        &catalogs,
        files,
        code,
        &mut compiled,
        &mut budget,
    );
    let mut now = |name: &str| {
        names
            .has(name)
            .then(|| budget.read_now(storage, &format!("{path}/{name}")))
            .flatten()
    };
    let (keymap, styles) = (now("keymap.ini"), now("styles.css"));
    let mut later: Vec<String> = LATER
        .iter()
        .filter(|(name, _)| names.has(name))
        .map(|(name, format3)| (format!("{path}/{name}"), *format3))
        .filter(|(file, format3)| budget.allow(storage, file, *format3))
        .map(|(file, _)| file)
        .collect();
    if names.has("points") {
        later.extend(point_files(storage, path, &mut budget));
    }
    let roles = fonts_for_roles(&source, path, &mut budget);
    let tables = hyphenation_tables(&source, path, code, &mut budget);
    let (family, faces) = family(&source, path);
    let mut fonts = match names.has("fonts") {
        true => font_files(storage, &format!("{path}/fonts"), false),
        false => Vec::new(),
    };
    // A family's faces are not fonts of their own as well.
    fonts.retain(|file| !faces.iter().any(|(_, face)| face == file));
    Ok(DataPlugin {
        manifest,
        folder: path.to_string(),
        translations: RefCell::new(translations),
        catalog_files: RefCell::new(compiled),
        keymap: RefCell::new(keymap),
        styles: RefCell::new(styles),
        storage: Some(shared.clone()),
        later: RefCell::new(later),
        roles: RefCell::new(roles),
        problems: RefCell::new(budget.problems),
        fonts: RefCell::new(fonts),
        extensions: RefCell::new(extensions(&source, path)),
        faces: RefCell::new(faces),
        family,
        loose_faces: RefCell::new(Vec::new()),
        hyphenation: RefCell::new(tables),
        code,
    })
}

/// The hyphenation tables `[hyphenation]` in `source` names, for a data
/// plugin: a code plugin's come with its registration, not from plugin.ini.
fn hyphenation_tables(
    source: &str,
    path: &str,
    code: bool,
    budget: &mut Budget,
) -> Vec<hyphenation::Table> {
    let mut tables = hyphenation::tables(source, path, budget.formats, &mut budget.problems);
    if code && !tables.is_empty() {
        budget.problems.push(format!(
            "{path}/plugin.ini: [hyphenation] left out, read only for data plugins"
        ));
        tables.clear();
    }
    tables
}

/// The font files in `dir` and its folders (one level, e.g. a folder per
/// family), by name: OpenType/TrueType files, and font packs if `packs`
/// (a plugin's packs are named in its `plugin.ini` instead).
fn font_files(storage: &dyn Storage, dir: &str, packs: bool) -> Vec<String> {
    let mut files: Vec<String> = Vec::new();
    let mut folders: Vec<String> = Vec::new();
    list_fonts(storage, dir, packs, &mut files, Some(&mut folders));
    for folder in &folders {
        list_fonts(storage, folder, packs, &mut files, None);
    }
    paperpaw_sort::sort(&mut files);
    files
}

/// Adds the font files in `dir` to `files` and its folders to `folders`.
fn list_fonts(
    storage: &dyn Storage,
    dir: &str,
    packs: bool,
    files: &mut Vec<String>,
    mut folders: Option<&mut Vec<String>>,
) {
    let _ = storage.list(dir, &mut |entry| {
        let path = format!("{dir}/{}", entry.name);
        if entry.is_dir {
            if let Some(folders) = folders.as_deref_mut()
                && !entry.name.starts_with('.')
            {
                folders.push(path);
            }
        } else if entry.name.starts_with('.') {
            // Hidden: another system's notes on a file (`._Literata.ttf`).
        } else if ["ttf", "otf", "ttc"]
            .iter()
            .any(|ext| paperpaw_sdk::files::has_extension(entry.name, ext))
            || (packs && paperpaw_sdk::files::has_extension(entry.name, "ppfp"))
        {
            files.push(path);
        }
    });
}

/// The fonts of `dirs` as one plugin, `font-folder`; `None` without any.
/// OpenType/TrueType files (where scalable fonts are drawn) give their
/// family and style themselves; font packs (`.ppfp`, no `plugin.ini`
/// needed) are faces of the family their file name says ([`pack_face`]).
pub(crate) fn font_folder(storage: &dyn Storage, dirs: &[String]) -> Option<DataPlugin> {
    let (packs, fonts): (Vec<String>, Vec<String>) = dirs
        .iter()
        .flat_map(|d| font_files(storage, d, true))
        .partition(|path| paperpaw_sdk::files::has_extension(path, "ppfp"));
    // Without scalable fonts, OpenType/TrueType files are of no use.
    let fonts = match cfg!(feature = "outline") {
        true => fonts,
        false => Vec::new(),
    };
    if fonts.is_empty() && packs.is_empty() {
        return None;
    }
    let loose = packs
        .into_iter()
        .map(|path| {
            let file = path.rsplit('/').next().unwrap_or(&path);
            let (family, key) = pack_face(file);
            (family, key, path)
        })
        .collect();
    Some(DataPlugin {
        manifest: Manifest::new(
            Id::new_static("font-folder"),
            "Font folder",
            Version::new(0, 1, 0),
        )
        .description("The fonts in the fonts folder, to read with.")
        .uses(paperpaw_sdk::Uses::NOTHING),
        // Folders the shell names; not removed by PaperPaw.
        folder: String::new(),
        translations: RefCell::new(Vec::new()),
        catalog_files: RefCell::new(Vec::new()),
        keymap: RefCell::new(None),
        styles: RefCell::new(None),
        storage: None,
        later: RefCell::new(Vec::new()),
        roles: RefCell::new(Vec::new()),
        problems: RefCell::new(Vec::new()),
        fonts: RefCell::new(fonts),
        extensions: RefCell::new(Vec::new()),
        faces: RefCell::new(Vec::new()),
        family: None,
        loose_faces: RefCell::new(loose),
        hyphenation: RefCell::new(Vec::new()),
        code: false,
    })
}

/// The family and face (a key of [`FACES`]) a font pack's file name
/// gives: the style after the last `-`, `_` or space (`Regular`, `Bold`,
/// `Italic` or `Oblique`, `BoldItalic`, `Bold-Italic` and the like, in
/// any case), the family before it, e.g. `Literata-BoldItalic.ppfp` the
/// bold italic face of Literata; a name without a style is the regular
/// face of a family of that name.
fn pack_face(file: &str) -> (String, &'static str) {
    const STYLES: [(&str, &str); 6] = [
        ("bolditalic", "bold-italic"),
        ("boldoblique", "bold-italic"),
        ("italic", "italic"),
        ("oblique", "italic"),
        ("bold", "bold"),
        ("regular", "regular"),
    ];
    let stem = file.rsplit_once('.').map_or(file, |(stem, _)| stem).trim();
    let squeezed: String = stem
        .chars()
        .filter(|c| !matches!(c, '-' | '_' | ' '))
        .flat_map(char::to_lowercase)
        .collect();
    for (style, key) in STYLES {
        if !squeezed.ends_with(style) {
            continue;
        }
        // Where the style starts in the name, separators and all: as
        // many letters from the end as it has.
        let mut letters = style.len();
        let mut start = stem.len();
        for (at, c) in stem.char_indices().rev() {
            if letters == 0 {
                break;
            }
            if !matches!(c, '-' | '_' | ' ') {
                letters -= c.len_utf8().min(letters);
            }
            start = at;
        }
        let family = stem[..start].trim_end_matches(['-', '_', ' ']);
        // A style needs a family before it, apart from it.
        if !family.is_empty() && stem[family.len()..].starts_with(['-', '_', ' ']) {
            return (String::from(family), key);
        }
    }
    (String::from(stem), "regular")
}

/// The packs `[extends]` in a `plugin.ini` names: their paths under the
/// plugin's `fonts` folder and the fonts they extend. A pack may extend
/// several, listed with commas (formats 5): it is opened once for all.
fn extensions(source: &str, path: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for entry in paperpaw_config::parse(source).filter_map(Result::ok) {
        if entry.section != "extends" {
            continue;
        }
        for font in entry.value.as_str().split(',').map(str::trim) {
            if !font.is_empty() {
                out.push((format!("{path}/fonts/{}", entry.key), font.to_string()));
            }
        }
    }
    out
}

/// The family `[family]` in a `plugin.ini` names, with its faces' keys and
/// paths under the plugin's `fonts` folder; no faces without a name. A
/// plugin may bring faces of a family without its regular one, e.g. the
/// bold italic face of a family built in that leaves it out (the
/// firmware's Literata), bound to its role with `[fonts]`.
fn family(source: &str, path: &str) -> (Option<String>, Vec<(&'static str, String)>) {
    let mut name = None;
    let mut faces = Vec::new();
    for entry in paperpaw_config::parse(source).filter_map(Result::ok) {
        if entry.section != "family" {
            continue;
        }
        let value = entry.value.as_str().trim().to_string();
        if entry.key == "name" {
            name = Some(value);
        } else if let Some((key, ..)) = FACES.iter().find(|(k, ..)| *k == entry.key)
            && !value.is_empty()
            && !value.contains("..")
        {
            faces.push((*key, format!("{path}/fonts/{value}")));
        }
    }
    match name.filter(|n| !n.is_empty()) {
        Some(name) if !faces.is_empty() => (Some(name), faces),
        _ => (None, Vec::new()),
    }
}

/// An id segment from a file name: `Book Antiqua-Bold.ttf` gives
/// `book-antiqua-bold`.
fn slug(file: &str) -> String {
    let stem = file.rsplit_once('.').map_or(file, |(stem, _)| stem);
    let mut out = String::new();
    for c in stem.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = String::from(out.trim_end_matches('-'));
    if out.is_empty() {
        String::from("font")
    } else {
        out
    }
}

fn read(storage: &dyn Storage, path: &str, max: usize) -> Option<String> {
    let data = paperpaw_sdk::read_file(storage, path, max).ok()?;
    String::from_utf8(data).ok()
}

/// The `[plugin]` section of a `plugin.ini`, and the formats it is made
/// for. The id defaults to the folder's name; the version to 0.1.0, the
/// formats to 1.
fn parse_manifest(source: &str, folder: &str) -> Result<(Manifest, u32), String> {
    let (mut id, mut name, mut description, mut version) = (None, None, None, None);
    let (mut formats, mut requires) = (None, None);
    for entry in paperpaw_config::parse(source) {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.section != "plugin" {
            continue;
        }
        let value = entry.value.as_str().into_owned();
        match entry.key {
            "id" => id = Some(value),
            "name" => name = Some(value),
            "description" => description = Some(value),
            "version" => version = Some(value),
            "formats" => formats = Some(value),
            "requires" => requires = Some(value),
            _ => {}
        }
    }
    let mut made_for = 1;
    if let Some(text) = formats {
        made_for = text
            .trim()
            .parse()
            .map_err(|_| format!("invalid formats `{text}`"))?;
        if made_for > FORMATS {
            return Err(format!(
                "made for a later PaperPaw (formats {made_for}; this one reads {FORMATS})"
            ));
        }
    }
    let id = id.unwrap_or_else(|| folder.to_string());
    let id = Id::parse(&id)
        .ok()
        .filter(|id| id.as_str() == id.namespace())
        .ok_or_else(|| format!("invalid id `{id}`"))?;
    // Records name plugins in 64 bytes (installers keep their folder names to
    // 40, a card folder may be longer); `core` is the host's (ui.md 4.3).
    if id.as_str().len() > 64 || id.as_str() == crate::RESERVED_PLUGIN_ID {
        return Err(format!("invalid id `{id}`: over 64 bytes or reserved"));
    }
    let version = match version {
        Some(text) => {
            Version::try_parse(&text).map_err(|why| format!("invalid version `{text}`: {why}"))?
        }
        None => Version::new(0, 1, 0),
    };
    // Plugins it needs (the store fetches them): named as said, in
    // formats 4 and later.
    if let Some(text) = requires {
        if made_for < 4 {
            return Err(String::from("`requires` needs formats 4"));
        }
        paperpaw_sdk::requires::parse(&text, id.as_str())?;
    }
    let name = name.unwrap_or_else(|| id.as_str().to_string());
    let manifest = Manifest::new(id, name, version)
        .description(description.unwrap_or_default())
        .uses(paperpaw_sdk::Uses::NOTHING);
    Ok((manifest, made_for))
}

#[cfg(test)]
mod tests;
