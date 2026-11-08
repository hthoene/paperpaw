//! Adding to a page of the home screen, in two steps: first the
//! ready-made layouts (see `layouts`), the kinds of widgets (time, books,
//! reading ...), the tools, *Add a page* and *Remove this page*; then, for a kind, its widgets not on the page yet, each
//! drawn as it would show (a gallery, as phones offer widgets), or the
//! tools as a list. A widget or tool goes into the first free cells of
//! the page (a widget as wide as the page and as high as it needs, a tool
//! one cell); the page is kept at once.

use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, settings};
use paperpaw_sdk::ui::{self, ListView, UiStyle};
use paperpaw_sdk::widgets::{self, Widget, categories};
use paperpaw_sdk::{
    Canvas, Color, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, Rect, Screen,
};

use crate::pages::{self, MAX_PAGES, STATUS, STATUS_BAR, TOOLS};
use paperpaw_sdk::widgets::grid::{self, Cells, What};

/// Parameter: the page added to (0 the first).
pub(crate) const PAGE: &str = "page";
/// Parameter: the kind of widgets shown (one of
/// `widgets::categories` or a plugin's own), or [`TOOLS_KIND`].
const KIND: &str = "kind";
/// The kind standing for the tools.
const TOOLS_KIND: &str = "home.edit.tools";
/// Rows a widget's preview takes at most.
const PREVIEW_ROWS: u8 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Row {
    /// A kind of widgets (or the tools), and how many there are.
    Kind(String, usize),
    /// A widget or tool, and the translation key of its name.
    Add(What, String),
    AddPage,
    RemovePage,
    /// Empties the layout kept for a screen on its side.
    ResetWide,
}

/// The icon of a kind of widgets (any plugin's), or of the tools.
fn kind_icon<'e>(env: &Env<'e>, kind: &str) -> &'e str {
    if kind == TOOLS_KIND {
        "apps"
    } else {
        env.widgets().category_icon(kind)
    }
}

/// Every widget, and every tool not on `page` yet, with its kind and the
/// translation key of its name: a widget may be placed more than once,
/// each placement with settings of its own.
fn addable(env: &Env<'_>, page: usize) -> Vec<(String, What, String)> {
    let all = crate::shown(env).2;
    let shown = all
        .get(page)
        .map(|p| pages::expand(env, &p.items))
        .unwrap_or_default();
    let on_page = |what: &What| shown.iter().any(|i| i.what == *what);
    env.widgets()
        .iter()
        .filter(|(id, _)| {
            let id = id.as_str();
            // Empty cells are the grid's spaces; the status line is
            // above it where shown.
            !widgets::repeatable(id)
                && id != TOOLS
                && !(id == STATUS && env.settings().read(&STATUS_BAR))
        })
        .map(|(id, title)| {
            let kind = env
                .widgets()
                .category(id.as_str())
                .unwrap_or(categories::OTHER);
            (
                kind.to_string(),
                What::Widget(id.as_str().to_string()),
                title.to_string(),
            )
        })
        .chain(
            pages::tools(env)
                .into_iter()
                .map(|(action, title)| (TOOLS_KIND.to_string(), What::Tool(action), title)),
        )
        .filter(|(_, what, _)| matches!(what, What::Widget(_)) || !on_page(what))
        .collect()
}

/// A placement of widget `id` (`id@n`) that no page of either layout holds
/// and that has no values set: each widget added gets one, so that its
/// settings are its own.
fn placement(env: &Env<'_>, id: &str) -> String {
    widgets::free_placement(env, id, |placed| pages::holds(env, placed))
}

