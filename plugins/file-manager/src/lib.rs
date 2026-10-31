//! A file manager: every folder and file of the device's storage, to
//! open, rename, move, copy and delete, and to make folders.
//!
//! - Tapping a folder opens it, *Back* goes to the folder above.
//! - Tapping a file, or holding any entry (with buttons: a long press on
//!   confirm), offers what can be done with it; deleting asks first.
//! - *Move* and *Copy* pick the entry: then go to the folder it belongs in
//!   and choose *Paste here* in the bar at the bottom.
//! - Files starting with `.` (e.g. PaperPaw's own data) are hidden unless
//!   the setting *Show hidden files* says otherwise.
//!
//! With buttons only, down from the last entry reaches the bar.

#![no_std]

extern crate alloc;

mod folder;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::ask;
use paperpaw_sdk::builtin::{actions, events, groups, menus, params};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::replies::{Confirmed, Typed};
use paperpaw_sdk::ui::choice;
use paperpaw_sdk::ui::{self, ListChange, ListView, UiStyle};
use paperpaw_sdk::{
    Arg, Canvas, Command, Cx, Env, FixedAction, FixedScreen, Gesture, GestureKind, Handled, Id,
    Manifest, MenuEntry, Params, Plugin, Rect, RefreshMode, RegisterError, Registrar, Screen,
    SettingSpec, Storage, StorageError,
};

use paperpaw_sdk::files::{self as ops, Entry};
use paperpaw_sdk::setting::Setting;

use folder::Folder;

pub const PLUGIN: Id = Id::new_static("file-manager");
/// Opens the file manager.
pub const OPEN: Id = Id::new_static("file-manager.open");
/// Shows files and folders starting with `.`.
pub const HIDDEN: Setting<bool> = Setting::new("file-manager.hidden");
const SCREEN: Id = Id::new_static("file-manager.browser");
/// Replies of the choice overlay and the keyboard.
const CHOSEN: Event<choice::Picked> = Event::new("file-manager.chosen");
const RENAMED: Event<Typed> = Event::new("file-manager.renamed");
const FOLDER_NAMED: Event<Typed> = Event::new("file-manager.folder-named");
/// The answer to whether to delete the entry at its context's path.
const DELETE: Event<Confirmed> = Event::new("file-manager.delete");
/// Translation keys of the choices: this, then the option.
const OPTION_LABELS: &str = "file-manager.option.";

/// What was picked to be moved or copied.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Picked {
    path: String,
    copy: bool,
}

/// The buttons of the bar at the bottom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Button {
    NewFolder,
    Paste,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    List,
    Bar(usize),
}

/// The file manager plugin.
#[derive(Debug, Default)]
pub struct FileManager;

/// Whether hidden files show.
static SETTINGS: [SettingSpec; 1] = [SettingSpec::switch(
    HIDDEN.static_str(),
    "file-manager.setting.hidden",
    false,
    groups::LIBRARY,
)
.advanced()
.description("file-manager.setting.hidden.help")];

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

static ACTIONS: [FixedAction; 1] = [FixedAction::new(OPEN, "file-manager.title")
    .handler(|_, _, out| out.push(Command::OpenScreen(SCREEN, Params::new())))];
static ENTRIES: [MenuEntry; 1] = [MenuEntry::new(menus::TOOLS, OPEN).order(45)];
static ICONS: [(Id, &str); 1] = [(OPEN, "folder")];

static SCREENS: [FixedScreen; 1] = [FixedScreen::new(
    SCREEN,
    "file-manager.title",
    |env, params| Box::new(Browser::new(env, params)),
)];

impl Plugin for FileManager {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "File manager", paperpaw_sdk::package_version!())
            .description(
                "Every folder and file on the device: open, rename, move, copy and delete them.",
            )
            .how(&HOW)
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_settings(&SETTINGS)?;
        r.add_screens(&SCREENS)?;
        r.add_actions(&ACTIONS)?;
        r.add_menu_entries(&ENTRIES)?;
        r.add_icons(&ICONS)?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

struct Browser {
    style: UiStyle,
    header: ui::Header,
    list: ListView,
    bar: Rect,
    /// The shown folder; empty at the top of the storage.
    dir: String,
    /// How many entries it has.
    len: usize,
    /// The entries of the visible page, the first of them number `first`:
    /// only a page is held, however large the folder.
    page: Vec<Entry>,
    first: usize,
    picked: Option<Picked>,
    focus: Focus,
    /// What happened last (an error, or what was done).
    message: Option<String>,
}

