//! The home screen: pages of widgets and tools in a grid, like a phone's.
//!
//! - **Pages**: each a grid of cells (4 columns by default, rows to fit
//!   the screen) in a setting of its own (see `pages`; the grid is the
//!   SDK's, `paperpaw_sdk::widgets::grid`): by default
//!   the clock, the book being read and the recent ones, then a page with
//!   all tools. The status line sits above the grid. Swipes and the page
//!   buttons go from page to page; dots below show where one is.
//! - **Items**: widgets (the home screen's own, see `blocks`, and any
//!   plugin's, `Registrar::add_widget`) and tools (actions, as icons),
//!   each taking a rectangle of cells; empty cells stay empty.
//! - **Targets**: what an item does when tapped (open a book, a tool).
//!   With buttons, left and right (or up and down) go from target to
//!   target and confirm runs it; the first press shows where the focus is.
//! - **Editing**: the pencil below the page (reached with the buttons too),
//!   a long press or *Edit home screen* in the main menu (`home.edit-pages`,
//!   also where the pencil is switched off) shows the grid: drag an item to move it, by its
//!   corner to size it; tap it for its options (move or size it with the
//!   buttons, its settings, remove it). *Add* lists what can be added,
//!   and adds or removes pages (see `add`).
//! - **Tools**: the entries of the `home.actions` menu (the library by
//!   default) and of the tools menu; plugins add their own with
//!   `Registrar::add_menu_entry`.
//!
//! The recently read books and their covers come from the library service
//! (`paperpaw_sdk::library`), which follows the standard reading events,
//! so any reader plugin works.
//! The home screen is bound to `core.home`; replace it with any other
//! screen by configuration or with a plugin of higher priority.

#![no_std]

extern crate alloc;

mod add;
mod blocks;
mod covers;
mod drawer;
mod pages;
mod recent;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, events, params, slots};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::help::{self, Zone};
use paperpaw_sdk::ui::choice;
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::widgets;
use paperpaw_sdk::widgets::grid::{self, Geometry, GridResponse, GridView, Item, What};
use paperpaw_sdk::{
    Canvas, Color, Command, Cx, Env, FixedAction, FixedScreen, Gesture, Handled, Hint, Id,
    Manifest, MenuEntry, Params, Plugin, PointerEvent, Priority, Rect, RefreshMode, RegisterError,
    Registrar, Screen,
};

pub const PLUGIN: Id = Id::new_static("home");
pub const SCREEN: Id = Id::new_static("home.dashboard");
/// Shortcuts at the head of the home screen's grid of tools.
pub const ACTIONS: Id = Id::new_static("home.actions");
/// Actions below the page, as icons left of the pencil: any plugin adds
/// its own (`Registrar::add_menu_entry`); none at first.
pub const FOOTER: Id = Id::new_static("home.footer");
/// Adding a widget or tool to a page.
pub(crate) const ADD_SCREEN: Id = Id::new_static("home.add-widget");
/// Every tool at once (swipe up on the home screen).
pub const ALL_TOOLS: Id = Id::new_static("home.all-tools");
/// Edits the home screen's pages: in the main menu, so that buttons alone
/// reach it, also without the pencil.
pub const EDIT_PAGES: Id = Id::new_static("home.edit-pages");
const DRAWER_SCREEN: Id = Id::new_static("home.drawer");
/// An item's options were chosen.
const CHOSEN: Event<choice::Picked> = Event::new("home.item-chosen");

/// The home screen plugin.
#[derive(Debug, Default)]
pub struct Home;

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

static FIXED: [FixedAction; 2] = [
    FixedAction::new(ALL_TOOLS, "home.drawer")
        .handler(|_, _, out| out.push(Command::OpenScreen(DRAWER_SCREEN, Params::new()))),
    // Shown, the home screen edits itself; from elsewhere it opens
    // editing.
    FixedAction::new(EDIT_PAGES, "home.edit-pages").handler(|_, _, out| {
        let params = Params::new().with(params::MODE, "edit");
        out.push(Command::OpenSlot(slots::HOME, params));
    }),
];
static FIXED_ENTRIES: [MenuEntry; 2] = [
    MenuEntry::new(paperpaw_sdk::builtin::menus::MAIN, EDIT_PAGES).order(40),
    MenuEntry::new(ACTIONS, actions::LIBRARY).order(10),
];
static FIXED_ICONS: [(Id, &str); 2] = [(ALL_TOOLS, "apps"), (EDIT_PAGES, "pencil")];

