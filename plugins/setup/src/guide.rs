//! The guide's screen, a page for each step and one at the end: dots for
//! how far it is, the question large with a word on it below, the answer
//! in a card (its value, a switch, or − and + with a sample of the text),
//! and *Next* as the button that is always right; *Skip* on the first
//! page, *Back* on the others.
//!
//! Buttons: up and down (or left and right) move between the card and
//! the buttons; on a number, left and right change it. *Select* chooses,
//! *Back* goes a page back; on the first page it asks whether to skip
//! the setup, as a stray press there would end it. Page buttons turn
//! pages. Touch: tap.
//!
//! A choice of many opens the settings plugin's list; without it, the
//! card steps through the options.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, events, fonts};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::semantics::Role;
use paperpaw_sdk::setup::SetupPage;
use paperpaw_sdk::ui::choice::Picked;
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::{
    Canvas, Color, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, Point, Rect,
    RefreshMode, Screen, SettingKind, SettingSpec, SettingValue,
};

use crate::mark_done;

/// Whether to skip the setup, asked by *Back* on the first page.
const SKIP_CHOSEN: Event<Picked> = Event::new("setup.skip-chosen");
/// Lines of the explanation at most; at the end, with no card, more.
const HELP_LINES: usize = 6;
const END_LINES: usize = 12;

/// What can be chosen on a page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    /// The answer's card.
    Card,
    Next,
    /// *Skip* on the first page, *Back* on the others.
    Other,
}

/// Where things are on a page.
#[derive(Debug, Default, Clone, Copy)]
struct Layout {
    dots: Rect,
    title: Rect,
    help: Rect,
    card: Option<Rect>,
    /// − and + of a number, in the card.
    stepper: Option<(Rect, Rect)>,
    /// The two options of a choice of two, side by side in the card.
    segments: Option<(Rect, Rect)>,
    next: Rect,
    other: Rect,
    /// Laid out close, where the page did not fit with its roomy margins
    /// (the largest text on a small screen): *Skip* says it short.
    tight: bool,
}

pub(crate) struct Guide {
    /// The steps asked on this device: as they were when it opened, and
    /// anew where a step's setting shows only while another has a value.
    steps: Vec<Id>,
    /// The page shown; one past the last step is the end.
    page: usize,
    focus: Target,
    /// Whether the focus shows (once a button was used; at once without
    /// touch).
    shown: bool,
    /// The number being changed with the buttons, as on the settings
    /// pages.
    edit: ui::NumberEdit,
    style: UiStyle,
    layout: Layout,
}

/// The height of a line of text at `size`.
fn line(env: &Env<'_>, style: &UiStyle, size: f32) -> u16 {
    style
        .font(env)
        .map_or(size * 1.3, |f| f.line_metrics(size).line_height) as u16
}

/// The font a sample of the reading text is in: the reader's (the UI
/// font's sizes may stop short of those the reader has).
fn sample_font<'a>(env: &Env<'a>, style: &UiStyle) -> Option<paperpaw_sdk::FontHandle<'a>> {
    env.fonts()
        .role(fonts::READING.as_str())
        .or_else(|| style.font(env))
}

/// The height of the whole sample `text` at `size`, wrapped to `width`.
fn sample_height(env: &Env<'_>, style: &UiStyle, size: f32, width: u16, text: &str) -> u16 {
    let Some(font) = sample_font(env, style) else {
        return (size * 1.3) as u16 * 2;
    };
    let lines = font.wrap(size, text, f32::from(width)).len().max(1);
    font.line_metrics(size).line_height as u16 * lines as u16
}

/// Draws the sample `text` at `size` in the reading font in `area`,
/// wrapped: the lines that fit, at least one, the last ending in "…"
/// where the rest does not fit (at large sizes).
fn draw_sample(ui: &mut ui::Ui<'_>, area: Rect, size: f32, text: &str) {
    let (env, style) = (ui.env(), ui.style());
    let Some(font) = sample_font(env, style) else {
        return;
    };
    ui.canvas().describe(Role::Text, text, area);
    let height = font.line_metrics(size).line_height.max(1.0);
    let fit = ((f32::from(area.height) / height) as usize).max(1);
    let width = f32::from(area.width);
    let lines = font.wrap(size, text, width);
    let cut = lines.len() > fit;
    let mut top = area.y;
    for (i, range) in lines.into_iter().take(fit).enumerate() {
        let row = Rect::new(area.x, top, area.width, height as u16);
        let mut line = String::from(text[range].trim_end());
        // The last line shown, with more after it: words go until "…"
        // fits after them.
        if cut && i + 1 == fit {
            while font.measure(size, &alloc::format!("{line}…")) > width {
                match line.rfind(' ') {
                    Some(end) => line.truncate(end),
                    None => break,
                }
            }
            line.push('…');
        }
        let x = f32::from(area.x);
        ui::Label::new(&line)
            .px(size)
            .color(Color::BLACK)
            .font(font)
            .draw_from(ui, x, row);
        top += height as u16;
    }
}