impl Browser {
    fn new(env: &Env<'_>, params: &Params) -> Self {
        let style = UiStyle::new(env);
        let header = ui::Header::new(env, &style, true);
        let display = env.device().display;
        // The bar of buttons along the bottom: they keep the side margin
        // from the edge too.
        let bar_height = ui::edge_bar_height(&style);
        let top = header.area().height + style.gap();
        // The bar, and a line for messages above it.
        let bottom = bar_height + style.row() + style.gap();
        let area = Rect::new(
            0,
            top,
            display.width,
            display.height.saturating_sub(top + bottom),
        );
        let mut browser = Self {
            list: ListView::new(area, u32::from(style.row())).focus_on_keys(env),
            bar: Rect::new(0, display.height - bar_height, display.width, bar_height),
            dir: params.get(params::PATH).unwrap_or_default().to_string(),
            len: 0,
            page: Vec::new(),
            first: 0,
            picked: None,
            focus: Focus::List,
            message: None,
            style,
            header,
        };
        browser.load(env, None);
        browser
    }

    fn folder<'a>(&'a self, env: &'a Env<'_>) -> Folder<'a> {
        Folder {
            storage: env.storage(),
            dir: &self.dir,
            hidden: env.settings().read(&HIDDEN),
        }
    }

    /// The entry number `index`, if its page is held.
    fn entry(&self, index: usize) -> Option<&Entry> {
        self.page.get(index.checked_sub(self.first)?)
    }

    /// Shows the folder as it is now, reading only the page shown: the one
    /// with entry `at` on it, focused (where it went, the one at its
    /// place), else the one the focus is on.
    fn load(&mut self, env: &Env<'_>, at: Option<Entry>) {
        let rows = self.list.rows().max(1);
        let focus = self.list.focus();
        let folder = self.folder(env);
        let shown = (|| {
            let len = folder.count()?;
            if len == 0 {
                return Ok((0, 0, 0, Vec::new()));
            }
            let mut focus = focus.min(len - 1);
            if let Some(at) = &at {
                let (first, page) = folder.page_of(at, rows)?;
                if let Some(i) = page.iter().position(|e| e.name == at.name) {
                    return Ok((len, first, first + i, page));
                }
                focus = folder.rank(at)?.min(len - 1);
            }
            let page = folder.page(focus / rows, rows, len)?;
            Ok((len, focus - focus % rows, focus, page))
        })();
        match shown {
            Ok((len, first, focus, page)) => {
                self.len = len;
                self.first = first;
                self.list.set_focus(focus);
                self.page = page;
            }
            Err(e) => {
                self.len = 0;
                self.first = 0;
                self.list.set_focus(0);
                self.page = Vec::new();
                self.message = Some(error(env, e));
            }
        }
    }

    /// Reads the page the focus is on, if it is not held: the next or the
    /// one before from the one held, as the buttons go.
    fn show_focus(&mut self, env: &Env<'_>) {
        let rows = self.list.rows().max(1);
        let first = self.list.focus() - self.list.focus() % rows;
        if first == self.first && !self.page.is_empty() || self.len == 0 {
            return;
        }
        let folder = self.folder(env);
        let page = if first == self.first + rows {
            folder.after(self.page.last(), rows)
        } else if first + rows == self.first {
            folder.before(self.page.first(), rows)
        } else {
            folder.page(first / rows, rows, self.len)
        };
        match page {
            Ok(page) => {
                self.first = first;
                self.page = page;
            }
            Err(e) => {
                self.message = Some(error(env, e));
                self.load(env, None);
            }
        }
    }

    fn buttons(&self) -> Vec<Button> {
        match &self.picked {
            Some(_) => Vec::from([Button::Paste, Button::Cancel]),
            None => Vec::from([Button::NewFolder]),
        }
    }

    fn button_rect(&self, index: usize) -> Rect {
        let count = self.buttons().len() as u16;
        let margin = self.style.margin();
        let gap = self.style.gap() * 2;
        let width = (self.bar.width - 2 * margin - gap * (count - 1)) / count;
        let band = ui::edge_bar_band(&self.style, self.bar, false);
        Rect::new(
            margin + index as u16 * (width + gap),
            band.y,
            width,
            band.height,
        )
    }

    fn redraw(&self, cx: &mut Cx<'_, '_>) {
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Fast);
    }

    /// Opens folder `dir` from its first entry.
    fn enter(&mut self, dir: String, cx: &mut Cx<'_, '_>) {
        self.open_at(dir, None, cx);
    }

    /// Opens folder `dir` at entry `at` (from its first without).
    fn open_at(&mut self, dir: String, at: Option<Entry>, cx: &mut Cx<'_, '_>) {
        self.dir = dir;
        self.list.set_focus(0);
        self.focus = Focus::List;
        let env = *cx.env();
        self.load(&env, at);
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
    }

    /// Back to the folder above; `false` at the top.
    fn up(&mut self, cx: &mut Cx<'_, '_>) -> bool {
        if self.dir.is_empty() {
            return false;
        }
        // The folder just left stays chosen.
        let left = folder_entry(ops::name(&self.dir));
        let parent = ops::parent(&self.dir).to_string();
        self.open_at(parent, Some(left), cx);
        true
    }

    /// Shows the folder again, at the entry `name` (where it went, at its
    /// place), after something was done to it.
    fn reload_at(&mut self, env: &Env<'_>, name: &str) {
        let path = ops::join(&self.dir, name);
        let entry = Entry::new(name.to_string(), ops::is_dir(env.storage(), &path), 0);
        self.load(env, Some(entry));
    }

    /// Tapped or confirmed: a folder opens, a file offers its actions.
    fn activate(&mut self, index: usize, cx: &mut Cx<'_, '_>) {
        let Some(entry) = self.entry(index).cloned() else {
            return;
        };
        let path = ops::join(&self.dir, &entry.name);
        if entry.is_dir {
            self.enter(path, cx);
        } else {
            self.offer(index, cx);
        }
    }

    /// The actions for entry `index`.
    fn offer(&mut self, index: usize, cx: &mut Cx<'_, '_>) {
        let Some(entry) = self.entry(index) else {
            return;
        };
        let path = ops::join(&self.dir, &entry.name);
        let env = *cx.env();
        let mut options = Vec::new();
        if !entry.is_dir && env.formats().supports_file(env.storage(), &path) {
            options.push("open");
            // Where more than one reading app reads it.
            if env.readers().for_book(&env, &path).len() > 1 {
                options.push("open-with");
            }
        }
        if entry.is_dir && env.readers().len() > 1 {
            options.push("reading-app");
        }
        options.extend(["rename", "move", "copy", "delete"]);
        cx.command(
            paperpaw_sdk::ask::choice(&entry.name, &CHOSEN)
                .about(&path)
                .options(OPTION_LABELS, &options)
                .command(),
        );
    }

    fn press(&mut self, button: Button, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        match button {
            Button::NewFolder => cx.command(
                ask::text(env.i18n().tr("file-manager.new-folder"), &FOLDER_NAMED).command(),
            ),
            Button::Cancel => {
                self.picked = None;
                self.message = None;
                self.focus = Focus::List;
                self.redraw(cx);
            }
            Button::Paste => {
                let Some(picked) = self.picked.take() else {
                    return;
                };
                let target = ops::free_path(env.storage(), &self.dir, ops::name(&picked.path));
                let mut note = None;
                let result = if picked.copy {
                    ops::copy(env.storage(), &picked.path, &target)
                } else if ops::same_path(env.storage(), ops::parent(&picked.path), &self.dir) {
                    // Moved where it is: nothing to do.
                    Ok(())
                } else if let Some(refused) = refused(&env, &picked.path) {
                    self.message = Some(refused);
                    return self.redraw(cx);
                } else {
                    let moved = ops::move_to(env.storage(), &picked.path, &target);
                    if moved.is_ok() {
                        note = tell_moved(&env, &picked.path, &target, cx);
                    } else {
                        tell_partly_moved(&env, &picked.path, &target, cx);
                    }
                    moved
                };
                if result.is_ok() && picked.copy {
                    cx.command(paperpaw_sdk::library::changed());
                }
                self.message = match result {
                    Ok(()) => note,
                    Err(e) => Some(error(&env, e)),
                };
                self.focus = Focus::List;
                self.reload_at(&env, ops::name(&target));
                cx.request_redraw();
                cx.request_refresh(RefreshMode::Partial);
            }
        }
    }

    /// The choice made in the overlay for `path`.
    fn chosen(&mut self, option: &str, path: &str, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let name = ops::name(path).to_string();
        match option {
            "open" => cx.command(paperpaw_sdk::readers::open(path)),
            // This time; *Always open it with …* in the list keeps one.
            "open-with" => cx.command(paperpaw_sdk::readers::ask_once(path)),
            "reading-app" => cx.command(paperpaw_sdk::readers::ask(
                paperpaw_sdk::readers::Choice::Folder(path),
            )),
            "rename" => cx.command(
                ask::text(env.i18n().tr("file-manager.rename"), &RENAMED)
                    .initial(name)
                    .with(params::PATH, path.to_string())
                    .command(),
            ),
            "move" | "copy" => {
                let copy = option == "copy";
                self.picked = Some(Picked {
                    path: path.to_string(),
                    copy,
                });
                let key = if copy {
                    "file-manager.picked-copy"
                } else {
                    "file-manager.picked-move"
                };
                self.message = Some(env.i18n().format(key, &[("name", Arg::Str(&name))]));
                self.redraw(cx);
            }
            "delete" if is_own_data(env.storage(), path) => {
                self.message = Some(env.i18n().tr("file-manager.error.own-data").to_string());
                self.redraw(cx);
            }
            "delete" => {
                // What goes: a folder with everything in it, a book with
                // what is kept for it.
                let key = if ops::is_dir(env.storage(), path) {
                    "file-manager.delete-folder-question"
                } else if env.formats().supports_file(env.storage(), path) {
                    "file-manager.delete-book-question"
                } else {
                    "file-manager.delete-question"
                };
                let title = env.i18n().format(key, &[("name", Arg::Str(&name))]);
                // Asked as every yes or no question: the focus starts on
                // the safe answer.
                cx.command(
                    ask::confirm(title, &DELETE)
                        .yes(env.i18n().tr("file-manager.option.delete-yes"))
                        .no(env.i18n().tr("file-manager.option.cancel"))
                        .with(params::PATH, path.to_string())
                        .command(),
                );
            }
            _ => {}
        }
    }

    /// Deletes `path`, as the reader answered yes.
    fn delete(&mut self, path: &str, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if is_own_data(env.storage(), path) {
            self.message = Some(env.i18n().tr("file-manager.error.own-data").to_string());
            return self.redraw(cx);
        }
        // The books in it, said gone once it is: what is kept for them
        // goes too.
        // At most `MAX_TOLD`: the data of those beyond stays until the
        // storage is cleaned up.
        let mut books = Vec::new();
        paperpaw_sdk::library::books_in(env.storage(), env.formats(), path, &mut |book| {
            if books.len() < MAX_TOLD {
                books.push(String::from(book));
            }
        });
        let at = Entry::new(
            ops::name(path).to_string(),
            ops::is_dir(env.storage(), path),
            0,
        );
        let deleted = ops::delete(env.storage(), path);
        if deleted.is_ok() {
            for path in books {
                cx.command(events::BOOK_REMOVED.emit(events::BookRemoved::new(path)));
            }
            cx.command(paperpaw_sdk::library::changed());
        }
        self.message = deleted.err().map(|e| error(&env, e));
        // Picked to be moved or copied, and gone with it.
        if (self.picked.as_ref()).is_some_and(|p| ops::is_within(env.storage(), &p.path, path)) {
            self.picked = None;
        }
        // The focus where it was.
        self.load(&env, Some(at));
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
    }

    /// What the focus draws over: its row, or the bar below the list.
    fn focus_rect(&self) -> Rect {
        match self.focus {
            Focus::List => self
                .list
                .row_rect(self.list.focus() % self.list.rows().max(1)),
            Focus::Bar(_) => self.bar,
        }
    }

    fn move_focus(&mut self, delta: isize, cx: &mut Cx<'_, '_>) {
        let len = self.len;
        let bar = self.buttons().len();
        let (left, first) = (self.focus_rect(), self.first);
        // The list and the bar below it, one round: past the last button
        // to the first entry, and back.
        self.focus = match self.focus {
            Focus::List if delta > 0 && (len == 0 || self.list.focus() + 1 >= len) => Focus::Bar(0),
            Focus::List if delta < 0 && self.list.focus() == 0 => Focus::Bar(bar - 1),
            Focus::List => {
                let change = self.list.move_focus(delta, len);
                if change == ListChange::Page {
                    cx.request_refresh(RefreshMode::Partial);
                }
                Focus::List
            }
            Focus::Bar(i) if delta > 0 && i + 1 < bar => Focus::Bar(i + 1),
            Focus::Bar(_) if delta > 0 => {
                self.list.set_focus(0);
                Focus::List
            }
            Focus::Bar(0) => {
                self.list.set_focus(len.saturating_sub(1));
                Focus::List
            }
            Focus::Bar(i) => Focus::Bar(i - 1),
        };
        let env = *cx.env();
        self.show_focus(&env);
        if self.first == first {
            // The same page: the row or bar the focus left and the one
            // it came to.
            cx.request_redraw_area(left);
            cx.request_redraw_area(self.focus_rect());
            cx.request_refresh(RefreshMode::Fast);
        } else {
            self.redraw(cx);
        }
    }
}

