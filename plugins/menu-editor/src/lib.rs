//! Customizing menus on the device: reorder, remove and add entries of any
//! menu, or return it to how its plugins define it. Changes apply at once
//! and are saved in `[menus]` of `paperpaw.ini`.
//!
//! One cannot lock oneself out: a menu that leads to the settings (directly
//! or through submenus) keeps doing so, and this editor stays on the
//! settings page. Back arrows and buttons do not depend on menus anyway.
//! Nor can a menu be put into one of its own submenus: such a circle
//! would hide both from the list. Menus caught in one all the same (by
//! an edited `paperpaw.ini`) are listed, and *Reset all* resets every
//! menu changed, listed or not.
//!
//! Reached from the main settings page (menu `core.settings-pages`).

#![no_std]

extern crate alloc;

mod native_entry;
/// Interface-7 menu state and typed edits; screen integration is in progress.
pub mod native_menu;
#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;
/// The interface-7 plugin.
pub mod native_plugin;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, menus};
use paperpaw_sdk::ui::reorder::Reorder;
use paperpaw_sdk::ui::reorder::ReorderStep;
use paperpaw_sdk::ui::{self, ListView, UiStyle};
use paperpaw_sdk::{
    Canvas, Color, Command, Cx, Env, FixedAction, FixedScreen, Gesture, GestureKind, Handled, Id,
    Manifest, MenuEntry, Params, Plugin, PointerEvent, Rect, RefreshMode, RegisterError, Registrar,
    Screen,
};

pub const PLUGIN: Id = Id::new_static("menu-editor");
/// Opens the editor.
pub const OPEN: Id = Id::new_static("menu-editor.open");
const MENUS_SCREEN: Id = Id::new_static("menu-editor.menus");
const ENTRIES_SCREEN: Id = Id::new_static("menu-editor.entries");
const ENTRY_SCREEN: Id = Id::new_static("menu-editor.entry");
const ADD_SCREEN: Id = Id::new_static("menu-editor.add");
/// An entry moved; parameter `index`: where it is now.
const MOVED: Id = Id::new_static("menu-editor.moved");

/// Parameters: the menu being edited and an entry's index in it.
const MENU: &str = "menu";
const INDEX: &str = "index";

/// What must stay reachable from every menu that reaches it by default.
const PROTECTED: [Id; 2] = [actions::SETTINGS, OPEN];

/// The menu editor plugin.
#[derive(Debug, Default)]
pub struct MenuEditor;

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

static ACTIONS: [FixedAction; 1] = [FixedAction::new(OPEN, "menu-editor.title")
    .handler(|_, _, out| out.push(Command::OpenScreen(MENUS_SCREEN, Params::new())))];
static ENTRIES: [MenuEntry; 1] = [MenuEntry::new(menus::SETTINGS, OPEN).order(210)];
static ICONS: [(Id, &str); 1] = [(OPEN, "menu-2")];

static SCREENS: [FixedScreen; 4] = [
    FixedScreen::new(MENUS_SCREEN, "menu-editor.title", |env, _| {
        Box::new(MenusScreen::open(env))
    }),
    FixedScreen::new(ENTRIES_SCREEN, "menu-editor.title", |env, p| {
        Box::new(EntriesScreen::open(env, p))
    }),
    FixedScreen::new(ENTRY_SCREEN, "menu-editor.title", |env, p| {
        Box::new(EntryScreen::open(env, p))
    }),
    FixedScreen::new(ADD_SCREEN, "menu-editor.title", |env, p| {
        Box::new(AddScreen::open(env, p))
    }),
];

impl Plugin for MenuEditor {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "Menu editor", paperpaw_sdk::package_version!())
            .description("Reorders, removes and adds the entries of every menu.")
            .how(&HOW)
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screens(&SCREENS)?;
        r.add_actions(&ACTIONS)?;
        r.add_menu_entries(&ENTRIES)?;
        r.add_icons(&ICONS)?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

// ---- Menus and their entries -------------------------------------------