/// Draws `text` left-aligned in `area`, wrapped, at most `max` lines.
#[allow(
    clippy::too_many_arguments,
    reason = "its inputs are separate things; a struct of them would serve this call alone"
)]
fn paragraph(ui: &mut ui::Ui<'_>, area: Rect, size: f32, text: &str, max: usize, color: Color) {
    let Some(font) = ui.font() else {
        return;
    };
    ui.canvas().describe(Role::Text, text, area);
    let height = font.line_metrics(size).line_height;
    let mut top = area.y;
    for range in font
        .wrap(size, text, f32::from(area.width))
        .into_iter()
        .take(max)
    {
        let row = Rect::new(area.x, top, area.width, height as u16);
        ui::Label::new(&text[range])
            .px(size)
            .color(color)
            .draw_from(ui, f32::from(area.x), row);
        top += height as u16;
    }
}

/// The steps asked on this device now: not those of a setting shown
/// only while another has another value.
fn steps(env: &Env<'_>) -> Vec<Id> {
    env.setup()
        .applying(env)
        .into_iter()
        .filter(|s| match &s.page {
            paperpaw_sdk::setup::SetupPage::Setting(id) => env
                .settings()
                .spec(id.as_str())
                .is_none_or(|spec| env.settings().shows(spec)),
            _ => true,
        })
        .map(|s| s.id.clone())
        .collect()
}

impl Guide {
    pub(crate) fn new(env: &Env<'_>) -> Self {
        let mut guide = Self {
            steps: steps(env),
            page: 0,
            focus: Target::Next,
            shown: ui::focus_shown(env),
            edit: ui::NumberEdit::default(),
            style: UiStyle::new(env),
            layout: Layout::default(),
        };
        guide.layout = guide.layout(env);
        guide
    }

    /// The step of the page shown; `None` at the end.
    fn step<'a>(&self, env: &'a Env<'_>) -> Option<&'a paperpaw_sdk::setup::SetupStepSpec> {
        let id = self.steps.get(self.page)?;
        env.setup().applying(env).into_iter().find(|s| s.id == *id)
    }

    /// The setting the page asks for, if it is one.
    fn setting<'a>(&self, env: &'a Env<'_>) -> Option<&'a SettingSpec> {
        let SetupPage::Setting(id) = &self.step(env)?.page else {
            return None;
        };
        env.settings().specs().find(|s| s.id == *id)
    }

    /// The values of a choice of two, and their names.
    fn two(&self, env: &Env<'_>) -> Option<[(String, String); 2]> {
        let spec = self.setting(env)?;
        if !ui::two_options(env, spec) {
            return None;
        }
        let values: Vec<String> = match &spec.kind {
            SettingKind::Choice(options) => options.iter().map(|o| o.to_string()).collect(),
            SettingKind::Language => env
                .i18n()
                .languages()
                .into_iter()
                .map(|(tag, _)| tag.to_string())
                .collect(),
            _ => return None,
        };
        let named = |v: &String| (v.clone(), ui::option_name(env, spec, v));
        Some([named(values.first()?), named(values.get(1)?)])
    }

    /// Chooses option `index` of a choice of two.
    fn pick(&mut self, index: usize, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if let (Some(spec), Some(options)) = (self.setting(&env), self.two(&env)) {
            let value = SettingValue::Text(options[index].0.clone().into());
            cx.command(Command::SetSetting(spec.id.clone(), value));
            self.redraw(cx, RefreshMode::Fast);
        }
    }

    fn help_lines(&self) -> usize {
        if self.page >= self.steps.len() {
            END_LINES
        } else {
            HELP_LINES
        }
    }