/// What to tell the user about a failed operation.
fn error(env: &Env<'_>, e: paperpaw_sdk::StorageError) -> String {
    use paperpaw_sdk::StorageError as E;
    let key = match e {
        E::NotFound => "file-manager.error.not-found",
        E::InvalidPath => "file-manager.error.invalid",
        E::Unsupported => "file-manager.error.unsupported",
        E::Full => "file-manager.error.full",
        _ => "file-manager.error.io",
    };
    env.i18n().tr(key).to_string()
}

/// A name for a file or folder that every file system takes as it is, the
/// memory card's (FAT) and Windows' too: one line, none of `/ \ : * ? " <
/// > |`, not ending in a dot (which they drop), not `.` or `..`.
fn valid_name(name: &str) -> Option<&str> {
    let name = name.trim();
    let bad = name.is_empty()
        || name.ends_with('.')
        || name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|', '\0'])
        || name.chars().any(char::is_control);
    (!bad).then_some(name)
}

/// A folder named `name`, as an entry to find in a listing.
fn folder_entry(name: &str) -> Entry {
    Entry::new(name.to_string(), true, 0)
}

/// Renames `path` to `name` in its folder: where another entry has that
/// name, to a free one beside it (`name (2)`). A name that differs in case
/// only is the same one on storages that ignore case (the memory card):
/// there it is renamed through a free name between, as such a storage takes
/// both names for one. The path it has now, or why not.
fn rename(storage: &dyn Storage, path: &str, name: &str) -> Result<String, StorageError> {
    let dir = ops::parent(path);
    let own = ops::name(path);
    let other = {
        let mut other = false;
        storage.list(dir, &mut |e| other |= e.name == name && e.name != own)?;
        other
    };
    if other || own.to_lowercase() != name.to_lowercase() {
        let target = ops::free_path(storage, dir, name);
        ops::move_to(storage, path, &target)?;
        return Ok(target);
    }
    let target = ops::join(dir, name);
    let between = ops::free_path(storage, dir, &format!(".{own}.renaming"));
    ops::move_to(storage, path, &between)?;
    if let Err(e) = ops::move_to(storage, &between, &target) {
        // Back as it was.
        let _ = ops::move_to(storage, &between, path);
        return Err(e);
    }
    Ok(target)
}

