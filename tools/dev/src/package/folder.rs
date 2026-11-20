//! A data plugin folder looked at as files: what the installer, the
//! kernel's loader and the plugin library take, without running anything.
//!
//! The rules come from three places, kept in step with them:
//!
//! - the SDK's `install` module, which the store and the fonts browser
//!   download with: `is_plugin_name`, `is_safe_path`, [`MAX_FILES`]
//!   and [`Package::is_valid`];
//! - the kernel's loader (`crates/paperpaw-core/src/data_plugin.rs`):
//!   which files it reads, how large they may be, `[extends]` and
//!   `[family]` in `plugin.ini`;
//! - the plugin library at paperpaw.org (its `packages.rs` and
//!   `catalogue.rs`): the kinds of files a package may hold, its size,
//!   and an id and a version named in `plugin.ini` that its catalogue
//!   entry repeats.

use paperpaw_core::kit::install::{self, Digest, MAX_FILES, Package, PackageFile};
use paperpaw_sdk::{DATA_FORMATS, Id, Version};

use crate::report::{Finding, Report};
use crate::target::DataPlugin;

/// Largest data file the kernel reads (a catalog, a key map, the
/// settings): larger ones it leaves out.
pub const MAX_DATA_FILE: u64 = 64 * 1024;
/// Largest `plugin.ini` the kernel reads.
pub const MAX_MANIFEST: u64 = 4 * 1024;
/// The plugin library's rules (`tools/publish/store-rules.json`, the
/// site's own): the kinds of files it takes in a package (font packs,
/// `ppfp`, only in `fonts/`, hyphenation tables, `pphp`, only in `hyphenation/`, the root certificates of `tls-roots`, `roots.pack` only in the folder, as the kernel reads them), the most a package
/// holds unpacked, the longest id, the groups.
fn rules() -> &'static paperpaw_publish::Rules {
    static RULES: std::sync::OnceLock<paperpaw_publish::Rules> = std::sync::OnceLock::new();
    RULES.get_or_init(paperpaw_publish::Rules::built_in)
}

/// The keys of `[plugin]` the kernel reads.
const PLUGIN_KEYS: [&str; 6] = [
    "id",
    "name",
    "description",
    "version",
    "formats",
    "requires",
];
/// The faces of a family, as `[family]` names them.
const FACES: [&str; 4] = ["regular", "bold", "italic", "bold-italic"];
/// Files at the top of the folder that the kernel reads.
const TOP_FILES: [&str; 10] = [
    "plugin.ini",
    "settings.ini",
    "keymap.ini",
    "styles.css",
    "presets.ini",
    "marks.ini",
    "menus.ini",
    "actions.ini",
    "icons.ini",
    "setup.ini",
];

/// The files the kernel reads only for a plugin made for a later version
/// of the data formats (`formats` in `plugin.ini`), with that version
/// (`crates/paperpaw-core/src/data_plugin.rs`): `marks.ini` came with 2,
/// the others with 3. `points/*.txt` came with 3 too.
const LATER_FORMATS: [(&str, u32); 5] = [
    ("marks.ini", 2),
    ("menus.ini", 3),
    ("actions.ini", 3),
    ("icons.ini", 3),
    ("setup.ini", 3),
];

/// The version of the data formats that brought `points/*.txt`.
const POINTS_FORMATS: u32 = 3;

/// The version of the data formats that brought `i18n/*.ppcat` (ADR 0033).
const CATALOG_FORMATS: u32 = 4;

/// The version of the data formats that brought `[fonts]` in `plugin.ini`.
const FONTS_FORMATS: u32 = 3;

/// The most steps an action of `actions.ini` runs (`MAX_STEPS` in the
/// kernel's `data_plugin.rs`).
const MAX_STEPS: usize = 16;

/// One file of the folder.
#[derive(Debug, Clone)]
pub struct File {
    /// Its path in the folder, with `/`.
    pub path: String,
    pub size: u64,
    pub bytes: std::rc::Rc<[u8]>,
}

/// A line of an ini file: section, key, value and line number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub section: String,
    pub key: String,
    pub value: String,
    pub line: u32,
}

/// What `plugin.ini` says.
#[derive(Debug, Clone, Default)]
pub struct Manifest {
    pub id: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub version: Option<String>,
    /// `formats`, where given and a number.
    pub formats: Option<u32>,
    /// `requires` (data formats 4): the plugins it needs, as written.
    pub requires: Option<String>,
    /// `[extends]`: a pack in `fonts/` and the font it extends.
    pub extends: Vec<Line>,
    /// `[family]`: its name and faces.
    pub family: Vec<Line>,
    /// `[fonts]`: roles bound to fonts (data formats 3).
    pub fonts: Vec<Line>,
}