/// The kinds that have something to add, in the order of
/// `Widgets::kinds` (plugins' own kinds after the kernel's), the tools
/// last.
fn kinds(order: &[&str], found: &[(String, What, String)]) -> Vec<(String, usize)> {
    let mut kinds: Vec<(String, usize)> = Vec::new();
    for (kind, _, _) in found {
        match kinds.iter_mut().find(|(k, _)| k == kind) {
            Some((_, count)) => *count += 1,
            None => kinds.push((kind.clone(), 1)),
        }
    }
    let rank = |kind: &str| {
        if kind == TOOLS_KIND {
            order.len() + 1
        } else {
            order.iter().position(|k| *k == kind).unwrap_or(order.len())
        }
    };
    paperpaw_sdk::sort::sort_by_cached_key(&mut kinds, |(kind, _)| rank(kind));
    kinds
}

pub(crate) struct AddScreen {
    style: UiStyle,
    header: ui::Header,
    list: ListView,
    page: usize,
    /// The kind shown, if one was chosen.
    kind: Option<String>,
    rows: Vec<Row>,
    /// Widgets as they would show, for the gallery: each with its place
    /// on the screen (its name above it), page by page.
    gallery: Vec<Preview>,
}

/// A widget in the gallery.
struct Preview {
    /// Index of its row.
    row: usize,
    /// The page of the gallery it is on.
    page: usize,
    /// Its name, and the widget drawn below it.
    title: Rect,
    area: Rect,
    widget: Option<Box<dyn Widget>>,
}

impl AddScreen {
    pub(crate) fn new(env: &Env<'_>, params: &Params) -> Self {
        let style = UiStyle::new(env);
        let header = ui::Header::new(env, &style, true);
        let display = env.device().display;
        let top = header.area().height + style.gap();
        let area = Rect::new(0, top, display.width, display.height.saturating_sub(top));
        let page = params.get(PAGE).and_then(|p| p.parse().ok()).unwrap_or(0);
        let kind = params.get(KIND).map(ToString::to_string);
        let found = addable(env, page);
        let rows: Vec<Row> = match &kind {
            Some(kind) => found
                .into_iter()
                .filter(|(k, _, _)| k == kind)
                .map(|(_, what, title)| Row::Add(what, title))
                .collect(),
            None => {
                let mut rows: Vec<Row> = kinds(&env.widgets().kinds().collect::<Vec<_>>(), &found)
                    .into_iter()
                    .map(|(kind, count)| Row::Kind(kind, count))
                    .collect();
                let all = crate::shown(env).2.len();
                if all < MAX_PAGES {
                    rows.push(Row::AddPage);
                }
                if all > 1 {
                    rows.push(Row::RemovePage);
                }
                if pages::wide_kept(env) {
                    rows.push(Row::ResetWide);
                }
                rows
            }
        };
        let mut screen = Self {
            list: ListView::new(area, u32::from(style.row())).focus_on_keys(env),
            style,
            header,
            page,
            kind,
            rows,
            gallery: Vec::new(),
        };
        if screen.is_gallery() {
            screen.gallery = screen.lay_out(env, area);
            screen.make_page(env);
        }
        screen
    }

    /// Makes the widgets of the gallery's page with the focus, where they
    /// are not made yet: what they show read, each in its place, before
    /// the page is drawn. Those of the other pages go (with what they
    /// keep, e.g. covers made small).
    fn make_page(&mut self, env: &Env<'_>) {
        let page = self.gallery_page();
        for preview in &mut self.gallery {
            if preview.page != page {
                preview.widget = None;
                continue;
            }
            let Some(Row::Add(What::Widget(id), _)) = self.rows.get(preview.row) else {
                continue;
            };
            if preview.widget.is_none() {
                preview.widget = env.widgets().make(id, env).map(|mut widget| {
                    widget.place(env, preview.area);
                    widget
                });
            }
        }
    }

    /// Whether widgets are shown as they would show, rather than as a list.
    fn is_gallery(&self) -> bool {
        self.kind.as_deref().is_some_and(|k| k != TOOLS_KIND)
    }