impl Screen for Browser {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let len = self.len;
        let title = if self.dir.is_empty() {
            env.i18n().tr("file-manager.device").to_string()
        } else {
            ops::name(&self.dir).to_string()
        };
        let mut ui = ui::Ui::new(canvas, env, &style);
        self.header
            .draw(&mut ui, &title, None, self.list.paging(len));
        if len == 0 {
            ui.empty(
                self.list.area(),
                "folder-open",
                "",
                env.i18n().tr("file-manager.empty"),
            );
        }
        let decimal = env.i18n().tr("file-manager.decimal");
        let room = ui::icon_room(ui.env(), ui.style(), true);
        let (page, first) = (&self.page, self.first);
        let active = self.focus == Focus::List;
        self.list.set_active(active);
        self.list.draw(&mut ui, len, |ui, index, rect, focused| {
            let Some(entry) = index.checked_sub(first).and_then(|i| page.get(i)) else {
                return;
            };
            let value = if entry.is_dir {
                String::from("›")
            } else {
                ops::size(entry.size, decimal)
            };
            // Folders with a folder before their name, files with a
            // page: told apart at a glance.
            let ink = style.focus_ink(focused);
            let icon = if entry.is_dir { "folder" } else { "file" };
            let rest = ui.row_icon(rect, room, Some(icon), ink);
            ui::Row::value(&entry.name, &value)
                .focused(focused)
                .draw(ui, rest);
        });
        // The message line above the bar.
        if let Some(message) = &self.message {
            let line = Rect::new(0, self.bar.y - style.row(), self.bar.width, style.row());
            ui::Label::new(message)
                .px(style.small())
                .color(style.faint(ui::Ink::NOTE))
                .draw_from(&mut ui, style.margin() as f32, line);
        }
        for (i, button) in self.buttons().into_iter().enumerate() {
            let label = env.i18n().tr(match button {
                Button::NewFolder => "file-manager.new-folder",
                Button::Paste => "file-manager.paste",
                Button::Cancel => "file-manager.cancel",
            });
            let focused = self.focus == Focus::Bar(i);
            let on = button == Button::Paste;
            ui.button(self.button_rect(i), label, on, focused);
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let a = action;
        if [actions::UP, actions::LEFT, actions::PREVIOUS].contains(a) {
            self.move_focus(-1, cx);
        } else if [actions::DOWN, actions::RIGHT, actions::NEXT].contains(a) {
            self.move_focus(1, cx);
        } else if *a == actions::NEXT_PAGE || *a == actions::PREV_PAGE {
            let rows = self.list.rows() as isize;
            let delta = if *a == actions::NEXT_PAGE {
                rows
            } else {
                -rows
            };
            if self.list.move_focus(delta, self.len) != ListChange::None {
                let env = *cx.env();
                self.show_focus(&env);
                cx.request_redraw();
                cx.request_refresh(RefreshMode::Partial);
            }
        } else if *a == actions::CONFIRM {
            match self.focus {
                Focus::List => self.activate(self.list.focus(), cx),
                Focus::Bar(i) => {
                    if let Some(button) = self.buttons().get(i).copied() {
                        self.press(button, cx);
                    }
                }
            }
        } else if *a == actions::MENU && self.focus == Focus::List {
            self.offer(self.list.focus(), cx);
        } else if *a == actions::BACK {
            if !self.up(cx) {
                cx.command(Command::Back);
            }
        } else {
            return Handled::No;
        }
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        let paging = self.list.paging(self.len);
        if gesture.kind == GestureKind::Tap
            && let Some(action) = self
                .header
                .tap(cx.env(), &self.style, gesture.start, paging)
        {
            // Back goes up a folder first.
            if action == actions::BACK {
                if !self.up(cx) {
                    cx.command(Command::Back);
                }
            } else {
                cx.command(Command::run(action));
            }
            return Handled::Yes;
        }
        if let Some(i) =
            (0..self.buttons().len()).find(|&i| self.button_rect(i).contains(gesture.start))
        {
            if gesture.kind == GestureKind::Tap
                && let Some(button) = self.buttons().get(i).copied()
            {
                self.press(button, cx);
            }
            return Handled::Yes;
        }
        let Some(index) = self.list.hit(gesture.start, self.len) else {
            return Handled::No;
        };
        self.list.set_focus(index);
        self.focus = Focus::List;
        cx.request_redraw_area(self.list.area());
        match gesture.kind {
            GestureKind::Tap => self.activate(index, cx),
            GestureKind::LongPress => self.offer(index, cx),
            _ => return Handled::No,
        }
        Handled::Yes
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if let Some(picked) = CHOSEN.read(event, params) {
            self.chosen(&picked.option, &picked.path, cx);
        } else if let Some(answer) = DELETE.read(event, params) {
            if answer.yes {
                let path = answer.context.get(params::PATH).unwrap_or_default();
                self.delete(path, cx);
            }
        } else if let Some(typed) = RENAMED.read(event, params) {
            let path = typed
                .context
                .get(params::PATH)
                .unwrap_or_default()
                .to_string();
            let text = typed.text.as_str();
            let Some(name) = valid_name(text) else {
                self.message = Some(env.i18n().tr("file-manager.error.invalid").to_string());
                return self.redraw(cx);
            };
            if name == ops::name(&path) {
                return;
            }
            if let Some(refused) = refused(&env, &path) {
                self.message = Some(refused);
                return self.redraw(cx);
            }
            let at = match rename(env.storage(), &path, name) {
                Ok(target) => {
                    self.message = tell_moved(&env, &path, &target, cx);
                    ops::name(&target).to_string()
                }
                Err(e) => {
                    self.message = Some(error(&env, e));
                    ops::name(&path).to_string()
                }
            };
            self.reload_at(&env, &at);
            self.redraw(cx);
        } else if let Some(typed) = FOLDER_NAMED.read(event, params) {
            let text = typed.text.as_str();
            let Some(name) = valid_name(text) else {
                if !text.trim().is_empty() {
                    self.message = Some(env.i18n().tr("file-manager.error.invalid").to_string());
                    self.redraw(cx);
                }
                return;
            };
            let target = ops::free_path(env.storage(), &self.dir, name);
            self.message = env
                .storage()
                .create_dir(&target)
                .err()
                .map(|e| error(&env, e));
            self.load(&env, Some(folder_entry(ops::name(&target))));
            self.redraw(cx);
        }
    }
}