/// A data plugin folder as read.
#[derive(Debug, Clone, Default)]
pub struct Folder {
    pub files: Vec<File>,
    pub manifest: Manifest,
    /// `settings.ini`, line by line.
    pub settings: Vec<Line>,
    /// `presets.ini`, line by line.
    pub presets: Vec<Line>,
    /// `keymap.ini`, line by line.
    pub keymap: Vec<Line>,
    pub has_presets: bool,
}

impl Folder {
    pub fn file(&self, path: &str) -> Option<&File> {
        self.files.iter().find(|f| f.path == path)
    }

    /// The id it is installed as: `plugin.ini`'s, else the folder's name.
    pub fn id<'a>(&'a self, plugin: &'a DataPlugin) -> &'a str {
        self.manifest.id.as_deref().unwrap_or(&plugin.id)
    }
}

/// Checks `plugin`'s folder into `report`; what was read, unless the
/// folder could not be read at all.
pub fn check(plugin: &DataPlugin, report: &mut Report) -> Option<Folder> {
    let id = plugin.id.clone();
    let mut add = |finding: Finding| report.add(finding.plugin(id.clone()));
    // Every file, hidden ones too (the installer and the website see
    // them), by path.
    let files = plugin
        .files
        .iter()
        .map(|(path, bytes)| File {
            path: path.clone(),
            size: bytes.len() as u64,
            bytes: bytes.clone(),
        })
        .collect();
    let mut folder = Folder {
        files,
        ..Folder::default()
    };
    manifest(plugin, &mut folder, &mut add);
    folder.settings = lines(&folder, "settings.ini", &mut add);
    folder.presets = lines(&folder, "presets.ini", &mut add);
    folder.keymap = lines(&folder, "keymap.ini", &mut add);
    folder.has_presets = folder.file("presets.ini").is_some();
    later_formats(&folder, &mut add);
    actions(&folder, folder.id(plugin), &mut add);
    menus(&folder, &mut add);
    files_and_kinds(plugin, &folder, &mut add);
    catalogs(&folder, &mut add);
    fonts(&folder, &mut add);
    pictures(plugin, &folder, &mut add);
    conventions::check(&folder, &mut add);
    package(plugin, &folder, &mut add);
    store_entry(&folder, &mut add);
    Some(folder)
}

/// `store/store.ini`, what only the store reads: its `group` one of the
/// store's, as the plugin library refuses others.
fn store_entry(folder: &Folder, add: &mut dyn FnMut(Finding)) {
    if folder.file("store/store.ini").is_none() {
        return;
    }
    for line in lines(folder, "store/store.ini", add) {
        if line.section == "store"
            && line.key == "group"
            && !rules().groups.iter().any(|g| g == line.value.trim())
        {
            add(Finding::error(
                "package.store-group",
                format!("store/store.ini:{}", line.line),
                format!(
                    "`{}` is no group of the store; the plugin library refuses it. One of: {}.",
                    line.value.trim(),
                    rules().groups.join(", ")
                ),
            ));
        }
    }
}

/// The extension of `path`, lower-case.
fn extension(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.')
        .map(|(_, x)| x.to_ascii_lowercase())
        .unwrap_or_default()
}

/// A text file the kernel reads: present, small enough, UTF-8.
fn text(folder: &Folder, path: &str, max: u64, add: &mut dyn FnMut(Finding)) -> Option<String> {
    let file = folder.file(path)?;
    if file.size > max {
        add(Finding::error(
            "package.files",
            path,
            format!(
                "{} KB, but PaperPaw reads such files only up to {} KB and leaves this one out; make it smaller or split it.",
                file.size.div_ceil(1024),
                max / 1024
            ),
        ));
        return None;
    }
    match std::str::from_utf8(&file.bytes) {
        Ok(text) => Some(text.to_string()),
        Err(_) => {
            add(Finding::error(
                "package.files",
                path,
                "Not UTF-8 text, so PaperPaw leaves it out; save it as UTF-8.",
            ));
            None
        }
    }
}

/// An ini file's lines; syntax errors reported.
fn lines(folder: &Folder, path: &str, add: &mut dyn FnMut(Finding)) -> Vec<Line> {
    let Some(source) = text(folder, path, MAX_DATA_FILE, add) else {
        return Vec::new();
    };
    parse(&source, path, add)
}

fn parse(source: &str, path: &str, add: &mut dyn FnMut(Finding)) -> Vec<Line> {
    let mut out = Vec::new();
    for entry in paperpaw_config::parse(source) {
        match entry {
            Ok(entry) => out.push(Line {
                section: entry.section.to_string(),
                key: entry.key.to_string(),
                value: entry.value.as_str().into_owned(),
                line: entry.line,
            }),
            Err(error) => add(Finding::error(
                "package.syntax",
                format!("{path}:{}", error.line),
                format!(
                    "{error}; PaperPaw refuses the file (and with plugin.ini or settings.ini the whole plugin)."
                ),
            )),
        }
    }
    out
}

