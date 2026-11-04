//! The home screen: the picture, the pages of widgets and tools in a grid,
//! the status bar and, below, the dots of the pages and the pencil.
//!
//! The grid is the SDK's ([`paperpaw_sdk::widgets::grid`]), as the
//! built-in home screen's: drag an item to move it, by its corner to size
//! it, tap it for its options. Widgets sit on plates over the picture (the
//! grid paints each item's cells in the page's paper first). The bars are
//! this screen's own: the status bar is drawn here; the row of hints
//! below belongs to the kernel, which this screen asks to show it always,
//! never or after a press ([`Screen::hints`]).

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, params};
use paperpaw_sdk::document::ImageRef;
use paperpaw_sdk::event::Event;
use paperpaw_sdk::ui::{self, UiStyle, choice};
use paperpaw_sdk::widgets::grid::{self, Geometry, GridResponse, GridView, Item, What};
use paperpaw_sdk::widgets::{self, Plate, next_minute};
use paperpaw_sdk::{
    Canvas, Color, Command, Cx, Env, Gesture, GestureKind, Handled, Hint, HintVisibility, Id,
    Instant, Params, PointerEvent, Rect, RefreshMode, Screen,
};

use crate::bar::{self, Look, Placing, Visible};
use crate::picture::{self, Failure, Recipe, Render};
use crate::settings;
use crate::{ADD_SCREEN, PLUGIN, SCREEN, add, pages};

/// An item's options were chosen.
const CHOSEN: Event<choice::Picked> = Event::new("picture-home.item-chosen");

/// The built-in home screen's action that edits the pages, bound in the
/// main menu: it reaches this screen too, which stands in for it.
const EDIT_PAGES: Id = Id::new_static("home.edit-pages");

/// The pencil and the add button below the grid.
const PENCIL: u16 = 0;
const ADD: u16 = 1;

/// How long a bar that shows after a press stays, as the kernel's row of
/// hints does.
const BAR_MS: u64 = 4000;

#[allow(missing_debug_implementations, reason = "a screen")]
pub(crate) struct Dashboard {
    style: UiStyle,
    page: usize,
    pages: usize,
    grid: GridView,
    /// The widgets on the page, by their ids.
    shown: Vec<String>,
    stale: bool,
    hidden: bool,
    /// The bar's plate, where it has room (any visibility but off).
    bar: Option<Rect>,
    /// The footer, the pencil and the add button.
    footer: Rect,
    pencil: Rect,
    plus: Rect,
    /// The picture behind the page, made for this screen.
    picture: Option<ImageRef>,
    /// What the picture is to be made of, while it is not made yet.
    pending: Option<Recipe>,
    /// How the picture is rendered, as the settings said at the last layout.
    render: Render,
    /// What the widgets sit on, as the settings said at the last layout.
    plate: Plate,
    /// The notice "preparing" shows before the picture is made.
    preparing: bool,
    /// What failed to be made, so that it is not tried again and again.
    failed: Option<String>,
    /// The key of the picture made last, so that one made is not made again.
    made: Option<String>,
    /// When a bar that shows after a press goes again.
    revealed_until: Option<Instant>,
    /// How bright the picture is behind the bar where it has no plate.
    behind: Option<u8>,
    /// How the row of hints below shows, read when the screen is made.
    hints: Option<HintVisibility>,
    /// A bar shows after a press (the status bar or the row of hints).
    auto: bool,
}

/// Whether the screen is on its side: wider than high.
fn landscape(env: &Env<'_>) -> bool {
    let display = env.device().display;
    display.width > display.height
}

/// How a bar setting says it shows.
fn visible(
    env: &Env<'_>,
    setting: &paperpaw_sdk::setting::Setting<paperpaw_sdk::setting::Text>,
) -> Visible {
    Visible::of(env.settings().text(setting.as_str()))
}

/// How the row of hints below shows: always, never or after a press.
fn hint_visibility(env: &Env<'_>) -> HintVisibility {
    match visible(env, &settings::HINTS) {
        Visible::Off => HintVisibility::Hidden,
        Visible::Auto => HintVisibility::OnPress,
        Visible::On => HintVisibility::Shown,
    }
}