static SCREENS: [FixedScreen; 3] = [
    FixedScreen::new(SCREEN, "home.title", |env, params| {
        Box::new(Dashboard::new(env, params))
    }),
    FixedScreen::new(ADD_SCREEN, "home.edit.add", |env, params| {
        Box::new(add::AddScreen::new(env, params))
    }),
    FixedScreen::new(DRAWER_SCREEN, "home.drawer", |env, _| {
        Box::new(drawer::DrawerScreen::new(env))
    }),
];

impl Plugin for Home {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "Home screen", paperpaw_sdk::package_version!())
            .description(
                "The start page: pages of widgets, such as the book being read and the tools.",
            )
            .how(&HOW)
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screens(&SCREENS)?;
        r.bind_slot(slots::HOME, SCREEN, Priority::DEFAULT)?;
        // The grid's columns or rows changed: the pages fitted to it,
        // what has no room moved to the next page, nothing lost.
        r.on(events::SETTING_CHANGED, |env, changed, out| {
            let grid = [pages::COLUMNS.static_str(), pages::ROWS.static_str()];
            if !grid.contains(&changed.id.as_str()) {
                return;
            }
            if let Some(fitted) = pages::reflow(env, &frame(env).1) {
                out.extend(pages::store_all(&fitted));
            }
        })?;
        r.add_actions(&FIXED)?;
        r.add_icons(&FIXED_ICONS)?;
        // As on phones: up on the home screen shows every tool.
        r.add_keymap(Cow::Borrowed(
            "[keymap.core.home]\nswipe.up = home.all-tools\n",
        ))?;
        // The first time: how the gestures go, or the buttons.
        help::add(
            r,
            SCREEN.as_str(),
            &[
                help::HelpHint::touch("home.tips.quick").zone(Zone::new(0, 0, 1000, 120)),
                help::HelpHint::touch("home.tips.back").zone(Zone::new(0, 400, 150, 300)),
                help::HelpHint::touch("home.tips.home"),
                help::HelpHint::touch("home.tips.tools"),
                help::HelpHint::touch("home.tips.arrange"),
                help::HelpHint::buttons("home.tips.move"),
                help::HelpHint::buttons("home.tips.select"),
                help::HelpHint::buttons("home.tips.menu"),
                help::HelpHint::buttons("home.tips.panel"),
            ],
        )?;
        r.add_menu_entries(&FIXED_ENTRIES)?;
        pages::register(r)?;
        blocks::register(r)?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

/// The home screen's own buttons below the grid; the footer's actions
/// from [`FOOTER_BASE`] on.
const PENCIL: u16 = 0;
const ADD: u16 = 1;
const FOOTER_BASE: u16 = 16;

#[derive(Debug)]
struct Dashboard {
    style: UiStyle,
    page: usize,
    pages: usize,
    grid: GridView,
    /// The status line above the grid, if shown.
    status: Option<Rect>,
    /// The dots and the pencil (when editing: add and done) below.
    footer: Rect,
    pencil: Rect,
    plus: Rect,
    /// The footer's actions ([`FOOTER`]), each where its icon is.
    actions: Vec<(Rect, Id)>,
    /// Recent books whose covers are to be looked up, one a tick.
    missing: Vec<String>,
    /// How many of the recent books the page's widgets show.
    books: usize,
    /// The widgets on the page, by their ids (`home.recent@1`).
    shown: Vec<String>,
    /// What the page shows changed (a setting, the library): laid out
    /// anew before it is drawn next, once for many changes.
    stale: bool,
}

use widgets::next_minute;

/// Where the parts of the home screen go: the status line, the grid, the
/// footer.
fn frame(env: &Env<'_>) -> (Option<Rect>, Geometry, Rect) {
    let style = UiStyle::new(env);
    let footer_height = style.row();
    let display = env.device().display;
    let margin = style.margin();
    let gap = style.gap();
    let footer = Rect::new(
        0,
        display.height.saturating_sub(footer_height),
        display.width,
        footer_height,
    );
    let mut top = gap / 2;
    let status = env.settings().read(&pages::STATUS_BAR).then(|| {
        let height = blocks::status_height(env, display.width);
        let r = Rect::new(
            margin,
            top,
            display.width.saturating_sub(2 * margin),
            height,
        );
        top += height;
        r
    });
    // Half a gap wider than the margins on either side, as the grid
    // leaves half a gap round its cells: they line up with the margins.
    // A margin narrower than that is as wide as that, inside the screen.
    let outer = margin.max(gap / 2);
    let area = Rect::new(
        outer - gap / 2,
        top,
        display.width.saturating_sub(2 * outer) + gap,
        footer.y.saturating_sub(top),
    );
    let columns = env
        .settings()
        .read(&pages::COLUMNS)
        .clamp(2, i64::from(grid::MAX_COLUMNS)) as u8;
    let rows = env
        .settings()
        .read(&pages::ROWS)
        .clamp(0, i64::from(grid::MAX_ROWS)) as u8;
    (status, Geometry::new(env, area, columns, rows), footer)
}