/// The files of later data formats: readable INI, and read at all only
/// where `plugin.ini` says a version of the formats that has them (the
/// kernel leaves them out otherwise, and says so).
fn later_formats(folder: &Folder, add: &mut dyn FnMut(Finding)) {
    let says = folder.manifest.formats.unwrap_or(1);
    let points = folder
        .files
        .iter()
        .filter(|f| f.path.starts_with("points/") && f.path.ends_with(".txt"))
        .map(|f| (f.path.as_str(), POINTS_FORMATS));
    let compiled = folder
        .files
        .iter()
        .filter(|f| f.path.starts_with("i18n/") && f.path.ends_with(".ppcat"))
        .map(|f| (f.path.as_str(), CATALOG_FORMATS));
    let files = LATER_FORMATS
        .iter()
        .filter(|(path, _)| folder.file(path).is_some())
        .map(|&(path, formats)| (path, formats))
        .chain(points)
        .chain(compiled);
    if let Some(line) = folder.manifest.fonts.first()
        && says < FONTS_FORMATS
    {
        add(Finding::error(
            "package.formats",
            format!("plugin.ini:{}", line.line),
            format!(
                "PaperPaw reads [fonts] only for a plugin made for data formats {FONTS_FORMATS} or \
                 later, and plugin.ini says {says}: it is left out. Write `formats = \
                 {FONTS_FORMATS}` in [plugin]."
            ),
        ));
    }
    for (path, needs) in files {
        if path.ends_with(".ini") {
            let _ = lines(folder, path, add);
        }
        if says < needs {
            add(Finding::error(
                "package.formats",
                path,
                format!(
                    "PaperPaw reads this file only for a plugin made for data formats {needs} or \
                     later, and plugin.ini says {says}: it is left out. Write `formats = \
                     {needs}` in [plugin]."
                ),
            ));
        }
    }
}

