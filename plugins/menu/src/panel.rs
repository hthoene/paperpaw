//! A menu shown as a panel of controls at the bottom of the screen, for
//! quick changes while reading: switches become buttons (several to a
//! row), numbers get `−` and `+` (on touch screens those marked as
//! sliders, such as the frontlight, are sliders across the screen, as on
//! the settings pages), choices with few options become segments, other
//! choices open their list, actions become buttons. Changes apply at
//! once and the panel stays open. Entries show the label, switch and
//! badge their plugin gives them now; a faint one is passed by the focus
//! and does nothing. On touch screens its buttons are as
//! large as a fingertip.
//!
//! Bound to the `core.panel` slot with the parameter `menu`; what the
//! panel shows is that menu, so users and plugins choose its entries like
//! any menu's. With the parameter `mode` set to `top` it hangs from the
//! top of the screen instead (the quick panel).

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, events, params, slots};
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::{
    Canvas, Color, Command, Cx, Env, Gesture, GestureKind, Handled, Hint, Id, MenuItem, Params,
    PointerEvent, PointerPhase, Rect, RefreshMode, Screen, SettingKind, SettingSpec, SettingValue,
};

use crate::track::Track;
use crate::{choose, is_menu};

/// Most switches or buttons in one row.
const PER_ROW: usize = 3;
/// Most options shown as segments; more open a list.
const MAX_SEGMENTS: usize = 4;
/// Most of the screen the panel may cover.
const MAX_SHARE: f32 = 0.8;
/// Most of it the quick panel may cover, hanging from the top as phones'
/// quick settings do: below it, room is left to tap to close it.
const MAX_SHARE_TOP: f32 = 0.95;

/// What tapping a control does.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Control {
    /// Switches a setting on or off.
    Toggle(Id),
    /// Changes a number by a step down (`-1`) or up (`1`).
    Step(Id, i64),
    /// A number's slider, its whole row: set by touch (see
    /// `on_pointer`), or with the buttons once chosen.
    Slide(Id),
    /// Sets a choice to this option.
    Option(Id, String),
    /// Opens the setting's list or editor.
    Open(Id),
    /// Runs an action or opens a submenu.
    Run(Id),
}

impl Control {
    /// The menu entry it is part of.
    fn id(&self) -> &Id {
        match self {
            Control::Toggle(id)
            | Control::Step(id, _)
            | Control::Slide(id)
            | Control::Option(id, _)
            | Control::Open(id)
            | Control::Run(id) => id,
        }
    }

    /// It can be used now: its entry is not shown faint.
    fn enabled(&self, env: &Env<'_>) -> bool {
        env.menus().state(env, self.id().as_str()).enabled
    }
}

/// A row: an optional label on the left, then its controls.
#[derive(Debug)]
struct Row {
    area: Rect,
    /// The page it is on.
    page: usize,
    /// The setting the label names (a number's or a choice's).
    label: Option<Id>,
    controls: Vec<(Control, Rect)>,
}

pub(crate) struct PanelScreen {
    rows: Vec<Row>,
    /// It hangs from the top of the screen.
    top: bool,
    style: UiStyle,
    region: Rect,
    /// The control chosen with buttons: its place in reading order.
    focus: Option<usize>,
    /// The focused slider is moved with the buttons: its value before.
    editing: Option<i64>,
    /// A slider's knob taken along by a finger: its row and its value
    /// before.
    drag: ui::slider::SliderDrag,
    dragged: Option<(usize, i64)>,
    page: usize,
    pager: Option<ui::Pager>,
    /// What it was opened with, to make it anew (see `on_event`).
    params: Params,
}

fn spec<'e>(env: &Env<'e>, id: &Id) -> Option<&'e SettingSpec> {
    env.settings().specs().find(|spec| spec.id == *id)
}

/// What a row of the panel shows, before it is placed.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Line {
    /// Switches or buttons side by side.
    Buttons(Vec<Control>),
    /// A number set with a slider across the screen, its title and
    /// value above it; the more its title needs than a line (see
    /// [`ui::slider::title_extra`]).
    Slider(Id, u16),
    /// A number's `−` and `+`, placed by its value when drawn.
    Stepper(Id),
    /// A choice's options side by side.
    Segments(Id, Vec<String>),
    /// A setting opened in its list or editor.
    Open(Id),
}

/// The sizes the panel is laid out with.
#[derive(Debug, Clone, Copy)]
struct Geometry {
    /// The screen's.
    width: u16,
    height: u16,
    gap: u16,
    /// Between buttons side by side: wider on touch screens, so that a
    /// finger hits the one it means.
    spacing: u16,
    row: u16,
    /// Rows with a slider.
    slider: u16,
    border: u16,
    /// The labels of settings, on the left of their rows.
    label: u16,
    /// The band above it the screen's status line keeps (hanging from
    /// the top only): the panel leaves it as it is.
    head: u16,
    /// The bar that turns pages, where there are several.
    pager: u16,
    /// It hangs from the top of the screen.
    top: bool,
    /// The sizes buttons are made from.
    style: UiStyle,
}

impl Geometry {
    /// The margin at the screen's sides, the style's.
    fn margin(&self) -> u16 {
        self.style.margin()
    }
}

/// Where the panel's parts go.
#[derive(Debug)]
struct Layout {
    rows: Vec<Row>,
    region: Rect,
    pager: Option<Rect>,
}

