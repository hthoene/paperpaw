//! Which reading app opens a book (#260): the reading apps native plugins
//! declare (`reader.spec`, `docs/abi/document.md` section 1) chosen for a book,
//! else for the nearest folder it is in, else for its format (the setting
//! `core.reading-app.<format id>`), else the default one (`core.reading-app`),
//! else the screen bound to `core.reader`. A choice of an app that is gone, or
//! that does not read the book's format, is passed over silently.
//!
//! - **Where it is decided.** Every open of the reader slot that names a book
//!   (`command.open` with the hand-over param `path` and no `root`, and
//!   `Ask::OpenBook`): an open that names its screen (mode 3, *Open with…*)
//!   opens there.
//! - **Kept choices.** An open that carries the hand-over param
//!   [`KEEP`] (`book` or `folder`) keeps its screen's app for the book or its
//!   folder; one without a screen (mode 1, *Standard*) forgets the choice.
//!   They live in one small file ([`FILE`], at most [`LIMIT`] bytes, the
//!   oldest dropped first), read once at the start and written when a choice
//!   changes: nothing is read from the card when a book opens.
//! - **Hidden with one app.** With fewer than two reading apps nothing is
//!   resolved, the settings offer nothing to choose and are not listed
//!   ([`nothing_to_choose`]).
//! - **Passed on.** A reading app that cannot open its book after all says
//!   so (`command.pass_on`, #369 G): the book opens in the next app of the
//!   same order that reads it, then the slot's own screen, then any other
//!   app that reads its format, the apps tried left out (at most
//!   [`PASS_MAX`]); with none left the reader closes and a notice says so.
//!   Nothing of it is kept.
use crate::Kernel;
use crate::native_host::declaration_store::DeclarationStore;
use crate::native_host::format_index::{extensions_of, has_extension};
use alloc::borrow::Cow;
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::builtin::{params, settings as core_settings, slots};
use paperpaw_sdk::{Id, Params, SettingKind, SettingSpec, SettingValue, Settings, StorageError};

/// The kept choices: a line each, `b` (a book) or `f` (a folder), a tab, the
/// reading app's id, a tab, the path (without `/` at its ends).
pub(crate) const FILE: &str = paperpaw_sdk::data_path!("reading-apps.txt");

/// The most bytes of kept choices (about 60 books and folders).
pub(crate) const LIMIT: usize = 4 * 1024;

/// The hand-over param of an open that keeps its reading app: `book` or
/// `folder`.
pub(crate) const KEEP: &str = "keep";

/// The prefix of the per-format settings.
const PER_FORMAT: &str = "core.reading-app.";

/// The reading apps' part of the reading settings, keeping apps not installed
/// now.
static IN_APPS: paperpaw_sdk::SettingExtra = paperpaw_sdk::SettingExtra::new()
    .section("core.section.reading-apps")
    .keeps_unknown();

/// What a kept choice is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Keep {
    Book,
    Folder,
}

impl Keep {
    fn parse(text: &str) -> Option<Self> {
        match text {
            "book" => Some(Self::Book),
            "folder" => Some(Self::Folder),
            _ => None,
        }
    }

    const fn mark(self) -> &'static str {
        match self {
            Self::Book => "b",
            Self::Folder => "f",
        }
    }
}

/// The kept choices and the registry generation the settings were offered
/// at.
#[derive(Debug, Default)]
pub(crate) struct ReadingApps {
    /// The file's text as read (or last written).
    kept: String,
    /// The file could not be read whole: it is not overwritten.
    unreadable: bool,
    /// The registry generation of the last offer; 0 for none yet.
    synced: u32,
    /// The book the reader slot was opened with last.
    book: Option<String>,
    /// The reading apps (their screens) that passed `book` on.
    tried: Vec<Id>,
    /// The open in progress is a pass: `tried` stays.
    passing: bool,
}

/// The most reading apps one book is passed on from: a pass beyond it ends
/// with the notice, never a loop.
pub(crate) const PASS_MAX: usize = 4;

/// A path as choices are kept by: without `/` at its ends.
fn plain(path: &str) -> &str {
    path.trim_matches('/')
}

/// The folder of `path` (empty for the top of the card).
fn folder_of(path: &str) -> &str {
    plain(path)
        .rsplit_once('/')
        .map_or("", |(folder, _)| folder)
}