/// Whether `value` of a `run` line reads as several steps separated by
/// commas, as `run` was first written: each part a step (an action, a
/// setting and a value, `open:`, `emit:`). A step a line now, a comma
/// part of it.
fn several_steps(value: &str) -> bool {
    let step = |part: &str| {
        let part = part.trim();
        let id = part.split_once('=').map_or(part, |(id, _)| id).trim();
        ["notify:", "open:", "emit:"]
            .iter()
            .any(|p| part.starts_with(p))
            || (!id.is_empty()
                && id.contains('.')
                && id
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-@".contains(&b)))
    };
    !value.trim_start().starts_with("notify:")
        && value.matches(',').count() >= 1
        && value.split(',').all(step)
}

/// The keys of the plugin's translations, all languages together.
fn message_keys(folder: &Folder) -> std::collections::BTreeSet<String> {
    let mut keys = std::collections::BTreeSet::new();
    for file in &folder.files {
        let path = file.path.as_str();
        if !(path.starts_with("i18n/") && path.ends_with(".ini") && path.matches('/').count() == 1)
        {
            continue;
        }
        let source = String::from_utf8_lossy(&file.bytes).into_owned();
        if let Ok(catalog) = paperpaw_i18n::Catalog::parse(source) {
            keys.extend(catalog.keys().map(String::from));
        }
    }
    keys
}

/// `actions.ini` as the kernel takes it, or refuses it and the plugin
/// with it: a step a `run` line, at most [`MAX_STEPS`] steps an action,
/// none running an action of the same file, events emitted only of the
/// plugin's own; and what it would show other than meant: steps written
/// on one line separated by commas (one step now, the commas part of
/// it), a notification of no translation key.
fn actions(folder: &Folder, plugin: &str, add: &mut dyn FnMut(Finding)) {
    if folder.file("actions.ini").is_none() {
        return;
    }
    // Its syntax is told already (`later_formats`).
    let lines = lines(folder, "actions.ini", &mut |_| {});
    // As the kernel names them (`own_id`): the section, in the plugin's
    // namespace.
    let own = |name: &str| {
        if name.starts_with(&format!("{plugin}.")) {
            name.to_string()
        } else {
            format!("{plugin}.{name}")
        }
    };
    let mut found: Vec<(String, u32, Vec<String>)> = Vec::new();
    for line in lines.iter().filter(|l| !l.section.is_empty()) {
        let id = own(&line.section);
        if found.last().is_none_or(|(last, ..)| *last != id) {
            found.push((id, line.line, Vec::new()));
        }
        if line.key != "run" || line.value.trim().is_empty() {
            continue;
        }
        let place = format!("actions.ini:{}", line.line);
        if several_steps(&line.value) {
            add(Finding::warning(
                "package.actions",
                &place,
                format!(
                    "`run = {}` is one step, its commas part of it: PaperPaw takes a step a `run` line. \
                     Write each step on a `run` line of its own.",
                    line.value.trim()
                ),
            ));
        }
        if let Some((.., steps)) = found.last_mut() {
            steps.push(line.value.trim().to_string());
        }
    }
    let keys = message_keys(folder);
    for line in lines.iter().filter(|l| l.key == "run") {
        let Some(key) = line.value.trim().strip_prefix("notify:").map(str::trim) else {
            continue;
        };
        if !keys.contains(key) {
            add(Finding::warning(
                "package.actions",
                format!("actions.ini:{}", line.line),
                format!(
                    "`notify:{key}` names no text of the plugin's translations, so the reader shows \
                     `{key}` as it is: `notify:` takes a translation key, e.g. \
                     `notify:{plugin}.<name>` with `{plugin}.<name> = ...` in i18n/en.ini."
                ),
            ));
        }
    }
    let ids: Vec<&str> = found.iter().map(|(id, ..)| id.as_str()).collect();
    let refused = "PaperPaw refuses actions.ini, and with it the whole plugin, which the plugin manager then shows as Error";
    for (id, line, steps) in &found {
        let place = format!("actions.ini:{line}");
        if steps.len() > MAX_STEPS {
            add(Finding::error(
                "package.actions",
                &place,
                format!(
                    "`{id}` runs {} steps, at most {MAX_STEPS}: {refused}. Fewer steps fit \
                     (settings chosen together can be a preset of presets.ini).",
                    steps.len()
                ),
            ));
        }
        for step in steps {
            let ran = ["notify:", "open:", "emit:"]
                .iter()
                .all(|p| !step.starts_with(p))
                && !step.contains('=');
            if ran && ids.contains(&step.as_str()) {
                add(Finding::error(
                    "package.actions",
                    &place,
                    format!(
                        "`{id}` runs `{step}`, an action of this same file: {refused}. Put \
                         its steps into `{id}` instead."
                    ),
                ));
            }
            let event = step.strip_prefix("emit:").map(str::trim);
            if let Some(event) = event.filter(|e| e.split('.').next() != Some(plugin)) {
                add(Finding::error(
                    "package.actions",
                    &place,
                    format!(
                        "`{id}` emits `{event}`, an event of another plugin: {refused}. An \
                         action emits only events of its own plugin (`{plugin}.<name>`)."
                    ),
                ));
            }
        }
    }
}

/// `menus.ini` as the kernel reads it: another plugin's menu as
/// `[menu <id>]` (with `add`, `hide`), a menu of one's own as any other
/// section (with `title`, `entries`).
fn menus(folder: &Folder, add: &mut dyn FnMut(Finding)) {
    if folder.file("menus.ini").is_none() {
        return;
    }
    // Its syntax is told already (`later_formats`).
    for line in lines(folder, "menus.ini", &mut |_| {}) {
        let place = format!("menus.ini:{}", line.line);
        let foreign = line.section.strip_prefix("menu ");
        match (line.key.as_str(), foreign) {
            ("title" | "entries", Some(menu)) => add(Finding::error(
                "package.menus",
                &place,
                format!(
                    "`{}` makes a menu of one's own, but [menu {menu}] is another plugin's menu: \
                     PaperPaw refuses menus.ini, and with it the whole plugin. Put it in a \
                     section of the menu's own name, e.g. [tools].",
                    line.key
                ),
            )),
            ("add" | "hide", None) if !line.section.is_empty() => add(Finding::warning(
                "package.menus",
                &place,
                format!(
                    "`{}` changes another menu: write its section as [menu {}].",
                    line.key, line.section
                ),
            )),
            _ => {}
        }
    }
}

/// `requires`: formats 4, and plugin ids as the kernel reads them (the
/// kernel refuses a plugin whose `requires` it cannot read).
fn requires(manifest: &Manifest, place: &str, add: &mut dyn FnMut(Finding)) {
    let Some(text) = &manifest.requires else {
        return;
    };
    if manifest.formats.is_none_or(|formats| formats < 4) {
        add(Finding::error(
            "package.requires",
            place,
            "`requires` needs `formats = 4` (or later): PaperPaw refuses the plugin otherwise.",
        ));
    }
    let own = manifest.id.as_deref().unwrap_or_default();
    if let Err(why) = paperpaw_sdk::requires::parse(text, own) {
        add(Finding::error(
            "package.requires",
            place,
            format!("`requires = {text}` is refused: {why}."),
        ));
    }
}

/// `plugin.ini`: present, readable, its `[plugin]` section as the kernel
/// and the plugin library want it.
#[expect(
    clippy::too_many_lines,
    reason = "longer than the limit from before it was set; split it when it is next changed"
)]
fn manifest(plugin: &DataPlugin, folder: &mut Folder, add: &mut dyn FnMut(Finding)) {
    let place = "plugin.ini";
    if folder.file(place).is_none() {
        add(Finding::error(
            "package.manifest",
            place,
            "No plugin.ini: neither the device nor the plugin library takes the folder as a plugin.",
        ));
        return;
    }
    let Some(source) = text(folder, place, MAX_MANIFEST, add) else {
        return;
    };
    let lines = parse(&source, place, add);
    let manifest = &mut folder.manifest;
    for line in &lines {
        let at = format!("{place}:{}", line.line);
        match line.section.as_str() {
            "plugin" => {
                let value = Some(line.value.trim().to_string());
                match line.key.as_str() {
                    "id" => manifest.id = value,
                    "name" => manifest.name = value,
                    "description" => manifest.description = value,
                    "version" => manifest.version = value,
                    "requires" => manifest.requires = value,
                    "formats" => match line.value.trim().parse::<u32>() {
                        Ok(n) => manifest.formats = Some(n),
                        Err(_) => add(Finding::error(
                            "package.formats",
                            at,
                            format!(
                                "`formats = {}` is no number, so PaperPaw refuses the plugin; write `formats = {DATA_FORMATS}`.",
                                line.value
                            ),
                        )),
                    },
                    other => add(Finding::warning(
                        "package.manifest",
                        at,
                        format!(
                            "`{other}` is not read in [plugin] (only {}); a typo?",
                            PLUGIN_KEYS.join(", ")
                        ),
                    )),
                }
            }
            "extends" => manifest.extends.push(line.clone()),
            "family" => manifest.family.push(line.clone()),
            "fonts" => manifest.fonts.push(line.clone()),
            "" => add(Finding::warning(
                "package.manifest",
                at,
                format!(
                    "`{}` is outside any section and not read; put it under [plugin].",
                    line.key
                ),
            )),
            other => add(Finding::warning(
                "package.manifest",
                at,
                format!(
                    "[{other}] is not read in plugin.ini (only [plugin], [extends], [family] and [fonts])."
                ),
            )),
        }
    }
    let manifest = &folder.manifest;
    match &manifest.id {
        None => add(Finding::error(
            "package.id",
            place,
            format!(
                "plugin.ini names no id: the device takes the folder's name, but the plugin library needs `id = {}` in [plugin].",
                plugin.id
            ),
        )),
        Some(id) => {
            if !install::is_plugin_name(id) {
                add(Finding::error(
                    "package.id",
                    place,
                    format!(
                        "`{id}` is no plugin id: lower-case letters, digits and `-`, at most 40, not starting with `-`."
                    ),
                ));
            } else if id.ends_with('-') || id.len() > rules().max_id {
                add(Finding::error(
                    "package.id",
                    place,
                    format!(
                        "The plugin library takes no id like `{id}`: at most {} characters, not ending with `-`.",
                        rules().max_id
                    ),
                ));
            }
            let folder_name = plugin
                .folder
                .as_ref()
                .and_then(|folder| folder.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if !folder_name.is_empty() && &folder_name != id {
                add(Finding::info(
                    "package.id",
                    place,
                    format!(
                        "The folder is named `{folder_name}`, the id is `{id}`: the installer puts it in a folder `{id}`."
                    ),
                ));
            }
        }
    }
    requires(manifest, place, add);
    match &manifest.version {
        None => add(Finding::error(
            "package.version",
            place,
            "plugin.ini names no version: the device takes 0.1.0, but the plugin library needs `version = 1.0.0` (or so) in [plugin], the same as its catalogue entry.",
        )),
        Some(text) if Version::try_parse(text).is_err() => {
            let why = Version::try_parse(text).err().map(|why| why.to_string());
            add(Finding::error(
                "package.version",
                place,
                format!(
                    "`version = {text}` is no version, so PaperPaw refuses the plugin: {}; write it like 1.0.0.",
                    why.unwrap_or_default()
                ),
            ));
        }
        // Three numbers, a pre-release after them (1.2.0-beta.1) or not,
        // no build metadata: as the library orders them.
        Some(text)
            if text.contains('+')
                || text
                    .split('-')
                    .next()
                    .unwrap_or_default()
                    .split('.')
                    .count()
                    != 3 =>
        {
            add(Finding::error(
                "package.version",
                place,
                format!(
                    "The plugin library takes versions like 1.2.0 or 1.2.0-beta.1 only, not `{text}`."
                ),
            ));
        }
        Some(_) => {}
    }
    if manifest.name.as_deref().is_none_or(str::is_empty) {
        add(Finding::warning(
            "package.manifest",
            place,
            "No name in [plugin]: the plugin manager shows its id instead.",
        ));
    }
    if manifest.description.as_deref().is_none_or(str::is_empty) {
        add(Finding::warning(
            "package.manifest",
            place,
            "No description in [plugin]: the plugin manager shows nothing under its name.",
        ));
    }
    match manifest.formats {
        Some(n) if n > DATA_FORMATS => add(Finding::error(
            "package.formats",
            place,
            format!(
                "Made for data formats {n}, but this PaperPaw reads {DATA_FORMATS} and leaves the plugin out."
            ),
        )),
        Some(0) => add(Finding::warning(
            "package.formats",
            place,
            format!(
                "`formats = 0` is no data format; the first is 1, write `formats = {DATA_FORMATS}`."
            ),
        )),
        Some(_) => {}
        None if lines
            .iter()
            .any(|l| l.section == "plugin" && l.key == "formats") => {}
        None => add(Finding::info(
            "package.formats",
            place,
            format!("No `formats`: read as 1; `formats = {DATA_FORMATS}` says it outright."),
        )),
    }
    if !folder.manifest.family.is_empty() {
        family(folder, add);
    }
    for line in &folder.manifest.extends {
        let at = format!("{place}:{}", line.line);
        let path = format!("fonts/{}", line.key);
        if !install::is_safe_path(&path) || line.key.contains('/') {
            add(Finding::error(
                "package.fonts",
                at.clone(),
                format!(
                    "`{}` is no file name in fonts/: name a pack there, e.g. `cyrillic.ppfp = fonts-default.literata`.",
                    line.key
                ),
            ));
        } else if folder.file(&path).is_none() {
            add(Finding::error(
                "package.fonts",
                at.clone(),
                format!("[extends] names {path}, which is not in the folder."),
            ));
        }
        if Id::parse(line.value.trim()).is_err() {
            add(Finding::error(
                "package.fonts",
                at,
                format!(
                    "`{}` is no font id, so PaperPaw refuses the plugin; name one such as fonts-default.literata.",
                    line.value
                ),
            ));
        }
    }
}

/// `[family]`: a name and a regular face, faces that are files.
fn family(folder: &Folder, add: &mut dyn FnMut(Finding)) {
    let place = "plugin.ini";
    let lines = &folder.manifest.family;
    let name = lines
        .iter()
        .find(|l| l.key == "name")
        .is_some_and(|l| !l.value.trim().is_empty());
    let has = |face: &str| {
        lines
            .iter()
            .any(|l| l.key == face && !l.value.trim().is_empty())
    };
    let any_face = FACES.iter().any(|face| has(face));
    if !name || !any_face {
        add(Finding::error(
            "package.fonts",
            place,
            "[family] needs a name and a face; without them PaperPaw leaves the family out.",
        ));
    } else if !has("regular") && folder.manifest.fonts.is_empty() {
        // A family built in may lack a face (the firmware's Literata has
        // no bold italic): a plugin brings it and binds it with [fonts].
        add(Finding::warning(
            "package.fonts",
            place,
            "[family] has no regular face and [fonts] binds none to a role: the faces are listed, but nothing uses them.",
        ));
    }
    for line in lines {
        let at = format!("{place}:{}", line.line);
        if line.key == "name" {
            continue;
        }
        if !FACES.contains(&line.key.as_str()) {
            add(Finding::warning(
                "package.fonts",
                at,
                format!(
                    "`{}` is no face of a family (only {}); it is not read.",
                    line.key,
                    FACES.join(", ")
                ),
            ));
            continue;
        }
        let value = line.value.trim();
        let path = format!("fonts/{value}");
        if value.contains("..") || !install::is_safe_path(&path) {
            add(Finding::error(
                "package.fonts",
                at,
                format!("`{value}` is no file in fonts/; PaperPaw does not read it."),
            ));
        } else if folder.file(&path).is_none() {
            add(Finding::error(
                "package.fonts",
                at,
                format!(
                    "The {} face is {path}, which is not in the folder.",
                    line.key
                ),
            ));
        }
    }
}

/// Whether the kernel reads `path` (and as what), given `plugin.ini`.
fn read_as(folder: &Folder, path: &str, id: &str) -> Option<&'static str> {
    if TOP_FILES.contains(&path) {
        return Some("data");
    }
    let parts: Vec<&str> = path.split('/').collect();
    let ext = extension(path);
    if parts.len() == 2 && parts[0] == "i18n" && path.ends_with(".ini") {
        return Some("catalog");
    }
    if parts.len() == 2 && parts[0] == "i18n" && ext == "ppcat" {
        return Some("catalog");
    }
    if path == "roots.pack" {
        return Some("root certificates");
    }
    if parts.len() == 2 && parts[0] == "points" && ext == "txt" {
        return Some("points");
    }
    if parts[0] == "fonts" {
        let named = folder
            .manifest
            .extends
            .iter()
            .any(|l| format!("fonts/{}", l.key) == path)
            || folder
                .manifest
                .family
                .iter()
                .any(|l| l.key != "name" && format!("fonts/{}", l.value.trim()) == path);
        if named {
            return Some("font");
        }
        let scalable = ["ttf", "otf", "ttc"].contains(&ext.as_str());
        let shallow = parts.len() == 2 || (parts.len() == 3 && !parts[1].starts_with('.'));
        if scalable && shallow {
            return Some("font");
        }
    }
    // A picture settings.ini names, or one in a folder it names (the
    // sleep screen's pictures).
    let own = format!("plugins/{id}/{path}");
    let picture = ["png", "jpg", "jpeg", "gif"].contains(&ext.as_str());
    let named_by_settings = folder.settings.iter().any(|l| {
        let value = l.value.trim().trim_end_matches('/');
        value == own || (picture && own.starts_with(&format!("{value}/")))
    });
    if named_by_settings {
        return Some("picture");
    }
    None
}

