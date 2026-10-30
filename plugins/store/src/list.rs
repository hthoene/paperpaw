//! The store's start: *Search*, the plugins installed (with how many
//! have an update), what the library picked (recommended, new, popular,
//! recently updated, to discover; the newest where it picks nothing), then
//! the groups with how many plugins each has. Each opens a list of its own ([`crate::view`]); *Fonts* is
//! in the list of looks. The list
//! kept from the last visit shows at once, while a fresh one is fetched.
//!
//! However long the list, the screen keeps only how many plugins each
//! group has; what is installed is counted in one pass through the index
//! on the storage when the list or the plugins change (never while
//! drawing).

use alloc::format;
use alloc::string::ToString;
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, events, params};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::replies::{HttpReply, Typed};
use paperpaw_sdk::ui::sections::Sections;
use paperpaw_sdk::ui::sections::{Item, Section};
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::{
    Command, Cx, Env, Gesture, GestureKind, Handled, Hint, HttpRequest, Id, Params, Rect,
    RefreshMode, Screen, ask,
};

use crate::catalog::{self, Entry, GROUPS, Index, SECTIONS};
use crate::view::View;
use crate::{CATALOG, PAGE_SCREEN, VIEW_SCREEN, rows};

/// The answer with the list.
const LOADED: Event<HttpReply> = Event::new("store.loaded");
/// What is searched for.
const SEARCHED: Event<Typed> = Event::new("store.searched");
/// Parameter of a plugin's page: its place in the index.
pub(crate) const ENTRY: &str = "entry";

/// Why there is no list to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Trouble {
    None,
    Loading,
    Offline,
    Failed,
}

/// What an entry of the start is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    Search,
    Installed,
    New,
    /// What the library picked (of [`SECTIONS`]).
    Section(usize),
    Group(usize),
}

impl Row {
    fn icon(self) -> &'static str {
        match self {
            Row::Search => "search",
            Row::Installed => "circle-check",
            Row::New => "sparkles",
            Row::Section(s) => SECTION_ICONS[s],
            Row::Group(g) => GROUP_ICONS[g],
        }
    }
}

/// The icons of the sections, as [`SECTIONS`].
const SECTION_ICONS: [&str; 5] = ["star", "sparkles", "chart-bar", "circle-arrow-up", "bulb"];

/// The icons of the groups, as [`GROUPS`].
const GROUP_ICONS: [&str; 12] = [
    "book",
    "file-text",
    "books",
    "refresh",
    "home",
    "palette",
    "note",
    "vocabulary",
    "rss",
    "apps",
    "puzzle",
    "settings",
];

/// How many plugins the start counts: per group, installed, with an
/// update.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Counts {
    groups: [usize; GROUPS.len()],
    installed: usize,
    updates: usize,
    /// The plugins of each section this device can install.
    sections: [usize; SECTIONS.len()],
}

pub(crate) struct ListScreen {
    style: UiStyle,
    header: ui::Header,
    area: Rect,
    /// The list read; `None` until one was.
    index: Option<Index>,
    counts: Counts,
    /// The free space, said beside the title (read with the counts, never
    /// while drawing).
    room: Option<alloc::string::String>,
    /// The entries, in order: those of the first section, then the groups.
    rows: Vec<Row>,
    first: usize,
    list: Sections,
    trouble: Trouble,
    /// The focus is shown: always with buttons only, on touch screens
    /// once a button moved it.
    keys: bool,
    /// A fresh list was asked for since the screen opened.
    asked: bool,
    /// Offline mode: the offer to go online for the list.
    offer: ui::offline::OfflineOffer,
}

impl ListScreen {
    pub(crate) fn new(env: &Env<'_>) -> Self {
        // Those installed before origins were kept: the store's.
        paperpaw_sdk::install::adopt(env);
        let style = UiStyle::new(env);
        let header = ui::Header::new(env, &style, true);
        let display = env.device().display;
        let top = header.area().height + style.gap();
        let area = Rect::new(0, top, display.width, display.height.saturating_sub(top));
        let mut screen = Self {
            area,
            index: env
                .catalog_device()
                .and_then(|_| Index::kept(env.storage())),
            counts: Counts::default(),
            room: None,
            rows: Vec::new(),
            first: 0,
            list: Sections::new(area, 0, 1, Vec::new()),
            trouble: Trouble::None,
            keys: ui::focus_shown(env),
            asked: false,
            offer: ui::offline::OfflineOffer::new(),
            style,
            header,
        };
        screen.count(env);
        screen
    }