    fn is_number(&self, env: &Env<'_>) -> bool {
        self.setting(env)
            .is_some_and(|s| matches!(s.kind, SettingKind::Int { .. }))
    }

    /// The size of the sample of the text, where the step's number is a
    /// size of the text (points): as the setting is now.
    fn sample_size(&self, env: &Env<'_>) -> Option<f32> {
        let spec = self.setting(env)?;
        (spec.extra().unit.as_deref() == Some("core.unit.pt")).then(|| {
            env.metrics()
                .pt(env.settings().int(spec.id.as_str()) as f32)
        })
    }

    /// What can be chosen on the page, top to bottom.
    fn targets(&self) -> Vec<Target> {
        let mut targets = Vec::new();
        if self.layout.card.is_some() {
            targets.push(Target::Card);
        }
        targets.extend([Target::Next, Target::Other]);
        targets
    }

    /// The question and its explanation.
    fn texts(&self, env: &Env<'_>) -> (String, String) {
        let Some(step) = self.step(env) else {
            return (
                env.i18n().tr("setup.end.title").to_string(),
                env.i18n().tr("setup.end.help").to_string(),
            );
        };
        let setting = self.setting(env);
        let title = if step.title.is_empty() {
            setting.map_or("", |s| env.i18n().tr(&s.title))
        } else {
            env.i18n().tr(&step.title)
        };
        let help = if step.help.is_empty() {
            setting
                .and_then(|s| s.description.as_deref())
                .map_or("", |d| env.i18n().tr(d))
        } else {
            env.i18n().tr(&step.help)
        };
        (title.to_string(), help.to_string())
    }

    /// The card's height, `width` wide, for buttons `button` high: a
    /// number's − value + and a sample below it (the whole text at that
    /// size, in the reading font, as far as there is room: then it ends
    /// in "…"), else a row; none on the last page.
    fn card_height(&self, env: &Env<'_>, width: u16, button: u16) -> Option<u16> {
        let style = &self.style;
        let gap = style.gap();
        self.step(env)?;
        if !self.is_number(env) {
            return Some(button + gap * 2);
        }
        let below = match self.sample_size(env) {
            Some(size) => {
                let inner = width.saturating_sub(2 * style.margin());
                sample_height(env, style, size, inner, env.i18n().tr("setup.sample"))
            }
            None => line(env, style, style.text()) * 2,
        };
        Some(button + gap * 2 + below + gap * 2)
    }

    /// Where things are on the page: roomy, or close where it would not
    /// fit so (the largest text on a small screen).
    fn layout(&self, env: &Env<'_>) -> Layout {
        match self.layout_with(env, false) {
            (roomy, true) => roomy,
            _ => self.layout_with(env, true).0,
        }
    }

