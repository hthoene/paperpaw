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
//!                  `Registrar::contribute`): keyboard layouts, About lines
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

use paperpaw_platform::Storage;
use paperpaw_sdk::FontFace;
use paperpaw_sdk::presets::Preset;
use paperpaw_sdk::{Id, Manifest, Plugin, RegisterError, Registrar, Version};

mod formats;
pub(crate) mod hyphenation;
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

impl Plugin for DataPlugin {
    fn manifest(&self) -> Manifest {
        self.manifest.clone()
    }

    #[expect(
        clippy::too_many_lines,
        reason = "longer than the limit from before it was set; split it when it is next changed"
    )]
    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
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
        for (path, extends) in self.extensions.take() {
            let name = slug(path.rsplit('/').next().unwrap_or(&path));
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
        let id = &self.manifest.id;
        if let Some(source) = self.read_later("actions.ini") {
            for action in actions(id, &source)? {
                r.add_action(action.spec)?;
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
        // The points' files, one at a time, at most `MAX_POINT_LINES`
        // lines in all.
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
        for (role, font) in self.roles.take() {
            let (role, font) = (parsed_id(&role, 0)?, parsed_id(&font, 0)?);
            r.bind_font(role, font, paperpaw_sdk::Priority::DEFAULT)?;
        }
        if let Some(source) = self.read_later("marks.ini") {
            let marks = paperpaw_sdk::decorations::MarkRules::parse(&self.manifest.id, &source)?;
            for (role, title, look) in marks.roles {
                paperpaw_sdk::decorations::add_role(r, &role, title, look)?;
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
    names.sort_unstable();
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
    names.sort_unstable();
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
    let mut translations = Vec::new();
    let catalogs = format!("{path}/i18n");
    let mut files: Vec<String> = Vec::new();
    let mut compiled: Vec<String> = Vec::new();
    let _ = storage.list(&catalogs, &mut |entry| {
        // `DE.INI` too: a file copied from a memory card may have it so.
        if !entry.is_dir && paperpaw_sdk::files::has_extension(entry.name, "ini") {
            files.push(entry.name.to_string());
        }
        // Read as needed, so not counted (ADR 0033).
        if !entry.is_dir && paperpaw_sdk::files::has_extension(entry.name, "ppcat") {
            compiled.push(format!("{catalogs}/{}", entry.name));
        }
    });
    files.sort_unstable();
    compiled.sort_unstable();
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
    let mut translations_left = MAX_TRANSLATIONS;
    for file in files {
        let path = format!("{catalogs}/{file}");
        let size = storage
            .size(&path)
            .map_or(usize::MAX, |s| usize::try_from(s).unwrap_or(usize::MAX));
        if size > translations_left.min(MAX_FILE) {
            budget
                .problems
                .push(format!("{path}: left out, more than the plugin may take"));
            continue;
        }
        if let Some(text) = read(storage, &path, MAX_FILE) {
            translations_left = translations_left.saturating_sub(text.len());
            translations.push(text);
        }
    }
    let keymap = budget.read_now(storage, &format!("{path}/keymap.ini"));
    let styles = budget.read_now(storage, &format!("{path}/styles.css"));
    let mut later: Vec<String> = LATER
        .iter()
        .map(|(name, format3)| (format!("{path}/{name}"), *format3))
        .filter(|(file, format3)| budget.allow(storage, file, *format3))
        .map(|(file, _)| file)
        .collect();
    later.extend(point_files(storage, path, &mut budget));
    let roles = fonts_for_roles(&source, path, &mut budget);
    let code = paperpaw_sdk::native::CODE_FILES
        .iter()
        .any(|file| storage.size(&format!("{path}/{file}")).is_ok());
    let mut tables = hyphenation::tables(&source, path, budget.formats, &mut budget.problems);
    // A code plugin's tables come with its registration, not from plugin.ini.
    if code && !tables.is_empty() {
        budget.problems.push(format!(
            "{path}/plugin.ini: [hyphenation] left out, read only for data plugins"
        ));
        tables.clear();
    }
    let (family, faces) = family(&source, path);
    let mut fonts = font_files(storage, &format!("{path}/fonts"), false);
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
    files.sort_unstable();
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
/// plugin's `fonts` folder and the fonts they extend.
fn extensions(source: &str, path: &str) -> Vec<(String, String)> {
    paperpaw_config::parse(source)
        .filter_map(Result::ok)
        .filter(|entry| entry.section == "extends")
        .map(|entry| {
            (
                format!("{path}/fonts/{}", entry.key),
                entry.value.as_str().trim().to_string(),
            )
        })
        .collect()
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
mod tests {
    use super::*;
    use paperpaw_sdk::MemoryStorage;

    fn shared(storage: MemoryStorage) -> Rc<dyn Storage> {
        Rc::new(storage)
    }

    /// Every data plugin in `dir` that loads, the others reported.
    fn discover(
        storage: &Rc<dyn Storage>,
        dir: &str,
        warn: &mut dyn FnMut(String),
    ) -> Vec<DataPlugin> {
        folders(storage.as_ref(), dir, warn)
            .iter()
            .filter_map(|path| load_folder(storage, path).map_err(&mut *warn).ok())
            .collect()
    }

    #[test]
    fn plugins_switched_off_keep_only_what_lists_them() {
        let source = "[meta]\nlanguage = de\nname = Deutsch\n\n[messages]\nbig.plugin.name = Groß\nbig.plugin.description = Größer.\nreader.title = Lesen\n";
        assert_eq!(
            listing(source, "big.plugin."),
            "[meta]\nlanguage = de\nname = Deutsch\n\n[messages]\nbig.plugin.name = Groß\nbig.plugin.description = Größer.\n"
        );
    }

    #[test]
    fn finds_folders_with_a_manifest() {
        let storage = MemoryStorage::new()
            .with(
                "plugins/large/plugin.ini",
                "[plugin]\nname = Large print\ndescription = Bigger text.\nversion = 1.2.0\n",
            )
            .with("plugins/large/settings.ini", "[core]\nui-size = 14\n")
            .with("plugins/large/styles.css", "p { line-height: 1.6 }")
            .with("plugins/large/i18n/de.ini", "[meta]\nlanguage = de\n")
            .with("plugins/large/i18n/FR.INI", "[meta]\nlanguage = fr\n")
            .with("plugins/broken/readme.txt", "")
            .with("plugins/bad-id/plugin.ini", "[plugin]\nid = a.b\n");
        let mut warnings = Vec::new();
        let storage = shared(storage);
        let plugins = discover(&storage, "plugins", &mut |w| warnings.push(w));
        assert_eq!(plugins.len(), 1);
        let large = &plugins[0];
        assert_eq!(large.manifest.id.as_str(), "large");
        assert_eq!(large.manifest.name, "Large print");
        assert_eq!(large.manifest.version, Version::new(1, 2, 0));
        assert_eq!(large.translations.borrow().len(), 2, "any case");
        assert!(
            large
                .later
                .borrow()
                .iter()
                .any(|p| p.ends_with("settings.ini"))
        );
        assert!(large.keymap.borrow().is_none());
        assert_eq!(
            large.styles.borrow().as_deref(),
            Some("p { line-height: 1.6 }")
        );
        assert_eq!(warnings.len(), 2, "{warnings:?}");
    }

    #[test]
    fn a_pre_release_a_version_does_not_hold_is_refused_saying_so() {
        let storage = MemoryStorage::new()
            .with(
                "plugins/a/plugin.ini",
                "[plugin]
name = A
version = 1.0.0-beta.2
",
            )
            .with(
                "plugins/b/plugin.ini",
                "[plugin]
name = B
version = 1.0.0-gamma
",
            );
        let mut warnings = Vec::new();
        let storage = shared(storage);
        let plugins = discover(&storage, "plugins", &mut |w| warnings.push(w));
        assert_eq!(plugins.len(), 1, "{warnings:?}");
        assert_eq!(plugins[0].manifest.version.pre_name(), Some("beta"));
        assert_eq!(warnings.len(), 1);
        assert!(
            warnings[0].contains("invalid version `1.0.0-gamma`")
                && warnings[0].contains("alpha, beta, dev, pre or rc"),
            "{warnings:?}"
        );
    }

    #[test]
    fn a_packs_file_name_gives_its_family_and_face() {
        let of = |family: &str, key| (String::from(family), key);
        assert_eq!(
            pack_face("Literata-BoldItalic.ppfp"),
            of("Literata", "bold-italic")
        );
        assert_eq!(
            pack_face("Literata-Bold-Italic.ppfp"),
            of("Literata", "bold-italic")
        );
        assert_eq!(
            pack_face("Noto Serif Bold Italic.ppfp"),
            of("Noto Serif", "bold-italic")
        );
        assert_eq!(
            pack_face("noto_serif_italic.PPFP"),
            of("noto_serif", "italic")
        );
        assert_eq!(pack_face("Fredoka-Oblique.ppfp"), of("Fredoka", "italic"));
        assert_eq!(
            pack_face("Fredoka-BoldOblique.ppfp"),
            of("Fredoka", "bold-italic")
        );
        assert_eq!(pack_face("Fredoka-Bold.ppfp"), of("Fredoka", "bold"));
        assert_eq!(pack_face("Fredoka-Regular.ppfp"), of("Fredoka", "regular"));
        // No style: the regular face; a style alone or run into the
        // name is no style.
        assert_eq!(pack_face("Bookerly.ppfp"), of("Bookerly", "regular"));
        assert_eq!(pack_face("Bold.ppfp"), of("Bold", "regular"));
        assert_eq!(pack_face("Seabold.ppfp"), of("Seabold", "regular"));
        assert_eq!(pack_face("Ä-Bold.ppfp"), of("Ä", "bold"));
        assert_eq!(pack_face("ÄİBold.ppfp"), of("ÄİBold", "regular"));
    }

    #[test]
    fn packs_loose_in_a_font_folder_are_faces_of_their_families() {
        let storage = shared(
            MemoryStorage::new()
                .with("fonts/Literata-Regular.ppfp", "x")
                .with("fonts/Literata-Bold.ppfp", "x")
                .with("fonts/greek/Literata-Italic.ppfp", "x")
                .with("fonts/Bookerly.ppfp", "x")
                .with("other/Bookerly.ppfp", "x")
                .with("fonts/notes.txt", "")
                .with("fonts/._Literata-Bold.ppfp", "x"),
        );
        let dirs = [String::from("fonts"), String::from("other")];
        let folder = font_folder(storage.as_ref(), &dirs).unwrap();
        assert!(folder.fonts.borrow().is_empty());
        let mut r = Registrar::new(Id::new_static("font-folder"));
        folder.register(&mut r).unwrap();
        let faces: Vec<(String, String, bool, bool)> = r
            .contributions()
            .iter()
            .filter_map(|c| match c {
                paperpaw_sdk::Contribution::FontFace(face) => Some((
                    face.id.to_string(),
                    String::from(face.family.as_ref()),
                    face.bold,
                    face.italic,
                )),
                _ => None,
            })
            .collect();
        let face = |id: &str, family: &str, bold, italic| {
            (String::from(id), String::from(family), bold, italic)
        };
        assert_eq!(
            faces,
            [
                face("font-folder.bookerly-regular", "Bookerly", false, false),
                face("font-folder.literata-bold", "Literata", true, false),
                face("font-folder.literata-regular", "Literata", false, false),
                face("font-folder.literata-italic", "Literata", false, true),
                // The same name in another folder: kept apart.
                face("font-folder.bookerly-regular-2", "Bookerly", false, false),
            ]
        );
        // A plugin's own packs are named in its plugin.ini: not fonts.
        let plugin = shared(
            MemoryStorage::new()
                .with("plugins/greek/plugin.ini", "[plugin]\n")
                .with("plugins/greek/fonts/literata.ppfp", "x"),
        );
        let found = discover(&plugin, "plugins", &mut |_| {});
        assert!(found[0].fonts.borrow().is_empty());
    }

    #[test]
    fn finds_fonts_in_plugins_and_font_folders() {
        let storage = MemoryStorage::new()
            .with(
                "plugins/serif/plugin.ini",
                "[plugin]
",
            )
            .with("plugins/serif/fonts/Book Antiqua-Bold.TTF", "x")
            .with("plugins/serif/fonts/readme.txt", "")
            .with("fonts/a.otf", "x")
            .with("more-fonts/b.ttc", "x")
            .with("more-fonts/family/c.ttf", "x");
        let storage = shared(storage);
        let plugins = discover(&storage, "plugins", &mut |_| {});
        assert_eq!(
            *plugins[0].fonts.borrow(),
            ["plugins/serif/fonts/Book Antiqua-Bold.TTF"]
        );
        let dirs = [String::from("fonts"), String::from("more-fonts")];
        // Scalable fonts only where they are drawn.
        let folder = font_folder(storage.as_ref(), &dirs);
        match cfg!(feature = "outline") {
            true => assert_eq!(
                *folder.unwrap().fonts.borrow(),
                ["fonts/a.otf", "more-fonts/b.ttc", "more-fonts/family/c.ttf"]
            ),
            false => assert!(folder.is_none()),
        }
        assert!(font_folder(storage.as_ref(), &[String::from("nothing")]).is_none());
        assert_eq!(slug("Book Antiqua-Bold.TTF"), "book-antiqua-bold");
        // Packs that extend other fonts, named in plugin.ini.
        let source = "[plugin]
id = cyrillic
[extends]
literata.ppfp = fonts-default.literata
";
        assert_eq!(
            extensions(source, "plugins/cyrillic"),
            [(
                String::from("plugins/cyrillic/fonts/literata.ppfp"),
                String::from("fonts-default.literata")
            )]
        );
        assert_eq!(slug("….ttf"), "font");
    }

    #[test]
    fn families_name_their_faces() {
        let storage = MemoryStorage::new()
            .with(
                "plugins/font-round/plugin.ini",
                "[plugin]
name = Round
[family]
name = Round
regular = Round.ttf
bold = round-bold.ppfp
oblique = nothing.ppfp
italic = ../escape.ppfp
",
            )
            .with("plugins/font-round/fonts/Round.ttf", "x")
            .with("plugins/font-round/fonts/Extra.ttf", "x");
        let plugin = load_folder(&shared(storage), "plugins/font-round/").unwrap();
        assert_eq!(plugin.folder(), "plugins/font-round");
        assert_eq!(plugin.family.as_deref(), Some("Round"));
        assert_eq!(
            *plugin.faces.borrow(),
            [
                (
                    "regular",
                    String::from("plugins/font-round/fonts/Round.ttf")
                ),
                (
                    "bold",
                    String::from("plugins/font-round/fonts/round-bold.ppfp")
                ),
            ]
        );
        // A face is no font of its own as well.
        assert_eq!(
            *plugin.fonts.borrow(),
            ["plugins/font-round/fonts/Extra.ttf"]
        );
        // A face of a family without its regular face: e.g. a bold
        // italic face the firmware leaves out.
        assert_eq!(
            family(
                "[family]
bold-italic = a.ppfp
name = Literata
",
                "p"
            ),
            (
                Some(String::from("Literata")),
                alloc::vec![("bold-italic", String::from("p/fonts/a.ppfp"))]
            )
        );
        // No family without a name, nor without a face.
        assert_eq!(
            family(
                "[family]
name = A
",
                "p"
            ),
            (None, Vec::new())
        );
        assert_eq!(
            family(
                "[family]
regular = a.ppfp
",
                "p"
            ),
            (None, Vec::new())
        );
    }

    #[test]
    fn presets_are_read_a_section_each() {
        let source = "# Layouts
[nook]
kind = home.layout
name = Reading nook
                      description = The book, large.
home.page-1 = home.continue 0/0 4x3
                      home.rows = 4
[plain]
home.status-bar = off
";
        let presets = presets(&Id::new_static("cosy"), source).unwrap();
        assert_eq!(presets.len(), 2);
        let nook = &presets[0];
        assert_eq!(nook.id.as_str(), "cosy.nook");
        assert_eq!(nook.kind.as_str(), "home.layout");
        assert_eq!(nook.title, "Reading nook");
        assert_eq!(nook.value("home.page-1"), Some("home.continue 0/0 4x3"));
        assert_eq!(nook.value("home.rows"), Some("4"));
        // Without a kind, the plugin's own.
        assert_eq!(presets[1].kind.as_str(), "cosy");
        assert!(
            super::presets(
                &Id::new_static("cosy"),
                "[a]
kind = Not An Id
"
            )
            .is_err()
        );
    }

    #[test]
    fn plugins_made_for_later_formats_are_left_out() {
        let ini = |formats: &str| format!("[plugin]\nname = Large print\n{formats}");
        assert!(parse_manifest(&ini(""), "large").is_ok());
        assert!(parse_manifest(&ini("formats = 1\n"), "large").is_ok());
        assert!(parse_manifest(&ini("formats = 2\n"), "large").is_ok());
        assert_eq!(parse_manifest(&ini(""), "large").unwrap().1, 1);
        assert!(parse_manifest(&ini("formats = 3\n"), "large").is_ok());
        assert!(parse_manifest(&ini("formats = 4\n"), "large").is_ok());
        let later = parse_manifest(&ini("formats = 5\n"), "large").unwrap_err();
        assert!(later.contains("later PaperPaw"), "{later}");
        assert!(parse_manifest(&ini("formats = new\n"), "large").is_err());
    }

    /// `requires` names plugins as said, in formats 4 and later; else the
    /// plugin is not loaded, with why.
    #[test]
    fn requirements_are_checked_with_the_manifest() {
        let ini = |rest: &str| format!("[plugin]\nname = Cosy\n{rest}");
        assert!(
            parse_manifest(&ini("formats = 4\nrequires = stats 1.0.0, news\n"), "cosy").is_ok()
        );
        let old = parse_manifest(&ini("formats = 3\nrequires = stats\n"), "cosy").unwrap_err();
        assert!(old.contains("formats 4"), "{old}");
        let own = parse_manifest(&ini("formats = 4\nrequires = cosy\n"), "cosy").unwrap_err();
        assert!(own.contains("itself"), "{own}");
        assert!(parse_manifest(&ini("formats = 4\nrequires = Not Ok\n"), "cosy").is_err());
    }

    /// Fonts bound to roles in `[fonts]` of `plugin.ini`.
    #[test]
    fn fonts_are_bound_to_roles() {
        assert_eq!(
            roles(
                "[plugin]
id = x
[fonts]
core.font-ui = x.semi
"
            ),
            [(String::from("core.font-ui"), String::from("x.semi"))]
        );
    }

    /// Files of formats 3 are read only for a plugin made for them; they
    /// are held within a budget, and lines for points within a limit.
    #[test]
    fn files_of_formats_3_are_read_within_limits() {
        let older = MemoryStorage::new()
            .with(
                "plugins/old/plugin.ini",
                "[plugin]\nformats = 2\n[fonts]\ncore.font-ui = old.x\n",
            )
            .with("plugins/old/menus.ini", "[old.m]\ntitle = M\n");
        let plugin = load_folder(&shared(older), "plugins/old").unwrap();
        assert!(plugin.later.borrow().is_empty());
        assert!(plugin.roles.borrow().is_empty());
        assert_eq!(plugin.take_problems().len(), 2);
        let lines: String = (0..MAX_POINT_LINES + 5)
            .map(|n| format!("line {n}\n"))
            .collect();
        let big = alloc::vec![b'#'; MAX_FORMAT3_DATA + 1];
        let newer = MemoryStorage::new()
            .with("plugins/new/plugin.ini", "[plugin]\nformats = 3\n")
            .with("plugins/new/points/core.about.txt", lines.as_str())
            .with("plugins/new/actions.ini", big);
        let plugin = load_folder(&shared(newer), "plugins/new").unwrap();
        let later = plugin.later.borrow().clone();
        assert_eq!(later, ["plugins/new/points/core.about.txt"], "too large");
        assert_eq!(plugin.take_problems().len(), 1);
        let mut r = Registrar::new(Id::new_static("new"));
        plugin.register(&mut r).unwrap();
        let lines = r
            .contributions()
            .iter()
            .filter(|c| c.kind() == "to-point")
            .count();
        assert_eq!(lines, MAX_POINT_LINES);
        // All files of a plugin together: within one budget.
        let file = alloc::vec![b'#'; MAX_FILE - 1];
        let many = MemoryStorage::new()
            .with("plugins/many/plugin.ini", "[plugin]\n")
            .with("plugins/many/keymap.ini", file.clone())
            .with("plugins/many/settings.ini", file.clone())
            .with("plugins/many/presets.ini", file);
        let plugin = load_folder(&shared(many), "plugins/many").unwrap();
        assert!(plugin.keymap.borrow().is_some());
        assert_eq!(plugin.later.borrow().len(), 0);
        assert_eq!(plugin.take_problems().len(), 2);
    }
}
