//! A list of plugins: those of a group, those installed (those with an
//! update first), the newest, those a library picked for its start (a
//! section), or those a search found. A group's list shows the library's
//! fair order of the day where it has one, and can be ordered by name,
//! newest first or most recently updated first; long lists can jump to a
//! letter (from A to Z) and to a page. The plugins to show are picked through the
//! index on the storage once, as the list opens or its order changes, and
//! kept there; the screen holds only those of the page shown.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, events, params};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::install::{Origins, Progress};
use paperpaw_sdk::replies::Typed;
use paperpaw_sdk::ui::choice::Picked;
use paperpaw_sdk::ui::{self, ListChange, ListView, UiStyle};
use paperpaw_sdk::{
    Arg, Canvas, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, Rect, RefreshMode,
    Screen, ask,
};

use crate::catalog::{self, Entry, Index, Order, SECTIONS, name_key};
use crate::letters::LETTER;
use crate::list::{LIST_CHANGED, group_name, open_page, section_name};
use crate::room::Room;
use crate::{DOWNLOADED, INSTALLER, LETTERS_SCREEN, queue, rows};

/// Parameters: which list, its order.
const VIEW: &str = "view";
pub(crate) const ORDER: &str = "order";
/// The newest plugins shown at most.
const NEWEST: usize = 30;

/// The answer to *Sort by*.
const SORTED: Event<Picked> = Event::new("store.sorted");
/// The page to go to.
const JUMPED: Event<Typed> = Event::new("store.jumped");

/// Which plugins a list shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum View {
    Group(usize),
    Installed,
    New,
    /// A section of the library's start (of [`SECTIONS`]).
    Section(usize),
    Search(String),
}

impl View {
    pub(crate) fn params(&self) -> Params {
        let view = match self {
            View::Group(group) => alloc::format!("group:{group}"),
            View::Installed => "installed".to_string(),
            View::New => "new".to_string(),
            View::Section(section) => alloc::format!("section:{section}"),
            View::Search(_) => "search".to_string(),
        };
        let params = Params::new().with(VIEW, view);
        match self {
            View::Search(text) => params.with(params::TEXT, text.clone()),
            _ => params,
        }
    }

    fn of(params: &Params) -> View {
        let view = params.get(VIEW).unwrap_or_default();
        match view {
            "installed" => View::Installed,
            "new" => View::New,
            "search" => View::Search(params.get(params::TEXT).unwrap_or_default().to_string()),
            view if view.starts_with("section:") => View::Section(
                view.strip_prefix("section:")
                    .and_then(|s| s.parse().ok())
                    .filter(|&s| s < SECTIONS.len())
                    .unwrap_or(0),
            ),
            _ => View::Group(
                view.strip_prefix("group:")
                    .and_then(|g| g.parse().ok())
                    .filter(|&g| g < catalog::GROUPS.len())
                    .unwrap_or(0),
            ),
        }
    }
}

/// A row above the plugins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Top {
    /// The fonts browser, with the looks.
    Fonts,
    /// The order, to choose another.
    Sort,
    /// Go to a letter, from A to Z.
    Letter,
    /// Updates all plugins of the list that have an update.
    UpdateAll,
}

pub(crate) struct ViewScreen {
    style: UiStyle,
    header: ui::Header,
    view: View,
    order: Order,
    index: Option<Index>,
    top: Vec<Top>,
    /// How many plugins the list has.
    hits: usize,
    list: ListView,
    status: Rect,
    /// The plugins of the page shown: their place in the index.
    page: Vec<(usize, Entry)>,
    job: Option<Progress>,
    /// The plugins waiting in the update queue (read with the job, never
    /// while drawing).
    queued: Vec<String>,
    /// The plugins of the installed list with an update.
    updates: Vec<String>,
    /// Which versions the store installed (read with the list, never
    /// while drawing).
    origins: Origins,
    /// What the installed list says at its foot of the room left (read
    /// with the list, never while drawing).
    room: Option<String>,
}