/// Whether a bar shows after a press.
/// What the widgets sit on: white, as the built-in home screen; a gray or
/// colour; or nothing.
fn widget_plate(env: &Env<'_>) -> Plate {
    match widgets::colour_spec(env.settings().text(settings::PLATE.as_str())) {
        widgets::ColourSpec::Is(colour) => Plate::Colour(colour),
        widgets::ColourSpec::None => Plate::None,
        _ => Plate::Paper,
    }
}

fn auto(env: &Env<'_>) -> bool {
    // A device without buttons has no row of hints to show.
    visible(env, &settings::BAR) == Visible::Auto
        || (visible(env, &settings::HINTS) == Visible::Auto && env.device().input.buttons)
}

/// The pages' items for this screen: as kept upright; on its side placed
/// anew in order.
fn page_items(env: &Env<'_>, items: &[Item]) -> Vec<Item> {
    let expanded = pages::expand(env, items);
    if landscape(env) {
        expanded.into_iter().map(|i| Item::new(i.what)).collect()
    } else {
        expanded
    }
}

/// Where the parts go on this screen: the bar's plate (where it has a
/// strip), the grid, and the footer. The bar's strip is kept free, shown
/// or not, so that the page does not move when it comes.
fn frame(env: &Env<'_>) -> (Option<Rect>, Geometry, Rect) {
    let style = UiStyle::new(env);
    let display = env.device().display;
    let (width, height) = (display.width, display.height);
    let wide = width > height;
    let margin = style.margin();
    let gap = widgets::gap(env);
    let footer = Rect::new(0, height.saturating_sub(style.row()), width, style.row());
    let mut top = gap / 2;
    let mut bottom = footer.y;
    let placing = Placing::of(env);
    let plate = (visible(env, &settings::BAR) != Visible::Off).then(|| {
        let high = bar::height(env);
        let strip = if placing.bottom {
            bottom = bottom.saturating_sub(high);
            Rect::new(0, bottom, width, high)
        } else {
            let strip = Rect::new(0, top, width, high);
            top += high;
            strip
        };
        bar::rect(env, placing, strip)
    });
    // Half a gap wider than the margins on either side, as the grid leaves
    // half a gap round its cells: they line up with the margins.
    let outer = margin.max(gap / 2);
    let area = Rect::new(
        outer - gap / 2,
        top,
        width.saturating_sub(2 * outer) + gap,
        bottom.saturating_sub(top),
    );
    let columns = env
        .settings()
        .read(&settings::COLUMNS)
        .clamp(2, i64::from(grid::MAX_COLUMNS)) as u8;
    let (columns, rows) = if wide {
        (grid::columns_for(area, columns), 0)
    } else {
        let rows = env
            .settings()
            .read(&settings::ROWS)
            .clamp(0, i64::from(grid::MAX_ROWS)) as u8;
        (columns, rows)
    };
    (plate, Geometry::new(env, area, columns, rows), footer)
}

/// The grid of the page on this screen.
pub(crate) fn geometry(env: &Env<'_>) -> Geometry {
    frame(env).1
}

impl Dashboard {
    pub(crate) fn new(env: &Env<'_>, params: &Params) -> Self {
        // Opened to edit a page (from its setting), or by *Edit home
        // screen*.
        let page_set = params.get(params::ID).and_then(pages::slot_of).map(|slot| {
            pages::pages(env)
                .iter()
                .position(|p| p.slot == slot)
                .unwrap_or(0)
        });
        let editing = page_set.or_else(|| (params.get(params::MODE) == Some("edit")).then_some(0));
        let mut grid = GridView::asking(env, CHOSEN);
        grid.set_host(SCREEN);
        grid.set_editing(editing.is_some() && !landscape(env));
        Self {
            style: UiStyle::new(env),
            page: editing.unwrap_or(0),
            pages: 1,
            grid,
            shown: Vec::new(),
            stale: true,
            hidden: false,
            bar: None,
            footer: Rect::default(),
            pencil: Rect::default(),
            plus: Rect::default(),
            picture: None,
            pending: None,
            render: Render::Automatic,
            plate: Plate::Paper,
            preparing: false,
            failed: None,
            made: None,
            revealed_until: None,
            behind: None,
            hints: Some(hint_visibility(env)),
            auto: auto(env),
        }
    }