    /// The page laid out roomy, or `tight`: the margins menus have, less
    /// room between its parts, buttons as high as elsewhere, side by
    /// side where one above the other does not fit. And whether all of
    /// it fits.
    fn layout_with(&self, env: &Env<'_>, tight: bool) -> (Layout, bool) {
        let style = &self.style;
        let display = env.device().display;
        let (w, h) = (display.width, display.height);
        // A little more room than menus have, but not twice theirs, which
        // left wide margins too little width.
        let gap = style.gap();
        let margin = if tight {
            style.margin()
        } else {
            style.margin() + gap
        };
        let (after_dots, after_help) = if tight { (2, 1) } else { (4, 3) };
        let width = w.saturating_sub(2 * margin);
        // The text flows: the question, its explanation right below, the
        // card below that; the buttons at the bottom.
        let lines = |size: f32, text: &str, max: usize| -> u16 {
            let count = style.font(env).map_or(1, |f| {
                f.wrap(size, text, f32::from(width)).len().clamp(1, max)
            });
            line(env, style, size) * count as u16
        };
        let (title_text, help_text) = self.texts(env);
        let mut y = margin;
        let dots = Rect::new(margin, y, width, gap);
        y += dots.height + gap * after_dots;
        let title = Rect::new(margin, y, width, lines(style.title() * 1.3, &title_text, 2));
        y += title.height + gap;
        let help = Rect::new(
            margin,
            y,
            width,
            lines(style.small(), &help_text, self.help_lines()),
        );
        y += help.height + gap * after_help;
        let wanted = |button: u16| self.card_height(env, width, button);
        // The buttons a row high, one above the other at the bottom; close,
        // as high as buttons are elsewhere, and side by side (*Next* on the
        // right) where one above the other does not fit.
        let fits = |button: u16, beside: bool| {
            let rows = if beside { 1 } else { 2 };
            let top = h.saturating_sub(margin + rows * button + (rows - 1) * gap * 2);
            y + wanted(button).unwrap_or(0) + gap <= top
        };
        let small = ui::button_height(style, Rect::new(0, 0, width, style.control_row()));
        let (button, beside) = match tight {
            false => (style.row(), false),
            true => (small, !fits(small, false)),
        };
        let fit = fits(button, beside);
        let bottom = h.saturating_sub(margin + button);
        let (next, other) = if beside {
            let half = width.saturating_sub(gap) / 2;
            (
                Rect::new(margin + half + gap, bottom, width - half - gap, button),
                Rect::new(margin, bottom, half, button),
            )
        } else {
            let above = bottom.saturating_sub(gap * 2 + button);
            (
                Rect::new(margin, above, width, button),
                Rect::new(margin, bottom, width, button),
            )
        };
        let wanted = wanted(button);
        let (card, stepper, segments) = match wanted {
            None => (None, None, None),
            Some(height) => {
                let number = self.is_number(env);
                let card = Rect::new(margin, y, width, height.min(next.y.saturating_sub(y + gap)));
                let stepper = number.then(|| {
                    let size = button;
                    let top = card.y + gap;
                    let inset = style.margin();
                    (
                        Rect::new(card.x + inset, top, size, size),
                        Rect::new(card.x + card.width - inset - size, top, size, size),
                    )
                });
                let segments = self.two(env).map(|_| {
                    // Close, the two use the card's whole width.
                    let inset = if tight { 0 } else { style.margin() / 2 };
                    // Wide margins and a short card leave the buttons
                    // their height.
                    let above = inset.min(gap).min(card.height / 6);
                    let row = Rect::new(
                        card.x + inset,
                        card.y + above,
                        card.width.saturating_sub(2 * inset),
                        card.height.saturating_sub(2 * above),
                    );
                    let cells = ui::columns(row, 2, inset.max(gap / 2));
                    (cells[0], cells[1])
                });
                (Some(card), stepper, segments)
            }
        };
        let layout = Layout {
            dots,
            title,
            help,
            card,
            stepper,
            segments,
            next,
            other,
            tight,
        };
        (layout, fit)
    }

    fn redraw(&mut self, cx: &mut Cx<'_, '_>, mode: RefreshMode) {
        let env = *cx.env();
        self.layout = self.layout(&env);
        cx.request_redraw();
        cx.request_refresh(mode);
    }

    /// Turns to page `page` (or ends the guide past its last).
    fn turn(&mut self, page: isize, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if page < 0 {
            return;
        }
        let page = page as usize;
        self.edit.leave(&env, cx);
        if page > self.steps.len() {
            cx.command(Command::Back);
            // What tells that it was not kept shows over the screen below.
            if let Some(notice) = mark_done(&env) {
                cx.command(notice);
            }
            return;
        }
        self.page = page;
        self.focus = Target::Next;
        self.redraw(cx, RefreshMode::Partial);
    }

    fn skip(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        cx.command(Command::Back);
        if let Some(notice) = mark_done(&env) {
            cx.command(notice);
        }
    }

    /// Asks whether to skip the setup: going on comes first.
    fn ask_skip(&self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        cx.command(
            paperpaw_sdk::ask::choice(env.i18n().tr("setup.skip-question"), &SKIP_CHOSEN)
                .options("setup.option.", &["stay", "skip"])
                .command(),
        );
    }

    /// Changes the number by `step`.
    fn change(&mut self, step: i64, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if let Some(spec) = self.setting(&env)
            && let Some(value) = ui::next_value(&env, spec, step)
        {
            cx.command(Command::SetSetting(spec.id.clone(), value));
            self.redraw(cx, RefreshMode::Fast);
        }
    }