/// Whether `path` is in `folder` (at any depth), ASCII case ignored (FAT).
fn is_in(path: &str, folder: &str) -> bool {
    !folder.is_empty()
        && path.len() > folder.len()
        && path
            .get(..folder.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(folder))
        && path.as_bytes().get(folder.len()) == Some(&b'/')
}

/// The kept lines: (kind, reader, path).
fn lines(kept: &str) -> impl Iterator<Item = (&str, &str, &str)> {
    kept.lines().filter_map(|line| {
        let (kind, rest) = line.split_once('\t')?;
        let (reader, path) = rest.split_once('\t')?;
        (!path.is_empty()).then_some((kind, reader, path))
    })
}

/// The reader kept for the book at `path`, else for the nearest folder it is
/// in that `fits`.
fn kept_for<'k>(kept: &'k str, path: &str, fits: &dyn Fn(&str) -> bool) -> Option<&'k str> {
    let book = lines(kept)
        .find(|(kind, reader, at)| *kind == "b" && at.eq_ignore_ascii_case(path) && fits(reader))
        .map(|(_, reader, _)| reader);
    book.or_else(|| {
        lines(kept)
            .filter(|(kind, reader, at)| *kind == "f" && is_in(path, at) && fits(reader))
            .max_by_key(|(_, _, at)| at.len())
            .map(|(_, reader, _)| reader)
    })
}

/// `kept` with the choice for `keep` of `path` set to `reader` (`None`
/// forgets it), the oldest lines dropped where it would pass [`LIMIT`].
/// `None` where the heap has no room.
fn changed(kept: &str, keep: Keep, path: &str, reader: Option<&str>) -> Option<String> {
    let mut rest: Vec<&str> = Vec::new();
    rest.try_reserve_exact(kept.lines().count()).ok()?;
    rest.extend(kept.lines().filter(|line| {
        let mut parts = line.splitn(3, '\t');
        let (kind, at) = (parts.next(), parts.nth(1));
        !(kind == Some(keep.mark()) && at.is_some_and(|at| at.eq_ignore_ascii_case(path)))
    }));
    let new_line = reader.map(|reader| [keep.mark(), "\t", reader, "\t", path, "\n"]);
    let new_len: usize = new_line.iter().flatten().map(|part| part.len()).sum();
    let mut total: usize = rest.iter().map(|line| line.len() + 1).sum::<usize>() + new_len;
    let mut skip = 0;
    while total > LIMIT && skip < rest.len() {
        total -= rest[skip].len() + 1;
        skip += 1;
    }
    let mut text = String::new();
    text.try_reserve_exact(total).ok()?;
    for line in &rest[skip..] {
        text.push_str(line);
        text.push('\n');
    }
    for part in new_line.iter().flatten() {
        text.push_str(part);
    }
    Some(text)
}

/// The id of the format that reads `path` (`document.md` section 1): by the
/// longest extension; of its claimants the user's `core.formats.<extension>`,
/// else the higher priority, else the first loaded. Written into `out`.
fn format_of(store: &DeclarationStore, settings: &Settings, path: &str, out: &mut String) {
    let mut longest = 0;
    store.each_format(&mut |format| {
        for extension in extensions_of(format.spec.extensions) {
            if extension.len() > longest && has_extension(path, extension) {
                longest = extension.len();
            }
        }
    });
    if longest == 0 {
        return;
    }
    let extension = path.get(path.len() - longest..).unwrap_or_default();
    let mut key = String::new();
    if key.try_reserve_exact(13 + extension.len()).is_err() {
        return;
    }
    key.push_str("core.formats.");
    key.extend(extension.chars().map(|c| c.to_ascii_lowercase()));
    let wanted = settings.text(&key);
    let mut best: Option<(bool, i32)> = None;
    store.each_format(&mut |format| {
        let claims =
            extensions_of(format.spec.extensions).any(|e| e.eq_ignore_ascii_case(extension));
        if !claims {
            return;
        }
        let chosen = !wanted.is_empty() && is_id(wanted, format.plugin, format.spec.name);
        let better = best.is_none_or(|(was_chosen, priority)| {
            !was_chosen && (chosen || format.spec.priority > priority)
        });
        if better
            && out
                .try_reserve_exact(format.plugin.len() + 1 + format.spec.name.len())
                .is_ok()
        {
            out.clear();
            out.push_str(format.plugin);
            out.push('.');
            out.push_str(format.spec.name);
            best = Some((chosen, format.spec.priority));
        }
    });
}

