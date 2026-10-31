//! A menu as a bar at the top of the screen, as the reader opens its
//! menu: a title and a line about where one is (e.g. the book, chapter and
//! page), then the entries as icons with short labels, several to a row.
//!
//! Opened like any menu, with the parameter `mode` set to `bar`, and
//! optionally `title` and `text` for the two lines, and `page` shown at the
//! end of the second (always whole; the text is shortened instead); with
//! `action`, tapping the two lines runs it (e.g. the book's information).
//! Labels are the
//! translation of an entry's title key followed by `.short` where there is
//! one (`reader.toc.short` = "Contents"), else its title.
//!
//! Its text is in the status lines' font, as the lines it opens over.
//!
//! Switches show their state (an icon on black is on) and change in
//! place, as do entries their plugin shows as switches; an entry's label
//! and badge are as its plugin says now, and a faint one is passed by the
//! focus and does nothing; choices open their list; settings with a page of their own
//! open it, numbers and text their group's settings page; submenus open
//! as a list below; actions close the bar and run.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, events, params, slots};
use paperpaw_sdk::semantics::Role;
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::{
    Canvas, Color, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, Rect, RefreshMode,
    Screen, SettingKind, SettingValue,
};

use crate::{DEPTH, SCREEN, is_menu};

/// The bar's font: the reading status lines' (it opens over a book), as
/// `core.status-font` chooses.
fn bar_font<'e>(env: &Env<'e>) -> Option<paperpaw_sdk::FontHandle<'e>> {
    env.fonts()
        .role(paperpaw_sdk::builtin::fonts::STATUS.as_str())
}

/// The value of [`params::MODE`] for a bar.
pub(crate) const MODE: &str = "bar";

/// Icons in a bar are this much larger than body text.
const ICON_SCALE: f32 = 1.6;

/// An entry of the bar.
#[derive(Debug)]
struct Entry {
    id: Id,
    /// Its short title, translated (its plugin may show another label).
    title: String,
    icon: Option<String>,
}

impl Entry {
    /// What it shows now: its label and badge.
    fn label(&self, env: &Env<'_>) -> String {
        env.menus().state(env, self.id.as_str()).text(&self.title)
    }

    /// It can be chosen now.
    fn enabled(&self, env: &Env<'_>) -> bool {
        env.menus().state(env, self.id.as_str()).enabled
    }
}

#[derive(Debug)]
pub(crate) struct BarScreen {
    entries: Vec<Entry>,
    style: UiStyle,
    region: Rect,
    title: String,
    text: String,
    page: String,
    /// Where the title and text are.
    info: Rect,
    /// Run when they are tapped.
    info_action: Option<Id>,
    /// Where each entry is drawn (as wide as its icon or label).
    cells: Vec<Rect>,
    /// Where each entry is tapped.
    targets: Vec<Rect>,
    focus: usize,
    /// The focus is shown (buttons used, or no touch screen).
    keys: bool,
    /// What it was opened with, to make it anew (see `on_event`).
    params: Params,
}