    /// Chooses what `target` stands for.
    fn choose(&mut self, target: Target, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        match target {
            Target::Next => self.turn(self.page as isize + 1, cx),
            Target::Other if self.page == 0 => self.skip(cx),
            Target::Other => self.turn(self.page as isize - 1, cx),
            Target::Card => {
                let Some(step) = self.step(&env) else {
                    return;
                };
                if let SetupPage::Screen(screen) = &step.page {
                    cx.command(Command::OpenScreen(screen.clone(), Params::new()));
                    return;
                }
                let Some(spec) = self.setting(&env) else {
                    return;
                };
                // Its editor, the keyboard, the list of its values; else
                // the next value; a number is changed with the buttons as
                // on the settings pages (by touch, its − and +).
                match ui::setting_opens(&env, spec) {
                    Some(command) => cx.command(command),
                    None if !matches!(spec.kind, SettingKind::Int { .. }) => self.change(1, cx),
                    None if self.shown => {
                        let spec = spec.clone();
                        self.edit.start(&env, &spec, cx);
                        self.redraw(cx, RefreshMode::Fast);
                    }
                    None => {}
                }
            }
        }
    }

    fn move_focus(&mut self, delta: isize, cx: &mut Cx<'_, '_>) {
        let before = self.focus;
        if !self.shown {
            self.shown = true;
        } else {
            let targets = self.targets();
            let at = targets.iter().position(|t| *t == self.focus).unwrap_or(0);
            self.focus = targets[ui::step_focus(at, delta, targets.len())];
        }
        // What the focus left and came to.
        ui::ListChange::focus(self.area_of(before), self.area_of(self.focus)).request(cx);
    }

    /// What a focus on `target` draws over: the answer's card, or a
    /// button and its frame.
    fn area_of(&self, target: Target) -> Rect {
        let l = &self.layout;
        match target {
            Target::Card => l.card.unwrap_or(l.next),
            Target::Next => ui::button_reach(&self.style, l.next),
            Target::Other => ui::button_reach(&self.style, l.other),
        }
    }

    fn draw_dots(&self, canvas: &mut Canvas<'_>) {
        let l = &self.layout;
        let count = self.steps.len() + 1;
        let r = (i32::from(l.dots.height) / 2).max(3);
        let gap = r * 4;
        let mut x =
            i32::from(l.dots.x) + i32::from(l.dots.width) / 2 - (count as i32 - 1) * gap / 2;
        let y = i32::from(l.dots.y) + r;
        for i in 0..count {
            let d = (r * 2) as u32;
            if i <= self.page {
                canvas.fill_round_rect(x - r, y - r, d, d, r as u32, Color::BLACK);
            } else {
                canvas.stroke_round_rect(x - r, y - r, d, d, 2, r as u32, Color::BLACK);
            }
            x += gap;
        }
    }

    fn draw_card(&self, ui: &mut ui::Ui<'_>, card: Rect) {
        let env = ui.env();
        let style = &self.style;
        let focused = self.shown && self.focus == Target::Card;
        let thickness = if focused {
            u32::from(style.line()) * 3
        } else {
            u32::from(style.line()).max(1)
        };
        // Two options side by side need no frame of their own, but for
        // the focus.
        if focused || self.layout.segments.is_none() {
            ui::stroke_round(ui.canvas(), style, card, thickness, Color::BLACK);
        }
        let inner = Rect::new(
            card.x + style.margin(),
            card.y,
            card.width.saturating_sub(2 * style.margin()),
            style.row(),
        );
        let Some(font) = ui.font() else {
            return;
        };
        let (label, more) = match (self.step(env).map(|s| &s.page), self.setting(env)) {
            // What its plugin says of it now (e.g. "Connected"), else
            // that it opens.
            (Some(SetupPage::Screen(_)), _) => {
                let said = self
                    .step(env)
                    .and_then(|step| env.menus().state(env, step.id.as_str()).badge);
                let label = said.map_or_else(
                    || env.i18n().tr("setup.open-page").to_string(),
                    alloc::borrow::Cow::into_owned,
                );
                (label, true)
            }
            (_, Some(spec)) => (ui::setting_value(env, spec), ui::opens_screen(env, spec)),
            _ => (String::new(), false),
        };
        ui.canvas()
            .describe(Role::Button, &label, card)
            .focused(focused);
        if let (Some(options), Some((left, right)), Some(spec)) =
            (self.two(env), self.layout.segments, self.setting(env))
        {
            let current = ui::setting_value(env, spec);
            for ((_, name), rect) in options.iter().zip([left, right]) {
                ui.button(rect, name, *name == current, false);
            }
            return;
        }
        if let Some(spec) = self.setting(env) {
            match spec.kind {
                SettingKind::Bool => {
                    let on = env.settings().bool(spec.id.as_str());
                    ui.switch(inner, on, false);
                    return;
                }
                SettingKind::Int { .. } => {
                    let Some((minus, plus)) = self.layout.stepper else {
                        return;
                    };
                    // Filled while the buttons change it, as on the
                    // settings pages.
                    let editing = self.edit.active();
                    ui.button(minus, "−", editing, false);
                    ui.button(plus, "+", editing, false);
                    // The value large between them.
                    let between = Rect::new(
                        minus.x + minus.width,
                        minus.y,
                        plus.x.saturating_sub(minus.x + minus.width),
                        minus.height,
                    );
                    let x = f32::from(between.x)
                        + (f32::from(between.width) - font.measure(style.title(), &label)) / 2.0;
                    ui::Label::new(&label)
                        .px(style.title())
                        .color(Color::BLACK)
                        .draw_from(ui, x, between);
                    // A sample of the text at that size, where it is one.
                    if let Some(size) = self.sample_size(env) {
                        let top = minus.y + minus.height + style.gap();
                        let sample = Rect::new(
                            inner.x,
                            top,
                            inner.width,
                            (card.y + card.height).saturating_sub(top + style.gap()),
                        );
                        let mut clipped = ui.clipped(sample);
                        draw_sample(&mut clipped, sample, size, env.i18n().tr("setup.sample"));
                    }
                    return;
                }
                _ => {}
            }
        }
        let arrow = if more { " ›" } else { "" };
        let label = alloc::format!("{label}{arrow}");
        draw_card_label(ui, (card, inner), &label);
    }