/// Whether `id` is a menu.
fn is_menu(env: &Env<'_>, id: &str) -> bool {
    env.menus().menu_ids().iter().any(|m| m.as_str() == id)
}

/// The display name of an entry: an action, a setting or a submenu.
fn name(env: &Env<'_>, id: &str) -> String {
    let key = env.actions().get(id).map(|a| a.title()).or_else(|| {
        env.settings()
            .specs()
            .find(|s| s.id.as_str() == id)
            .map(|s| s.title.as_ref())
    });
    match key {
        Some(key) => env.i18n().tr(key).to_string(),
        None => name_of_menu(env, id),
    }
}

/// The entries of `menu` as shown now.
fn entries(env: &Env<'_>, menu: &str) -> Vec<Id> {
    env.menus().ids(menu).into_iter().cloned().collect()
}

/// Whether `from` leads to `target`, directly or through submenus, with
/// `ids` giving each menu's entries.
fn reaches(ids: &dyn Fn(&str) -> Vec<Id>, from: &str, target: &Id, seen: &mut Vec<String>) -> bool {
    if seen.iter().any(|s| s == from) {
        return false;
    }
    seen.push(from.to_string());
    ids(from)
        .iter()
        .any(|id| id == target || reaches(ids, id.as_str(), target, seen))
}

/// Whether redefining `menu` as `proposed` keeps every protected entry
/// reachable from every menu that reaches it as its plugins define it.
fn keeps_access(env: &Env<'_>, menu: &str, proposed: &[Id]) -> bool {
    let all = env.menus().menu_ids();
    let is_menu = |id: &str| all.iter().any(|m| m.as_str() == id);
    let defaults = |m: &str| -> Vec<Id> {
        if is_menu(m) {
            env.menus().default_ids(m).into_iter().cloned().collect()
        } else {
            Vec::new()
        }
    };
    let after = |m: &str| -> Vec<Id> {
        if m == menu {
            proposed.to_vec()
        } else if is_menu(m) {
            entries(env, m)
        } else {
            Vec::new()
        }
    };
    all.iter().all(|m| {
        PROTECTED.iter().all(|target| {
            !reaches(&defaults, m.as_str(), target, &mut Vec::new())
                || reaches(&after, m.as_str(), target, &mut Vec::new())
        })
    })
}

/// Whether putting `entry` into `menu` would make a circle: `entry` is a
/// menu that leads to `menu` (or is it).
fn makes_circle(env: &Env<'_>, menu: &str, entry: &Id) -> bool {
    if entry.as_str() == menu {
        return true;
    }
    let Ok(target) = Id::parse(menu) else {
        return false;
    };
    let ids = |m: &str| -> Vec<Id> {
        if is_menu(env, m) {
            entries(env, m)
        } else {
            Vec::new()
        }
    };
    is_menu(env, entry.as_str()) && reaches(&ids, entry.as_str(), &target, &mut Vec::new())
}

/// Saves `menu` as `ids`.
fn save(menu: &str, ids: Vec<Id>, cx: &mut Cx<'_, '_>) {
    if let Ok(menu) = Id::parse(menu) {
        cx.command(Command::SetMenu(menu, Some(ids)));
    }
}

/// A full screen with a header and a list, whose first rows may be moved
/// by dragging.
struct Frame {
    style: UiStyle,
    header: ui::Header,
    list: ListView,
    reorder: Reorder,
}

impl Frame {
    fn new(env: &Env<'_>) -> Self {
        let style = UiStyle::new(env);
        let display = env.device().display;
        let header = ui::Header::new(env, &style, true);
        let top = header.area().height + style.gap();
        let area = Rect::new(0, top, display.width, display.height - top);
        Self {
            list: ListView::new(area, u32::from(style.row())).focus_on_keys(env),
            style,
            header,
            reorder: Reorder::default(),
        }
    }