/// Whether `id` is `<plugin>.<name>`.
fn is_id(id: &str, plugin: &str, name: &str) -> bool {
    id.strip_prefix(plugin)
        .and_then(|rest| rest.strip_prefix('.'))
        .is_some_and(|rest| rest == name)
}

/// Whether a reader reads `format` (it lists none: every format).
fn reads(formats: &str, format: &str) -> bool {
    formats.is_empty() || formats.split('\n').any(|f| f == format)
}

/// How many reading apps live plugins declare.
fn count(store: &DeclarationStore) -> usize {
    let mut readers = 0;
    store.each_reader(&mut |_| readers += 1);
    readers
}

/// The screen of the reading app `id` if it reads `format`.
fn screen_of(store: &DeclarationStore, id: &str, format: &str) -> Option<Id> {
    let mut found = None;
    store.each_reader(&mut |reader| {
        if found.is_none()
            && is_id(id, reader.plugin, reader.spec.name)
            && reads(reader.spec.formats, format)
        {
            found = Id::try_parse(reader.spec.screen).ok();
        }
    });
    found
}

/// The screen of the reading app chosen for the book at `path`, if any (see
/// the module documentation).
pub(crate) fn chosen(
    store: &DeclarationStore,
    settings: &Settings,
    kept: &str,
    path: &str,
) -> Option<Id> {
    if count(store) < 2 {
        return None;
    }
    let path = plain(path);
    let mut format = String::new();
    format_of(store, settings, path, &mut format);
    let fits = |id: &str| screen_of(store, id, &format).is_some();
    if let Some(reader) = kept_for(kept, path, &fits) {
        return screen_of(store, reader, &format);
    }
    let mut key = String::new();
    if !format.is_empty()
        && key
            .try_reserve_exact(PER_FORMAT.len() + format.len())
            .is_ok()
    {
        key.push_str(PER_FORMAT);
        key.push_str(&format);
        if let Some(screen) = screen_of(store, settings.text(&key), &format) {
            return Some(screen);
        }
    }
    screen_of(
        store,
        settings.text(core_settings::READING_APP.as_str()),
        &format,
    )
}

/// The screen of the next reading app for the book at `path` after the
/// screens in `tried`: the resolver's order (book, folder, format, default),
/// then the slot's own `bound` screen, then every app that reads the book's
/// format, as declared.
fn next_after(
    store: &DeclarationStore,
    settings: &Settings,
    (kept, path): (&str, &str),
    bound: Option<&Id>,
    tried: &[Id],
) -> Option<Id> {
    let path = plain(path);
    let mut format = String::new();
    format_of(store, settings, path, &mut format);
    let fresh = |screen: &Id| !tried.contains(screen);
    let app = |id: &str| screen_of(store, id, &format).filter(|screen| fresh(screen));
    let fits = |id: &str| app(id).is_some();
    if let Some(screen) = kept_for(kept, path, &fits).and_then(app) {
        return Some(screen);
    }
    let mut key = String::new();
    if !format.is_empty()
        && key
            .try_reserve_exact(PER_FORMAT.len() + format.len())
            .is_ok()
    {
        key.push_str(PER_FORMAT);
        key.push_str(&format);
        if let Some(screen) = app(settings.text(&key)) {
            return Some(screen);
        }
    }
    if let Some(screen) = app(settings.text(core_settings::READING_APP.as_str())) {
        return Some(screen);
    }
    if let Some(screen) = bound.filter(|screen| fresh(screen)) {
        return screen.try_clone();
    }
    let mut found = None;
    store.each_reader(&mut |reader| {
        if found.is_none() && reads(reader.spec.formats, &format) {
            found = Id::try_parse(reader.spec.screen)
                .ok()
                .filter(|screen| fresh(screen));
        }
    });
    found
}

/// `n` in decimal, into `out`.
fn decimal(n: i32, out: &mut [u8; 12]) -> &str {
    let mut at = out.len();
    let mut rest = n.unsigned_abs();
    loop {
        at -= 1;
        out[at] = b'0' + (rest % 10) as u8;
        rest /= 10;
        if rest == 0 {
            break;
        }
    }
    if n < 0 {
        at -= 1;
        out[at] = b'-';
    }
    core::str::from_utf8(&out[at..]).unwrap_or_default()
}