    /// Counts the plugins anew, and the entries with them.
    fn count(&mut self, env: &Env<'_>) {
        let focus = self.list.focus();
        self.room = crate::room::Room::of(env, None).short(env);
        let mut counts = Counts::default();
        if let Some(index) = &self.index {
            for &(group, count) in index.runs() {
                counts.groups[group] += count;
            }
            let origins = paperpaw_sdk::install::Origins::read(env.storage());
            // A few dozen ids at most: those the library picked.
            let picked: Vec<Vec<alloc::string::String>> = (0..SECTIONS.len())
                .map(|s| {
                    if index.fair() && index.section_len(s) > 0 {
                        index.section(env.storage(), s)
                    } else {
                        Vec::new()
                    }
                })
                .collect();
            index.scan(env.storage(), &mut |_, entry: Entry| {
                if entry.installed(env).is_some() {
                    counts.installed += 1;
                    counts.updates += usize::from(entry.newer_in(env, &origins));
                }
                for (count, ids) in counts.sections.iter_mut().zip(&picked) {
                    *count += usize::from(ids.contains(&entry.id));
                }
            });
        }
        self.counts = counts;
        self.rows.clear();
        if self.index.is_none() {
            self.list = Sections::new(self.area, 0, 1, Vec::new());
            return;
        }
        self.rows.push(Row::Search);
        if counts.installed > 0 {
            self.rows.push(Row::Installed);
        }
        // What the library picked, where it picks; else the newest.
        let picked: Vec<usize> = (0..SECTIONS.len())
            .filter(|&s| counts.sections[s] > 0)
            .collect();
        if picked.is_empty() {
            self.rows.push(Row::New);
        }
        self.rows.extend(picked.into_iter().map(Row::Section));
        self.first = self.rows.len();
        // The looks also without plugins of their own: *Fonts* is there.
        let fonts = env.actions().contains(crate::FONTS.as_str());
        for (group, &count) in counts.groups.iter().enumerate() {
            if count > 0 || (fonts && GROUPS[group] == "look") {
                self.rows.push(Row::Group(group));
            }
        }
        let sections = alloc::vec![
            Section {
                heading: false,
                count: self.first,
            },
            // No heading: the icons tell the groups apart, and so the
            // start fits on one page of the X4.
            Section {
                heading: false,
                count: self.rows.len() - self.first,
            },
        ];
        let heading = ui::heading_height(env, &self.style);
        self.list = Sections::new(self.area, heading, self.style.row(), sections);
        self.list.focus_on(focus);
    }

    fn redraw(&self, cx: &mut Cx<'_, '_>) {
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
    }

    /// Fetches the list anew.
    fn fetch(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let Some(device) = env.catalog_device() else {
            return;
        };
        if !env.caps().network {
            self.trouble = Trouble::Offline;
            return self.redraw(cx);
        }
        let list = env.settings().read(&CATALOG);
        // In the library's fair order, with what it picks for a start;
        // a library that knows neither sends its list as before.
        let url = alloc::format!(
            "{}&order=fair",
            catalog::list_url(
                list.trim(),
                device,
                env.i18n().language(),
                crate::community(&env),
            )
        );
        let mut request = HttpRequest::new("GET", &url, LOADED);
        request = request.save_to(catalog::list_part());
        cx.command(Command::http(request));
        self.trouble = Trouble::Loading;
        self.redraw(cx);
    }

    fn loaded(&mut self, reply: &HttpReply, cx: &mut Cx<'_, '_>) {
        self.trouble = Trouble::None;
        let env = *cx.env();
        let fetched = catalog::list_part();
        let index = catalog::Device::of(&env)
            .filter(|_| (200..300).contains(&reply.status))
            .and_then(|device| catalog::take(env.storage(), &fetched, &device));
        match index {
            Some(index) => {
                self.index = Some(index);
                self.trouble = Trouble::None;
                self.count(&env);
            }
            // Another screen of the store took the same answer first (the
            // store opened twice): its index is the fresh one.
            None if (200..300).contains(&reply.status) && !env.storage().exists(&fetched) => {
                if let Some(index) = Index::kept(env.storage()) {
                    self.index = Some(index);
                    self.trouble = Trouble::None;
                    self.count(&env);
                }
            }
            None => {
                let _ = env.storage().remove(&fetched);
                // The list kept still serves; no answer at all is no
                // connection.
                if self.index.is_none() {
                    self.trouble = if reply.status == 0 {
                        Trouble::Offline
                    } else {
                        Trouble::Failed
                    };
                }
            }
        }
        self.redraw(cx);
    }