    /// Draws `len` rows, each label and value as `row` gives them; the
    /// first `movable` with a grip.
    fn draw<'r>(
        &mut self,
        canvas: &mut Canvas<'_>,
        env: &Env<'_>,
        title: &str,
        len: usize,
        row: impl Fn(usize) -> (&'r str, &'r str),
        movable: usize,
    ) {
        let style = self.style;
        let mut ui = ui::Ui::new(canvas, env, &style);
        self.header
            .draw(&mut ui, title, None, self.list.paging(len));
        self.list.draw(&mut ui, len, |ui, index, rect, focused| {
            let (label, value) = row(index);
            let rect = if index < movable {
                Reorder::draw_grip(ui.canvas(), env, &style, rect, focused);
                Reorder::content(rect, &style)
            } else {
                rect
            };
            ui::Row::value(label, value).focused(focused).draw(ui, rect);
        });
        self.reorder.draw(&mut ui, &self.list);
    }

    /// Follows a finger dragging one of the first `movable` rows: where
    /// it was let go, the row moved (`from`, `to`).
    fn on_pointer(
        &mut self,
        event: &PointerEvent,
        movable: usize,
        cx: &mut Cx<'_, '_>,
    ) -> (Handled, Option<(usize, usize)>) {
        let Some(step) = self.reorder.on_pointer(event, &self.list, movable) else {
            return (Handled::No, None);
        };
        if step != ReorderStep::Same {
            cx.request_redraw();
            cx.request_refresh(RefreshMode::Fast);
        }
        match step {
            ReorderStep::Dropped { from, to } if from != to => {
                self.list.set_focus(to);
                (Handled::Yes, Some((from, to)))
            }
            _ => (Handled::Yes, None),
        }
    }

    /// Moves the focus for navigation actions; `Some(index)` to choose an
    /// entry, `None` for other actions.
    fn on_action(&mut self, action: &Id, len: usize, cx: &mut Cx<'_, '_>) -> Option<Handled> {
        let rows = self.list.rows() as isize;
        let a = action;
        let delta = ui::list_step(a, rows)?;
        self.list.move_focus_in(cx.env(), delta, len).request(cx);
        Some(Handled::Yes)
    }

    /// The row a tap chose; header taps run their action.
    fn on_tap(&mut self, gesture: Gesture, len: usize, cx: &mut Cx<'_, '_>) -> Option<usize> {
        if gesture.kind != GestureKind::Tap {
            return None;
        }
        let paging = self.list.paging(len);
        if let Some(action) = self
            .header
            .tap(cx.env(), &self.style, gesture.start, paging)
        {
            cx.command(Command::run(action));
            return None;
        }
        let index = self.list.hit(gesture.start, len)?;
        if index != self.list.focus() {
            self.list.set_focus(index);
            cx.request_redraw();
            cx.request_refresh(RefreshMode::Fast);
        }
        Some(index)
    }
}

// ---- The list of menus --------------------------------------------------

struct MenusScreen {
    /// Menus with their names; submenus follow their menu as
    /// "Menu › Submenu".
    menus: Vec<(Id, String)>,
    /// What the list shows, made when the menus change.
    rows: Vec<(String, String)>,
    frame: Frame,
}

impl MenusScreen {
    fn open(env: &Env<'_>) -> Self {
        let all = env.menus().menu_ids();
        let is_sub = |m: &Id| {
            all.iter()
                .any(|other| env.menus().ids(other.as_str()).contains(&m))
        };
        let mut roots: Vec<(Id, String)> = all
            .iter()
            .filter(|m| !is_sub(m))
            .map(|m| ((*m).clone(), name_of_menu(env, m.as_str())))
            .collect();
        roots.sort_by_key(|(_, name)| name.to_lowercase());
        let mut menus = Vec::new();
        for (root, name) in roots {
            add_with_submenus(env, root, name, &mut menus);
        }
        // Menus in a circle are no one's root: listed after the others.
        for menu in all {
            let name = name_of_menu(env, menu.as_str());
            add_with_submenus(env, menu.clone(), name, &mut menus);
        }
        let mut screen = Self {
            menus,
            rows: Vec::new(),
            frame: Frame::new(env),
        };
        screen.rows = screen.list(env);
        screen
    }