impl Dashboard {
    /// The home screen, laid out when it shows (or is drawn first).
    fn new(env: &Env<'_>, params: &Params) -> Self {
        // Opened to edit a page (from its setting): that page, where it
        // shows.
        let page_set = params.get(params::ID).and_then(pages::slot_of).map(|slot| {
            pages::pages(env)
                .iter()
                .position(|p| p.slot == slot)
                .unwrap_or(0)
        });
        // Opened by *Edit home screen*.
        let editing = page_set.or_else(|| (params.get(params::MODE) == Some("edit")).then_some(0));
        let mut grid = GridView::asking(env, CHOSEN);
        grid.set_host(SCREEN);
        grid.set_editing(editing.is_some());
        let (status, _, footer) = frame(env);
        Self {
            style: UiStyle::new(env),
            page: editing.unwrap_or(0),
            pages: 1,
            grid,
            status,
            footer,
            pencil: Rect::default(),
            plus: Rect::default(),
            actions: Vec::new(),
            missing: Vec::new(),
            books: 0,
            shown: Vec::new(),
            stale: true,
        }
    }

    /// Places the page's items in the grid, and the buttons below.
    fn layout(&mut self, env: &Env<'_>) {
        let style = UiStyle::new(env);
        let display = env.device().display;
        let (status, geometry, footer) = frame(env);
        let pages = pages::pages(env);
        self.pages = pages.len();
        self.page = self.page.min(self.pages - 1);
        let items = pages::expand(env, &pages[self.page].items);
        self.shown = items
            .iter()
            .filter_map(|item| match &item.what {
                What::Widget(id) => Some(id.clone()),
                _ => None,
            })
            .collect();
        // The pencil (done, when editing) at the right of the footer, if
        // it is wanted; add at the left when editing.
        let margin = style.margin();
        let side = footer.height;
        self.pencil = Rect::new(display.width - margin - side, footer.y, side, side);
        self.plus = Rect::new(margin, footer.y, side, side);
        let editing = self.grid.editing();
        let mut buttons = Vec::new();
        // The footer's actions left of the pencil, from the right, as many
        // as leave the dots of the pages their room in the middle.
        self.actions.clear();
        if !editing {
            let pencil = env.settings().read(&pages::FOOTER_PENCIL);
            let mut right = if pencil {
                self.pencil.x
            } else {
                self.pencil.x + side
            };
            let least = display.width / 2 + side;
            for item in env.menus().shown_items(env, FOOTER.as_str()) {
                if item.section || right < least + side {
                    continue;
                }
                right -= side;
                self.actions
                    .push((Rect::new(right, footer.y, side, side), item.id.clone()));
            }
            for (at, (area, _)) in self.actions.iter().enumerate().rev() {
                buttons.push((*area, FOOTER_BASE + at as u16));
            }
        }
        if editing {
            buttons.push((self.plus, ADD));
        }
        if editing || env.settings().read(&pages::FOOTER_PENCIL) {
            buttons.push((self.pencil, PENCIL));
        }
        self.grid
            .set_tool_labels(env.settings().read(&blocks::TOOL_LABELS));
        self.grid.layout(env, geometry, &items, &buttons);
        self.books = blocks::books_shown(env, &items);
        self.status = status;
        self.footer = footer;
        self.style = style;
        self.stale = false;
    }

    /// Lays the page out anew before it is drawn next.
    fn mark_stale(&mut self, cx: &mut Cx<'_, '_>) {
        self.stale = true;
        cx.request_redraw();
    }

    /// Lays the screen out again and draws it.
    fn relayout(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        self.layout(&env);
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
    }

    /// Does what the grid asks for.
    fn respond(&mut self, response: GridResponse, cx: &mut Cx<'_, '_>) -> Handled {
        let handled = response.handled();
        match response {
            GridResponse::Changed(items) => self.store(&items, cx),
            GridResponse::Editing(_) => self.relayout(cx),
            GridResponse::Button(PENCIL) => self.set_editing(!self.grid.editing(), cx),
            GridResponse::Button(ADD) => cx.command(Command::OpenScreen(
                ADD_SCREEN,
                Params::new().with(add::PAGE, format!("{}", self.page)),
            )),
            GridResponse::Button(n) if n >= FOOTER_BASE => {
                if let Some((_, action)) = self.actions.get(usize::from(n - FOOTER_BASE)) {
                    cx.command(Command::run(action.clone()));
                }
            }
            _ => {}
        }
        handled
    }