    /// The gallery's previews in `area`, as large as each widget would be
    /// on the page (up to [`PREVIEW_ROWS`] rows), as many to a page as
    /// fit. Widgets are made when their page shows
    /// ([`Self::make_page`]).
    fn lay_out(&self, env: &Env<'_>, area: Rect) -> Vec<Preview> {
        let style = self.style;
        let geometry = crate::shown(env).1;
        let title_h = widgets::line_height(env, style.title()) + style.gap() / 2;
        let gap = style.gap();
        let bottom = area.y + area.height;
        let mut previews = Vec::new();
        let (mut page, mut y) = (0, area.y);
        for (row, entry) in self.rows.iter().enumerate() {
            let Row::Add(what, _) = entry else {
                continue;
            };
            let (w, h) = geometry.size_of(env, what);
            let cells = Cells::new(0, 0, w, h.clamp(1, PREVIEW_ROWS));
            let at = geometry.rect(cells);
            let height = title_h + at.height + 2 * gap;
            if y + height > bottom && y > area.y {
                page += 1;
                y = area.y;
            }
            let x = at.x;
            previews.push(Preview {
                row,
                page,
                title: Rect::new(x, y + gap, at.width, title_h),
                area: Rect::new(x, y + gap + title_h, at.width, at.height),
                widget: None,
            });
            y += height;
        }
        previews
    }

    fn gallery_pages(&self) -> usize {
        self.gallery.last().map_or(1, |p| p.page + 1)
    }

    /// The gallery's page with the focus.
    fn gallery_page(&self) -> usize {
        self.gallery
            .iter()
            .find(|p| p.row == self.list.focus())
            .map_or(0, |p| p.page)
    }

    fn paging(&self) -> Option<ui::Paging> {
        if self.is_gallery() {
            let pages = self.gallery_pages();
            (pages > 1).then(|| ui::Paging::new(self.gallery_page(), pages))
        } else {
            self.list.paging(self.rows.len())
        }
    }

    fn choose(&self, index: usize, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let (wide, geometry, mut all) = crate::shown(&env);
        // The layout shown is kept; for a screen on its side that is the
        // whole derived layout, from the first change on.
        let in_layout = wide.then_some(&geometry);
        match self.rows.get(index) {
            Some(Row::Kind(kind, _)) => {
                let params = Params::new()
                    .with(PAGE, self.page.to_string())
                    .with(KIND, kind.clone());
                cx.command(Command::OpenScreen(crate::ADD_SCREEN, params));
                return;
            }
            Some(Row::Add(what, _)) => {
                let what = match what {
                    What::Widget(id) => What::Widget(placement(&env, id)),
                    other => other.clone(),
                };
                let Some(page) = all.get(self.page) else {
                    return;
                };
                // The page as shown, every item with its place, and the new one.
                let shown = pages::expand(&env, &page.items);
                let Some(added) = grid::add(&env, &geometry, &shown, what.clone()) else {
                    let message = env.i18n().tr("home.grid.no-room").to_string();
                    cx.command(Command::Notify(message));
                    return;
                };
                for command in pages::store(&env, page.slot, &added, in_layout) {
                    cx.command(command);
                }
                // It is in the other layout too, where the grid puts it in
                // the first free cells.
                for command in pages::add_to_other(&env, wide, page.slot, what) {
                    cx.command(command);
                }
            }
            Some(Row::AddPage) => {
                // After the others, kept as a space until it is filled.
                all.push(pages::Page {
                    slot: all.len(),
                    items: Vec::new(),
                });
                for command in pages::store_all(&env, &all, wide) {
                    cx.command(command);
                }
            }
            Some(Row::RemovePage) => {
                if self.page < all.len() && all.len() > 1 {
                    let gone = all.remove(self.page).items;
                    for command in pages::store_all(&env, &all, wide) {
                        cx.command(command);
                    }
                    // What its widgets' placements set goes with them, unless
                    // the other layout still shows them.
                    let other = pages::other_layer(&env, wide);
                    let held = |id: &str| {
                        all.iter()
                            .chain(&other)
                            .flat_map(|p| &p.items)
                            .any(|i| matches!(&i.what, What::Widget(w) if w == id))
                    };
                    for item in gone {
                        if let What::Widget(id) = item.what
                            && !held(&id)
                        {
                            for value in env.settings().placement_values(&id) {
                                cx.command(Command::ResetSetting(value));
                            }
                        }
                    }
                }
            }
            Some(Row::ResetWide) => {
                for command in pages::reset_wide() {
                    cx.command(command);
                }
            }
            None => return,
        }
        cx.command(Command::Back);
        // Back past the kinds too, to the page.
        if self.kind.is_some() {
            cx.command(Command::Back);
        }
    }