impl BarScreen {
    #[expect(
        clippy::too_many_lines,
        reason = "longer than the limit from before it was set; split it when it is next changed"
    )]
    pub(crate) fn new(env: &Env<'_>, params: &Params) -> Self {
        let style = UiStyle::new(env);
        let display = env.device().display;
        let menu = params.get(params::MENU).unwrap_or_default();
        let entries: Vec<Entry> = env
            .menus()
            .shown_items(env, menu)
            .into_iter()
            .filter(|item| !item.section && !paperpaw_sdk::Menus::hidden(params, item.id.as_str()))
            // A setting shown only while another has a value.
            .filter(|item| {
                env.settings()
                    .spec(item.id.as_str())
                    .is_none_or(|s| env.settings().shows(s))
            })
            .map(|item| Entry {
                id: item.id.clone(),
                title: env
                    .i18n()
                    .get(&alloc::format!("{}.short", item.title))
                    .unwrap_or(env.i18n().tr(item.title))
                    .to_string(),
                icon: item.icon.map(String::from),
            })
            .collect();
        let title = params.get(params::TITLE).unwrap_or_default().to_string();
        let text = params.get(params::TEXT).unwrap_or_default().to_string();
        let page = params.get(params::PAGE).unwrap_or_default().to_string();
        let (line, small) = bar_font(env).map_or((style.text(), style.small()), |font| {
            (
                font.line_metrics(style.text()).line_height,
                font.line_metrics(style.small()).line_height,
            )
        });
        let gap = style.gap();
        let info_height = if title.is_empty() && text.is_empty() && page.is_empty() {
            0
        } else {
            (line + small) as u16 + gap
        };
        let info = Rect::new(0, gap, display.width, info_height);
        // Columns as wide as their entries, justified between the margins:
        // the first starts under the title, the last ends with the page.
        // As many to a row as fit with a gap between them.
        let margin = style.margin();
        let room = f32::from(display.width.saturating_sub(2 * margin));
        let size = style.text() * ICON_SCALE;
        let widths: Vec<f32> = entries
            .iter()
            .map(|e| {
                let label = bar_font(env).map_or(0.0, |f| f.measure(style.small(), &e.label(env)));
                let icon = e
                    .icon
                    .as_deref()
                    .map_or(0.0, |i| ui::icon_width(env, i, size));
                (label.max(icon) + 0.99) as u32 as f32
            })
            .collect();
        let least_gap = f32::from(gap) * 2.0;
        let count = entries.len().max(1);
        let column_widths = |columns: usize| -> Vec<f32> {
            (0..columns)
                .map(|c| {
                    widths
                        .iter()
                        .skip(c)
                        .step_by(columns)
                        .fold(0.0f32, |a, &w| a.max(w))
                })
                .collect()
        };
        let columns = (1..=count)
            .rev()
            .find(|&n| column_widths(n).iter().sum::<f32>() + least_gap * (n - 1) as f32 <= room)
            .unwrap_or(1);
        // Rows filled evenly (four and three rather than five and two).
        let rows = count.div_ceil(columns);
        let columns = count.div_ceil(rows);
        let column_width = column_widths(columns);
        let spare = (room - column_width.iter().sum::<f32>()).max(0.0);
        let between = if columns > 1 {
            spare / (columns - 1) as f32
        } else {
            0.0
        };
        let lefts: Vec<f32> = column_width
            .iter()
            .scan(f32::from(margin), |x, &w| {
                let left = *x;
                *x += w + between;
                Some(left)
            })
            .collect();
        let cell_height = (size + small) as u16 + 2 * gap;
        let top = info.y + info.height + gap;
        let cells: Vec<Rect> = (0..entries.len())
            .map(|i| {
                let (row, column) = (i / columns, i % columns);
                Rect::new(
                    lefts[column] as u16,
                    top + row as u16 * cell_height,
                    column_width[column] as u16,
                    cell_height,
                )
            })
            .collect();
        // Taps count up to halfway to the next entry.
        let half = (between / 2.0) as u16;
        let targets = cells
            .iter()
            .map(|c| Rect::new(c.x.saturating_sub(half), c.y, c.width + 2 * half, c.height))
            .collect();
        let border = (u32::from(style.line()) * 2) as u16;
        let height = top + rows as u16 * cell_height + gap + border;
        // The first entry that can be chosen.
        let focus = entries.iter().position(|e| e.enabled(env)).unwrap_or(0);
        Self {
            entries,
            region: Rect::new(0, 0, display.width, height.min(display.height)),
            title,
            text,
            page,
            info,
            info_action: params.get(params::ACTION).and_then(|a| Id::parse(a).ok()),
            cells,
            targets,
            focus,
            keys: ui::focus_shown(env),
            style,
            params: params.clone(),
        }
    }

    /// Whether entry `index` is a switch that is on: a setting, or an
    /// entry its plugin shows as one.
    fn on(env: &Env<'_>, id: &Id) -> bool {
        if let Some(on) = env.menus().state(env, id.as_str()).checked {
            return on;
        }
        env.settings()
            .specs()
            .find(|spec| spec.id == *id)
            .is_some_and(|spec| spec.kind == SettingKind::Bool && env.settings().bool(id.as_str()))
    }

    fn run(&mut self, index: usize, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        // Shown faint by its plugin: nothing to do now.
        let Some(entry) = self.entries.get(index).filter(|e| e.enabled(&env)) else {
            return;
        };
        let id = entry.id.clone();
        if is_menu(&env, &id) {
            cx.command(Command::OpenScreen(
                SCREEN,
                Params::new()
                    .with(params::MENU, id.as_str().to_string())
                    .with(DEPTH, "1"),
            ));
            return;
        }
        let Some(spec) = env.settings().specs().find(|spec| spec.id == id) else {
            cx.command(Command::Back);
            cx.command(Command::run(id));
            return;
        };
        // Its own page (e.g. the lock's, which asks for the code), the
        // keyboard for text, the list of its values.
        if let Some(command) = ui::setting_opens(&env, spec) {
            cx.command(command);
            return;
        }
        match &spec.kind {
            SettingKind::Bool => {
                let value = SettingValue::Bool(!env.settings().bool(id.as_str()));
                cx.command(Command::SetSetting(id, value));
                cx.request_redraw();
                cx.request_refresh(RefreshMode::Fast);
            }
            SettingKind::Choice(_) | SettingKind::Language => {
                if let Some(value) = ui::next_value(&env, spec, 1) {
                    cx.command(Command::SetSetting(id, value));
                    cx.request_redraw();
                    cx.request_refresh(RefreshMode::Fast);
                }
            }
            // Numbers have no room here: their settings page.
            _ => {
                let group = env.settings().group_of(spec);
                cx.command(Command::OpenSlot(
                    slots::SETTINGS,
                    Params::new().with(params::ID, group.as_str().to_string()),
                ));
            }
        }
    }

    fn move_focus(&mut self, delta: isize, cx: &mut Cx<'_, '_>) {
        if !self.keys {
            // The first button press shows where the focus is.
            self.keys = true;
        } else {
            // Past entries shown faint, unless all are.
            let env = *cx.env();
            let len = self.entries.len();
            let mut focus = ui::step_focus(self.focus, delta, len);
            for _ in 0..len {
                if self.entries.get(focus).is_none_or(|e| e.enabled(&env)) {
                    break;
                }
                focus = ui::step_focus(focus, delta, len);
            }
            if self.entries.get(focus).is_some_and(|e| e.enabled(&env)) {
                self.focus = focus;
            }
        }
        cx.request_redraw_area(self.region);
        cx.request_refresh(RefreshMode::Fast);
    }
}