    fn activate(&mut self, entry: usize, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let open = |view: View| Command::OpenScreen(VIEW_SCREEN, view.params());
        match self.rows.get(entry).copied() {
            Some(Row::Search) => {
                cx.command(ask::text(env.i18n().tr("store.search"), &SEARCHED).command());
            }
            Some(Row::Installed) => cx.command(open(View::Installed)),
            Some(Row::New) => cx.command(open(View::New)),
            Some(Row::Section(section)) => cx.command(open(View::Section(section))),
            Some(Row::Group(group)) => cx.command(open(View::Group(group))),
            None => {
                if self.retry() {
                    self.fetch(cx);
                }
            }
        }
    }

    /// Whether the offer to go online shows: no list, for Offline mode.
    fn offering(&self, env: &Env<'_>) -> bool {
        self.list.is_empty() && self.trouble == Trouble::Offline && self.offer.shows(env)
    }

    /// Whether *Select* tries again: nothing to show for want of the
    /// list.
    fn retry(&self) -> bool {
        self.list.is_empty() && matches!(self.trouble, Trouble::Offline | Trouble::Failed)
    }

    /// The page and the number of pages, where there is more than one.
    fn paging(&self) -> Option<ui::Paging> {
        let paging = self.list.paging();
        (paging.pages > 1).then_some(paging)
    }

    /// What entry `row` says at its end.
    fn value(&self, row: Row) -> alloc::string::String {
        match row {
            Row::Installed => self.counts.installed.to_string(),
            // The looks without plugins: only the fonts are there.
            Row::Group(group) if self.counts.groups[group] == 0 => "›".to_string(),
            Row::Group(group) => self.counts.groups[group].to_string(),
            Row::Search | Row::New | Row::Section(_) => "›".to_string(),
        }
    }
}

/// The command that opens the page of plugin `n` of the index.
pub(crate) fn open_page(n: usize, plugin: &Entry) -> Command {
    Command::OpenScreen(
        PAGE_SCREEN,
        Params::new()
            .with(params::ID, plugin.id.clone())
            .with(ENTRY, n.to_string()),
    )
}

/// The name of section `section` (of [`SECTIONS`]).
pub(crate) fn section_name(env: &Env<'_>, section: usize) -> alloc::string::String {
    let key = format!("store.section.{}", SECTIONS[section]);
    env.i18n().tr(&key).to_string()
}

/// The name of group `group`.
pub(crate) fn group_name(env: &Env<'_>, group: usize) -> alloc::string::String {
    let key = format!("store.group.{}", GROUPS[group]);
    env.i18n().tr(&key).to_string()
}

impl Screen for ListScreen {
    fn draw(&mut self, canvas: &mut paperpaw_sdk::Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        // A header note may give way to paging on a narrow screen. The
        // loading status must remain visible while the saved list serves.
        let title = if self.trouble == Trouble::Loading && self.index.is_some() {
            env.i18n().tr("store.loading")
        } else {
            env.i18n().tr("store.title")
        };
        let note = self
            .room
            .as_deref()
            .filter(|_| self.index.is_some() && self.paging().is_none());
        let mut ui = ui::Ui::new(canvas, env, &style);
        self.header.draw(&mut ui, title, note, self.paging());
        if self.offering(env) {
            return self.offer.draw(&mut ui, self.area);
        }
        if self.list.is_empty() {
            let (icon, title, text) = match self.trouble {
                _ if env.catalog_device().is_none() => {
                    ("cloud", "store.unsupported", "store.unsupported.help")
                }
                Trouble::Offline => ("wifi-off", "store.offline", "store.offline.help"),
                Trouble::Failed => ("cloud", "store.unreachable", "store.unreachable.help"),
                _ => ("cloud-download", "store.loading", ""),
            };
            let text = if text.is_empty() {
                ""
            } else {
                ui::tr_input(env, text)
            };
            ui.empty(self.area, icon, env.i18n().tr(title), text);
            return;
        }
        let room = ui::icon_room(ui.env(), ui.style(), true);
        self.list
            .draw(&mut ui, self.keys, |ui, item, rect, focused| {
                let entry = match item {
                    Item::Heading(_) => return,
                    Item::Entry(entry) => entry,
                    _ => return,
                };
                let Some(&row) = self.rows.get(entry) else {
                    return;
                };
                let ink = style.focus_ink(focused);
                let rest = ui.row_icon(rect, room, Some(row.icon()), ink);
                let name = match row {
                    Row::Search => env.i18n().tr("store.search"),
                    Row::Installed => env.i18n().tr("store.installed-list"),
                    Row::New => env.i18n().tr("store.new"),
                    Row::Section(section) => &section_name(env, section),
                    Row::Group(group) => &group_name(env, group),
                };
                if row == Row::Installed && self.counts.updates > 0 {
                    // How many have an update, marked as in their rows.
                    let count = self.counts.updates as i64;
                    let text = env.i18n().plural("store.updates-count", count, &[]);
                    let right = f32::from(rect.x + rect.width) - style.margin() as f32;
                    let band = rest;
                    rows::draw_pill(ui, band, right, &text, focused);
                    ui::Row::plain(name).focused(focused).draw(ui, rest);
                    return;
                }
                let value = self.value(row);
                ui::Row::value(name, &value).focused(focused).draw(ui, rest);
            });
    }

