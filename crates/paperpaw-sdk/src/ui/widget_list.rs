//! Editing a list of widgets shown above each other (a
//! [`WidgetPlace`](crate::widgets::WidgetPlace) of kind
//! [`Stack`](crate::widgets::PlaceKind::Stack)), as a list that buttons
//! work as well as taps: the widgets (dragged by their grip to move them;
//! choosing one offers to move it up or down, its own settings where it
//! has some, or to remove it), then *Add a widget*. Every change is kept
//! at once.
//!
//! The kernel shows these screens
//! ([`screens::WIDGET_LIST`], bound
//! to slot [`EDIT_WIDGETS`](crate::builtin::slots::EDIT_WIDGETS), and
//! [`screens::ADD_WIDGET`]) for a
//! setting that lists widgets (parameter
//! [`ID`](crate::builtin::params::ID)), e.g. the sleep screen's: such
//! settings name the slot as their editor.

use alloc::borrow::Cow;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::builtin::{actions, events, params, screens, slots};
use crate::event::Event;
use crate::status::item_ids;
use crate::ui::choice;
use crate::ui::reorder::{Reorder, ReorderStep};
use crate::ui::{self, ListView, UiStyle};
use crate::widgets;
use crate::{
    Canvas, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, PointerEvent, Rect,
    RefreshMode, Screen, SettingValue,
};

/// A widget's options were chosen.
const CHOSEN: Event<choice::Picked> = Event::new("core.widget-list-chosen");
const OPTION_LABELS: &str = "core.widgets.edit.";

/// What is edited: a setting that lists widgets.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    Setting(String),
}

/// What the parameters ask to edit.
fn target_of(params: &Params) -> Target {
    Target::Setting(params.get(params::ID).unwrap_or_default().to_string())
}

impl Target {
    /// Its widgets.
    fn widgets(&self, env: &Env<'_>) -> Vec<String> {
        match self {
            Target::Setting(id) => item_ids(env.settings().text(id))
                .map(ToString::to_string)
                .collect(),
        }
    }

    /// The commands that keep `widgets` as its list.
    fn store(&self, widgets: Vec<String>) -> Vec<Command> {
        match self {
            Target::Setting(id) => Id::parse(id)
                .ok()
                .map(|id| {
                    Command::SetSetting(id, SettingValue::Text(Cow::Owned(widgets.join(", "))))
                })
                .into_iter()
                .collect(),
        }
    }

    /// The parameters that ask to edit it.
    fn params(&self) -> Params {
        match self {
            Target::Setting(id) => Params::new().with(params::ID, id.clone()),
        }
    }