impl ViewScreen {
    pub(crate) fn new(env: &Env<'_>, params: &Params) -> Self {
        let style = UiStyle::new(env);
        let header = ui::Header::new(env, &style, true);
        let display = env.device().display;
        let top = header.area().height + style.gap();
        let view = View::of(params);
        // The installed list says at its foot how much room is left, and
        // on the X4's stock layout that a full install gives more: as many
        // lines as that takes.
        let line = if view == View::Installed {
            let one = (style.small() * 1.3) as u16 + style.gap() * 2;
            match Room::of(env, None).line(env) {
                Some(text) => one.max(ui::note_line_height(env, &style, display.width, &text)),
                None => one,
            }
        } else {
            0
        };
        let status = Rect::new(0, display.height - line, display.width, line);
        let area = Rect::new(0, top, display.width, status.y.saturating_sub(top));
        let index = Index::kept(env.storage());
        let order = index.as_ref().map_or(Order::Name, |index| {
            index.order(params.get(ORDER).unwrap_or_default())
        });
        let mut screen = Self {
            list: ListView::new(area, rows::height(&style)).focus_on_keys(env),
            status,
            style,
            header,
            view,
            order,
            index,
            top: Vec::new(),
            hits: 0,
            page: Vec::new(),
            job: INSTALLER.progress(env.storage()),
            updates: Vec::new(),
            origins: Origins::default(),
            queued: queue::waiting(env),
            room: None,
        };
        screen.pick(env);
        screen
    }

    /// Picks the plugins of the list anew.
    fn pick(&mut self, env: &Env<'_>) {
        let Some(index) = &self.index else {
            return;
        };
        let storage = env.storage();
        let mut updates = Vec::new();
        self.origins = Origins::read(storage);
        let origins = &self.origins;
        self.hits = match &self.view {
            View::Group(group) => {
                let group = *group;
                index.pick(
                    storage,
                    |_, e| (e.group == group).then_some(0),
                    self.order,
                    usize::MAX,
                )
            }
            View::Installed => {
                self.room = Room::of(env, None).line(env);
                index.pick(
                    storage,
                    |_, e| {
                        let newer = e.newer_in(env, origins);
                        if newer {
                            updates.push(e.id.clone());
                        }
                        e.installed(env)
                            .filter(|_| !e.switched_off(env))
                            .map(|_| u32::from(newer))
                    },
                    Order::Rank,
                    usize::MAX,
                )
            }
            View::New => index.pick(storage, |_, _| Some(0), Order::Added, NEWEST),
            View::Section(section) => {
                // In the library's order: the first the highest.
                let ids = index.section(storage, *section);
                index.pick(
                    storage,
                    |_, e| {
                        let at = ids.iter().position(|id| *id == e.id)?;
                        Some((ids.len() - at) as u32)
                    },
                    Order::Rank,
                    usize::MAX,
                )
            }
            View::Search(text) => index.search(storage, text),
        };
        self.top.clear();
        match self.view {
            View::Group(group) => {
                let fonts = env.actions().contains(crate::FONTS.as_str());
                if fonts && catalog::GROUPS.get(group) == Some(&"look") {
                    self.top.push(Top::Fonts);
                }
                if self.hits > 0 {
                    self.top.push(Top::Sort);
                }
                // More than a page: a letter is quicker than paging.
                if self.hits > self.list.rows() {
                    self.top.push(Top::Letter);
                }
            }
            View::Installed if !updates.is_empty() => self.top.push(Top::UpdateAll),
            _ => {}
        }
        self.updates = updates;
        self.read_page(env);
    }

    /// Rows of the list: the one above the plugins, then the plugins.
    fn len(&self) -> usize {
        self.top.len() + self.hits
    }

    /// The plugin of row `row`, its number among those of the list.
    fn hit_of(&self, row: usize) -> Option<usize> {
        row.checked_sub(self.top.len())
    }

    /// The plugins of the page shown, read from the index.
    fn read_page(&mut self, env: &Env<'_>) {
        self.page.clear();
        let Some(index) = &self.index else {
            return;
        };
        let rows = self.list.rows();
        let first = self.list.page() * rows;
        for row in first..(first + rows).min(self.len()) {
            if let Some(hit) = self.hit_of(row)
                && let Some(n) = catalog::hit(env.storage(), hit)
                && let Some(plugin) = index.entry(env.storage(), n)
            {
                self.page.push((n, plugin));
            }
        }
    }

    /// The plugin of row `row`, if it is on the page shown.
    fn plugin(&self, row: usize) -> Option<&(usize, Entry)> {
        let first = self.list.page() * self.list.rows();
        let skip = if self.list.page() == 0 {
            self.top.len()
        } else {
            0
        };
        row.checked_sub(first + skip)
            .and_then(|at| self.page.get(at))
    }