/// Whether `path` is a folder of `folder` (a file is in it).
fn is_folder(folder: &Folder, path: &str) -> bool {
    let inside = format!("{}/", path.trim_end_matches('/'));
    folder.files.iter().any(|f| f.path.starts_with(&inside))
}

/// Every file: a path the installer takes, a kind the website takes, one
/// the kernel reads; their count and size.
fn files_and_kinds(plugin: &DataPlugin, folder: &Folder, add: &mut dyn FnMut(Finding)) {
    let id = folder.id(plugin).to_string();
    for file in &folder.files {
        let path = file.path.as_str();
        if !install::is_safe_path(path) {
            let hidden = path.split('/').any(|p| p.starts_with('.'));
            add(Finding::error(
                "package.files",
                path,
                if hidden {
                    "A hidden file: the installer takes no names starting with `.`; remove it from the folder.".to_string()
                } else {
                    "The installer takes only paths of letters, digits, `-`, `_` and `.` (at most 100 characters); rename it.".to_string()
                },
            ));
        }
        let ext = extension(path);
        if !rules().package_kinds.contains(&ext) {
            add(Finding::error(
                "package.website",
                path,
                format!(
                    "The plugin library at paperpaw.org takes only {} files in a data plugin.",
                    rules().package_kinds.join(", ")
                ),
            ));
        } else if ext == "ppfp" && !path.starts_with("fonts/") {
            add(Finding::error(
                "package.website",
                path,
                "Font packs (.ppfp) belong in fonts/, where the kernel reads them.",
            ));
        }
        if ext == "pack" && path != "roots.pack" {
            add(Finding::error(
                "package.website",
                path,
                "Only roots.pack (.pack), directly in the plugin's folder, is read, as the root certificates of `tls-roots`.",
            ));
        }
        if ext == "pphp" && !path.starts_with("hyphenation/") {
            add(Finding::error(
                "package.website",
                path,
                "Hyphenation tables (.pphp) belong in hyphenation/, where the kernel reads them.",
            ));
        }
        if is_test(path) {
            add(Finding::warning(
                "package.tests",
                path,
                "A test of the plugin (a flow or its pictures) would ship in the package; keep flows and their snapshots in a folder beside the plugin's.",
            ));
        } else if read_as(folder, path, &id).is_none() && !["txt", "md"].contains(&ext.as_str()) {
            let hint = match ext.as_str() {
                "ini" => {
                    " (it reads plugin.ini, settings.ini, keymap.ini, presets.ini, marks.ini, \
                     menus.ini, actions.ini, icons.ini, setup.ini and i18n/*.ini and *.ppcat)"
                }
                "css" => " (it reads styles.css)",
                "ppfp" => " (packs are read where [extends] or [family] in plugin.ini names them)",
                "ttf" | "otf" | "ttc" => " (fonts are read in fonts/ and one folder below it)",
                "png" => {
                    " (a picture is read where settings.ini names it, e.g. `logo = plugins/<id>/logo.png`)"
                }
                _ => "",
            };
            add(Finding::warning(
                "package.files",
                path,
                format!("PaperPaw does not read this file{hint}; a typo in its name or place?"),
            ));
        }
    }
    if folder.files.len() > MAX_FILES {
        add(Finding::error(
            "package.files",
            "",
            format!(
                "{} files, but the installer takes at most {MAX_FILES}; fewer files (e.g. fewer catalogs or fonts) fit.",
                folder.files.len()
            ),
        ));
    }
    let total: u64 = folder.files.iter().map(|f| f.size).sum();
    if total > rules().max_content {
        add(Finding::error(
            "package.website",
            "",
            format!(
                "{} MB unpacked, but the plugin library takes at most {} MB.",
                total / (1024 * 1024),
                rules().max_content / (1024 * 1024)
            ),
        ));
    }
    let _ = text(folder, "styles.css", MAX_DATA_FILE, add);
}