/// Whether `spec` is a reading-app setting with nothing to choose (fewer than
/// two apps read its books): not listed in the settings.
pub(crate) fn nothing_to_choose(spec: &SettingSpec) -> bool {
    let id = spec.id.as_str();
    (id == core_settings::READING_APP.as_str() || id.starts_with(PER_FORMAT))
        && matches!(&spec.kind, SettingKind::Choice(choices) if choices.len() < 2)
}

/// A copy of `text`, or `None` where the heap has no room.
fn owned(text: &str) -> Option<Cow<'static, str>> {
    let mut copy = String::new();
    copy.try_reserve_exact(text.len()).ok()?;
    copy.push_str(text);
    Some(Cow::Owned(copy))
}

/// The choices of a reading-app setting: *Standard*, then the apps that read
/// `format` (`None`: every app) where two or more do.
fn choices_for(store: &DeclarationStore, format: Option<&str>) -> Option<paperpaw_sdk::Choices> {
    static ONLY_DEFAULT: [&str; 1] = [core_settings::READING_APP_DEFAULT];
    let fits = |formats: &str| format.is_none_or(|format| reads(formats, format));
    let mut readers = 0;
    store.each_reader(&mut |reader| readers += usize::from(fits(reader.spec.formats)));
    if readers < 2 {
        return Some(paperpaw_sdk::Choices::Fixed(&ONLY_DEFAULT));
    }
    let mut list: Vec<Cow<'static, str>> = Vec::new();
    list.try_reserve_exact(readers + 1).ok()?;
    list.push(Cow::Borrowed(core_settings::READING_APP_DEFAULT));
    let mut failed = false;
    store.each_reader(&mut |reader| {
        if !fits(reader.spec.formats) || list.len() == list.capacity() {
            return;
        }
        let mut id = String::new();
        if id
            .try_reserve_exact(reader.plugin.len() + 1 + reader.spec.name.len())
            .is_err()
        {
            failed = true;
            return;
        }
        id.push_str(reader.plugin);
        id.push('.');
        id.push_str(reader.spec.name);
        list.push(Cow::Owned(id));
    });
    (!failed).then_some(paperpaw_sdk::Choices::Owned(list))
}