/// Most books whose moving (or deleting) is told at once, for one folder:
/// a folder of more is not moved ([`refused`]).
const MAX_TOLD: usize = 2000;

/// The folder PaperPaw had up to 0.3: taken over as it starts, and where
/// it could not take a file over (the new folder has one of that name)
/// the old one stays.
const OLD_DIR: &str = ".quireink";

/// Whether `path` is PaperPaw's own folder or lies in it, at any depth,
/// with every setting, note and statistic, or in the folder of its old
/// name: not deleted, renamed or moved here.
fn is_own_data(storage: &dyn Storage, path: &str) -> bool {
    [paperpaw_sdk::data::DIR, OLD_DIR]
        .iter()
        .any(|dir| ops::is_within(storage, path, dir))
}

/// Why `path` is not moved or renamed, if it is not: PaperPaw's own
/// folder, or a folder of more books than can be told moving
/// ([`MAX_TOLD`]), whose notes would stay behind.
fn refused(env: &Env<'_>, path: &str) -> Option<String> {
    if is_own_data(env.storage(), path) {
        return Some(env.i18n().tr("file-manager.error.own-data").to_string());
    }
    if !ops::is_dir(env.storage(), path) {
        return None;
    }
    let mut books = 0;
    paperpaw_sdk::library::books_in(env.storage(), env.formats(), path, &mut |_| books += 1);
    (books > MAX_TOLD).then(|| {
        let most = Arg::Int(MAX_TOLD as i64);
        env.i18n()
            .format("file-manager.too-many-books", &[("count", most)])
    })
}