/// The height of `line`'s row.
fn height(line: &Line, g: Geometry) -> u16 {
    match line {
        Line::Slider(_, extra) => g.slider + extra,
        _ => g.row,
    }
}

/// Places `lines` on the screen: at its bottom (or top), at most
/// [`MAX_SHARE`] of it high, in pages if they need more.
fn layout(lines: &[Line], g: Geometry) -> Layout {
    let gap = g.gap;
    let head = if g.top { g.head } else { 0 };
    let share = if g.top { MAX_SHARE_TOP } else { MAX_SHARE };
    let room = ((f32::from(g.height) * share) as u16).saturating_sub(g.border + 2 * gap + head);
    let heights: Vec<u16> = lines.iter().map(|line| height(line, g)).collect();
    // Too many rows: pages, with a bar to turn them.
    let paged = heights.iter().map(|h| u32::from(*h)).sum::<u32>() > u32::from(room);
    let bar = if paged { g.pager } else { 0 };
    let room = room.saturating_sub(bar);
    // Each row's page and where on it, rows filling a page in turn.
    let mut placed = Vec::with_capacity(lines.len());
    let (mut page, mut used, mut tallest) = (0, 0u16, 0u16);
    for h in &heights {
        if used > 0 && used + h > room {
            page += 1;
            used = 0;
        }
        placed.push((page, used));
        used += h;
        tallest = tallest.max(used);
    }
    let height = g.border + 2 * gap + tallest.max(g.row) + bar;
    // From the top below the screen's status line, which stays as it is.
    let region = if g.top {
        Rect::new(0, head, g.width, height)
    } else {
        Rect::new(0, g.height - height, g.width, height)
    };
    // The border on the side towards the rest of the screen.
    let inner_top = if g.top { region.y } else { region.y + g.border };
    let rows = lines
        .iter()
        .zip(heights)
        .zip(placed)
        .map(|((line, h), (page, at))| {
            let y = inner_top + gap + at;
            Row {
                page,
                ..place(line, Rect::new(0, y, g.width, h), g)
            }
        })
        .collect();
    let pager = paged.then(|| {
        let bottom = if g.top { g.border } else { 0 };
        Rect::new(0, region.y + region.height - bar - bottom, g.width, bar)
    });
    Layout {
        rows,
        region,
        pager,
    }
}

/// The controls of `line` in its row, `area`.
fn place(line: &Line, area: Rect, g: Geometry) -> Row {
    let inner = inset(area, &g);
    let rest = Rect::new(
        inner.x + g.label,
        inner.y,
        inner.width.saturating_sub(g.label),
        inner.height,
    );
    let (label, controls) = match line {
        Line::Buttons(controls) => {
            let cells = ui::columns(inner, controls.len(), g.spacing);
            (None, controls.iter().cloned().zip(cells).collect())
        }
        // Drawn as the settings pages draw it, across the row.
        Line::Slider(id, _) => (
            Some(id.clone()),
            alloc::vec![(Control::Slide(id.clone()), area)],
        ),
        // The − and + are placed by the value, as in menus.
        Line::Stepper(id) => {
            let controls = alloc::vec![
                (Control::Step(id.clone(), -1), area),
                (Control::Step(id.clone(), 1), area),
            ];
            (Some(id.clone()), controls)
        }
        Line::Segments(id, options) => {
            let cells = ui::columns(rest, options.len(), g.spacing);
            let controls = options
                .iter()
                .zip(cells)
                .map(|(o, r)| (Control::Option(id.clone(), o.clone()), r))
                .collect();
            (Some(id.clone()), controls)
        }
        // The whole row, as in lists: its text within the margins, its
        // focus as other rows' (a little in from the screen's edges).
        Line::Open(id) => (
            Some(id.clone()),
            alloc::vec![(Control::Open(id.clone()), area)],
        ),
    };
    Row {
        area,
        page: 0,
        label,
        controls,
    }
}