    fn hint(&self, action: &Id, env: &Env<'_>) -> Hint {
        if *action == actions::CONFIRM && self.retry() {
            Hint::Text(env.i18n().tr("store.retry").to_string())
        } else {
            Hint::Default
        }
    }

    /// Opened again while open: the list is fetched anew, as on opening.
    fn on_reopen(&mut self, _: &Params, cx: &mut Cx<'_, '_>) {
        self.asked = true;
        self.fetch(cx);
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        if !self.asked {
            self.asked = true;
            self.fetch(cx);
            // Updates waiting since the last visit (a restart, no
            // connection) go on.
            let env = *cx.env();
            let mut out = paperpaw_sdk::Commands::new();
            crate::queue::next(&env, &mut out);
            for command in out {
                cx.command(command);
            }
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let a = action;
        if *a != actions::BACK && self.offering(cx.env()) {
            if self.offer.on_action(a, cx).done() == Some(ui::offline::Offer::Online) {
                self.fetch(cx);
            }
            return Handled::Yes;
        }
        let back = [actions::UP, actions::LEFT, actions::PREVIOUS];
        let forward = [actions::DOWN, actions::RIGHT, actions::NEXT];
        if (back.contains(a) || forward.contains(a)) && !self.keys {
            // The first button press shows where the focus is.
            self.keys = true;
            self.redraw(cx);
            return Handled::Yes;
        }
        let change = if back.contains(a) {
            self.list.step(false)
        } else if forward.contains(a) {
            self.list.step(true)
        } else if *a == actions::NEXT_PAGE {
            self.list.turn(true)
        } else if *a == actions::PREV_PAGE {
            self.list.turn(false)
        } else if *a == actions::CONFIRM {
            let focus = if self.list.is_empty() {
                usize::MAX
            } else {
                self.list.focus()
            };
            self.activate(focus, cx);
            return Handled::Yes;
        } else {
            return Handled::No;
        };
        change.request(cx);
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        if let Some(action) = self
            .header
            .tap(cx.env(), &self.style, gesture.start, self.paging())
        {
            cx.command(Command::run(action));
            return Handled::Yes;
        }
        if self.offering(cx.env()) {
            if self.offer.on_gesture(&gesture, cx).done() == Some(ui::offline::Offer::Online) {
                self.fetch(cx);
            }
            return Handled::Yes;
        }
        if self.list.is_empty() {
            if self.area.contains(gesture.start) {
                self.activate(usize::MAX, cx);
                return Handled::Yes;
            }
            return Handled::No;
        }
        let Some(entry) = self.list.hit(gesture.start) else {
            return Handled::No;
        };
        self.list.set_focus(entry);
        self.activate(entry, cx);
        Handled::Yes
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        if let Some(reply) = LOADED.read(event, params) {
            self.loaded(&reply, cx);
        } else if let Some(typed) = SEARCHED.read(event, params) {
            let text = typed.text.trim();
            if !text.is_empty() {
                let view = View::Search(text.to_string());
                cx.command(Command::OpenScreen(VIEW_SCREEN, view.params()));
            }
        } else if *event == events::PLUGINS_CHANGED {
            // A plugin installed, updated or removed: counted anew.
            let env = *cx.env();
            self.count(&env);
            self.redraw(cx);
        }
    }
}
