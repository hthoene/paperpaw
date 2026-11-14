//! Choosing what a status line shows: a screen for any setting that lists
//! status items (or status lines), bound by the kernel as
//! [`screens::STATUS_ITEMS`](crate::builtin::screens::STATUS_ITEMS) and
//! [`screens::STATUS_LINES`](crate::builtin::screens::STATUS_LINES). Name
//! one as the setting's editor ([`SettingSpec::editor`](crate::SettingSpec)):
//! it lists the items shown, in their order, then the others; tapping one
//! shows or hides it (a newly shown one goes last), and shown items are
//! moved by dragging their grip. With buttons, *Menu* (a long press of
//! *Confirm*) takes the focused shown item along: the direction and page
//! buttons move it a place, *Confirm* or *Back* put it down. Items of
//! plugins not there now stay in the setting where they were.

use alloc::borrow::Cow;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::builtin::{actions, params};
use crate::status::item_ids;
use crate::ui::reorder::{Reorder, ReorderStep};
use crate::ui::{self, ListView, UiStyle};
use crate::{
    Canvas, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, PointerEvent, Rect,
    RefreshMode, Screen, SettingValue,
};

/// The value of [`params::MODE`] that lists status lines, not items.
pub const LINES: &str = "lines";

/// The screen; see the [module documentation](self).
pub struct ItemsScreen {
    setting: Option<Id>,
    /// Item ids and the translation keys of their names.
    items: Vec<(Id, String)>,
    style: UiStyle,
    header: ui::Header,
    list: ListView,
    reorder: Reorder,
    /// The focused shown item is being moved by the buttons.
    moving: bool,
}

impl ItemsScreen {
    /// For the setting [`params::ID`]; status lines with [`params::MODE`]
    /// [`LINES`], else status items.
    pub fn open(env: &Env<'_>, params: &Params) -> Self {
        let setting = params.get(params::ID).and_then(|id| Id::parse(id).ok());
        let items = if params.get(params::MODE) == Some(LINES) {
            env.status_items()
                .lines()
                .map(|(id, title)| (id.clone(), title.to_string()))
                .collect()
        } else {
            env.status_items()
                .iter()
                .map(|(id, title)| (id.clone(), title.to_string()))
                .collect()
        };
        let style = UiStyle::new(env);
        let display = env.device().display;
        let header = ui::Header::new(env, &style, true);
        let top = header.area().height + style.gap as u16;
        let area = Rect::new(0, top, display.width, display.height - top);
        Self {
            setting,
            items,
            list: ListView::readable(env, &style, area, style.row).focus_on_keys(env),
            reorder: Reorder::default(),
            moving: false,
            style,
            header,
        }
    }

    /// The items shown, in their order (only those there are), a line
    /// named by an id it had before by its own, once.
    fn chosen<'e>(&self, env: &Env<'e>) -> Vec<&'e str> {
        let mut chosen: Vec<&'e str> = Vec::new();
        let Some(setting) = &self.setting else {
            return chosen;
        };
        for c in item_ids(env.settings().text(setting.as_str())) {
            let c = env.status_items().canonical(c);
            if self.items.iter().any(|(id, _)| id.as_str() == c) && !chosen.contains(&c) {
                chosen.push(c);
            }
        }
        chosen
    }

    /// The rows: indexes into the items, the shown ones first.
    fn order(&self, env: &Env<'_>) -> Vec<usize> {
        let chosen = self.chosen(env);
        let at = |c: &str| self.items.iter().position(|(id, _)| id.as_str() == c);
        let mut order: Vec<usize> = chosen.iter().filter_map(|c| at(c)).collect();
        let rest: Vec<usize> = (0..self.items.len())
            .filter(|i| !order.contains(i))
            .collect();
        order.extend(rest);
        order
    }

    /// Keeps `chosen`, the items shown here in their new order. Items of
    /// the setting not listed here (of plugins not there now) keep their
    /// places among them.
    fn keep(&self, env: &Env<'_>, chosen: &[&str], cx: &mut Cx<'_, '_>) {
        if let Some(setting) = &self.setting {
            // A line by an id it had before is this one, kept by its own.
            let known = |c: &str| {
                let c = env.status_items().canonical(c);
                self.items.iter().any(|(id, _)| id.as_str() == c)
            };
            let mut chosen_left = chosen.iter();
            let mut all: Vec<&str> = item_ids(env.settings().text(setting.as_str()))
                .filter_map(|c| {
                    if known(c) {
                        chosen_left.next().copied()
                    } else {
                        Some(c)
                    }
                })
                .collect();
            all.extend(chosen_left);
            cx.command(Command::SetSetting(
                setting.clone(),
                SettingValue::Text(Cow::Owned(all.join(", "))),
            ));
        }
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Fast);
    }

    fn toggle(&mut self, row: usize, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let Some((id, _)) = self.order(&env).get(row).and_then(|&i| self.items.get(i)) else {
            return;
        };
        let mut chosen = self.chosen(&env);
        match chosen.iter().position(|c| *c == id.as_str()) {
            Some(at) => {
                chosen.remove(at);
            }
            None => chosen.push(id.as_str()),
        }
        self.keep(&env, &chosen, cx);
    }

    /// Moves the shown item with the focus `delta` places, within the shown
    /// ones (the buttons' way of moving it).
    fn shift(&mut self, delta: isize, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let mut chosen = self.chosen(&env);
        let from = self.list.focus();
        let to = from
            .saturating_add_signed(delta)
            .min(chosen.len().saturating_sub(1));
        if from >= chosen.len() || to == from {
            return;
        }
        ui::reorder::moved(&mut chosen, from, to);
        self.list.set_focus(to);
        self.keep(&env, &chosen, cx);
    }

    /// Whether the buttons can take the focused item along: the focus
    /// shows on a shown item, and another is shown to move it past.
    fn can_move(&self, env: &Env<'_>) -> bool {
        let shown = self.chosen(env).len();
        self.list.shown() && self.list.focus() < shown && shown > 1
    }
}