    fn turn(&mut self, forward: bool, cx: &mut Cx<'_, '_>) {
        let to = if forward {
            (self.page + 1 < self.pages).then_some(self.page + 1)
        } else {
            self.page.checked_sub(1)
        };
        if let Some(to) = to {
            self.page = to;
            self.grid.reset_focus();
            self.relayout(cx);
        }
    }

    /// Begins or ends editing the page.
    fn set_editing(&mut self, on: bool, cx: &mut Cx<'_, '_>) {
        self.grid.set_editing(on);
        self.relayout(cx);
    }

    /// Keeps the page as `items`.
    fn store(&self, items: &[Item], cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if let Some(page) = pages::pages(&env).get(self.page) {
            cx.command(pages::store(&env, page.slot, items));
        }
    }

    fn draw_footer(&self, ui: &mut ui::Ui<'_>) {
        let (env, style) = (ui.env(), self.style);
        let f = self.footer;
        let editing = self.grid.editing();
        // Status items at the left, where adding goes while editing.
        if !editing {
            let font = style.font(env);
            if let Some(font) = font {
                let margin = style.margin();
                let width = (f.width / 2).saturating_sub(margin + style.gap() * 3);
                let line = paperpaw_sdk::status::StatusLine::new(
                    Rect::new(margin, f.y, width, f.height),
                    [env.settings().read(&pages::FOOTER_ITEMS), "", ""],
                    style.small(),
                    style.faint(ui::Ink::NOTE),
                );
                line.draw(ui.canvas(), env, &font, &Params::new());
            }
        }
        // A dot a page, the current one filled.
        if self.pages > 1 && (editing || env.settings().read(&pages::FOOTER_DOTS)) {
            let dot = (u32::from(style.gap()) * 2 / 3).max(6);
            let spacing = dot * 2;
            let total = dot * self.pages as u32 + (spacing - dot) * (self.pages as u32 - 1);
            let mut x = i32::from(f.x) + (i32::from(f.width) - total as i32) / 2;
            let y = i32::from(f.y) + (i32::from(f.height) - dot as i32) / 2;
            for page in 0..self.pages {
                pages::draw_dot(ui, (x, y), dot, page == self.page);
                x += spacing as i32;
            }
        }
        let size = style.title();
        let mut icon = |name: &str, area: Rect, ink: Color| {
            let w = ui::icon_width(env, name, size);
            let x = f32::from(area.x) + (f32::from(area.width) - w) / 2.0;
            ui.icon(name, size, x, area, ink);
        };
        if editing {
            icon("plus", self.plus, Color::BLACK);
            icon("check", self.pencil, Color::BLACK);
        } else if env.settings().read(&pages::FOOTER_PENCIL) {
            icon("pencil", self.pencil, style.faint(ui::Ink::NOTE));
        }
        if !editing {
            for (area, action) in &self.actions {
                let name = env.menus().icon(action.as_str()).unwrap_or("puzzle");
                let w = ui::icon_width(env, name, size);
                let x = f32::from(area.x) + (f32::from(area.width) - w) / 2.0;
                ui.icon(name, size, x, *area, style.faint(ui::Ink::NOTE));
                let title = env
                    .menus()
                    .title(action.as_str())
                    .map_or(action.as_str(), |t| env.i18n().tr(t));
                ui.canvas()
                    .describe(paperpaw_sdk::semantics::Role::Button, title, *area);
            }
        }
    }
}

impl Screen for Dashboard {
    /// A screen above needs the memory: the widgets go (covers made
    /// small, what they read), made again when the page is drawn next.
    fn on_squeeze(&mut self, _: &paperpaw_sdk::realtime::Squeeze, _: &mut Cx<'_, '_>) -> Handled {
        self.grid.let_go();
        self.stale = true;
        Handled::Yes
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        if self.stale {
            self.layout(env);
        }
        let style = self.style;
        let mut ui = ui::Ui::new(canvas, env, &style);
        if let Some(status) = self.status {
            blocks::draw_status(ui.canvas(), env, status);
        }
        self.draw_footer(&mut ui);
        self.grid.draw(ui.canvas(), env);
    }