    /// The menus, each marked where changed, and *Reset all* where any is.
    fn list(&self, env: &Env<'_>) -> Vec<(String, String)> {
        let mut rows: Vec<(String, String)> = self
            .menus
            .iter()
            .map(|(m, name)| {
                let changed = env.menus().is_customized(m.as_str());
                let value = if changed {
                    env.i18n().tr("menu-editor.changed")
                } else {
                    ""
                };
                (name.clone(), alloc::format!("{value} ›"))
            })
            .collect();
        if changed_menus(env).next().is_some() {
            rows.push((
                env.i18n().tr("menu-editor.reset-all").to_string(),
                String::new(),
            ));
        }
        rows
    }

    fn choose(&mut self, index: usize, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        match self.menus.get(index) {
            Some((menu, _)) => cx.command(Command::OpenScreen(
                ENTRIES_SCREEN,
                Params::new().with(MENU, menu.as_str().to_string()),
            )),
            // Everything as the plugins define it, listed or not.
            None => {
                for menu in changed_menus(&env) {
                    cx.command(Command::SetMenu(menu.clone(), None));
                }
                self.frame.list.set_focus(0);
            }
        }
    }
}

/// The menus the user changed.
fn changed_menus<'e>(env: &Env<'e>) -> impl Iterator<Item = &'e Id> {
    let menus = env.menus();
    menus
        .menu_ids()
        .into_iter()
        .filter(move |m| menus.is_customized(m.as_str()))
}

/// Adds `menu` and, after it, its submenus.
fn add_with_submenus(env: &Env<'_>, menu: Id, name: String, out: &mut Vec<(Id, String)>) {
    if out.iter().any(|(m, _)| *m == menu) {
        return;
    }
    let subs: Vec<Id> = entries(env, menu.as_str())
        .into_iter()
        .filter(|id| is_menu(env, id.as_str()))
        .collect();
    out.push((menu, name.clone()));
    for sub in subs {
        let sub_name = alloc::format!("{name} › {}", name_of_menu(env, sub.as_str()));
        add_with_submenus(env, sub, sub_name, out);
    }
}

/// The display name of `menu`: its translation, or, for the menu of a
/// setting's chooser (its more ways to choose, e.g. fonts to get), the
/// setting's name after its group's, as it is found in the settings.
fn name_of_menu(env: &Env<'_>, menu: &str) -> String {
    if let Some(name) = env.i18n().get(menu) {
        return name.to_string();
    }
    let settings = env.settings();
    let Some(spec) = settings.spec(menu) else {
        let key = env.actions().get(menu).map_or(menu, |a| a.title());
        return env.i18n().tr(key).to_string();
    };
    let group = settings.group_of(spec);
    let title = env.i18n().tr(&spec.title);
    match settings.groups().find(|g| g.id == group) {
        Some(group) => alloc::format!("{} › {title}", env.i18n().tr(&group.title)),
        None => title.to_string(),
    }
}

impl Screen for MenusScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let title = env.i18n().tr("menu-editor.title");
        let rows = &self.rows;
        let row = |i: usize| (rows[i].0.as_str(), rows[i].1.as_str());
        self.frame.draw(canvas, env, title, rows.len(), row, 0);
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let len = self.rows.len();
        if *action == actions::CONFIRM {
            self.choose(self.frame.list.focus(), cx);
            return Handled::Yes;
        }
        self.frame.on_action(action, len, cx).unwrap_or(Handled::No)
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        let len = self.rows.len();
        match self.frame.on_tap(gesture, len, cx) {
            Some(index) => {
                self.choose(index, cx);
                Handled::Yes
            }
            None => tap_result(gesture),
        }
    }

    fn on_event(&mut self, event: &Id, _: &Params, cx: &mut Cx<'_, '_>) {
        if *event == paperpaw_sdk::builtin::events::MENU_CHANGED {
            self.rows = self.list(cx.env());
            cx.request_redraw();
            cx.request_refresh(RefreshMode::Fast);
        }
    }
}