    fn activate(&mut self, row: usize, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if let Some(&top) = self.top.get(row) {
            match top {
                Top::Fonts => cx.command(Command::run(crate::FONTS)),
                Top::Letter => {
                    let params = Params::new().with(VIEW, self.view_key());
                    cx.command(Command::OpenScreen(LETTERS_SCREEN, params));
                }
                Top::Sort => {
                    let orders = self.index.as_ref().map_or(&[][..], Index::orders);
                    let options: Vec<&str> = orders.iter().map(|o| o.key()).collect();
                    cx.command(
                        paperpaw_sdk::ask::choice(env.i18n().tr("store.sort"), &SORTED)
                            .options("store.order.", &options)
                            .command(),
                    );
                }
                Top::UpdateAll => {
                    let mut out = paperpaw_sdk::Commands::new();
                    queue::add(&env, &self.updates, &mut out);
                    for command in out {
                        cx.command(command);
                    }
                    self.job = INSTALLER.progress(env.storage());
                    self.queued = queue::waiting(&env);
                    self.redraw(cx);
                }
            }
            return;
        }
        if let Some((n, plugin)) = self.plugin(row) {
            cx.command(open_page(*n, plugin));
        }
    }

    /// What says which list this is, for a letter chosen for it.
    fn view_key(&self) -> String {
        self.view.params().get(VIEW).unwrap_or_default().to_string()
    }

    /// Shows the plugins from `letter` on, from A to Z: the first whose
    /// name sorts at or after it, found by halving.
    fn go_to_letter(&mut self, letter: &str, env: &Env<'_>) {
        if self.order != Order::Name {
            self.order = Order::Name;
            self.pick(env);
        }
        let Some(index) = &self.index else {
            return;
        };
        let storage = env.storage();
        let target = name_key(letter);
        let key_of = |hit: usize| {
            catalog::hit(storage, hit)
                .and_then(|n| index.entry(storage, n))
                .map_or(u64::MAX, |entry| name_key(&entry.name))
        };
        let (mut low, mut high) = (0, self.hits);
        while low < high {
            let mid = (low + high) / 2;
            if key_of(mid) < target {
                low = mid + 1;
            } else {
                high = mid;
            }
        }
        let row = (self.top.len() + low).min(self.len().saturating_sub(1));
        self.list.set_focus(row);
        self.list.set_shown(true);
        self.read_page(env);
    }

    /// Asks which page to go to.
    fn ask_page(&self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let pages = self.list.pages(self.len()) as i64;
        let title = env
            .i18n()
            .format("store.go-to-page", &[("pages", Arg::Int(pages))]);
        cx.command(ask::text(title, &JUMPED).number().command());
    }

    fn changed(&mut self, change: ListChange, cx: &mut Cx<'_, '_>) {
        if change == ListChange::Page {
            let env = *cx.env();
            self.read_page(&env);
        }
        change.request(cx);
    }

    fn title(&self, env: &Env<'_>) -> String {
        match &self.view {
            View::Group(group) => group_name(env, *group),
            View::Installed => env.i18n().tr("store.installed-list").to_string(),
            View::New => env.i18n().tr("store.new").to_string(),
            View::Section(section) => section_name(env, *section),
            View::Search(text) => env
                .i18n()
                .format("store.search.title", &[("query", Arg::Str(text))]),
        }
    }

    fn redraw(&self, cx: &mut Cx<'_, '_>) {
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
    }
}