impl PanelScreen {
    pub(crate) fn open(env: &Env<'_>, params: &Params) -> Self {
        let menu = params.get(params::MENU).unwrap_or_default();
        let top = params.get(params::MODE) == Some("top");
        let style = UiStyle::new(env);
        let display = env.device().display;
        let width = display.width;
        let margin = style.margin();
        let mut items = env.menus().shown_items(env, menu);
        // A setting shown only while another has a value.
        items.retain(|item| {
            env.settings()
                .spec(item.id.as_str())
                .is_none_or(|s| env.settings().shows(s))
        });
        // Labels as wide as the longest needs, at most half the row;
        // sliders have their titles above them.
        let font = style.font(env);
        let measure = |text: &str| font.as_ref().map_or(0.0, |f| f.measure(style.text(), text));
        let longest = items
            .iter()
            .filter_map(|item| spec(env, item.id))
            .filter(|s| !matches!(s.kind, SettingKind::Bool) && !ui::slider::shows(env, s))
            .map(|s| measure(env.i18n().tr(&s.title)))
            .fold(0.0, f32::max);
        let label =
            ((longest as u16).saturating_add(style.gap() * 2)).min((width - 2 * margin) / 2);
        let gap = style.gap();
        // As high as a status line at the top of a screen (the home
        // screen's): it stays in sight above the panel.
        let head = if top {
            paperpaw_sdk::widgets::line_height(env, style.small()) + style.gap() / 2
        } else {
            0
        };
        // By touch, buttons a fingertip large and a little apart.
        let touch = env.device().input.touch;
        let geometry = Geometry {
            width,
            height: display.height,
            gap,
            spacing: if touch { gap } else { gap / 2 },
            row: style.touch_row(u32::from(style.row())) as u16,
            // Without the room below that rows of lists keep: the band
            // has room around its buttons.
            slider: ui::slider::row_height(&style, false).saturating_sub(gap),
            border: (u32::from(style.line()) * 2).max(2) as u16,
            label,
            head,
            pager: style.touch_row(u32::from(ui::Pager::height(&style))) as u16,
            top,
            style,
        };
        let layout = layout(&lines(env, &style, &items, geometry), geometry);
        // With buttons, on the first control that can be used.
        let first = layout
            .rows
            .iter()
            .flat_map(|row| row.controls.iter())
            .position(|(c, _)| c.enabled(env))
            .unwrap_or(0);
        Self {
            rows: layout.rows,
            style,
            region: layout.region,
            top,
            focus: (!touch).then_some(first),
            editing: None,
            drag: ui::slider::SliderDrag::default(),
            dragged: None,
            page: 0,
            pager: layout.pager.map(|area| ui::Pager::new(env, area)),
            params: params.clone(),
        }
    }
}

/// What the panel shows of `items`, row by row: switches and buttons
/// several to a row (as many as stay readable; switches and buttons not
/// mixed), numbers with their `−` and `+` (sliders for those marked so,
/// on touch screens), choices with few options as segments, anything
/// else to open.
fn lines(env: &Env<'_>, style: &UiStyle, items: &[MenuItem<'_>], g: Geometry) -> Vec<Line> {
    let font = style.font(env);
    let label_of = |control: &Control| -> String {
        match control {
            Control::Toggle(id) => spec(env, id).map_or(String::new(), |s| {
                alloc::format!("• {}", button_text(env, id, env.i18n().tr(&s.title)))
            }),
            Control::Run(id) => button_text(env, id, &action_title(env, id)),
            _ => String::new(),
        }
    };
    let small = |text: &str| {
        font.as_ref()
            .map_or(0.0, |f| f.measure(style.small(), text))
    };
    // As many buttons to a row as stay readable.
    let per_row = |group: &[Control]| -> usize {
        let inner = f32::from(g.width - 2 * g.margin());
        (1..=PER_ROW.min(group.len()))
            .rev()
            .find(|&n| {
                let cell = inner / n as f32 - 2.0 * g.gap as f32;
                group.iter().all(|c| small(&label_of(c)) <= cell)
            })
            .unwrap_or(1)
    };
    let flush = |group: &mut Vec<Control>, lines: &mut Vec<Line>| {
        if group.is_empty() {
            return;
        }
        // As few rows as the widest label allows, as evenly filled as
        // can be (four buttons two and two, not three and one).
        let rows = group.len().div_ceil(per_row(group));
        let n = group.len().div_ceil(rows);
        for chunk in core::mem::take(group).chunks(n) {
            lines.push(Line::Buttons(chunk.to_vec()));
        }
    };
    // Where a setting's controls go, beside its label.
    let inner = inset(Rect::new(0, 0, g.width, g.row), &g);
    let rest = Rect::new(
        inner.x + g.label,
        inner.y,
        inner.width.saturating_sub(g.label),
        inner.height,
    );
    let mut lines = Vec::new();
    let mut group: Vec<Control> = Vec::new();
    for item in items {
        let id = item.id.clone();
        let setting = spec(env, &id).filter(|s| s.extra().editor.is_none());
        let control = match setting.map(|s| &s.kind) {
            Some(SettingKind::Bool) => Some(Control::Toggle(id.clone())),
            None if spec(env, &id).is_none() => Some(Control::Run(id.clone())),
            _ => None,
        };
        if let Some(control) = control {
            // Switches and buttons share rows, but not with each other;
            // those that follow each other are shared out evenly among
            // rows (see `flush`).
            let same = group.first().is_some_and(|first| {
                matches!(first, Control::Toggle(_)) == matches!(control, Control::Toggle(_))
            });
            if !group.is_empty() && !same {
                flush(&mut group, &mut lines);
            }
            group.push(control);
            continue;
        }
        flush(&mut group, &mut lines);
        let line = match spec(env, &id).map(|s| &s.kind) {
            Some(SettingKind::Int { .. }) if setting.is_some_and(|s| ui::slider::shows(env, s)) => {
                // A long title on more lines: the row grows, never cut.
                let extra = setting.map_or(0, |s| slider_extra(env, style, g.width, s));
                Line::Slider(id, extra)
            }
            Some(SettingKind::Int { .. }) if setting.is_some() => Line::Stepper(id),
            Some(SettingKind::Choice(options))
                if setting.is_some() && fits_segments(env, style, options, rest) =>
            {
                Line::Segments(id, options.iter().map(|o| o.to_string()).collect())
            }
            _ => Line::Open(id),
        };
        lines.push(line);
    }
    flush(&mut group, &mut lines);
    lines
}

impl PanelScreen {
    fn pages(&self) -> usize {
        self.rows.last().map_or(1, |row| row.page + 1)
    }