/// Whether `path` is one of the plugin's tests (see `flows`): a flow, or
/// a file in a `flows`, `tests` or `snapshots` folder.
fn is_test(path: &str) -> bool {
    extension(path) == "flow"
        || path
            .split('/')
            .rev()
            .skip(1)
            .any(|part| matches!(part, "flows" | "tests" | "snapshots"))
}

/// Translation catalogs: read by the kernel and the website alike.
fn catalogs(folder: &Folder, add: &mut dyn FnMut(Finding)) {
    for file in &folder.files {
        let path = file.path.as_str();
        if !(path.starts_with("i18n/") && path.ends_with(".ini") && path.matches('/').count() == 1)
        {
            continue;
        }
        let Some(source) = text(folder, path, MAX_DATA_FILE, add) else {
            continue;
        };
        if let Err(error) = paperpaw_i18n::Catalog::parse(source) {
            add(Finding::error(
                "package.i18n",
                path,
                format!(
                    "{error}: PaperPaw does not load the plugin, and the plugin library refuses it."
                ),
            ));
        }
    }
}

/// Font files and packs are what they say they are.
fn fonts(folder: &Folder, add: &mut dyn FnMut(Finding)) {
    let mut scalable = false;
    let mut packs = false;
    for file in &folder.files {
        let ext = extension(&file.path);
        let expected: &[&[u8]] = match ext.as_str() {
            "ttf" => &[b"\x00\x01\x00\x00", b"true", b"OTTO"],
            "otf" => &[b"OTTO", b"\x00\x01\x00\x00"],
            "ttc" => &[b"ttcf"],
            "ppfp" => &[b"PPFP", b"QIFP"],
            _ => continue,
        };
        if ext == "ppfp" {
            packs = true;
        } else {
            scalable = true;
        }
        if !expected.iter().any(|magic| file.bytes.starts_with(magic)) {
            add(Finding::error(
                "package.fonts",
                file.path.as_str(),
                format!("Not a .{ext} file inside, so PaperPaw cannot draw with it."),
            ));
        }
    }
    if scalable && !packs {
        add(Finding::info(
            "package.fonts",
            "fonts/",
            "Font files (.ttf, .otf, .ttc) are drawn where scalable fonts are (Android, the emulator), not on the X4, which draws packs (.ppfp).",
        ));
    }
}