impl Screen for BarScreen {
    fn region(&self, _: &Env<'_>) -> Option<Rect> {
        Some(self.region)
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let mut ui = ui::Ui::new(canvas, env, &style);
        let r = self.region;
        ui.canvas().fill(r, Color::WHITE);
        let border = (u32::from(style.line()) * 2) as u16;
        ui.canvas().fill(
            Rect::new(r.x, r.y + r.height - border, r.width, border),
            Color::BLACK,
        );
        let Some(font) = bar_font(env) else {
            return;
        };
        let margin = style.margin() as f32;
        if self.info.height > 0 {
            let line = font.line_metrics(style.text()).line_height as u16;
            let title = Rect::new(0, self.info.y, r.width, line);
            let text = Rect::new(0, self.info.y + line, r.width, self.info.height - line);
            ui::Label::new(&self.title)
                .px(style.text())
                .font(font)
                .color(Color::BLACK)
                .draw_from(&mut ui, margin, title);
            let width = f32::from(r.width) - 2.0 * margin;
            let (chapter, page, page_width) =
                ui::fit_pair(&font, style.small(), &self.text, &self.page, width, margin);
            let gray = style.faint(ui::Ink::DETAIL);
            ui::Label::new(&chapter)
                .px(style.small())
                .font(font)
                .color(gray)
                .draw_from(&mut ui, margin, text);
            let x = f32::from(r.width) - margin - page_width;
            ui::Label::new(&page)
                .px(style.small())
                .font(font)
                .color(gray)
                .draw_from(&mut ui, x, text);
        }
        let size = style.text() * ICON_SCALE;
        let small_line = font.line_metrics(style.small()).line_height as u16;
        for (i, (entry, cell)) in self.entries.iter().zip(&self.cells).enumerate() {
            let on = Self::on(env, &entry.id);
            let enabled = entry.enabled(env);
            let inner = *cell;
            // Switched on or focused: a frame a little around the entry.
            let pad = style.gap() / 2;
            let frame = Rect::new(
                inner.x.saturating_sub(pad),
                inner.y,
                inner.width + 2 * pad,
                inner.height,
            );
            // Told as a button: what it is, which has the focus, which is on.
            let focused = self.keys && i == self.focus;
            ui.canvas()
                .describe(Role::Button, &entry.label(env), frame)
                .focused(focused)
                .on(on.then_some(true));
            if on {
                let back = if enabled {
                    Color::BLACK
                } else {
                    style.faint(ui::Ink::NOTE)
                };
                ui::fill_round(ui.canvas(), &style, frame, back);
            }
            if focused {
                ui::stroke_round(
                    ui.canvas(),
                    &style,
                    frame,
                    ui::button_border(&style),
                    Color::BLACK,
                );
            }
            let ink = match (on, enabled) {
                (true, _) => Color::WHITE,
                (false, true) => Color::BLACK,
                (false, false) => style.faint(ui::Ink::NOTE),
            };
            let icon_area = Rect::new(inner.x, inner.y + style.gap(), inner.width, size as u16);
            if let Some(icon) = &entry.icon {
                let width = ui::icon_width(env, icon, size);
                let x = f32::from(inner.x) + (f32::from(inner.width) - width) / 2.0;
                ui.icon(icon, size, x, icon_area, ink);
            }
            let label_area = Rect::new(
                inner.x,
                icon_area.y + icon_area.height,
                inner.width,
                small_line,
            );
            let room = f32::from(inner.width) + 1.0;
            let label = ui::fit(&font, style.small(), &entry.label(env), room);
            let x = f32::from(inner.x)
                + (f32::from(inner.width) - font.measure(style.small(), &label)) / 2.0;
            ui::Label::new(&label)
                .px(style.small())
                .font(font)
                .color(ink)
                .draw_from(&mut ui, x, label_area);
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let a = action;
        // Turning pages (also by swiping) does not show the focus on
        // touch screens; directions do.
        if !self.keys && (*a == actions::NEXT_PAGE || *a == actions::PREV_PAGE) {
            return Handled::Yes;
        }
        if [
            actions::LEFT,
            actions::UP,
            actions::PREVIOUS,
            actions::PREV_PAGE,
        ]
        .contains(a)
        {
            self.move_focus(-1, cx);
        } else if [
            actions::RIGHT,
            actions::DOWN,
            actions::NEXT,
            actions::NEXT_PAGE,
        ]
        .contains(a)
        {
            self.move_focus(1, cx);
        } else if *a == actions::CONFIRM {
            self.run(self.focus, cx);
        } else if *a == actions::MENU {
            cx.command(Command::Back);
        } else {
            return Handled::No;
        }
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        if let Some(index) = self.targets.iter().position(|c| c.contains(gesture.start)) {
            self.focus = index;
            self.run(index, cx);
        } else if let Some(action) = self.info_action.clone()
            && self.info.contains(gesture.start)
        {
            cx.command(Command::Back);
            cx.command(Command::run(action));
        }
        Handled::Yes
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        // A switch shown here may have changed; entries shown only while a
        // setting has a value come or go with it.
        if let Some(changed) = events::SETTING_CHANGED.read(event, params) {
            let env = *cx.env();
            let before = self.region;
            if env.settings().governs(changed.id.as_str()) {
                // Anew, the focus on the entry it was on (where it stays).
                let focused = self.entries.get(self.focus).map(|e| e.id.clone());
                let keys = self.keys;
                let opened = self.params.clone();
                *self = BarScreen::new(&env, &opened);
                if let Some(at) =
                    focused.and_then(|id| self.entries.iter().position(|e| e.id == id))
                {
                    self.focus = at;
                }
                self.keys = keys;
            }
            // What it covered before too, should it have shrunk.
            cx.request_redraw_area(before.union(&self.region));
        }
    }
}