    /// The rows of the page shown, with the number of controls before them.
    fn visible_rows(&self) -> impl Iterator<Item = (usize, &Row)> {
        let page = self.page;
        self.rows
            .iter()
            .scan(0, |count, row| {
                let at = *count;
                *count += row.controls.len();
                Some((at, row))
            })
            .filter(move |(_, row)| row.page == page)
    }

    /// The page with control `index`.
    fn page_of(&self, index: usize) -> usize {
        let mut count = 0;
        for row in &self.rows {
            count += row.controls.len();
            if index < count {
                return row.page;
            }
        }
        0
    }

    /// The focused control if it is a slider: its setting and range.
    fn focused_slider<'e>(&self, env: &Env<'e>) -> Option<(&'e SettingSpec, ui::slider::Slider)> {
        match self.focus.and_then(|f| self.controls().nth(f)) {
            Some((Control::Slide(id), _)) => {
                let spec = spec(env, id)?;
                Some((spec, ui::slider::Slider::of(spec)?))
            }
            _ => None,
        }
    }

    /// The row of the slider moving now, by touch or with the buttons.
    fn moving_row(&self) -> Option<Rect> {
        match self.dragged {
            Some((row, _)) => self.rows.get(row).map(|r| r.area),
            None => {
                self.editing?;
                let (_, area) = self.controls().nth(self.focus?)?;
                Some(*area)
            }
        }
    }

    /// Moving the focused slider with the buttons: a step, or done
    /// (keeping the value or not).
    fn on_editing(&mut self, action: &Id, before: i64, cx: &mut Cx<'_, '_>) -> Handled {
        let env = *cx.env();
        let Some((spec, slider)) = self.focused_slider(&env) else {
            self.editing = None;
            return Handled::No;
        };
        let id = spec.id.clone();
        if let Some(step) = ui::slider::key_step(action) {
            let now = env.settings().int(id.as_str());
            // The size of book text on to the next that looks different.
            let value = ui::shown_step(&env, spec, now, slider.stepped(now, step));
            show(cx, id, value);
            return Handled::Yes;
        }
        let keep = *action == actions::CONFIRM;
        if !keep && *action != actions::BACK && *action != actions::MENU {
            return Handled::No;
        }
        self.editing = None;
        let value = env.settings().int(id.as_str());
        let row = self.rows_of(&id).unwrap_or(self.region);
        done(
            &env,
            cx,
            id,
            (keep && value != before).then_some(value),
            row,
        );
        Handled::Yes
    }

    /// Every control in reading order.
    /// Where the rows on the page shown that show setting `id` are, all
    /// of them in one rectangle; `None` if none does.
    fn rows_of(&self, id: &Id) -> Option<Rect> {
        self.rows
            .iter()
            .filter(|row| row.page == self.page)
            .filter(|row| {
                row.label.as_ref() == Some(id) || row.controls.iter().any(|(c, _)| c.id() == id)
            })
            .map(|row| row.area)
            .reduce(|a, b| a.union(&b))
    }

    /// Where the row holding control `index` (in reading order) is.
    fn row_area(&self, index: usize) -> Option<Rect> {
        let mut count = 0;
        self.rows.iter().find_map(|row| {
            count += row.controls.len();
            (index < count).then_some(row.area)
        })
    }

    fn controls(&self) -> impl Iterator<Item = &(Control, Rect)> {
        self.rows.iter().flat_map(|row| row.controls.iter())
    }

    /// Where the controls of `row` are now: a number's `−` and `+` follow
    /// its value's width.
    fn placed(&self, env: &Env<'_>, row: &Row) -> Vec<(Control, Rect)> {
        let steps = row.label.as_ref().and_then(|id| spec(env, id)).filter(|s| {
            matches!(s.kind, SettingKind::Int { .. })
                && matches!(row.controls.first(), Some((Control::Step(..), _)))
                && row.controls.len() == 2
        });
        match steps {
            Some(spec) => {
                let value = ui::setting_value(env, spec);
                match ui::stepper(env, &self.style, row.area, &value) {
                    Some((minus, plus)) => alloc::vec![
                        (row.controls[0].0.clone(), minus),
                        (row.controls[1].0.clone(), plus),
                    ],
                    None => Vec::new(),
                }
            }
            None => row.controls.clone(),
        }
    }

    fn activate(&mut self, control: &Control, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        // Shown faint by its plugin: nothing to do now.
        if !control.enabled(&env) {
            return;
        }
        let set = |cx: &mut Cx<'_, '_>, id: &Id, value: SettingValue| {
            if env.settings().get(id.as_str()) != Some(&value) {
                cx.command(Command::SetSetting(id.clone(), value));
            }
        };
        match control {
            Control::Toggle(id) => {
                let on = env.settings().bool(id.as_str());
                set(cx, id, SettingValue::Bool(!on));
            }
            Control::Step(id, step) => {
                if let Some(value) = spec(&env, id).and_then(|s| ui::next_value(&env, s, *step)) {
                    set(cx, id, value);
                }
            }
            Control::Option(id, option) => {
                set(cx, id, SettingValue::Text(option.clone().into()));
            }
            // By touch see `on_pointer`, with the buttons `on_action`.
            Control::Slide(_) => {}
            Control::Open(id) => {
                let Some(setting) = spec(&env, id) else {
                    return;
                };
                match ui::setting_opens(&env, setting) {
                    Some(command) => cx.command(command),
                    None => choose(&env, setting, cx),
                }
            }
            Control::Run(id) if is_menu(&env, id) => {
                cx.command(Command::Back);
                cx.command(Command::OpenSlot(
                    slots::MENU,
                    Params::new().with(params::MENU, id.as_str().to_string()),
                ));
            }
            Control::Run(id) => {
                cx.command(Command::Back);
                cx.command(Command::run(id.clone()));
            }
        }
    }

    fn redraw(&self, cx: &mut Cx<'_, '_>) {
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Fast);
    }

    fn draw_control(&self, ui: &mut ui::Ui<'_>, control: &Control, rect: Rect, focused: bool) {
        let env = ui.env();
        let state = env.menus().state(env, control.id().as_str());
        let (on, label): (bool, String) = match control {
            Control::Toggle(id) => {
                let name = spec(env, id).map_or("", |s| env.i18n().tr(&s.title));
                let mark = if env.settings().is_scoped(id.as_str()) {
                    "• "
                } else {
                    ""
                };
                (
                    state
                        .checked
                        .unwrap_or_else(|| env.settings().bool(id.as_str())),
                    alloc::format!("{mark}{}", state.text(name)),
                )
            }
            Control::Step(_, step) => (false, String::from(if *step < 0 { "−" } else { "+" })),
            Control::Slide(id) => {
                let focus = match (focused, self.editing) {
                    (false, _) => ui::slider::SliderFocus::None,
                    (true, None) => ui::slider::SliderFocus::Focused,
                    (true, Some(_)) => ui::slider::SliderFocus::Editing,
                };
                return draw_slider(ui, id, rect, focus);
            }
            Control::Option(id, option) => (
                env.settings().text(id.as_str()) == option,
                env.i18n().tr(option).to_string(),
            ),
            Control::Open(id) => {
                // A row: the setting's name, its value and an arrow.
                let Some(spec) = spec(env, id) else {
                    return;
                };
                if focused {
                    ui.focus(rect);
                }
                let value = alloc::format!("{} ›", ui::setting_value(env, spec));
                let title = state.text(env.i18n().tr(&spec.title));
                ui::Row::value(&title, &value)
                    .focused(focused)
                    .draw(ui, rect);
                return;
            }
            Control::Run(id) => (
                state.checked.unwrap_or(false),
                button_text(env, id, &action_title(env, id)),
            ),
        };
        if state.enabled {
            ui.button(rect, &label, on, focused);
        } else {
            ui.faint_button(rect, &label, focused);
        }
    }
}