impl core::fmt::Debug for ItemsScreen {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ItemsScreen")
            .field("setting", &self.setting)
            .finish_non_exhaustive()
    }
}

impl Screen for ItemsScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let title = self.setting.as_ref().map_or(String::new(), |s| {
            env.settings()
                .with_spec(s.as_str(), |spec| env.i18n().tr(&spec.title).to_string())
                .ok()
                .flatten()
                .unwrap_or_default()
        });
        let len = self.items.len();
        let mut ui = ui::Ui::new(canvas, env, &style);
        self.header
            .draw(&mut ui, &title, None, self.list.paging(len));
        let chosen = self.chosen(env);
        let order = self.order(env);
        let items = &self.items;
        self.list.draw(&mut ui, len, |ui, row, rect, focused| {
            let (id, name) = &items[order[row]];
            let on = chosen.iter().position(|c| *c == id.as_str());
            let rect = if on.is_some() {
                Reorder::draw_grip(ui.canvas(), env, &style, rect, focused);
                Reorder::content(rect, &style)
            } else {
                rect
            };
            // Shown items carry their place: "On (2)".
            let value = match on {
                Some(at) if chosen.len() > 1 => {
                    format!("{} ({})", env.i18n().tr("core.state.on"), at + 1)
                }
                Some(_) => env.i18n().tr("core.state.on").to_string(),
                None => env.i18n().tr("core.state.off").to_string(),
            };
            ui::Row::value(env.i18n().tr(name), &value)
                .focused(focused)
                .draw(ui, rect);
        });
        self.reorder.draw(&mut ui, &self.list);
        if self.moving {
            // The item the buttons move: a frame inside its focus, in the
            // focus's own ink (white on a dark bar, dark in a ring or on a
            // light bar), so it looks taken up, not only focused.
            let slot = self.list.focus() % self.list.rows();
            let row = self.list.row_rect(slot);
            let inset = (style.line * 3) as u16;
            let frame = Rect::new(
                row.x + inset,
                row.y + inset,
                row.width.saturating_sub(2 * inset),
                row.height.saturating_sub(2 * inset),
            );
            let r = style.radius(u32::from(frame.height));
            ui.canvas()
                .stroke_round(frame, style.line * 2, r, style.focus_ink(true));
        }
    }

    fn hint(&self, action: &Id, env: &Env<'_>) -> crate::Hint {
        let done = || crate::Hint::Text(env.i18n().tr("core.grid.done").to_string());
        match action {
            a if self.moving && [actions::CONFIRM, actions::BACK].contains(a) => done(),
            a if *a == actions::MENU && !self.moving && self.can_move(env) => {
                crate::Hint::Text(env.i18n().tr("core.move").to_string())
            }
            _ => crate::Hint::Default,
        }
    }

    fn on_pointer(&mut self, event: &PointerEvent, cx: &mut Cx<'_, '_>) -> Handled {
        let env = *cx.env();
        let mut chosen = self.chosen(&env);
        let Some(step) = self.reorder.on_pointer(event, &self.list, chosen.len()) else {
            return Handled::No;
        };
        // A finger takes over from the buttons.
        self.moving = false;
        match step {
            ReorderStep::Dropped { from, to } if from != to => {
                ui::reorder::moved(&mut chosen, from, to);
                self.list.set_focus(to);
                self.keep(&env, &chosen, cx);
            }
            ReorderStep::Same => {}
            _ => {
                cx.request_redraw();
                cx.request_refresh(RefreshMode::Fast);
            }
        }
        Handled::Yes
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let env = *cx.env();
        let a = action;
        if self.moving {
            if let Some(delta) = ui::list_step(a, 1) {
                self.shift(delta, cx);
            } else if [actions::CONFIRM, actions::BACK, actions::MENU].contains(a) {
                self.moving = false;
                cx.request_redraw();
                cx.request_refresh(RefreshMode::Fast);
            } else {
                return Handled::No;
            }
            return Handled::Yes;
        }
        if *a == actions::MENU && self.can_move(&env) {
            self.moving = true;
            cx.request_redraw();
            cx.request_refresh(RefreshMode::Fast);
            return Handled::Yes;
        }
        if *a == actions::CONFIRM {
            match self.list.confirm() {
                Some(row) => self.toggle(row, cx),
                None => self.list.focus_change().request(cx),
            }
            return Handled::Yes;
        }
        match self.list.navigate_in(cx.env(), a, self.items.len()) {
            Some(change) => {
                change.request(cx);
                Handled::Yes
            }
            None => Handled::No,
        }
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        self.moving = false;
        let paging = self.list.paging(self.items.len());
        let tapped = self
            .header
            .tap(cx.env(), &self.style, gesture.start, paging);
        if let Some(action) = tapped {
            cx.command(Command::run(action));
            return Handled::Yes;
        }
        match self.list.hit(gesture.start, self.items.len()) {
            Some(index) => {
                self.list.set_focus(index);
                self.toggle(index, cx);
                Handled::Yes
            }
            None => Handled::No,
        }
    }
}
