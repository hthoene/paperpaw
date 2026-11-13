//! The quick panel as a screen (`menu.panel`, slot `core.panel`): the controls
//! of `native_panel` read from the registry, drawn at the bottom of the screen
//! or hanging from its top, and what input does to them. Changes apply at once
//! and the panel stays open; the host tells the panel when a value changed.
use crate::native_data::Reader;
use crate::native_model::{self as model, Kind, Row, SettingInfo, SettingKind, Value};
use crate::native_panel::{
    self as panel, Control, Entry, Geometry, Layout, MAX_SEGMENTS, Measure, button_text,
};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled, Point, Rect,
    native::{
        Error, Host,
        callbacks::{DrawFrame, DrawPurpose, Input, InputGestureKind},
        commands::CommandSender,
        fonts::{Font, FontRole},
        formatting::Arg,
        settings::SettingValue,
        snapshot,
    },
};
use paperpaw_ui_kit::{Messages, Page, Paint, Style, mem::buffer, paint, sheet, text, text::copy};

/// Hand-over parameter: where the panel hangs.
pub const PARAM_MODE: &str = "mode";
/// The value of [`PARAM_MODE`] for the quick panel, hanging from the top.
pub const MODE_TOP: &str = "top";

/// A row of the menu with what the panel knows of it.
#[derive(Debug)]
struct Part {
    row: Row,
    info: Option<SettingInfo>,
    /// A choice's values and their translated labels (two to
    /// [`MAX_SEGMENTS`] of them), else empty.
    values: Vec<String>,
    labels: Vec<String>,
}

/// What opening something follows an input with: the panel stages its own
/// commands, the plugin opens screens (it holds the hand-over service).
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Handled, or not, with nothing to open.
    Handled(Handled),
    /// Open the screen `screen` for setting `key` (hand-over `id`).
    Screen { screen: String, key: String },
    /// Open `slot` with one hand-over parameter.
    Slot {
        slot: &'static str,
        param: &'static str,
        value: String,
    },
    /// Open a slot for a setting with a name of its own (a setting's editor).
    SettingSlot { slot: String, key: String },
}

/// A panel on screen.
#[derive(Debug, Default)]
pub struct PanelView {
    menu: String,
    top: bool,
    parts: Vec<Part>,
    layout: Option<Layout>,
    /// The control the buttons chose, in reading order.
    focus: Option<usize>,
    page: usize,
    touch: bool,
    /// The arrows of the pager as last drawn.
    header: Page,
    region: Rect,
    /// Made anew when it shows again.
    stale: bool,
}

/// The width of text in one font at one size.
struct FontMeasure<'a, 'cb> {
    font: &'a Font<'cb>,
    size: f32,
}

impl Measure for FontMeasure<'_, '_> {
    /// A text the font cannot measure counts as no width: it is shown cut
    /// by the drawing, never wrongly packed.
    fn width(&self, text: &str) -> f32 {
        self.font.measure_text(self.size, text).unwrap_or(0.0)
    }
}

impl PanelView {
    /// A panel for `menu`, hanging from the top when `mode` is [`MODE_TOP`].
    ///
    /// # Errors
    /// [`Error::NoMemory`] and the host's refusals.
    pub fn new(host: &Host<'_>, menu: &str, mode: &str) -> Result<Self, Error> {
        let touch = snapshot::capture_snapshot(host)?.has_touch();
        let mut view = Self {
            menu: copy(menu)?,
            top: mode == MODE_TOP,
            touch,
            ..Self::default()
        };
        view.read(host)?;
        Ok(view)
    }

    /// The menu's rows read anew; the layout is made when it is drawn.
    ///
    /// # Errors
    /// [`Error::NoMemory`] and the host's refusals; the view is unchanged.
    pub fn read(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let mut reader = Reader::open(host)?;
        let made = self.parts_from(host, &mut reader);
        let parts = reader.finish(made)?;
        self.parts = parts;
        self.layout = None;
        self.stale = false;
        Ok(())
    }

