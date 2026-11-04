//! Adding to a page, in two steps: first the kinds of widgets (time,
//! books, reading ...), the tools, *Add a page* and *Remove this page*;
//! then, for a kind, its widgets, or the tools, as a list. A widget or
//! tool goes into the first free cells of the page (a widget as wide as
//! the page and as high as it needs, a tool one cell); the page is kept
//! at once.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::ui::{self, ListView, UiStyle};
use paperpaw_sdk::widgets::grid::{self, What};
use paperpaw_sdk::widgets::{self, categories};
use paperpaw_sdk::{
    Canvas, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, Rect, Screen,
};

use crate::ADD_SCREEN;
use crate::pages::{self, Page, TOOLS};
use crate::settings::MAX_PAGES;

/// Parameter: the page added to (0 the first).
pub(crate) const PAGE: &str = "page";
/// Parameter: the kind of widgets shown (one of `widgets::categories` or
/// a plugin's own), or [`TOOLS_KIND`].
const KIND: &str = "kind";
/// The kind standing for the tools (and the translation key of its name).
const TOOLS_KIND: &str = "picture-home.add.tools";

/// A row of the list.
#[derive(Debug, Clone, PartialEq)]
enum Row {
    /// A kind of widgets, and how many there are to add.
    Kind(String, usize),
    /// A widget or tool, and the translation key of its name.
    Add(What, String),
    AddPage,
    RemovePage,
}

/// Every widget, and every tool not on the page yet, with its kind and the
/// translation key of its name: a widget may be placed more than once,
/// each placement with settings of its own.
fn addable(env: &Env<'_>, page: usize) -> Vec<(String, What, String)> {
    let all = pages::pages(env);
    let shown = all
        .get(page)
        .map(|p| pages::expand(env, &p.items))
        .unwrap_or_default();
    let on_page = |what: &What| shown.iter().any(|i| i.what == *what);
    env.widgets()
        .iter()
        .filter(|(id, _)| !widgets::repeatable(id.as_str()) && id.as_str() != TOOLS)
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

/// The kinds that have something to add, in the order of
/// `Widgets::kinds`, the tools last.
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
    kinds.sort_by_cached_key(|(kind, _)| rank(kind));
    kinds
}

#[allow(missing_debug_implementations, reason = "a screen")]
pub(crate) struct AddScreen {
    style: UiStyle,
    header: ui::Header,
    list: ListView,
    page: usize,
    /// The kind shown, if one was chosen.
    kind: Option<String>,
    rows: Vec<Row>,
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
                let all = pages::pages(env).len();
                if all < MAX_PAGES {
                    rows.push(Row::AddPage);
                }
                if all > 1 {
                    rows.push(Row::RemovePage);
                }
                rows
            }
        };
        Self {
            list: ListView::new(area, u32::from(style.row())).focus_on_keys(env),
            style,
            header,
            page,
            kind,
            rows,
        }
    }

    fn choose(&self, index: usize, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let mut all = pages::pages(&env);
        match self.rows.get(index) {
            Some(Row::Kind(kind, _)) => {
                let params = Params::new()
                    .with(PAGE, self.page.to_string())
                    .with(KIND, kind.clone());
                cx.command(Command::OpenScreen(ADD_SCREEN, params));
                return;
            }
            Some(Row::Add(what, _)) => {
                let what = match what {
                    What::Widget(id) => What::Widget(widgets::free_placement(&env, id, |placed| {
                        pages::holds(&env, placed)
                    })),
                    other => other.clone(),
                };
                let Some(page) = all.get(self.page) else {
                    return;
                };
                let geometry = crate::dashboard::geometry(&env);
                // The page as shown, every item with its place, and the new one.
                let shown = pages::expand(&env, &page.items);
                let Some(added) = grid::add(&env, &geometry, &shown, what) else {
                    let message = env.i18n().tr("picture-home.no-room").to_string();
                    cx.command(Command::Notify(message));
                    return;
                };
                for command in pages::store(&env, page.slot, &added) {
                    cx.command(command);
                }
            }
            Some(Row::AddPage) => {
                // After the others, kept as a space until it is filled.
                all.push(Page {
                    slot: all.len(),
                    items: Vec::new(),
                });
                for command in pages::store_all(&env, &all) {
                    cx.command(command);
                }
            }
            Some(Row::RemovePage) => {
                if self.page < all.len() && all.len() > 1 {
                    let gone = all.remove(self.page).items;
                    for command in pages::store_all(&env, &all) {
                        cx.command(command);
                    }
                    // What its widgets' placements set goes with them.
                    for item in gone {
                        if let What::Widget(id) = item.what {
                            for value in env.settings().placement_values(&id) {
                                cx.command(Command::ResetSetting(value));
                            }
                        }
                    }
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
        let key = self.kind.as_deref().unwrap_or("picture-home.add");
        env.i18n().tr(key).to_string()
    }
}

impl Screen for AddScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let len = self.rows.len();
        let title = self.header_title(env);
        let mut ui = ui::Ui::new(canvas, env, &style);
        self.header
            .draw(&mut ui, &title, None, self.list.paging(len));
        let rows = &self.rows;
        let room = ui::icon_room(env, &style, true);
        self.list.draw(&mut ui, len, |ui, index, rect, focused| {
            let ink = style.focus_ink(focused);
            let (icon, title, value) = match &rows[index] {
                Row::Kind(kind, count) => (
                    Some(if kind == TOOLS_KIND {
                        "apps"
                    } else {
                        env.widgets().category_icon(kind)
                    }),
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
                    env.i18n().tr("picture-home.add-page").to_string(),
                    String::new(),
                ),
                Row::RemovePage => (
                    Some("trash"),
                    env.i18n().tr("picture-home.remove-page").to_string(),
                    String::new(),
                ),
            };
            let rect = ui.row_icon(rect, room, icon, ink);
            ui::Row::value(&title, &value)
                .focused(focused)
                .draw(ui, rect);
        });
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let len = self.rows.len();
        if *action == actions::CONFIRM {
            self.choose(self.list.focus(), cx);
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
        let paging = self.list.paging(len);
        if let Some(action) = self.header.tap(&env, &self.style, gesture.start, paging) {
            if action == actions::BACK {
                cx.command(Command::run(action));
                return Handled::Yes;
            }
            return self.on_action(&action, cx);
        }
        match self.list.hit(gesture.start, len) {
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
        let order = [categories::TIME, categories::BOOKS];
        let found = [
            (
                String::from(TOOLS_KIND),
                What::Tool(actions::SETTINGS),
                String::new(),
            ),
            (
                String::from(categories::BOOKS),
                What::Widget("a".into()),
                String::new(),
            ),
            (
                String::from(categories::TIME),
                What::Widget("b".into()),
                String::new(),
            ),
            (
                String::from(categories::TIME),
                What::Widget("c".into()),
                String::new(),
            ),
        ];
        let kinds = kinds(&order, &found);
        let names: Vec<&str> = kinds.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(names, [categories::TIME, categories::BOOKS, TOOLS_KIND]);
        assert_eq!(kinds[0].1, 2);
    }
}