/// Tells the other plugins that the books at `from` are now at `to`
/// ([`paperpaw_sdk::library::tell_moved`]), so the data they keep by a
/// book's path (progress, notes, statistics) goes along, up to
/// [`MAX_TOLD`] books, and settings holding the folder follow it. Says so
/// where the data of some books could not go along.
fn tell_moved(env: &Env<'_>, from: &str, to: &str, cx: &mut Cx<'_, '_>) -> Option<String> {
    let more = paperpaw_sdk::library::tell_moved(env, from, to, MAX_TOLD, &mut |command| {
        cx.command(command);
    });
    more.then(|| env.i18n().tr("file-manager.moved-many").to_string())
}

/// Tells of the books a move cut short moved (copied, then not all
/// deleted): those at `to` and gone from `from`. The others are still
/// where they were, with what is kept for them.
fn tell_partly_moved(env: &Env<'_>, from: &str, to: &str, cx: &mut Cx<'_, '_>) {
    let (from, to) = (from.trim_matches('/'), to.trim_matches('/'));
    let mut told = 0;
    paperpaw_sdk::library::books_in(env.storage(), env.formats(), to, &mut |book| {
        let Some(rest) = book.strip_prefix(to) else {
            return;
        };
        let was = format!("{from}{rest}");
        if told < MAX_TOLD && !env.storage().exists(&was) {
            told += 1;
            cx.command(events::BOOK_MOVED.emit(events::BookMoved::new(was, book.to_string())));
        }
    });
}