    /// The target at `point`, if any.
    fn target_at(&self, point: Point) -> Option<Target> {
        let l = &self.layout;
        if l.next.contains(point) {
            Some(Target::Next)
        } else if l.other.contains(point) {
            Some(Target::Other)
        } else if l.card.is_some_and(|c| c.contains(point)) {
            Some(Target::Card)
        } else {
            None
        }
    }
}

impl Screen for Guide {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        canvas.clear(Color::WHITE);
        // As the page is now: the sample's card follows its size as it
        // is changed (and previewed).
        self.layout = self.layout(env);
        let style = self.style;
        let mut ui = ui::Ui::new(canvas, env, &style);
        let l = self.layout;
        self.draw_dots(ui.canvas());
        let (title, help) = self.texts(env);
        paragraph(
            &mut ui,
            l.title,
            style.title() * 1.3,
            &title,
            2,
            Color::BLACK,
        );
        paragraph(
            &mut ui,
            l.help,
            style.small(),
            &help,
            self.help_lines(),
            style.faint(ui::Ink::NOTE),
        );
        if let Some(card) = l.card {
            self.draw_card(&mut ui, card);
        }
        let end = self.page >= self.steps.len();
        let next = env.i18n().tr(if end { "setup.done" } else { "setup.next" });
        let other = env.i18n().tr(if self.page == 0 && l.tight {
            "setup.skip-short"
        } else if self.page == 0 {
            "setup.skip"
        } else {
            "setup.back"
        });
        let focus = |t| self.shown && self.focus == t;
        ui.button(l.next, next, true, focus(Target::Next));
        ui.button(l.other, other, false, focus(Target::Other));
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        // Back from a list or another screen: what was chosen shows.
        self.redraw(cx, RefreshMode::Partial);
    }

    fn hint(&self, action: &Id, env: &Env<'_>) -> paperpaw_sdk::Hint {
        if self.edit.active() {
            return self.edit.hint(action, env);
        }
        if *action == actions::BACK && self.page == 0 {
            paperpaw_sdk::Hint::Text(env.i18n().tr("setup.skip-short").to_string())
        } else {
            paperpaw_sdk::Hint::Default
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let env = *cx.env();
        let a = action;
        // A number being changed: as on the settings pages, but for the
        // page buttons, which keep it and turn the page as ever.
        if self.edit.active() {
            let spec = self.setting(&env).cloned();
            if *a == actions::NEXT_PAGE || *a == actions::PREV_PAGE {
                self.edit.finish(&env, spec.as_ref(), true, cx);
            } else {
                let handled = self.edit.on_action(&env, spec.as_ref(), a, cx).handled();
                self.redraw(cx, RefreshMode::Fast);
                return handled;
            }
        }
        if *a == actions::BACK {
            if self.page == 0 {
                self.ask_skip(cx);
            } else {
                self.turn(self.page as isize - 1, cx);
            }
        } else if *a == actions::NEXT_PAGE {
            self.turn(self.page as isize + 1, cx);
        } else if *a == actions::PREV_PAGE {
            self.turn(self.page as isize - 1, cx);
        } else if [actions::UP, actions::LEFT, actions::PREVIOUS].contains(a) {
            self.move_focus(-1, cx);
        } else if [actions::DOWN, actions::RIGHT, actions::NEXT].contains(a) {
            self.move_focus(1, cx);
        } else if *a == actions::CONFIRM {
            if self.shown {
                self.choose(self.focus, cx);
            } else {
                self.shown = true;
                self.redraw(cx, RefreshMode::Fast);
            }
        } else {
            return Handled::No;
        }
        Handled::Yes
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        // A step that shows only while an earlier one has a value comes
        // or goes with it; the page shown stays.
        if let Some(changed) = events::SETTING_CHANGED.read(event, params) {
            let env = *cx.env();
            // The page's own setting, changed elsewhere too: shown as it is
            // now (the sample at its size).
            if self.setting(&env).is_some_and(|spec| spec.id == changed.id) {
                self.layout = self.layout(&env);
                cx.request_redraw();
                cx.request_refresh(RefreshMode::Fast);
            }
            if env.settings().governs(changed.id.as_str()) {
                let shown = self.steps.get(self.page).cloned();
                self.steps = steps(&env);
                self.page = shown
                    .and_then(|id| self.steps.iter().position(|s| *s == id))
                    .unwrap_or(self.page.min(self.steps.len()));
                self.layout = self.layout(&env);
                cx.request_redraw();
            }
            return;
        }
        // A page that opens a screen shows what its plugin says of it
        // (e.g. connected): told again as the network comes or goes.
        if event == events::WIFI_CHANGED.id() {
            let env = *cx.env();
            if matches!(self.step(&env).map(|s| &s.page), Some(SetupPage::Screen(_))) {
                cx.request_redraw();
                cx.request_refresh(RefreshMode::Fast);
            }
            return;
        }
        if SKIP_CHOSEN
            .read(event, params)
            .is_some_and(|p| p.option == "skip")
        {
            self.skip(cx);
        }
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        self.shown = false;
        let env = *cx.env();
        self.edit.leave(&env, cx);
        if let Some((minus, plus)) = self.layout.stepper {
            if minus.contains(gesture.start) {
                self.change(-1, cx);
                return Handled::Yes;
            }
            if plus.contains(gesture.start) {
                self.change(1, cx);
                return Handled::Yes;
            }
        }
        if let Some((left, right)) = self.layout.segments {
            for (i, rect) in [left, right].into_iter().enumerate() {
                if rect.contains(gesture.start) {
                    self.pick(i, cx);
                    return Handled::Yes;
                }
            }
        }
        match self.target_at(gesture.start) {
            Some(target) => {
                self.focus = target;
                self.choose(target, cx);
                Handled::Yes
            }
            None => Handled::No,
        }
    }
}