    fn parts_from(&self, host: &Host<'_>, reader: &mut Reader<'_, '_>) -> Result<Vec<Part>, Error> {
        let mut rows = reader.menu(&self.menu)?;
        panel::cap_rows(&mut rows);
        let mut messages = Messages::new()?;
        let mut parts: Vec<Part> = Vec::new();
        parts
            .try_reserve_exact(rows.len())
            .map_err(|_| Error::NoMemory)?;
        for row in rows {
            let info = if row.kind == Kind::Setting {
                reader.setting(&row.item)?
            } else {
                None
            };
            let (mut values, mut labels) = (Vec::new(), Vec::new());
            if let Some(info) = info.as_ref().filter(|i| {
                i.kind == SettingKind::Choice && (2..=MAX_SEGMENTS as u32).contains(&i.choices)
            }) {
                values = reader.choice_values(&info.key)?;
                labels
                    .try_reserve_exact(values.len())
                    .map_err(|_| Error::NoMemory)?;
                for value in &values {
                    labels.push(messages.copy(host, value)?);
                }
            }
            parts.push(Part {
                row,
                info,
                values,
                labels,
            });
        }
        Ok(parts)
    }

    /// The menu it shows.
    #[must_use]
    pub fn menu(&self) -> &str {
        &self.menu
    }

    /// Made anew when it shows again.
    pub fn mark_stale(&mut self) {
        self.stale = true;
    }

    /// It must be read anew.
    #[must_use]
    pub const fn is_stale(&self) -> bool {
        self.stale
    }

    /// Where the panel was drawn last.
    #[must_use]
    pub const fn region(&self) -> Rect {
        self.region
    }