#[cfg(test)]
mod tests {
    use paperpaw_i18n::{Catalog, check};

    use super::*;

    #[test]
    fn translations_are_complete() {
        let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
        let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
        let issues = check::compare(&en, &de);
        assert!(issues.is_empty(), "{issues:?}");
    }

    #[test]
    fn names_are_checked() {
        assert_eq!(valid_name("  Neu  "), Some("Neu"));
        assert_eq!(valid_name(".hidden"), Some(".hidden"));
        assert_eq!(valid_name("a/b"), None);
        assert_eq!(valid_name(".."), None);
        assert_eq!(valid_name(""), None);
        // What the memory card and Windows refuse or change.
        for bad in ["a*b", "what?", "say \"hi\"", "a<b>", "a|b", "notes."] {
            assert_eq!(valid_name(bad), None, "{bad}");
        }
    }

    use alloc::collections::BTreeMap;
    use core::cell::RefCell;
    use paperpaw_sdk::{DirEntry, MemoryStorage};

    /// A storage that ignores case, as the memory card's file system does:
    /// files by their path in lower case, each with the spelling it was
    /// written with.
    #[derive(Default)]
    struct Folding {
        files: RefCell<BTreeMap<String, (String, Vec<u8>)>>,
    }

    impl Folding {
        fn with(self, path: &str, data: &str) -> Self {
            let file = (path.to_string(), data.as_bytes().to_vec());
            self.files.borrow_mut().insert(path.to_lowercase(), file);
            self
        }