    fn title(&self, env: &Env<'_>) -> String {
        match self {
            Target::Setting(id) => env
                .settings
                .with_spec(id, |s| env.i18n().tr(&s.title).to_string())
                .ok()
                .flatten()
                .unwrap_or_else(|| id.clone()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    Widget(usize),
    Add,
}

/// The list of widgets of a setting, edited.
#[derive(Debug)]
pub struct ListScreen {
    style: UiStyle,
    header: ui::Header,
    list: ListView,
    reorder: Reorder,
    target: Target,
}

impl ListScreen {
    /// The list of the setting `params` name.
    pub fn open(env: &Env<'_>, params: &Params) -> Self {
        let style = UiStyle::new(env);
        let header = ui::Header::new(env, &style, true);
        let display = env.device().display;
        let top = header.area().height + style.gap as u16;
        let area = Rect::new(0, top, display.width, display.height.saturating_sub(top));
        Self {
            list: ListView::readable(env, &style, area, style.row).focus_on_keys(env),
            reorder: Reorder::default(),
            style,
            header,
            target: target_of(params),
        }
    }

    fn rows(&self, env: &Env<'_>) -> Vec<Row> {
        let count = self.target.widgets(env).len();
        let mut rows: Vec<Row> = (0..count).map(Row::Widget).collect();
        rows.push(Row::Add);
        rows
    }

    fn redraw(&self, cx: &mut Cx<'_, '_>) {
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Fast);
    }

    fn run(cx: &mut Cx<'_, '_>, commands: Vec<Command>) {
        for command in commands {
            cx.command(command);
        }
    }

    fn choose(&mut self, index: usize, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let Some(&row) = self.rows(&env).get(index) else {
            return;
        };
        match row {
            Row::Widget(at) => {
                let list = self.target.widgets(&env);
                let Some(id) = list.get(at) else {
                    return;
                };
                let mut options = Vec::new();
                if at > 0 {
                    options.push("up");
                }
                if at + 1 < list.len() {
                    options.push("down");
                }
                if widgets::has_settings(&env, id) {
                    options.push("settings");
                }
                options.push("remove");
                let named = env.widgets.title_of(id);
                let title = named.as_deref().map_or(id.as_str(), |t| env.i18n().tr(t));
                cx.command(
                    crate::ask::choice(title, &CHOSEN)
                        .about(format!("{at}"))
                        .options(OPTION_LABELS, &options)
                        .command(),
                );
            }
            Row::Add => cx.command(Command::OpenScreen(
                screens::ADD_WIDGET,
                self.target.params(),
            )),
        }
    }

    fn chosen(&mut self, option: &str, at: usize, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let mut list = self.target.widgets(&env);
        if at >= list.len() {
            return;
        }
        match option {
            "up" if at > 0 => {
                list.swap(at, at - 1);
                self.list.set_focus(at - 1);
            }
            "down" if at + 1 < list.len() => {
                list.swap(at, at + 1);
                self.list.set_focus(at + 1);
            }
            "remove" => {
                list.remove(at);
            }
            "settings" => {
                let params = Params::new().with(params::ID, list[at].clone());
                cx.command(Command::OpenSlot(slots::SETTINGS, params));
                return;
            }
            _ => return,
        }
        let commands = self.target.store(list);
        Self::run(cx, commands);
        self.redraw(cx);
    }
}

impl Screen for ListScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let rows = self.rows(env);
        let title = self.target.title(env);
        let mut ui = ui::Ui::new(canvas, env, &style);
        self.header
            .draw(&mut ui, &title, None, self.list.paging(rows.len()));
        let list = self.target.widgets(env);
        let room = ui::icon_room(ui.env(), ui.style(), true);
        self.list
            .draw(&mut ui, rows.len(), |ui, index, rect, focused| {
                let ink = style.focus_ink(focused);
                let (icon, title, value) = match rows[index] {
                    Row::Widget(at) => {
                        let id = list.get(at).map_or("", String::as_str);
                        let named = env.widgets().title_of(id);
                        let name = named.as_deref().map_or(id, |t| env.i18n().tr(t));
                        (None, name.to_string(), "")
                    }
                    Row::Add => (
                        Some("plus"),
                        env.i18n().tr("core.widgets.edit.add").to_string(),
                        "›",
                    ),
                };
                let rect = if matches!(rows[index], Row::Widget(_)) {
                    Reorder::draw_grip(ui.canvas(), env, &style, rect, focused);
                    Reorder::content(rect, &style)
                } else {
                    rect
                };
                let rect = ui.row_icon(rect, room, icon, ink);
                ui::Row::value(&title, value)
                    .focused(focused)
                    .draw(ui, rect);
            });
        self.reorder.draw(&mut ui, &self.list);
    }