    fn header_title(&self, env: &Env<'_>) -> String {
        let key = self.kind.as_deref().unwrap_or("home.edit.add");
        env.i18n().tr(key).to_string()
    }

    fn draw_list(&mut self, ui: &mut ui::Ui<'_>) {
        let (env, style) = (ui.env(), self.style);
        let len = self.rows.len();
        let rows = &self.rows;
        let room = ui::icon_room(ui.env(), ui.style(), true);
        self.list.draw(ui, len, |ui, index, rect, focused| {
            let ink = style.focus_ink(focused);
            let (icon, title, value) = match &rows[index] {
                Row::Kind(kind, count) => (
                    Some(kind_icon(env, kind)),
                    env.i18n().tr(kind).to_string(),
                    count.to_string(),
                ),
                Row::Add(What::Tool(action), title) => (
                    env.menus().icon(action.as_str()),
                    env.i18n().tr(title).to_string(),
                    String::from("+"),
                ),
                Row::Add(_, title) => (None, env.i18n().tr(title).to_string(), String::from("+")),
                Row::AddPage => (
                    Some("plus"),
                    env.i18n().tr("home.edit.add-page").to_string(),
                    String::new(),
                ),
                Row::RemovePage => (
                    Some("trash"),
                    env.i18n().tr("home.edit.remove-page").to_string(),
                    String::new(),
                ),
                Row::ResetWide => (
                    Some("refresh"),
                    env.i18n().tr("home.edit.reset-wide").to_string(),
                    String::new(),
                ),
            };
            let rect = ui.row_icon(rect, room, icon, ink);
            ui::Row::value(&title, &value)
                .focused(focused)
                .draw(ui, rect);
        });
    }

    /// The gallery's page with the focus: each widget's name, and the
    /// widget as it would show; the focused one framed.
    fn draw_gallery(&self, ui: &mut ui::Ui<'_>) {
        let (env, style) = (ui.env(), self.style);
        let Some(font) = style.font(env) else {
            return;
        };
        let page = self.gallery_page();
        let focus = self.list.focus();
        let shown = self.list.shown();
        for preview in self.gallery.iter().filter(|p| p.page == page) {
            let Some(Row::Add(What::Widget(_), title)) = self.rows.get(preview.row) else {
                continue;
            };
            let focused = shown && preview.row == focus;
            let name = env.i18n().tr(title);
            ui.canvas()
                .describe(
                    paperpaw_sdk::semantics::Role::Row,
                    name,
                    preview.title.union(&preview.area),
                )
                .focused(focused);
            let name = ui::fit(&font, style.title(), name, f32::from(preview.title.width));
            ui::Label::new(&name)
                .px(style.title())
                .color(Color::BLACK)
                .draw_from(ui, f32::from(preview.title.x), preview.title);
            // The widget in its place, as on the page; round it, outside
            // it so that nothing of it is covered, a faint line so its
            // size shows, or a strong frame round the focused one.
            let around = preview.area;
            if let Some(widget) = &preview.widget {
                widget.draw(ui.canvas(), env, around);
            }
            let pad = (u32::from(style.gap()) / 2) as u16;
            let ring = Rect::new(
                around.x.saturating_sub(pad),
                around.y.saturating_sub(pad),
                around.width + 2 * pad,
                around.height + 2 * pad,
            );
            if focused {
                ui::stroke_round(
                    ui.canvas(),
                    &style,
                    ring,
                    u32::from(style.line()) * 3,
                    Color::BLACK,
                );
            } else {
                ui::stroke_round(
                    ui.canvas(),
                    &style,
                    ring,
                    u32::from(style.line()),
                    Color::gray(170),
                );
            }
        }
    }
}