// ---- The entries of one menu --------------------------------------------

struct EntriesScreen {
    menu: String,
    /// What the list shows, made when the menu changes.
    rows: Vec<(String, String)>,
    /// The menu's entries, the first rows (the rest is *Add*, *Reset*).
    count: usize,
    frame: Frame,
}

impl EntriesScreen {
    fn open(env: &Env<'_>, params: &Params) -> Self {
        let mut screen = Self {
            menu: params.get(MENU).unwrap_or_default().to_string(),
            rows: Vec::new(),
            count: 0,
            frame: Frame::new(env),
        };
        screen.read(env);
        screen
    }

    /// The rows anew, from the menu as it is.
    fn read(&mut self, env: &Env<'_>) {
        self.rows = self.list(env);
        self.count = entries(env, &self.menu).len();
    }

    /// The entries, then *Add* and, if the menu was changed, *Reset*.
    fn list(&self, env: &Env<'_>) -> Vec<(String, String)> {
        let mut rows: Vec<(String, String)> = entries(env, &self.menu)
            .iter()
            .map(|id| {
                let more = if is_menu(env, id.as_str()) { "›" } else { "" };
                (name(env, id.as_str()), more.to_string())
            })
            .collect();
        rows.push((
            env.i18n().tr("menu-editor.add").to_string(),
            "›".to_string(),
        ));
        if env.menus().is_customized(&self.menu) {
            rows.push((
                env.i18n().tr("menu-editor.reset").to_string(),
                String::new(),
            ));
        }
        rows
    }

    fn choose(&mut self, index: usize, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let count = entries(&env, &self.menu).len();
        let params = Params::new().with(MENU, self.menu.clone());
        if index < count {
            let params = params.with(INDEX, alloc::format!("{index}"));
            cx.command(Command::OpenScreen(ENTRY_SCREEN, params));
        } else if index == count {
            cx.command(Command::OpenScreen(ADD_SCREEN, params));
        } else if let Ok(menu) = Id::parse(&self.menu) {
            cx.command(Command::SetMenu(menu, None));
            self.frame.list.set_focus(0);
        }
    }
}

impl Screen for EntriesScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let title = name_of_menu(env, &self.menu);
        let rows = &self.rows;
        let row = |i: usize| (rows[i].0.as_str(), rows[i].1.as_str());
        self.frame
            .draw(canvas, env, &title, rows.len(), row, self.count);
    }

    /// Entries are moved by dragging their grip.
    fn on_pointer(&mut self, event: &PointerEvent, cx: &mut Cx<'_, '_>) -> Handled {
        let env = *cx.env();
        let mut ids = entries(&env, &self.menu);
        let (handled, moved) = self.frame.on_pointer(event, ids.len(), cx);
        if let Some((from, to)) = moved {
            ui::reorder::moved(&mut ids, from, to);
            save(&self.menu, ids, cx);
        }
        handled
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let len = self.rows.len();
        if *action == actions::CONFIRM {
            self.choose(self.frame.list.focus(), cx);
            return Handled::Yes;
        }
        self.frame.on_action(action, len, cx).unwrap_or(Handled::No)
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        let len = self.rows.len();
        match self.frame.on_tap(gesture, len, cx) {
            Some(index) => {
                self.choose(index, cx);
                Handled::Yes
            }
            None => tap_result(gesture),
        }
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        if *event == MOVED || *event == paperpaw_sdk::builtin::events::MENU_CHANGED {
            // The focus follows a moved entry.
            if let Some(focus) = params.get(INDEX).and_then(|i| i.parse().ok()) {
                self.frame.list.set_focus(focus);
            }
            self.read(cx.env());
            let len = self.rows.len();
            self.frame.list.set_len(len);
            cx.request_redraw();
            cx.request_refresh(RefreshMode::Fast);
        }
    }
}

// ---- What to do with an entry -------------------------------------------