    /// Whether the picture shows: not while the page is arranged, which
    /// needs its cells seen.
    fn shows_picture(&self) -> bool {
        self.picture.is_some() && !self.grid.editing()
    }

    /// Whether a bar that shows after a press is hidden now (the kernel's
    /// row of hints counts as one).
    fn auto_hidden(&self, env: &Env<'_>) -> bool {
        self.auto
            && !self.grid.editing()
            && self.revealed_until.is_none_or(|until| env.now() >= until)
    }

    /// Whether the status bar is drawn now.
    fn bar_shown(&self, env: &Env<'_>) -> bool {
        match visible(env, &settings::BAR) {
            Visible::Off => false,
            Visible::On => true,
            Visible::Auto => self.revealed_until.is_some_and(|until| env.now() < until),
        }
    }

    /// Places the page's items in the grid, the bar and the buttons below.
    fn layout(&mut self, env: &Env<'_>) {
        let style = UiStyle::new(env);
        let display = env.device().display;
        let (width, height) = (display.width, display.height);
        let wide = width > height;
        let margin = style.margin();
        let (plate, geometry, footer) = frame(env);
        let all = pages::pages(env);
        self.pages = all.len();
        self.page = self.page.min(self.pages - 1);
        let items = page_items(env, &all[self.page].items);
        self.shown = items
            .iter()
            .filter_map(|item| match &item.what {
                What::Widget(id) => Some(id.clone()),
                _ => None,
            })
            .collect();
        // The pencil (done, when editing) at the right of the footer; add
        // at the left when editing. Not on its side: the layout is made
        // upright.
        let side = footer.height;
        self.pencil = Rect::new(width - margin - side, footer.y, side, side);
        self.plus = Rect::new(margin, footer.y, side, side);
        let editing = self.grid.editing();
        let mut buttons = Vec::new();
        if editing {
            buttons.push((self.plus, ADD));
        }
        if editing || (env.settings().read(&settings::PENCIL) && !wide) {
            buttons.push((self.pencil, PENCIL));
        }
        self.grid.layout(env, geometry, &items, &buttons);
        self.bar = plate;
        self.footer = footer;
        self.style = style;
        self.stale = false;
        self.auto = auto(env);
        self.render = Render::of(env.settings().text(settings::RENDER.as_str()));
        self.hints = Some(hint_visibility(env));
        self.plate = widget_plate(env);
        self.find_picture(env);
        // Where the bar has no plate, its ink follows the picture.
        self.behind = match (self.bar, &self.picture) {
            (Some(plate), Some(image))
                if Look::of(env.settings().text(settings::BAR_LOOK.as_str())) == Look::Clear =>
            {
                picture::brightness(env, image, plate)
            }
            _ => None,
        };
    }

    /// The baked picture where it is made for this screen; else it is to
    /// be made.
    fn find_picture(&mut self, env: &Env<'_>) {
        let display = env.device().display;
        let Some(recipe) = Recipe::of(env, display.width, display.height) else {
            self.picture = None;
            self.pending = None;
            // The picture was taken away: its made copy is of no use.
            picture::forget_made(env);
            return;
        };
        if self.failed.as_deref() == Some(recipe.key().as_str()) {
            self.picture = None;
            self.pending = None;
            return;
        }
        match picture::baked(env, &recipe) {
            Some(image) => {
                self.picture = Some(image);
                self.pending = None;
            }
            // Made on this screen already, and still not found: it would be
            // made again and again. Taken as failed.
            None if self.made.as_deref() == Some(recipe.key().as_str()) => {
                self.failed = self.made.take();
                self.picture = None;
                self.pending = None;
            }
            None => {
                self.picture = None;
                self.pending = Some(recipe);
            }
        }
    }