/// A slider's row, as on the settings pages: its title and value (the
/// one shown while it moves), and the slider across the screen below, on
/// the focus bar when focused.
fn draw_slider(ui: &mut ui::Ui<'_>, id: &Id, rect: Rect, focus: ui::slider::SliderFocus) {
    let (env, style) = (ui.env(), ui.style());
    let Some((spec, track)) = spec(env, id).and_then(|s| Some((s, Track::of(env, s)?))) else {
        return;
    };
    if focus != ui::slider::SliderFocus::None {
        ui.focus(rect);
    }
    let shown = ui::setting_value(env, spec);
    let room = ui::number_room(env, style, [spec]);
    let at = track.at(env.settings().int(id.as_str()));
    let view = ui::slider::SliderView::new(rect, at, &shown)
        .title(env.i18n().tr(&spec.title))
        .room(&room)
        .focus(focus);
    track.slider().draw_row(ui.canvas(), env, style, &view);
}

/// How much higher than its line a slider's title is in a row `width`
/// wide ([`ui::slider::title_extra`]), beside the widest value it shows:
/// the same for every value, so its band stays where it was laid out.
fn slider_extra(env: &Env<'_>, style: &UiStyle, width: u16, spec: &SettingSpec) -> u16 {
    let room = ui::number_room(env, style, [spec]);
    ui::slider::title_extra(env, style, width, env.i18n().tr(&spec.title), &room)
}

/// The title of action `id`, translated.
fn action_title(env: &Env<'_>, id: &Id) -> String {
    let title = env
        .actions()
        .get(id.as_str())
        .map_or(id.as_str(), |a| a.title());
    env.i18n().tr(title).to_string()
}

/// What the button of entry `id` titled `title` says now: the label and
/// badge its plugin gives it.
fn button_text(env: &Env<'_>, id: &Id, title: &str) -> String {
    env.menus().state(env, id.as_str()).text(title)
}

/// Shows `value` for setting `id` without keeping it yet.
fn show(cx: &mut Cx<'_, '_>, id: Id, value: i64) {
    cx.command(Command::PreviewSettings(alloc::vec![(
        id,
        Some(SettingValue::Int(value))
    )]));
}

/// A slider is let go: `id` set to `value` if one is chosen, the value
/// shown meanwhile gone, and its `row` drawn again: cleanly after it was
/// moved (its quick steps leave ghosts), quickly after a step (a tap on
/// its `+`).
fn done(env: &Env<'_>, cx: &mut Cx<'_, '_>, id: Id, value: Option<i64>, row: Rect) {
    if let Some(value) = value {
        cx.command(Command::SetSetting(id, SettingValue::Int(value)));
    }
    let moved = env.settings().previewing();
    if moved {
        cx.command(Command::PreviewSettings(Vec::new()));
    }
    cx.request_redraw_area(row);
    cx.request_refresh(if moved {
        RefreshMode::Partial
    } else {
        RefreshMode::Fast
    });
}