    fn entries(&self) -> Result<Vec<Entry<'_>>, Error> {
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(self.parts.len())
            .map_err(|_| Error::NoMemory)?;
        for part in &self.parts {
            entries.push(Entry {
                row: &part.row,
                info: part.info.as_ref(),
                options: &part.labels,
            });
        }
        Ok(entries)
    }

    /// Whether control `index` can be used now (its row is not faint).
    fn enabled(&self, layout: &Layout, index: usize) -> bool {
        layout
            .controls()
            .nth(index)
            .and_then(|(c, _, _)| self.parts.get(c.row()))
            .is_some_and(|part| part.row.choosable())
    }

    /// The panel laid out for the screen and the font.
    fn lay_out(
        &self,
        font: &Font<'_>,
        style: &Style,
        width: u16,
        height: u16,
    ) -> Result<Layout, Error> {
        let g = Geometry {
            width,
            height,
            gap: style.gap,
            spacing: if self.touch { style.gap } else { style.gap / 2 },
            row: style.row,
            button: style.row.saturating_sub(style.gap),
            border: (style.line * 2).max(2),
            label: 0,
            pager: style.row,
            margin: style.margin,
            rtl: false,
            top: self.top,
        };
        let entries = self.entries()?;
        let label = panel::label_width(
            &entries,
            &FontMeasure {
                font,
                size: style.text,
            },
            &g,
        );
        let g = Geometry { label, ..g };
        let small = FontMeasure {
            font,
            size: style.small,
        };
        let lines = panel::lines(&entries, &small, &g)?;
        panel::layout(&lines, &g)
    }

    /// Draws the panel: the page the focus is on.
    ///
    /// # Errors
    /// Propagates allocation, font, catalog and drawing errors.
    pub fn draw(
        &mut self,
        host: &Host<'_>,
        frame: &mut DrawFrame<'_>,
        shown: bool,
    ) -> Result<Rect, Error> {
        let display = snapshot::capture_snapshot(host)?;
        let font = Font::acquire_role(host, FontRole::Ui)?;
        let style = Style::read(host, &display, &font)?;
        let layout = self.lay_out(
            &font,
            &style,
            display.display_width(),
            display.display_height(),
        )?;
        self.region = layout.region;
        self.page = self.page.min(layout.pages().saturating_sub(1));
        if self.focus.is_none() && !self.touch {
            self.focus = (0..layout.count()).find(|i| self.enabled(&layout, *i));
        }
        if !matches!(frame.purpose, DrawPurpose::Screen) {
            font.release()?;
            let region = layout.region;
            self.layout = Some(layout);
            return Ok(region);
        }
        let mut body = Vec::new();
        let mut header = Page::default();
        let made = self.prepare(host, &mut body, &mut header, &font, &style, &layout, shown);
        if made.is_ok() {
            paint::replay(frame, &font, body.iter())?;
        }
        font.release()?;
        made?;
        let region = layout.region;
        self.layout = Some(layout);
        self.header = header;
        Ok(region)
    }

    #[allow(clippy::too_many_arguments, reason = "what the panel is drawn from")]
    fn prepare(
        &self,
        host: &Host<'_>,
        body: &mut Vec<Paint>,
        header: &mut Page,
        font: &Font<'_>,
        style: &Style,
        layout: &Layout,
        shown: bool,
    ) -> Result<(), Error> {
        let r = layout.region;
        paint::push(body, Paint::Fill(r, style.paper))?;
        let border = (style.line * 2).max(2);
        let edge = if self.top {
            Rect::new(
                r.x,
                r.y.saturating_add(r.height).saturating_sub(border),
                r.width,
                border,
            )
        } else {
            Rect::new(r.x, r.y, r.width, border)
        };
        paint::push(body, Paint::Fill(edge, style.ink))?;
        let mut messages = Messages::new()?;
        if self.parts.is_empty() {
            let empty = messages.copy(host, "menu.empty")?;
            let x = f32::from(r.x)
                + (f32::from(r.width) - font.measure_text(style.text, &empty)?).max(0.0) / 2.0;
            return text::label(body, font, &empty, style.text, x, r, style.muted, Some(r));
        }
        let (on, off) = (
            messages.copy(host, "core.on")?,
            messages.copy(host, "core.off")?,
        );
        let mut index = 0;
        for row in &layout.rows {
            if row.page != self.page {
                index += row.controls.len();
                continue;
            }
            if let Some(part) = row.label.and_then(|i| self.parts.get(i)) {
                self.draw_label(
                    body,
                    font,
                    style,
                    row.area,
                    part,
                    row.controls.first().map(|(_, c)| *c),
                )?;
            }
            for (control, rect) in &row.controls {
                let focused = shown && self.focus == Some(index);
                self.draw_control(
                    body,
                    font,
                    style,
                    (host, &mut messages, (&on, &off)),
                    *control,
                    *rect,
                    focused,
                )?;
                index += 1;
            }
        }
        if let Some(bar) = layout.pager {
            let pages = layout.pages();
            let label = messages.format(
                host,
                "core.page-of",
                &[
                    Arg::Int {
                        name: "page",
                        value: self.page.saturating_add(1) as i64,
                    },
                    Arg::Int {
                        name: "pages",
                        value: pages as i64,
                    },
                ],
            )?;
            sheet::pager(
                body, header, font, style, bar, &label, self.page, pages, self.touch,
            )?;
        }
        Ok(())
    }

    /// A setting's title on the left of its row (a number's, a choice's).
    fn draw_label(
        &self,
        body: &mut Vec<Paint>,
        font: &Font<'_>,
        style: &Style,
        area: Rect,
        part: &Part,
        first: Option<Rect>,
    ) -> Result<(), Error> {
        // An opened setting draws its own title in its row.
        if matches!(part.info.as_ref().map(|i| i.kind), Some(kind) if kind != SettingKind::Int && kind != SettingKind::Choice)
            || part.info.as_ref().is_some_and(|i| i.editor_kind != 0)
        {
            return Ok(());
        }
        if part
            .info
            .as_ref()
            .is_some_and(|i| i.kind == SettingKind::Choice && part.labels.is_empty())
        {
            return Ok(());
        }
        let x = f32::from(style.margin);
        let end = first.map_or(area.x.saturating_add(area.width), |c| c.x);
        let clip = Rect::new(
            area.x,
            area.y,
            end.saturating_sub(area.x).saturating_sub(style.gap),
            area.height,
        );
        text::label(
            body,
            font,
            &part.row.title,
            style.text,
            x,
            clip,
            style.ink,
            Some(clip),
        )
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "geometry, look and state of one control"
    )]
    fn draw_control(
        &self,
        body: &mut Vec<Paint>,
        font: &Font<'_>,
        style: &Style,
        (host, messages, (on_text, off_text)): (&Host<'_>, &mut Messages, (&str, &str)),
        control: Control,
        rect: Rect,
        focused: bool,
    ) -> Result<(), Error> {
        let Some(part) = self.parts.get(control.row()) else {
            return Ok(());
        };
        let faint = !part.row.choosable();
        let thickness = u32::from(style.line).max(1);
        let ink = if faint { style.disabled } else { style.ink };
        let button = |body: &mut Vec<Paint>, label: &str, on: bool| -> Result<(), Error> {
            if on && !faint {
                paint::push(body, Paint::Focus(rect, style.corner, None, ink))?;
            } else {
                paint::push(body, Paint::Focus(rect, style.corner, Some(thickness), ink))?;
            }
            if focused {
                paint::push(
                    body,
                    Paint::Focus(rect.grow(2), style.corner, Some(thickness), style.ink),
                )?;
            }
            let color = if on && !faint { style.paper } else { ink };
            // Room as a button has: its border and a little air, not the page margin
            // (a round `−` was cut to `…`, "Refresh the screen" to "Refresh the s…").
            let air = style
                .line
                .max(1)
                .saturating_mul(4)
                .saturating_add(style.gap / 2);
            let room = f32::from(rect.width.saturating_sub(air)).max(1.0);
            let fitted = text::fit(font, style.small, label, room)?;
            let wide = font.measure_text(style.small, &fitted)?;
            let x = f32::from(rect.x) + (f32::from(rect.width) - wide).max(0.0) / 2.0;
            text::label(body, font, &fitted, style.small, x, rect, color, Some(rect))
        };
        match control {
            Control::Toggle(_) => {
                let on = match part.info.as_ref().and_then(|i| i.value.as_ref()) {
                    Some(Value::Bool(v)) => *v,
                    _ => part.row.checked(),
                };
                button(body, &button_text(&part.row)?, on)
            }
            Control::Run(_) => button(body, &button_text(&part.row)?, part.row.checked()),
            Control::Step(_, step) => {
                button(body, if step < 0 { "\u{2212}" } else { "+" }, false)?;
                if step > 0 {
                    self.draw_number(body, font, style, (host, messages), part, rect)?;
                }
                Ok(())
            }
            Control::Option(_, option) => {
                let now = match part.info.as_ref().and_then(|i| i.value.as_ref()) {
                    Some(Value::Text(v)) => v.as_str(),
                    _ => "",
                };
                let on = part.values.get(option).is_some_and(|v| v == now);
                let label = part.labels.get(option).map_or("", String::as_str);
                button(body, label, on)
            }
            Control::Open(_) => {
                let value = match &part.info {
                    Some(info) => crate::native_plugin::value_text(
                        info,
                        on_text,
                        off_text,
                        &mut |key, count| crate::native_plugin::say(messages, host, key, count),
                    )?,
                    None => String::new(),
                };
                let shown = text::concat(&[&value, " \u{203a}"])?;
                if focused {
                    paint::push(
                        body,
                        Paint::Focus(rect.inset(2, 2), style.corner, Some(thickness), style.ink),
                    )?;
                }
                let x = f32::from(style.margin);
                text::label(
                    body,
                    font,
                    &part.row.title,
                    style.text,
                    x,
                    rect,
                    ink,
                    Some(rect),
                )?;
                let wide = font.measure_text(style.small, &shown)?;
                let right =
                    f32::from(rect.x) + f32::from(rect.width) - f32::from(style.margin) - wide;
                text::label(
                    body,
                    font,
                    &shown,
                    style.small,
                    right.max(0.0),
                    rect,
                    style.muted,
                    Some(rect),
                )
            }
        }
    }

    /// A number's value between its `−` and its `+`, drawn beside the `+`.
    fn draw_number(
        &self,
        body: &mut Vec<Paint>,
        font: &Font<'_>,
        style: &Style,
        (host, messages): (&Host<'_>, &mut Messages),
        part: &Part,
        plus: Rect,
    ) -> Result<(), Error> {
        let Some(Value::Int(now)) = part.info.as_ref().and_then(|i| i.value.as_ref()) else {
            return Ok(());
        };
        let unit = part.info.as_ref().map_or("", |i| i.unit.as_str());
        let shown = crate::native_plugin::int_text(*now, unit, &mut |key, count| {
            crate::native_plugin::say(messages, host, key, count)
        })?;
        let wide = font.measure_text(style.small, &shown)?;
        let x = f32::from(plus.x) - style.gap as f32 - wide;
        let area = Rect::new(0, plus.y, plus.x, plus.height);
        text::label(
            body,
            font,
            &shown,
            style.small,
            x.max(0.0),
            area,
            style.ink,
            Some(area),
        )
    }

    /// What input does. Settings change and menus close through staged
    /// commands; what is to be opened is returned.
    ///
    /// # Errors
    /// Propagates allocation and command errors; an error drops what the
    /// callback staged.
    pub fn input(&mut self, host: &Host<'_>, input: &Input<'_>) -> Result<Outcome, Error> {
        let Some(layout) = self.layout.take() else {
            return Ok(Outcome::Handled(Handled::No));
        };
        let result = self.input_with(host, input, &layout);
        self.layout = Some(layout);
        result
    }

    fn input_with(
        &mut self,
        host: &Host<'_>,
        input: &Input<'_>,
        layout: &Layout,
    ) -> Result<Outcome, Error> {
        match *input {
            Input::Action { action, .. } => self.on_action(host, action, layout),
            Input::Gesture {
                position,
                kind: InputGestureKind::Tap,
                ..
            } => self.on_tap(host, position, layout),
            _ => Ok(Outcome::Handled(Handled::No)),
        }
    }

    fn on_tap(&mut self, host: &Host<'_>, at: Point, layout: &Layout) -> Result<Outcome, Error> {
        if !self.region.contains(at) {
            // A tap outside closes it.
            CommandSender::new(host, &mut [0; 64]).back()?;
            return Ok(Outcome::Handled(Handled::Yes));
        }
        match self.header.action(at) {
            Some("core.next-page") => {
                self.page = (self.page + 1).min(layout.pages().saturating_sub(1));
                self.focus = None;
                self.redraw(host)?;
                return Ok(Outcome::Handled(Handled::Yes));
            }
            Some("core.prev-page") => {
                self.page = self.page.saturating_sub(1);
                self.focus = None;
                self.redraw(host)?;
                return Ok(Outcome::Handled(Handled::Yes));
            }
            _ => {}
        }
        if let Some((_, control)) = layout.hit(self.page, at) {
            // The focus shown goes, as after other taps.
            if self.focus.take().is_some() {
                self.redraw(host)?;
            }
            return self.activate(host, control);
        }
        Ok(Outcome::Handled(Handled::Yes))
    }

    fn on_action(
        &mut self,
        host: &Host<'_>,
        action: &str,
        layout: &Layout,
    ) -> Result<Outcome, Error> {
        let yes = Ok(Outcome::Handled(Handled::Yes));
        match action {
            "core.confirm" => {
                let control = self
                    .focus
                    .and_then(|f| layout.controls().nth(f))
                    .map(|(c, _, _)| *c);
                match control {
                    Some(control) => return self.activate(host, control),
                    None => {
                        self.focus = Some(0);
                        self.redraw(host)?;
                    }
                }
                yes
            }
            "core.menu" | "core.back" => {
                CommandSender::new(host, &mut [0; 64]).back()?;
                yes
            }
            "core.up" | "core.left" | "core.previous" | "core.prev-page" => {
                self.move_focus(host, layout, -1)?;
                yes
            }
            "core.down" | "core.right" | "core.next" | "core.next-page" => {
                self.move_focus(host, layout, 1)?;
                yes
            }
            _ => Ok(Outcome::Handled(Handled::No)),
        }
    }

    fn move_focus(&mut self, host: &Host<'_>, layout: &Layout, delta: isize) -> Result<(), Error> {
        let count = layout.count();
        let enabled = |i: usize| self.enabled(layout, i);
        if let Some(focus) = panel::step_focus(count, self.focus, delta, enabled) {
            self.focus = Some(focus);
            self.page = layout.page_of(focus);
            self.redraw(host)?;
        }
        Ok(())
    }

    fn redraw(&self, host: &Host<'_>) -> Result<(), Error> {
        use paperpaw_sdk::native::commands::RedrawMode;
        CommandSender::new(host, &mut [0; 64]).queue_redraw(RedrawMode::Partial)
    }

    /// Does what tapping `control` does.
    fn activate(&mut self, host: &Host<'_>, control: Control) -> Result<Outcome, Error> {
        let handled = Ok(Outcome::Handled(Handled::Yes));
        let Some(part) = self.parts.get(control.row()) else {
            return handled;
        };
        // Shown faint by its plugin: nothing to do now.
        if !part.row.choosable() {
            return handled;
        }
        let mut scratch = buffer(1024)?;
        let mut commands = CommandSender::new(host, &mut scratch);
        match control {
            Control::Toggle(_) => {
                let Some(info) = &part.info else {
                    return handled;
                };
                if let Some(Value::Bool(on)) = model::stepped(info, 1) {
                    commands.set_setting(&info.key, SettingValue::Bool(on))?;
                }
            }
            Control::Step(_, step) => {
                let Some(info) = &part.info else {
                    return handled;
                };
                if let Some(Value::Int(next)) = model::stepped(info, i64::from(step)) {
                    commands.set_setting(&info.key, SettingValue::Int(next))?;
                }
            }
            Control::Option(_, option) => {
                let (Some(info), Some(value)) = (&part.info, part.values.get(option)) else {
                    return handled;
                };
                if !matches!(&info.value, Some(Value::Text(now)) if now == value) {
                    commands.set_setting(&info.key, SettingValue::Text(value))?;
                }
            }
            Control::Open(_) => return self.open_setting(part),
            Control::Run(_) if part.row.kind == Kind::Submenu => {
                commands.back()?;
                return Ok(Outcome::Slot {
                    slot: "core.menu",
                    param: model::PARAM_MENU,
                    value: copy(&part.row.item)?,
                });
            }
            Control::Run(_) => {
                commands.back()?;
                commands.run_action(&part.row.item)?;
            }
        }
        handled
    }

    /// The setting's list, editor or page.
    fn open_setting(&self, part: &Part) -> Result<Outcome, Error> {
        let Some(info) = &part.info else {
            return Ok(Outcome::Handled(Handled::Yes));
        };
        Ok(match model::act(info, false, true)? {
            model::Act::Screen(screen) => Outcome::Screen {
                screen,
                key: copy(&info.key)?,
            },
            model::Act::Slot(slot) => Outcome::SettingSlot {
                slot,
                key: copy(&info.key)?,
            },
            model::Act::Values => Outcome::Slot {
                slot: "core.choose-value",
                param: "id",
                value: copy(&info.key)?,
            },
            model::Act::Page => Outcome::Slot {
                slot: "core.settings",
                param: "focus",
                value: copy(&info.key)?,
            },
            _ => Outcome::Handled(Handled::Yes),
        })
    }
}