        fn paths(&self) -> Vec<String> {
            self.files
                .borrow()
                .values()
                .map(|(p, _)| p.clone())
                .collect()
        }
    }

    impl Storage for Folding {
        fn ignores_case(&self) -> bool {
            true
        }
        fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
            let files = self.files.borrow();
            let (_, data) = files
                .get(&path.to_lowercase())
                .ok_or(StorageError::NotFound)?;
            let from = (offset as usize).min(data.len());
            let n = buf.len().min(data.len() - from);
            buf[..n].copy_from_slice(&data[from..from + n]);
            Ok(n)
        }
        fn size(&self, path: &str) -> Result<u64, StorageError> {
            let files = self.files.borrow();
            let (_, data) = files
                .get(&path.to_lowercase())
                .ok_or(StorageError::NotFound)?;
            Ok(data.len() as u64)
        }
        fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
            let file = (path.to_string(), data.to_vec());
            self.files.borrow_mut().insert(path.to_lowercase(), file);
            Ok(())
        }
        fn append(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
            Err(StorageError::Unsupported)
        }
        fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
            // Folders only by the files in them (the tests keep them flat).
            let prefix = format!("{}/", dir.to_lowercase());
            let mut found = false;
            for (key, (path, data)) in self.files.borrow().iter() {
                if key.starts_with(&prefix) {
                    found = true;
                    visit(DirEntry::new(ops::name(path), false, data.len() as u64));
                }
            }
            if found {
                Ok(())
            } else {
                Err(StorageError::NotFound)
            }
        }
        fn remove(&self, path: &str) -> Result<(), StorageError> {
            self.files.borrow_mut().remove(&path.to_lowercase());
            Ok(())
        }
        fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
            let mut files = self.files.borrow_mut();
            let (_, data) = files
                .remove(&from.to_lowercase())
                .ok_or(StorageError::NotFound)?;
            files.insert(to.to_lowercase(), (to.to_string(), data));
            Ok(())
        }
    }

    /// On a storage that ignores case, a name that differs in case only is
    /// its own: no `(2)`, and nothing lost.
    #[test]
    fn a_new_case_only_is_the_same_name() {
        let storage = Folding::default()
            .with("books/alice.epub", "Alice")
            .with("books/other.epub", "Other");
        assert_eq!(
            rename(&storage, "books/alice.epub", "Alice.epub"),
            Ok(String::from("books/Alice.epub"))
        );
        assert_eq!(storage.paths(), ["books/Alice.epub", "books/other.epub"]);
        assert_eq!(storage.size("books/alice.EPUB"), Ok(5));
        // Another name taken: beside it.
        assert_eq!(
            rename(&storage, "books/Alice.epub", "OTHER.epub"),
            Ok(String::from("books/OTHER (2).epub"))
        );
    }

    /// A folder setting follows its folder as the storage names it.
    #[test]
    fn folders_follow_as_the_storage_names_them() {
        let card = Folding::default();
        let moved = |value| paperpaw_sdk::library::moved_folder(&card, value, "books", "Library");
        assert_eq!(moved("Books").as_deref(), Some("Library"));
        assert_eq!(moved("Books/Archive/").as_deref(), Some("Library/Archive"));
        assert_eq!(moved("books2"), None);
        assert_eq!(moved("Notes"), None);
        // Where case counts, `Books` is another folder.
        let disk = MemoryStorage::new();
        assert_eq!(
            paperpaw_sdk::library::moved_folder(&disk, "Books", "books", "Library"),
            None
        );
        assert_eq!(
            paperpaw_sdk::library::moved_folder(&disk, "books/Archive", "books", "Library")
                .as_deref(),
            Some("Library/Archive")
        );
    }

    /// Where case counts, a name another file has in another case is that
    /// file's: it is not replaced.
    #[test]
    fn where_case_counts_the_other_file_stays() {
        let storage = MemoryStorage::new()
            .with("books/alice.epub", "a")
            .with("books/Alice.epub", "A");
        assert_eq!(
            rename(&storage, "books/alice.epub", "Alice.epub"),
            Ok(String::from("books/Alice (2).epub"))
        );
        assert_eq!(storage.get("books/Alice.epub"), Some(b"A".to_vec()));
        assert_eq!(storage.get("books/Alice (2).epub"), Some(b"a".to_vec()));
    }
}