/// An overlay with what can be done with one entry: move it up or down,
/// or remove it (refused when that would lock one out).
struct EntryScreen {
    menu: String,
    index: usize,
    /// Shown instead of the options when a removal was refused.
    refused: bool,
    style: UiStyle,
    region: Rect,
    list: ListView,
}

const OPTIONS: [&str; 4] = [
    "menu-editor.up",
    "menu-editor.down",
    "menu-editor.remove",
    "menu-editor.cancel",
];

impl EntryScreen {
    fn open(env: &Env<'_>, params: &Params) -> Self {
        let style = UiStyle::new(env);
        let display = env.device().display;
        // No higher than the screen: the list pages where its rows are tall.
        let height = (u32::from(style.row()) * (OPTIONS.len() as u32 + 1)
            + u32::from(style.gap()) * 2)
            .min(u32::from(display.height)) as u16;
        let region = Rect::new(0, display.height - height, display.width, height);
        let list_area = Rect::new(
            0,
            region.y + style.row() + style.gap(),
            display.width,
            height.saturating_sub(style.row() + style.gap()),
        );
        Self {
            menu: params.get(MENU).unwrap_or_default().to_string(),
            index: params.get(INDEX).and_then(|i| i.parse().ok()).unwrap_or(0),
            refused: false,
            list: ListView::new(list_area, u32::from(style.row())).focus_on_keys(env),
            style,
            region,
        }
    }

    fn choose(&mut self, option: usize, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if self.refused {
            cx.command(Command::Back);
            return;
        }
        let mut ids = entries(&env, &self.menu);
        let i = self.index;
        if i >= ids.len() {
            cx.command(Command::Back);
            return;
        }
        let to = match option {
            0 if i > 0 => {
                ids.swap(i, i - 1);
                i - 1
            }
            1 if i + 1 < ids.len() => {
                ids.swap(i, i + 1);
                i + 1
            }
            2 => {
                let mut fewer = ids.clone();
                fewer.remove(i);
                if !keeps_access(&env, &self.menu, &fewer) {
                    self.refused = true;
                    cx.request_redraw();
                    cx.request_refresh(RefreshMode::Fast);
                    return;
                }
                ids = fewer;
                i.saturating_sub(1)
            }
            _ => {
                cx.command(Command::Back);
                return;
            }
        };
        cx.command(Command::Back);
        save(&self.menu, ids, cx);
        // Tells the list where the entry went, so it stays focused.
        cx.command(Command::Emit(
            MOVED,
            Params::new().with(INDEX, alloc::format!("{to}")),
        ));
    }
}

impl Screen for EntryScreen {
    fn region(&self, _: &Env<'_>) -> Option<Rect> {
        Some(self.region)
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let mut ui = ui::Ui::new(canvas, env, &style);
        let r = self.region;
        ui.canvas().fill(r, Color::WHITE);
        ui.canvas().fill_rect(
            i32::from(r.x),
            i32::from(r.y),
            u32::from(r.width),
            (u32::from(style.line()) * 2).max(2),
            Color::BLACK,
        );
        let Some(font) = ui.font() else {
            return;
        };
        let id = entries(env, &self.menu).get(self.index).cloned();
        let title = id.map_or(String::new(), |id| name(env, id.as_str()));
        let title_area = Rect::new(r.x, r.y + style.gap(), r.width, style.row());
        let x = style.margin() as f32;
        let title = ui::fit(&font, style.title(), &title, f32::from(r.width) - 2.0 * x);
        ui::Label::new(&title)
            .px(style.title())
            .color(Color::BLACK)
            .draw_from(&mut ui, x, title_area);
        if self.refused {
            let message = env.i18n().tr("menu-editor.locked");
            ui.message(self.list.area(), message);
            return;
        }
        self.list
            .draw(&mut ui, OPTIONS.len(), |ui, index, rect, focused| {
                let label = env.i18n().tr(OPTIONS[index]);
                let color = style.focus_ink(focused);
                ui::Label::new(label)
                    .px(style.text())
                    .color(color)
                    .draw_from(ui, x, rect);
            });
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let a = action;
        if *a == actions::CONFIRM {
            self.choose(self.list.focus(), cx);
            return Handled::Yes;
        }
        let delta = if [
            actions::UP,
            actions::LEFT,
            actions::PREVIOUS,
            actions::PREV_PAGE,
        ]
        .contains(a)
        {
            -1
        } else if [
            actions::DOWN,
            actions::RIGHT,
            actions::NEXT,
            actions::NEXT_PAGE,
        ]
        .contains(a)
        {
            1
        } else {
            return Handled::No;
        };
        self.list
            .move_focus_in(cx.env(), delta, OPTIONS.len())
            .request(cx);
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap || !self.region.contains(gesture.start) {
            return Handled::No;
        }
        if self.refused {
            cx.command(Command::Back);
        } else if let Some(index) = self.list.hit(gesture.start, OPTIONS.len()) {
            self.list.set_focus(index);
            self.choose(index, cx);
        }
        Handled::Yes
    }
}