    /// Hidden by a book or another screen: the widgets go with what they
    /// keep (covers made small), made anew when it shows again (or is
    /// drawn below an overlay left open).
    fn on_hide(&mut self) {
        self.grid.release();
        self.stale = true;
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        // The reader may have changed the recent books meanwhile.
        let env = *cx.env();
        // Each time it shows, the page as it is: made anew after it was
        // hidden (its widgets went) or changed, else what they show read
        // again. The focus where wanted.
        self.grid
            .set_focus_shown(ui::focus_shown(&env) && env.settings().read(&blocks::FOCUS_AT_ONCE));
        if self.stale {
            self.layout(&env);
        } else {
            self.grid.update_widgets(&env, cx);
        }
        cx.request_redraw();
        // The covers not known yet are looked up first, one a tick.
        self.missing = blocks::missing_covers(&env, self.books);
        self.missing.reverse();
        if self.missing.is_empty() {
            cx.wake_at(next_minute(&env));
        } else {
            cx.wake_at(env.now());
        }
    }

    /// Looks up the covers not known yet, one a tick; then, each minute,
    /// updates the widgets (the time) and draws those that changed.
    fn on_timer(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let mut changed = false;
        if let Some(path) = self.missing.pop() {
            if covers::look_up(&env, &path).is_some() {
                changed |= self.grid.update_widget(pages::CONTINUE, &env, cx);
                changed |= self.grid.update_widget(pages::RECENT, &env, cx);
            }
            if self.missing.is_empty() {
                cx.wake_at(next_minute(&env));
            } else {
                cx.wake_at(env.now().add_millis(20));
            }
        } else {
            if let Some(status) = self.status {
                cx.request_redraw_area(status);
            }
            changed = self.grid.update_live(&env, cx);
            cx.wake_at(next_minute(&env));
        }
        if changed {
            cx.request_refresh(RefreshMode::Fast);
        }
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        let response = self.grid.on_event(event, params, cx);
        let env = *cx.env();
        if response != GridResponse::Ignored {
            self.respond(response, cx);
            return;
        }
        match widgets::refresh(event, params, PLUGIN.as_str(), &self.shown) {
            widgets::Refresh::Remake => {
                // The widgets made anew, what they show read again.
                self.mark_stale(cx);
            }
            // What the widgets show read again, where they are made (when
            // hidden, they are made anew as it shows).
            widgets::Refresh::Read => {
                if !self.stale && self.grid.update_widgets(&env, cx) {
                    cx.request_refresh(RefreshMode::Fast);
                }
            }
            widgets::Refresh::ReadOne(id) => {
                if self.grid.update_widget(id.as_str(), &env, cx) {
                    cx.request_refresh(RefreshMode::Fast);
                }
            }
            widgets::Refresh::Live => {
                if let Some(status) = self.status {
                    cx.request_redraw_area(status);
                }
                self.grid.update_live(&env, cx);
                cx.request_refresh(RefreshMode::Fast);
            }
            _ => {}
        }
    }

    /// While the home screen is arranged, back ends it.
    fn hint(&self, action: &Id, env: &Env<'_>) -> Hint {
        self.grid.hint(action, env)
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        if *action == EDIT_PAGES {
            self.set_editing(!self.grid.editing(), cx);
            return Handled::Yes;
        }
        let response = self.grid.on_action(action, cx);
        if response != GridResponse::Ignored {
            return self.respond(response, cx);
        }
        if *action == actions::NEXT_PAGE || *action == actions::PREV_PAGE {
            self.turn(*action == actions::NEXT_PAGE, cx);
            return Handled::Yes;
        }
        Handled::No
    }

    /// While editing, the edges are the grid's.
    fn holds_edges(&self) -> bool {
        self.grid.holds_edges()
    }

    fn on_pointer(&mut self, event: &PointerEvent, cx: &mut Cx<'_, '_>) -> Handled {
        let response = self.grid.on_pointer(event, cx);
        self.respond(response, cx)
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        // An item of the status line above the grid that does something.
        if let (Some(status), paperpaw_sdk::GestureKind::Tap) = (self.status, gesture.kind) {
            let env = *cx.env();
            let target = blocks::status_bar_targets(&env, status)
                .into_iter()
                .find(|t| t.area.contains(gesture.start));
            if let Some(target) = target {
                cx.command(target.command);
                return Handled::Yes;
            }
        }
        let response = self.grid.on_gesture(gesture, cx);
        self.respond(response, cx)
    }
}

#[cfg(test)]
mod tests {
    use paperpaw_i18n::{Catalog, check};

    #[test]
    fn translations_are_complete() {
        let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
        let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
        let issues = check::compare(&en, &de);
        assert!(issues.is_empty(), "{issues:?}");
    }
}