impl Screen for AddScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let paging = self.paging();
        let title = self.header_title(env);
        let mut ui = ui::Ui::new(canvas, env, &style);
        self.header.draw(&mut ui, &title, None, paging);
        if self.is_gallery() {
            self.draw_gallery(&mut ui);
        } else {
            self.draw_list(&mut ui);
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let len = self.rows.len();
        if *action == actions::CONFIRM {
            self.choose(self.list.focus(), cx);
            return Handled::Yes;
        }
        if self.is_gallery() {
            let Some(delta) = ui::list_step(action, 2) else {
                return Handled::No;
            };
            // A page at a time: to the first widget of the next or the
            // previous page.
            let focus = if delta.abs() > 1 {
                let to = self.gallery_page() as isize + delta.signum();
                let to = to.clamp(0, self.gallery_pages() as isize - 1) as usize;
                self.gallery
                    .iter()
                    .find(|p| p.page == to)
                    .map_or(self.list.focus(), |p| p.row)
            } else if cx.env().settings().read(&settings::ARROWS_STAY_ON_PAGE) {
                // The arrow keys stay on the page: round among its widgets.
                let page = self.gallery_page();
                let on_page = self.gallery.iter().filter(|p| p.page == page);
                let rows: Vec<usize> = on_page.map(|p| p.row).collect();
                match rows.iter().position(|row| *row == self.list.focus()) {
                    Some(at) => rows[ui::step_focus(at, delta, rows.len())],
                    None => ui::step_focus(self.list.focus(), delta, len),
                }
            } else {
                ui::step_focus(self.list.focus(), delta, len)
            };
            self.list.set_focus(focus);
            self.list.set_shown(true);
            self.make_page(cx.env());
            cx.request_redraw();
            return Handled::Yes;
        }
        let rows = self.list.rows() as isize;
        let Some(delta) = ui::list_step(action, rows) else {
            return Handled::No;
        };
        self.list.move_focus_in(cx.env(), delta, len).request(cx);
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        let env = *cx.env();
        let len = self.rows.len();
        if let Some(action) = self
            .header
            .tap(&env, &self.style, gesture.start, self.paging())
        {
            if action == actions::BACK {
                cx.command(Command::run(action));
                return Handled::Yes;
            }
            return self.on_action(&action, cx);
        }
        let hit = if self.is_gallery() {
            let page = self.gallery_page();
            self.gallery
                .iter()
                .filter(|p| p.page == page)
                .find(|p| p.title.union(&p.area).contains(gesture.start))
                .map(|p| p.row)
        } else {
            self.list.hit(gesture.start, len)
        };
        match hit {
            Some(index) => {
                self.list.set_focus(index);
                self.choose(index, cx);
                Handled::Yes
            }
            None => Handled::No,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_follow_the_standard_order_tools_last() {
        let item = |kind: &str| {
            (
                kind.to_string(),
                What::Widget(String::from("w")),
                String::from("t"),
            )
        };
        let found = [
            item(TOOLS_KIND),
            item("acme.kind"),
            item(categories::BOOKS),
            item(categories::TIME),
            item(categories::BOOKS),
        ];
        let kinds = kinds(categories::ALL, &found);
        let names: Vec<&str> = kinds.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(
            names,
            [categories::TIME, categories::BOOKS, "acme.kind", TOOLS_KIND]
        );
        assert_eq!(kinds[1].1, 2);
    }
}