/// Whether a choice's options fit side by side in `area`, readably.
fn fits_segments(
    env: &Env<'_>,
    style: &UiStyle,
    options: &paperpaw_sdk::Choices,
    area: Rect,
) -> bool {
    let Some(font) = style.font(env) else {
        return false;
    };
    if options.is_empty() || options.len() > MAX_SEGMENTS {
        return false;
    }
    let cell = f32::from(area.width) / options.len() as f32 - 2.0 * style.gap() as f32;
    options
        .iter()
        .all(|o| font.measure(style.small(), env.i18n().tr(o)) <= cell)
}

/// `area` without `margin` left and right and a little above and below.
fn inset(area: Rect, g: &Geometry) -> Rect {
    let pad = (area.height - ui::button_height(&g.style, area)) / 2;
    area.inset(g.margin(), pad)
}

impl Screen for PanelScreen {
    fn region(&self, _: &Env<'_>) -> Option<Rect> {
        Some(self.region)
    }

    fn dims_below(&self) -> bool {
        true
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let mut ui = ui::Ui::new(canvas, env, &style);
        let r = self.region;
        ui.canvas().fill(r, Color::WHITE);
        let border = (u32::from(style.line()) * 2).max(2);
        let y = if self.top {
            i32::from(r.y + r.height) - border as i32
        } else {
            i32::from(r.y)
        };
        ui.canvas()
            .fill_rect(i32::from(r.x), y, u32::from(r.width), border, Color::BLACK);
        let Some(font) = ui.font() else {
            return;
        };
        if self.rows.is_empty() {
            ui.message(r, env.i18n().tr("menu.empty"));
            return;
        }
        for (first, row) in self.visible_rows() {
            let label = row.label.as_ref().and_then(|id| spec(env, id));
            // A number with − and + by its value (a slider row has three).
            let number = label.filter(|_| {
                matches!(row.controls.first(), Some((Control::Step(..), _)))
                    && row.controls.len() == 2
            });
            if let Some(spec) = number {
                // As in menus: title, then [−] value [+].
                let focus = match self.focus {
                    Some(f) if f == first => Some(false),
                    Some(f) if f == first + 1 => Some(true),
                    _ => None,
                };
                let title = env.i18n().tr(&spec.title);
                let value = ui::setting_value(env, spec);
                ui::Row::number(title, &value, &value)
                    .step_focus(focus)
                    .draw(&mut ui, row.area);
                continue;
            }
            if let Some(spec) = label
                && !matches!(
                    row.controls.first(),
                    Some((Control::Open(_) | Control::Slide(_), _))
                )
            {
                let x = style.margin() as f32;
                let first = row.controls.first().map_or(r.width, |(_, c)| c.x);
                let room = f32::from(first) - x - style.gap() as f32;
                let name = ui::fit(&font, style.text(), env.i18n().tr(&spec.title), room);
                ui::Label::new(&name)
                    .px(style.text())
                    .color(Color::BLACK)
                    .draw_from(&mut ui, x, row.area);
            }
            for (index, (control, rect)) in (first..).zip(row.controls.iter()) {
                self.draw_control(&mut ui, control, *rect, self.focus == Some(index));
            }
        }
        if let Some(pager) = &self.pager {
            pager.draw(&mut ui, ui::Paging::new(self.page, self.pages()));
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        if let Some(before) = self.editing {
            return self.on_editing(action, before, cx);
        }
        let count = self.controls().count();
        let a = action;
        if *a == actions::CONFIRM {
            let control = self
                .focus
                .and_then(|f| self.controls().nth(f))
                .map(|(c, _)| c.clone());
            match control {
                // A slider chosen: the buttons move it now.
                Some(Control::Slide(id)) => {
                    self.editing = Some(cx.env().settings().int(id.as_str()));
                    self.redraw(cx);
                }
                Some(control) => self.activate(&control, cx),
                None => {
                    self.focus = Some(0);
                    self.redraw(cx);
                }
            }
            return Handled::Yes;
        }
        if *a == actions::MENU || *a == actions::BACK {
            cx.command(Command::Back);
            return Handled::Yes;
        }
        let back = [
            actions::UP,
            actions::LEFT,
            actions::PREVIOUS,
            actions::PREV_PAGE,
        ];
        let forward = [
            actions::DOWN,
            actions::RIGHT,
            actions::NEXT,
            actions::NEXT_PAGE,
        ];
        let delta: isize = if back.contains(a) {
            -1
        } else if forward.contains(a) {
            1
        } else {
            return Handled::No;
        };
        if count > 0 {
            let env = *cx.env();
            let current = self.focus.map_or(-1, |f| f as isize);
            let step = |from: isize| (from + delta).rem_euclid(count as isize) as usize;
            // Past controls shown faint, unless all are.
            let mut focus = step(current);
            for _ in 0..count {
                if self
                    .controls()
                    .nth(focus)
                    .is_none_or(|(c, _)| c.enabled(&env))
                {
                    break;
                }
                focus = step(focus as isize);
            }
            if self
                .controls()
                .nth(focus)
                .is_some_and(|(c, _)| !c.enabled(&env))
            {
                focus = step(current);
            }
            let (before, page) = (self.focus, self.page);
            self.focus = Some(focus);
            self.page = self.page_of(focus);
            match before.and_then(|b| self.row_area(b)) {
                // On the same page, the rows the focus left and came to.
                Some(left) if self.page == page => {
                    cx.request_redraw_area(left);
                    if let Some(reached) = self.row_area(focus) {
                        cx.request_redraw_area(reached);
                    }
                    cx.request_refresh(RefreshMode::Fast);
                }
                _ => self.redraw(cx),
            }
        }
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if !self.region.contains(gesture.start) {
            return Handled::No;
        }
        if let Some(action) = self.pager.and_then(|p| p.tap(gesture.start)) {
            let pages = self.pages();
            self.page = if action == actions::NEXT_PAGE {
                (self.page + 1).min(pages - 1)
            } else {
                self.page.saturating_sub(1)
            };
            self.focus = None;
            self.redraw(cx);
            return Handled::Yes;
        }
        if gesture.kind == GestureKind::Tap {
            let env = *cx.env();
            let control = self
                .visible_rows()
                .flat_map(|(_, row)| self.placed(&env, row))
                .find(|(_, rect)| rect.contains(gesture.start))
                .map(|(c, _)| c);
            if let Some(control) = control {
                // The focus shown goes.
                if self.focus.take().is_some() {
                    self.redraw(cx);
                }
                self.activate(&control, cx);
            }
        }
        Handled::Yes
    }

    fn on_pointer(&mut self, event: &PointerEvent, cx: &mut Cx<'_, '_>) -> Handled {
        let env = *cx.env();
        if !env.device().input.touch {
            return Handled::No;
        }
        // The row of the knob taken, or of a finger coming down.
        let row = match self.dragged {
            Some((row, _)) => Some(row),
            None if event.phase == PointerPhase::Down => self
                .rows
                .iter()
                .position(|r| r.page == self.page && r.area.contains(event.point)),
            None => None,
        };
        let Some((row, id, track, area)) = row.and_then(|index| {
            let row = &self.rows[index];
            let [(Control::Slide(id), area)] = row.controls.as_slice() else {
                return None;
            };
            let track = spec(&env, id).and_then(|s| Track::of(&env, s))?;
            // Shown faint by its plugin: not moved.
            Control::Slide(id.clone()).enabled(&env).then_some(())?;
            Some((index, id.clone(), track, *area))
        }) else {
            return Handled::No;
        };
        let extra = spec(&env, &id).map_or(0, |s| slider_extra(&env, &self.style, area.width, s));
        let band = ui::slider::band_below(&self.style, area, extra);
        let value = env.settings().int(id.as_str());
        let Some(step) =
            self.drag
                .on_pointer(event, &self.style, track.slider(), band, track.at(value))
        else {
            return Handled::No;
        };
        let before = match self.dragged {
            Some((_, before)) => before,
            None => {
                // The focus hides, as after other taps.
                if self.focus.is_some() || self.editing.is_some() {
                    self.focus = None;
                    self.editing = None;
                    self.redraw(cx);
                }
                self.dragged = Some((row, value));
                value
            }
        };
        // Only the sizes of book text that look different.
        let shown = |at: i64| spec(&env, &id).map_or(at, |s| track.value(&env, s, before, at));
        match step {
            ui::slider::SliderStep::Moved(value) => show(cx, id.clone(), shown(value)),
            ui::slider::SliderStep::Same => {}
            ui::slider::SliderStep::Chosen(value) => {
                let value = shown(value);
                self.dragged = None;
                let row = self.rows[row].area;
                done(&env, cx, id, (value != before).then_some(value), row);
            }
            ui::slider::SliderStep::Cancelled => {
                self.dragged = None;
                let row = self.rows[row].area;
                done(&env, cx, id, None, row);
            }
            _ => {}
        }
        Handled::Yes
    }

    fn hint(&self, action: &Id, env: &Env<'_>) -> Hint {
        // While the buttons move a slider: what each key does to it, as
        // on the settings pages.
        if self.editing.is_some() {
            ui::editing_hint(action, env)
        } else {
            Hint::Default
        }
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        // Values shown may have changed; controls shown only while a
        // setting has a value come or go with it.
        if let Some(changed) = events::SETTING_CHANGED.read(event, params) {
            let env = *cx.env();
            let before = self.region;
            if env.settings().governs(changed.id.as_str()) {
                // Anew, the focus on the control it was on (where it stays).
                let focused = self
                    .focus
                    .and_then(|at| self.controls().nth(at).map(|(c, _)| c.clone()));
                let page = self.page;
                let opened = self.params.clone();
                *self = PanelScreen::open(&env, &opened);
                self.focus = focused.and_then(|c| self.controls().position(|(d, _)| *d == c));
                self.page = page.min(self.pages().saturating_sub(1));
                // What it covered before too, should it have shrunk.
                cx.request_redraw_area(before.union(&self.region));
                self.redraw(cx);
            } else if let Some(rows) = self.rows_of(&changed.id) {
                // Its rows alone: a step of a slider or a number shows
                // quickly.
                cx.request_redraw_area(rows);
                cx.request_refresh(RefreshMode::Fast);
            } else {
                self.redraw(cx);
            }
        } else if *event == events::PREVIEW_CHANGED
            && let Some(area) = self.moving_row()
        {
            // A slider moving: its row alone, quickly.
            cx.request_redraw_area(area);
            cx.request_refresh(RefreshMode::Fast);
        }
    }
}

#[cfg(test)]
mod layout_tests {
    use super::*;