// ---- Adding an entry ----------------------------------------------------

/// Everything that can go into a menu and is not in it yet: actions,
/// settings and other menus, by name.
struct AddScreen {
    menu: String,
    /// The candidates, sorted by name: a few bytes each, every action,
    /// setting and menu being one (the X4 has little memory).
    candidates: Vec<Candidate>,
    /// Their names, one after the other.
    names: String,
    /// Every menu, sorted: the last of the candidates' sources.
    menus: Vec<Id>,
    /// What is said beside the candidates a plugin took out of this menu
    /// (which plugin), by source: a few at most.
    notes: Vec<(u32, String)>,
    frame: Frame,
}

/// What can be added: where its id is (the actions, then the settings,
/// then [`AddScreen::menus`], counted together) and where its name is.
#[derive(Clone, Copy)]
struct Candidate {
    source: u32,
    start: u32,
    len: u16,
    /// A menu, which shows "›".
    menu: bool,
    /// Taken out of this menu by a plugin, which [`AddScreen::notes`]
    /// says.
    noted: bool,
}

/// Where the candidates come from, in order: an id and the key of its
/// name.
fn sources<'e>(env: &'e Env<'_>, menus: &'e [Id]) -> impl Iterator<Item = (&'e Id, &'e str)> {
    env.actions()
        .iter()
        .map(|a| (a.id(), a.title()))
        .chain(env.settings().specs().map(|s| (&s.id, s.title.as_ref())))
        .chain(menus.iter().map(|m| (m, m.as_str())))
}

/// Case left aside, without making the names anew.
fn by_name(a: &str, b: &str) -> core::cmp::Ordering {
    a.chars()
        .flat_map(char::to_lowercase)
        .cmp(b.chars().flat_map(char::to_lowercase))
        .then_with(|| a.cmp(b))
}

impl AddScreen {
    fn open(env: &Env<'_>, params: &Params) -> Self {
        let menu = params.get(MENU).unwrap_or_default().to_string();
        let present = entries(env, &menu);
        let menus: Vec<Id> = env.menus().menu_ids().into_iter().cloned().collect();
        let actions = env.actions().iter().count();
        let settings = actions + env.settings().specs().count();
        // An id among two sources once (an action's before a setting's).
        let wanted = |at: usize, id: &Id| {
            !present.contains(id)
                && (at < actions || !env.actions().contains(id.as_str()))
                && (at < settings || env.settings().spec(id.as_str()).is_none())
                && !makes_circle(env, &menu, id)
        };
        // Counted first, then made: no list or name grows as it is made.
        let (mut count, mut bytes) = (0, 0);
        for (at, (id, key)) in sources(env, &menus).enumerate() {
            if wanted(at, id) {
                count += 1;
                bytes += env.i18n().tr(key).len();
            }
        }
        let mut candidates = Vec::with_capacity(count);
        let mut names = String::with_capacity(bytes);
        let mut notes = Vec::new();
        for (at, (id, key)) in sources(env, &menus).enumerate() {
            if candidates.len() < count && wanted(at, id) {
                let name = env.i18n().tr(key);
                // Taken out of this menu by a plugin: which, beside it.
                let noted = env.menus().hidden_by(&menu, id.as_str()).map(|by| {
                    let plugin = env
                        .plugins()
                        .get(by.as_str())
                        .map_or(by.as_str(), |info| info.name(env.i18n()));
                    env.i18n().format(
                        "menu-editor.hidden-by",
                        &[("plugin", paperpaw_sdk::Arg::Str(plugin))],
                    )
                });
                candidates.push(Candidate {
                    source: at as u32,
                    start: names.len() as u32,
                    len: name.len().min(usize::from(u16::MAX)) as u16,
                    menu: menus.binary_search(id).is_ok(),
                    noted: noted.is_some(),
                });
                if let Some(note) = noted {
                    notes.push((at as u32, note));
                }
                names.push_str(name);
            }
        }
        let name = |c: &Candidate| &names[c.start as usize..][..usize::from(c.len)];
        candidates.sort_unstable_by(|a, b| by_name(name(a), name(b)).then(a.source.cmp(&b.source)));
        Self {
            menu,
            candidates,
            names,
            notes,
            menus,
            frame: Frame::new(env),
        }
    }