    /// Widgets are moved by dragging their grip.
    fn on_pointer(&mut self, event: &PointerEvent, cx: &mut Cx<'_, '_>) -> Handled {
        let env = *cx.env();
        let mut list = self.target.widgets(&env);
        let Some(step) = self.reorder.on_pointer(event, &self.list, list.len()) else {
            return Handled::No;
        };
        if let ReorderStep::Dropped { from, to } = step
            && from != to
        {
            ui::reorder::moved(&mut list, from, to);
            self.list.set_focus(to);
            let commands = self.target.store(list);
            Self::run(cx, commands);
        }
        if step != ReorderStep::Same {
            self.redraw(cx);
        }
        Handled::Yes
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let env = *cx.env();
        let len = self.rows(&env).len();
        let rows = self.list.rows() as isize;
        let a = action;
        let delta = if let Some(delta) = ui::list_step(a, rows) {
            delta
        } else if *a == actions::CONFIRM {
            self.choose(self.list.focus(), cx);
            return Handled::Yes;
        } else {
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
        let len = self.rows(&env).len();
        if let Some(action) =
            self.header
                .tap(&env, &self.style, gesture.start, self.list.paging(len))
        {
            cx.command(Command::run(action));
            return Handled::Yes;
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

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        if let Some(picked) = CHOSEN.read(event, params) {
            if let Ok(at) = picked.path.parse() {
                self.chosen(&picked.option, at, cx);
            }
        } else if *event == events::SETTING_CHANGED {
            self.redraw(cx);
        }
    }
}

/// Adding a widget: every widget not in the list yet (spaces always);
/// choosing one adds it at the end and goes back.
#[derive(Debug)]
pub struct AddScreen {
    style: UiStyle,
    header: ui::Header,
    list: ListView,
    target: Target,
    /// Ids and the translation keys of their names.
    widgets: Vec<(Id, String)>,
}

impl AddScreen {
    /// What can be added to the list of the setting `params` name.
    pub fn open(env: &Env<'_>, params: &Params) -> Self {
        let style = UiStyle::new(env);
        let header = ui::Header::new(env, &style, true);
        let display = env.device().display;
        let top = header.area().height + style.gap as u16;
        let area = Rect::new(0, top, display.width, display.height.saturating_sub(top));
        let target = target_of(params);
        let shown = target.widgets(env);
        let mut offered = env.widgets.offered();
        offered.retain(|(id, _)| {
            widgets::repeatable(id.as_str()) || !shown.iter().any(|s| s == id.as_str())
        });
        // By their kind, as `categories::ALL` orders them (the spaces,
        // which are room, after the widgets that show something).
        let rank = |id: &Id| {
            let kind = env.widgets().kind_of(id.as_str()).unwrap_or_default();
            let order = widgets::categories::ALL.iter().position(|k| *k == kind);
            (
                widgets::repeatable(id.as_str()),
                order.unwrap_or(usize::MAX),
            )
        };
        paperpaw_sort::sort_by_cached_key(&mut offered, |(id, _)| rank(id));
        Self {
            widgets: offered,
            list: ListView::readable(env, &style, area, style.row).focus_on_keys(env),
            style,
            header,
            target,
        }
    }

    fn add(&self, index: usize, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let Some((id, _)) = self.widgets.get(index) else {
            return;
        };
        let mut list = self.target.widgets(&env);
        list.push(id.as_str().to_string());
        for command in self.target.store(list) {
            cx.command(command);
        }
        cx.command(Command::Back);
    }
}

impl Screen for AddScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let len = self.widgets.len();
        let mut ui = ui::Ui::new(canvas, env, &style);
        let title = env.i18n().tr("core.widgets.edit.add");
        self.header
            .draw(&mut ui, title, None, self.list.paging(len));
        if len == 0 {
            let all = env.i18n().tr("core.widgets.edit.all-shown");
            return ui.message(self.list.area(), all);
        }
        let widgets = &self.widgets;
        self.list.draw(&mut ui, len, |ui, index, rect, focused| {
            // Its kind beside it, so that widgets of a kind are seen
            // together.
            let (id, title) = &widgets[index];
            let kind = env.widgets().kind_of(id.as_str()).unwrap_or_default();
            let title = env.i18n().tr(title);
            ui::Row::value(title, env.i18n().tr(&kind))
                .focused(focused)
                .draw(ui, rect);
        });
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let len = self.widgets.len();
        let rows = self.list.rows() as isize;
        let a = action;
        let delta = if let Some(delta) = ui::list_step(a, rows) {
            delta
        } else if *a == actions::CONFIRM {
            self.add(self.list.focus(), cx);
            return Handled::Yes;
        } else {
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
        let len = self.widgets.len();
        if let Some(action) =
            self.header
                .tap(&env, &self.style, gesture.start, self.list.paging(len))
        {
            cx.command(Command::run(action));
            return Handled::Yes;
        }
        match self.list.hit(gesture.start, len) {
            Some(index) => {
                self.add(index, cx);
                Handled::Yes
            }
            None => Handled::No,
        }
    }
}