    fn geometry() -> Geometry {
        // Margins of 20: wider than the plain style's.
        let style = UiStyle::PLAIN.with_margin(20);
        Geometry {
            width: 480,
            height: 800,
            gap: 10,
            spacing: 5,
            row: 60,
            slider: 150,
            border: 2,
            label: 150,
            head: 40,
            pager: 45,
            top: false,
            style,
        }
    }

    fn id(name: &'static str) -> Id {
        Id::new_static(name)
    }

    #[test]
    fn at_the_bottom_rows_follow_each_other() {
        let lines = [Line::Stepper(id("a.size")), Line::Open(id("a.font"))];
        let layout = layout(&lines, geometry());
        assert_eq!(layout.region.y + layout.region.height, 800);
        assert!(layout.pager.is_none());
        let [first, second] = layout.rows.as_slice() else {
            panic!("two rows");
        };
        assert_eq!((first.page, second.page), (0, 0));
        assert_eq!(second.area.y, first.area.y + 60);
        assert!(first.area.y >= layout.region.y + 2);
        assert_eq!(first.label, Some(id("a.size")));
    }

    /// From the top, below the band of the screen's status line, which
    /// it leaves as it is.
    #[test]
    fn from_the_top_the_status_line_stays_in_sight() {
        let g = Geometry {
            top: true,
            ..geometry()
        };
        let layout = layout(&[Line::Open(id("a.font"))], g);
        assert_eq!(layout.region.y, 40);
        assert!(layout.rows[0].area.y >= 40);
    }