impl Screen for ViewScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let title = self.title(env);
        let paging = self.list.paging(self.len());
        let mut ui = ui::Ui::new(canvas, env, &style);
        self.header.draw(&mut ui, &title, None, paging);
        if self.len() == 0 {
            let (icon, title, text) = match self.view {
                View::Search(_) => ("search", "store.search.none", "store.search.none.help"),
                _ => ("puzzle", "store.empty", ""),
            };
            ui.empty(
                self.list.area(),
                icon,
                env.i18n().tr(title),
                if text.is_empty() {
                    ""
                } else {
                    env.i18n().tr(text)
                },
            );
            return;
        }
        if let Some(room) = self.room.as_deref().filter(|_| self.status.height > 0) {
            ui.note_line(self.status, room);
        }
        let job = self.job.as_ref();
        let queued = self.queued.as_slice();
        let jump = self.list.pages(self.len()) > 1;
        let room = ui::icon_room(ui.env(), ui.style(), true);
        let top = &self.top;
        let order = self.order;
        let len = self.len();
        self.list.draw(&mut ui, len, |ui, row, rect, focused| {
            if let Some(&top) = top.get(row) {
                let ink = style.focus_ink(focused);
                let (icon, name, value) = match top {
                    Top::Fonts => ("typography", env.i18n().tr("store.fonts"), "›".to_string()),
                    Top::Sort => {
                        let key = alloc::format!("store.order.{}", order.key());
                        (
                            "sort-ascending",
                            env.i18n().tr("store.sort"),
                            env.i18n().tr(&key).to_string(),
                        )
                    }
                    Top::UpdateAll => (
                        "circle-arrow-up",
                        env.i18n().tr("store.update-all"),
                        String::new(),
                    ),
                    Top::Letter => (
                        "letter-case",
                        env.i18n().tr("store.letter"),
                        "›".to_string(),
                    ),
                };
                // Below the order of a long list: how to go to a page.
                if top == Top::Sort && jump {
                    let band = ui::title_area(&style, rect);
                    let rest = ui.row_icon(band, room, Some(icon), ink);
                    ui::Row::value(name, &value).focused(focused).draw(ui, rest);
                    let help = ui::tr_input(env, "store.jump.help");
                    rows::draw_note(ui, rect, rest.x, focused, help);
                    return;
                }
                let rest = ui.row_icon(rect, room, Some(icon), ink);
                ui::Row::value(name, &value).focused(focused).draw(ui, rest);
                return;
            }
            if let Some((_, plugin)) = self.plugin(row) {
                let origins = &self.origins;
                rows::draw(ui, rect, focused, plugin, job, queued, origins);
            }
        });
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        self.job = INSTALLER.progress(env.storage());
        self.queued = queue::waiting(&env);
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let len = self.len();
        if let Some(change) = self.list.navigate_in(cx.env(), action, len) {
            self.changed(change, cx);
        } else if *action == actions::CONFIRM {
            self.activate(self.list.focus(), cx);
        } else if *action == actions::MENU && self.list.pages(len) > 1 {
            // A long press of Select: go to a page.
            self.ask_page(cx);
        } else {
            return Handled::No;
        }
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        let len = self.len();
        let paging = self.list.paging(len);
        let env = *cx.env();
        let point = gesture.start;
        // Go to a page: a tap on the page number, or a long press on it or
        // on the arrows beside it (a larger target for a finger).
        let jump = match gesture.kind {
            GestureKind::Tap => self.header.on_page_number(&env, &self.style, point, paging),
            GestureKind::LongPress => self
                .header
                .tap(&env, &self.style, point, paging)
                .is_some_and(|action| action == actions::NEXT_PAGE || action == actions::PREV_PAGE),
            _ => return Handled::No,
        };
        if jump && self.list.pages(len) > 1 {
            self.ask_page(cx);
            return Handled::Yes;
        }
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        if let Some(action) = self
            .header
            .tap(cx.env(), &self.style, gesture.start, paging)
        {
            cx.command(Command::run(action));
            return Handled::Yes;
        }
        let Some(row) = self.list.hit(gesture.start, len) else {
            return Handled::No;
        };
        self.list.set_focus(row);
        self.activate(row, cx);
        Handled::Yes
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if let Some(typed) = LETTER.read(event, params) {
            if typed.context.get(VIEW) == Some(self.view_key().as_str()) {
                self.go_to_letter(&typed.text, &env);
                self.redraw(cx);
            }
        } else if let Some(picked) = SORTED.read(event, params) {
            self.order = self
                .index
                .as_ref()
                .map_or(Order::Name, |index| index.order(&picked.option));
            self.list.set_focus(0);
            self.pick(&env);
            self.redraw(cx);
        } else if let Some(typed) = JUMPED.read(event, params) {
            let len = self.len();
            if let Ok(page) = typed.text.trim().parse::<usize>() {
                let page = page.clamp(1, self.list.pages(len)) - 1;
                self.list.set_focus(page * self.list.rows());
                self.read_page(&env);
                self.redraw(cx);
            }
        } else if *event == LIST_CHANGED {
            // A fresh list took the place of the one this list picked its
            // plugins from: picked again from it, the focus kept where it
            // still is (else the rows would stay empty).
            self.index = Index::kept(env.storage());
            if self.index.is_none() {
                self.hits = 0;
                self.top.clear();
                self.page.clear();
            }
            self.pick(&env);
            self.list.set_len(self.len());
            self.read_page(&env);
            self.redraw(cx);
        } else if DOWNLOADED.read(event, params).is_some() || *event == events::PLUGINS_CHANGED {
            self.job = INSTALLER.progress(env.storage());
            self.queued = queue::waiting(&env);
            // What is installed and has updates changed.
            if *event == events::PLUGINS_CHANGED && self.view == View::Installed {
                self.pick(&env);
                self.list.set_len(self.len());
            }
            self.redraw(cx);
        }
    }
}