/// The per-format settings to offer: (setting id, format title key, choices).
type Offered = Vec<(String, Cow<'static, str>, paperpaw_sdk::Choices)>;

/// The per-format settings of `store`'s formats.
fn per_format(store: &DeclarationStore) -> Option<Offered> {
    let mut formats = 0;
    store.each_format(&mut |_| formats += 1);
    let mut offered: Offered = Vec::new();
    offered.try_reserve_exact(formats).ok()?;
    let mut failed = false;
    store.each_format(&mut |format| {
        let mut id = String::new();
        let len = PER_FORMAT.len() + format.plugin.len() + 1 + format.spec.name.len();
        if failed || id.try_reserve_exact(len).is_err() {
            failed = true;
            return;
        }
        id.push_str(PER_FORMAT);
        id.push_str(format.plugin);
        id.push('.');
        id.push_str(format.spec.name);
        let format_id = id.get(PER_FORMAT.len()..).unwrap_or_default();
        match (
            choices_for(store, Some(format_id)),
            owned(format.spec.title),
        ) {
            (Some(choices), Some(title)) if offered.len() < offered.capacity() => {
                offered.push((id, title, choices));
            }
            _ => failed = true,
        }
    });
    (!failed).then_some(offered)
}

impl Kernel {
    /// Reads the kept choices, once at the start.
    pub(crate) fn read_reading_apps(&mut self) {
        let apps = &mut self.shared.registry.reading_apps;
        match paperpaw_sdk::read_file(self.shared.storage.as_ref(), FILE, LIMIT) {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(text) => apps.kept = text,
                Err(_) => apps.unreadable = true,
            },
            Err(StorageError::NotFound) => {}
            Err(_) => apps.unreadable = true,
        }
    }

    /// The reading-app settings in line with the declared apps, where the
    /// registry moved since: the default one's choices, and one setting per
    /// format two or more apps read (made then, kept after). Where the heap
    /// has no room, it is tried again at the next call.
    pub(crate) fn offer_reading_apps(&mut self) {
        let generation = self.shared.registry_generation.get();
        if self.shared.registry.reading_apps.synced == generation {
            return;
        }
        let Some(directory) = self.shared.registry.native_declarations.as_ref() else {
            return;
        };
        let Ok(store) = directory.store.try_borrow() else {
            return;
        };
        let (Some(all), Some(offered)) = (choices_for(&store, None), per_format(&store)) else {
            return;
        };
        drop(store);
        let settings = &mut self.shared.settings;
        settings.set_choices(core_settings::READING_APP.as_str(), all);
        let mut defined = false;
        for (id, title, choices) in offered {
            if settings.spec(&id).is_some() {
                settings.set_choices(&id, choices);
                continue;
            }
            // Nothing to choose: no setting (memory on small devices), until
            // another app comes.
            if choices.len() < 2 {
                continue;
            }
            let Ok(setting) = Id::try_parse(&id) else {
                continue;
            };
            let mut spec = SettingSpec::new(
                setting,
                "",
                SettingKind::Choice(choices),
                SettingValue::Text(Cow::Borrowed(core_settings::READING_APP_DEFAULT)),
                paperpaw_sdk::builtin::groups::READING_BEHAVIOUR,
            )
            .description("core.setting.reading-app.format.help")
            .with_extra(&IN_APPS);
            spec.title = paperpaw_sdk::Str::new(title);
            defined |= settings.define(spec).is_ok();
        }
        self.shared.registry.reading_apps.synced = generation;
        // A format's choice read from the file before its setting was made.
        if defined {
            self.apply_late_config();
        }
    }

    /// The screen an open of the reader slot opens (`screen` where it names
    /// one): for a book, the one chosen for it, else the slot's own; a
    /// [`KEEP`] param keeps the choice first.
    pub(crate) fn reading_app_for_open(
        &mut self,
        screen: Option<Id>,
        params: &Params,
    ) -> Option<Id> {
        let Some((path, keep)) = self.book_opened(params) else {
            return screen;
        };
        // The book the slot holds now, for a pass on; a new book has no
        // apps tried yet.
        let apps = &mut self.shared.registry.reading_apps;
        if !apps.passing {
            apps.tried.clear();
        }
        apps.book = owned(&path).map(Cow::into_owned);
        if let Some(keep) = keep {
            self.keep_reading_app(keep, &path, screen.as_ref());
        }
        if screen.is_some() {
            return screen;
        }
        // The slot's own screen by name: in place of another reading app the
        // slot may show now.
        let registry = &self.shared.registry;
        let chosen = registry
            .native_declarations
            .as_ref()
            .and_then(|directory| directory.store.try_borrow().ok())
            .and_then(|store| {
                chosen(
                    &store,
                    &self.shared.settings,
                    &registry.reading_apps.kept,
                    &path,
                )
            });
        chosen.or_else(|| registry.resolve_slot(slots::READER.as_str()).cloned())
    }

    /// `command.pass_on` from the reading app of `key` (#369 G): the book goes
    /// to the next app that reads it (see the module); with none left, that
    /// reader closes and a notice says so. `reason` is logged.
    pub(crate) fn pass_book_on(&mut self, key: paperpaw_sdk::ScreenKey, reason: i32) {
        let Some(index) = self
            .stack
            .iter()
            .position(|active| paperpaw_sdk::ScreenKey::new(active.serial) == key)
        else {
            return;
        };
        if self.stack[index].slot.as_ref() != Some(&slots::READER) {
            return;
        }
        let Some(path) = self
            .shared
            .registry
            .reading_apps
            .book
            .as_deref()
            .and_then(owned)
        else {
            return;
        };
        let current = self.stack[index].id.try_clone();
        let mut status = [0; 12];
        self.try_warn_runtime(&[
            current.as_ref().map_or("", Id::as_str),
            " cannot open ",
            &path,
            " (status ",
            decimal(reason, &mut status),
            "): passed on",
        ]);
        let apps = &mut self.shared.registry.reading_apps;
        let room = apps.tried.len() < PASS_MAX && apps.tried.try_reserve(1).is_ok();
        let next = match current.filter(|_| room) {
            Some(current) => {
                apps.tried.push(current);
                let registry = &self.shared.registry;
                registry
                    .native_declarations
                    .as_ref()
                    .and_then(|directory| directory.store.try_borrow().ok())
                    .and_then(|store| {
                        next_after(
                            &store,
                            &self.shared.settings,
                            (&registry.reading_apps.kept, &path),
                            registry.resolve_slot(slots::READER.as_str()),
                            &registry.reading_apps.tried,
                        )
                    })
            }
            None => None,
        };
        let Some(screen) = next else {
            self.shared.registry.reading_apps.tried.clear();
            self.close_screen(key);
            let text = String::from(self.shared.translator.tr("core.reading-app.none"));
            self.output.commands.push(crate::Ask::Notify(text));
            return;
        };
        let params = Params::new().with(params::PATH, path.into_owned());
        self.shared.registry.reading_apps.passing = true;
        self.open_slot_command(slots::READER, Some(screen), params);
        self.shared.registry.reading_apps.passing = false;
    }

    /// The book an open of the reader slot names, and what it keeps: from the
    /// hand-over its native open names, else from `params`. None for a
    /// plugin's own file (`root`) or no path.
    fn book_opened(&self, params: &Params) -> Option<(String, Option<Keep>)> {
        let instances = &self.shared.native_instances;
        let (mut root, mut path, mut keep) = (false, None, None);
        match instances.pending_handover() {
            Some(Some(id)) => {
                let store = instances.ui_store()?;
                store.each_param(id, &mut |key, value| match key {
                    "root" => root = true,
                    params::PATH => path = owned(value),
                    KEEP => keep = Keep::parse(value),
                    _ => {}
                });
            }
            // A native open without a hand-over names no book.
            Some(None) => return None,
            None => {
                root = params.get("root").is_some();
                path = params.get(params::PATH).and_then(owned);
                keep = params.get(KEEP).and_then(Keep::parse);
            }
        }
        let path = path.filter(|path| !root && !plain(path).is_empty())?;
        Some((path.into_owned(), keep))
    }

    /// Keeps the app of `screen` for the book at `path` or its folder (none:
    /// forgets the choice), and writes the file.
    fn keep_reading_app(&mut self, keep: Keep, path: &str, screen: Option<&Id>) {
        let path = plain(path);
        let place = match keep {
            Keep::Book => path,
            Keep::Folder => folder_of(path),
        };
        if place.is_empty() || self.shared.registry.reading_apps.unreadable {
            return;
        }
        let mut reader = None;
        if let Some(screen) = screen {
            let Some(directory) = self.shared.registry.native_declarations.as_ref() else {
                return;
            };
            let Ok(store) = directory.store.try_borrow() else {
                return;
            };
            store.each_reader(&mut |decl| {
                if reader.is_none() && decl.spec.screen == screen.as_str() {
                    reader = [decl.plugin, ".", decl.spec.name].iter().try_fold(
                        String::new(),
                        |mut id, part| {
                            id.try_reserve_exact(part.len()).ok()?;
                            id.push_str(part);
                            Some(id)
                        },
                    );
                }
            });
            // Not a reading app: nothing to keep.
            if reader.is_none() {
                return;
            }
        }
        let apps = &mut self.shared.registry.reading_apps;
        let Some(text) = changed(&apps.kept, keep, place, reader.as_deref()) else {
            return;
        };
        apps.kept = text;
        let storage = self.shared.storage.as_ref();
        let _ = storage.create_dir(paperpaw_sdk::data::DIR);
        if storage.write(FILE, apps.kept.as_bytes()).is_err() {
            let text = String::from(self.shared.translator.tr("core.reading-app.not-kept"));
            self.output.commands.push(crate::Ask::Notify(text));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kept_file_never_passes_its_limit_and_the_oldest_choices_go_first() {
        let mut kept = String::new();
        for n in 0..200 {
            let path = alloc::format!("books/folder/a long name of book {n}.epub");
            kept = changed(&kept, Keep::Book, &path, Some("comics.app")).unwrap();
            assert!(kept.len() <= LIMIT);
        }
        assert!(kept.ends_with("book 199.epub\n"));
        assert!(!kept.contains("book 0.epub"));
        // Forgotten, the line goes; another kind of the same path stays.
        let kept = changed(&kept, Keep::Folder, "books/folder", Some("comics.app")).unwrap();
        let kept = changed(&kept, Keep::Book, "books/folder", None).unwrap();
        assert!(kept.contains("f\tcomics.app\tbooks/folder\n"));
    }
}