    #[test]
    fn too_many_rows_go_to_pages() {
        let lines: Vec<Line> = (0..20).map(|_| Line::Open(id("a.font"))).collect();
        let layout = layout(&lines, geometry());
        assert!(layout.rows.last().unwrap().page > 0);
        assert!(layout.region.height <= (800.0 * MAX_SHARE) as u16);
        let pager = layout.pager.expect("a pager");
        assert_eq!(pager.y + pager.height, 800);
        // A row of the second page stands where the first page's first one
        // does, and every row is within the panel above the pager.
        let second_page = layout.rows.iter().find(|r| r.page == 1).unwrap();
        assert_eq!(second_page.area.y, layout.rows[0].area.y);
        assert!(
            layout
                .rows
                .iter()
                .all(|r| r.area.bottom() <= u32::from(pager.y))
        );
    }

    #[test]
    fn sliders_take_their_height() {
        // Two sliders and six rows, 660 high: more than the panel may be
        // (640 less its border and gaps), so pages, the last two rows on
        // the second.
        let mut lines = alloc::vec![
            Line::Slider(id("a.light"), 0),
            Line::Slider(id("a.warmth"), 0)
        ];
        lines.extend((0..6).map(|_| Line::Open(id("a.font"))));
        let layout = layout(&lines, geometry());
        let rows = &layout.rows;
        assert_eq!(rows[0].area.height, 150);
        assert_eq!(rows[1].area.y, rows[0].area.y + 150);
        assert_eq!(rows[2].area.y, rows[1].area.y + 150);
        assert_eq!(rows[2].area.height, 60);
        let pages: Vec<usize> = rows.iter().map(|r| r.page).collect();
        assert_eq!(pages, [0, 0, 0, 0, 0, 0, 1, 1]);
        assert_eq!(rows[6].area.y, rows[0].area.y);
        let pager = layout.pager.expect("a pager");
        assert!(rows[5].area.bottom() <= u32::from(pager.y));
    }

    #[test]
    fn controls_share_their_row() {
        let buttons = Line::Buttons(alloc::vec![
            Control::Toggle(id("a.one")),
            Control::Toggle(id("a.two")),
            Control::Toggle(id("a.three")),
        ]);
        let row = place(&buttons, Rect::new(0, 100, 480, 60), geometry());
        assert_eq!(row.controls.len(), 3);
        let widths: Vec<u16> = row.controls.iter().map(|(_, r)| r.width).collect();
        // As wide as each other to a pixel, the last ending at the margin.
        assert!(widths.iter().all(|w| w.abs_diff(widths[0]) <= 1));
        assert_eq!(row.controls[2].1.right(), 480 - 20);
        assert_eq!(row.controls[1].1.x, row.controls[0].1.right() as u16 + 5);

        // A slider is its whole row, across the screen.
        let area = Rect::new(0, 0, 480, 150);
        let slider = place(&Line::Slider(id("a.light"), 0), area, geometry());
        assert_eq!(
            slider.controls,
            [(Control::Slide(id("a.light")), area)].as_slice()
        );

        let segments = Line::Segments(
            id("a.align"),
            alloc::vec![String::from("l"), String::from("r")],
        );
        let row = place(&segments, Rect::new(0, 0, 480, 60), geometry());
        assert_eq!(row.controls.len(), 2);
        assert_eq!(
            row.controls[1].0,
            Control::Option(id("a.align"), String::from("r"))
        );
    }
}