/// Pictures are PNG, and those settings name are there.
fn pictures(plugin: &DataPlugin, folder: &Folder, add: &mut dyn FnMut(Finding)) {
    let id = folder.id(plugin);
    for file in &folder.files {
        if extension(&file.path) == "png" && !file.bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            add(Finding::error(
                "package.pictures",
                file.path.as_str(),
                "Not a PNG picture inside; save it as PNG.",
            ));
        }
    }
    let own = format!("plugins/{id}/");
    for line in &folder.settings {
        let value = line.value.trim();
        let Some(path) = value.strip_prefix("plugins/") else {
            continue;
        };
        let at = format!("settings.ini:{}", line.line);
        match value.strip_prefix(&own) {
            Some(path) if folder.file(path).is_none() && !is_folder(folder, path) => {
                add(Finding::error(
                    "package.pictures",
                    at,
                    format!("`{}` names {path}, which is not in the folder.", line.key),
                ))
            }
            Some(_) => {}
            None => add(Finding::warning(
                "package.pictures",
                at,
                format!(
                    "`{}` names plugins/{path}, outside this plugin's own folder ({own}); installed, it is `{own}…`.",
                    line.key
                ),
            )),
        }
    }
}

/// The installer's own check of the package the store would download,
/// and its size.
fn package(plugin: &DataPlugin, folder: &Folder, add: &mut dyn FnMut(Finding)) {
    let id = folder.id(plugin).to_string();
    let files: Vec<PackageFile> = folder
        .files
        .iter()
        .map(|file| {
            PackageFile::new(
                file.path.clone(),
                format!(
                    "https://paperpaw.org/api/v1/plugins/{id}/1.0.0/{}",
                    file.path
                ),
                file.size,
                Digest::Sha1(paperpaw_digest::sha1::hex(&file.bytes)),
            )
        })
        .collect();
    let package = Package::new(
        id.clone(),
        folder.manifest.name.clone().unwrap_or_else(|| id.clone()),
    )
    .files(files);
    if !package.is_valid() {
        add(Finding::error(
            "package.files",
            "",
            "The SDK's installer refuses this package (see the findings above: id, paths, file count, plugin.ini).",
        ));
    }
    add(Finding::info(
        "package.size",
        "",
        format!(
            "{} files, {} to download.",
            package.files.len(),
            size_text(package.size())
        ),
    ));
}

/// `2.3 KB`, `512 bytes`, `4.1 MB`.
pub fn size_text(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} bytes")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

mod conventions;
#[cfg(test)]
mod tests;