    /// The id of the candidate at `index`.
    fn id(&self, env: &Env<'_>, index: usize) -> Option<Id> {
        let source = self.candidates.get(index)?.source as usize;
        sources(env, &self.menus)
            .nth(source)
            .map(|(id, _)| id.clone())
    }

    fn choose(&mut self, index: usize, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let Some(id) = self.id(&env, index) else {
            return;
        };
        if makes_circle(&env, &self.menu, &id) {
            return;
        }
        let mut ids = entries(&env, &self.menu);
        ids.push(id);
        cx.command(Command::Back);
        save(&self.menu, ids, cx);
    }
}

impl Screen for AddScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let title = env.i18n().tr("menu-editor.add");
        let (candidates, names, notes) = (&self.candidates, &self.names, &self.notes);
        let row = |i: usize| {
            let c = &candidates[i];
            let name = &names[c.start as usize..][..usize::from(c.len)];
            let beside = if c.noted {
                notes
                    .binary_search_by_key(&c.source, |(source, _)| *source)
                    .map_or("", |at| notes[at].1.as_str())
            } else if c.menu {
                "›"
            } else {
                ""
            };
            (name, beside)
        };
        self.frame
            .draw(canvas, env, title, candidates.len(), row, 0);
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let len = self.candidates.len();
        if *action == actions::CONFIRM {
            self.choose(self.frame.list.focus(), cx);
            return Handled::Yes;
        }
        self.frame.on_action(action, len, cx).unwrap_or(Handled::No)
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        let len = self.candidates.len();
        match self.frame.on_tap(gesture, len, cx) {
            Some(index) => {
                self.choose(index, cx);
                Handled::Yes
            }
            None => tap_result(gesture),
        }
    }
}

/// Taps are handled (also on the header); other gestures go on to the key
/// map, e.g. swipes to turn pages.
fn tap_result(gesture: Gesture) -> Handled {
    if gesture.kind == GestureKind::Tap {
        Handled::Yes
    } else {
        Handled::No
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reachability_follows_submenus_and_stops_at_cycles() {
        let menu = |name: &str| -> Vec<Id> {
            match name {
                "a.root" => alloc::vec![Id::new_static("a.x"), Id::new_static("a.sub")],
                "a.sub" => alloc::vec![Id::new_static("a.root"), Id::new_static("core.settings")],
                _ => Vec::new(),
            }
        };
        assert!(reaches(
            &menu,
            "a.root",
            &actions::SETTINGS,
            &mut Vec::new()
        ));
        assert!(!reaches(&menu, "a.root", &OPEN, &mut Vec::new()));
    }

    #[test]
    fn translations_are_complete() {
        use paperpaw_i18n::{Catalog, check};
        let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
        let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
        let issues = check::compare(&en, &de);
        assert!(issues.is_empty(), "{issues:?}");
    }
}