/// A card's `label` in its `inner` room: smaller where it does not fit,
/// down to the small size, then on two lines in it; only then shortened.
fn draw_card_label(ui: &mut ui::Ui<'_>, (card, inner): (Rect, Rect), label: &str) {
    let style = ui.style();
    let Some(font) = ui.font() else {
        return;
    };
    let width = f32::from(inner.width);
    let row = Rect::new(inner.x, card.y, inner.width, card.height);
    let (size, text) =
        paperpaw_sdk::widgets::fit_line(&font, style.title(), style.small(), label, width);
    let lines = font.wrap(style.small(), label, width);
    let line = font.line_metrics(style.small()).line_height as u16;
    if text != label && lines.len() == 2 && 2 * line <= card.height {
        let top = card.y + (card.height - 2 * line) / 2;
        for (i, range) in lines.into_iter().enumerate() {
            let at = Rect::new(inner.x, top + i as u16 * line, inner.width, line);
            let part = label[range].trim();
            let x = f32::from(inner.x);
            ui::Label::new(part)
                .px(style.small())
                .color(Color::BLACK)
                .draw_from(ui, x, at);
        }
        return;
    }
    let x = f32::from(inner.x);
    ui::Label::new(&text)
        .px(size)
        .color(Color::BLACK)
        .draw_from(ui, x, row);
}