    /// How a small change is shown: the fast refresh is black and white,
    /// which breaks the picture's grays into dots (as on the built-in home
    /// screen after a change); with grey levels the screen is refreshed in
    /// part, with its grays.
    fn quick(&self) -> RefreshMode {
        if self.picture.is_some() && self.render == Render::Grays {
            RefreshMode::Partial
        } else {
            RefreshMode::Fast
        }
    }

    fn relayout(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        self.layout(&env);
        self.wake(cx);
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
    }

    /// Asks to be woken for the next minute, the picture still to be
    /// made, or when a bar goes.
    fn wake(&self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let at = if self.pending.is_some() {
            env.now().add_millis(30)
        } else {
            let minute = next_minute(&env);
            self.revealed_until
                .map_or(minute, |until| until.min(minute))
        };
        cx.wake_at(at);
    }

    /// Makes the picture, in two steps so that the notice shows first.
    fn make_picture(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let Some(recipe) = self.pending.clone() else {
            return;
        };
        if !self.preparing {
            self.preparing = true;
            cx.request_redraw();
            cx.request_refresh(RefreshMode::Fast);
            return;
        }
        // The widgets go while the picture is decoded: the memory is wanted.
        self.grid.let_go();
        let made = picture::bake(&env, &recipe);
        self.preparing = false;
        self.pending = None;
        match made {
            Ok(image) => {
                self.made = Some(recipe.key());
                self.picture = Some(image);
            }
            Err(failure) => {
                self.failed = Some(recipe.key());
                let key = match failure {
                    Failure::Unreadable => "picture-home.unreadable",
                    Failure::Short => "picture-home.short",
                };
                cx.command(Command::Notify(env.i18n().tr(key).to_string()));
            }
        }
        self.layout(&env);
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Full);
    }

    /// Shows the bars that show after a press, for a few seconds.
    fn reveal(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        self.revealed_until = Some(env.now().add_millis(BAR_MS));
        if let Some(bar) = self.bar {
            cx.request_redraw_area(bar);
        }
        cx.request_refresh(RefreshMode::Partial);
        self.widen(cx);
        self.wake(cx);
    }

    /// Takes the bars that show after a press away again once their time
    /// is over.
    fn conceal(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if self.revealed_until.is_some_and(|until| env.now() >= until) {
            self.revealed_until = None;
            if let Some(bar) = self.bar {
                cx.request_redraw_area(bar);
            }
            cx.request_refresh(RefreshMode::Partial);
            self.widen(cx);
        }
    }

    /// With grey levels a change shows with the whole screen: where the
    /// panel shows its grays with a refresh of the whole screen, a small
    /// change over a picture in grays would wait for the next page.
    fn widen(&self, cx: &mut Cx<'_, '_>) {
        if self.picture.is_some() && self.render == Render::Grays {
            cx.request_redraw();
            cx.request_refresh(RefreshMode::Partial);
        }
    }

    fn respond(&mut self, response: GridResponse, cx: &mut Cx<'_, '_>) -> Handled {
        let handled = response.handled();
        if response != GridResponse::Ignored {
            self.widen(cx);
        }
        match response {
            GridResponse::Changed(items) => self.store(&items, cx),
            GridResponse::Editing(_) => self.relayout(cx),
            GridResponse::Button(PENCIL) => self.set_editing(!self.grid.editing(), cx),
            GridResponse::Button(ADD) => cx.command(Command::OpenScreen(
                ADD_SCREEN,
                Params::new().with(add::PAGE, format!("{}", self.page)),
            )),
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

    fn set_editing(&mut self, on: bool, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        // Pages are arranged upright: on its side they are derived.
        if on && landscape(&env) {
            cx.command(Command::Notify(
                env.i18n().tr("picture-home.edit-upright").to_string(),
            ));
            return;
        }
        self.grid.set_editing(on);
        self.relayout(cx);
    }

    /// Keeps the page as `items`.
    fn store(&self, items: &[Item], cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let all = pages::pages(&env);
        if let Some(page) = all.get(self.page) {
            for command in pages::store(&env, page.slot, items) {
                cx.command(command);
            }
        }
    }

    fn draw_footer(&self, ui: &mut ui::Ui<'_>) {
        let (env, style) = (ui.env(), self.style);
        let f = self.footer;
        let editing = self.grid.editing();
        let plated = self.shows_picture();
        let paper = Color::WHITE;
        // A dot a page, the current one filled, on a pill over a picture.
        if self.pages > 1 && (editing || env.settings().read(&settings::DOTS)) {
            let dot = (u32::from(style.gap()) * 2 / 3).max(6);
            let spacing = dot * 2;
            let total = dot * self.pages as u32 + (spacing - dot) * (self.pages as u32 - 1);
            let mut x = i32::from(f.x) + (i32::from(f.width) - total as i32) / 2;
            let y = i32::from(f.y) + (i32::from(f.height) - dot as i32) / 2;
            if plated {
                let pad = dot as i32;
                ui.canvas().fill_round_rect(
                    x - pad,
                    y - pad / 2,
                    total + 2 * pad as u32,
                    dot + pad as u32,
                    (dot + pad as u32) / 2,
                    paper,
                );
            }
            for page in 0..self.pages {
                draw_dot(ui, (x, y), dot, page == self.page);
                x += spacing as i32;
            }
        }
        let size = style.title();
        let mut icon = |name: &str, area: Rect, ink: Color, plate: bool| {
            if plate {
                let side = u32::from(area.width.min(area.height));
                ui.canvas().fill_round_rect(
                    i32::from(area.x),
                    i32::from(area.y),
                    side,
                    side,
                    side / 2,
                    paper,
                );
            }
            let w = ui::icon_width(env, name, size);
            let x = f32::from(area.x) + (f32::from(area.width) - w) / 2.0;
            ui.icon(name, size, x, area, ink);
        };
        if editing {
            icon("plus", self.plus, Color::BLACK, false);
            icon("check", self.pencil, Color::BLACK, false);
        } else if env.settings().read(&settings::PENCIL) && !landscape(env) {
            icon("pencil", self.pencil, style.faint(ui::Ink::NOTE), plated);
        }
    }

    /// The bar, where it shows.
    fn draw_bar(&self, ui: &mut ui::Ui<'_>) {
        let env = ui.env();
        let Some(plate) = self.bar else { return };
        if !self.bar_shown(env) {
            return;
        }
        let look = Look::of(env.settings().text(settings::BAR_LOOK.as_str()));
        let behind = if self.grid.editing() {
            None
        } else {
            self.behind
        };
        bar::draw(ui.canvas(), env, plate, look, behind);
    }
}

/// A dot of a page, `size` pixels wide at `x`, `y`, filled for the current
/// one.
fn draw_dot(ui: &mut ui::Ui<'_>, (x, y): (i32, i32), size: u32, filled: bool) {
    let style = ui.style();
    let radius = if style.pill() { size / 2 } else { 0 };
    if filled {
        ui.canvas()
            .fill_round_rect(x, y, size, size, radius, Color::BLACK);
    } else {
        let line = u32::from(style.line()).max(1);
        ui.canvas()
            .stroke_round_rect(x, y, size, size, line, radius, Color::BLACK);
    }
}

impl Screen for Dashboard {
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
        let display = env.device().display;
        let mut ui = ui::Ui::new(canvas, env, &style);
        if self.preparing {
            // Nothing else while the picture is made.
            ui.empty(
                Rect::new(0, 0, display.width, display.height),
                "camera",
                env.i18n().tr("picture-home.preparing"),
                env.i18n().tr("picture-home.preparing.text"),
            );
            return;
        }
        if self.shows_picture()
            && let Some(image) = &self.picture
        {
            picture::draw(ui.canvas(), env, image);
        }
        self.draw_bar(&mut ui);
        self.draw_footer(&mut ui);
        self.grid.draw_on(ui.canvas(), env, self.plate);
    }

    /// Read when the screen is made (and where the row would come).
    fn hints(&self) -> Option<HintVisibility> {
        self.hints
    }

    /// Whether the picture is shown in grays: where the user's setting for
    /// pictures says so too (automatic, as the built-in home screen), not
    /// where the picture is made in dots.
    fn pictures_in_grays(&self) -> bool {
        self.shows_picture() && self.render != Render::Dithered
    }

    /// Grey levels: the screen is shown so whenever it is on top (where the
    /// panel has few grays but shows more with a refresh of its own; other
    /// panels refresh as asked, and show the picture's dots).
    fn grays(&self) -> bool {
        self.shows_picture() && self.render == Render::Grays
    }

    /// While a bar that shows after a press is hidden, the first press is
    /// the screen's: it shows the bars and does nothing else.
    fn takes_buttons(&self) -> bool {
        self.auto && !self.grid.editing() && self.revealed_until.is_none()
    }

    fn on_button(&mut self, name: &str, _long: bool, cx: &mut Cx<'_, '_>) -> Handled {
        let env = *cx.env();
        // The first press only shows the bars: nothing else happens. The
        // power button is the kernel's.
        if name != "power" && self.auto_hidden(&env) {
            self.reveal(cx);
            return Handled::Yes;
        }
        Handled::No
    }

    fn on_resize(&mut self, cx: &mut Cx<'_, '_>) -> Handled {
        if self.hidden {
            self.stale = true;
        } else {
            self.relayout(cx);
        }
        Handled::Yes
    }

    fn on_hide(&mut self) {
        self.grid.release();
        self.stale = true;
        self.hidden = true;
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        self.hidden = false;
        let env = *cx.env();
        self.grid.set_focus_shown(
            ui::focus_shown(&env) && env.settings().read(&settings::FOCUS_AT_ONCE),
        );
        if self.stale {
            self.layout(&env);
        } else {
            self.grid.update_widgets(&env, cx);
        }
        cx.request_redraw();
        self.wake(cx);
    }

    fn on_timer(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if self.pending.is_some() {
            self.make_picture(cx);
            self.wake(cx);
            return;
        }
        self.conceal(cx);
        let mut changed = false;
        if let Some(bar) = self.bar.filter(|_| self.bar_shown(&env)) {
            cx.request_redraw_area(bar);
        }
        changed |= self.grid.update_live(&env, cx);
        self.wake(cx);
        if changed {
            cx.request_refresh(self.quick());
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
            widgets::Refresh::Remake => self.relayout(cx),
            widgets::Refresh::Read => {
                if !self.stale && self.grid.update_widgets(&env, cx) {
                    cx.request_refresh(self.quick());
                }
            }
            widgets::Refresh::ReadOne(id) => {
                if self.grid.update_widget(id.as_str(), &env, cx) {
                    cx.request_refresh(self.quick());
                }
            }
            widgets::Refresh::Live => {
                if let Some(bar) = self.bar {
                    cx.request_redraw_area(bar);
                }
                self.grid.update_live(&env, cx);
                cx.request_refresh(self.quick());
            }
            _ => {}
        }
    }

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

    fn holds_edges(&self) -> bool {
        self.grid.holds_edges()
    }

    fn on_pointer(&mut self, event: &PointerEvent, cx: &mut Cx<'_, '_>) -> Handled {
        let response = self.grid.on_pointer(event, cx);
        self.respond(response, cx)
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        let env = *cx.env();
        // An item of the bar that does something.
        if let (Some(plate), GestureKind::Tap) = (self.bar, gesture.kind)
            && self.bar_shown(&env)
        {
            let target = bar::targets(&env, plate)
                .into_iter()
                .find(|t| t.area.contains(gesture.start));
            if let Some(target) = target {
                cx.command(target.command);
                return Handled::Yes;
            }
        }
        let response = self.grid.on_gesture(gesture, cx);
        let handled = self.respond(response, cx);
        // A tap on nothing shows the bars that show after a press.
        if handled == Handled::No && gesture.kind == GestureKind::Tap && self.auto_hidden(&env) {
            self.reveal(cx);
            return Handled::Yes;
        }
        handled
    }
}
