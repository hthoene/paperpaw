//! The reader's bar as a screen: `menu.list` (slot `core.menu`) opened with
//! `mode=bar`, as the reader opens its menu. The rows of its menu come from the
//! registry, the layout of `native_bar` is made when it is drawn (kept for the
//! taps of the last completed frame), and what input does: the buttons move
//! the focus round the entries, *Select* chooses, the menu button and *Back*
//! close, a tap chooses an entry or, on the two lines, runs the action it was
//! opened with, a tap outside closes. Drawn in the status lines' font, as the
//! lines it opens over.
use crate::native_bar::{self as bar, Does, Handed, Hit, Key, Layout, Metrics};
use crate::native_data::Reader;
use crate::native_model::{self as model, Act, Kind, Row, SettingInfo};
use crate::native_panel::button_text;
use crate::native_plugin::{next_value, set_value};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled, Point, Rect,
    keys::InputMode,
    native::{
        Error, Host,
        callbacks::{DrawFrame, DrawPurpose, Input, InputGestureKind},
        canvas::{PartFlags, PartRole},
        commands::{CommandSender, RedrawMode},
        fonts::{Font, FontRole, IconGlyph},
        snapshot,
    },
};
use paperpaw_ui_kit::{
    Messages, Paint, Style, geometry, icons,
    mem::{buffer, push},
    paint, text,
    text::copy,
};

/// What an input asks the plugin to open: it holds the hand-over service.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// Handled, or not, with nothing to open.
    Handled(Handled),
    /// Open this menu as a list over the bar.
    Menu(String),
    /// Open what the entry of setting `key` opens.
    Setting { act: Act, key: String },
}

/// The bar on screen. As Beta 3 it keeps its texts, its entries and where
/// they were drawn.
#[derive(Debug, Default)]
pub(crate) struct BarView {
    menu: String,
    hide: String,
    title: String,
    text: String,
    page: String,
    /// Run by a tap on the two lines; empty for none.
    action: String,
    /// The entries, their titles short where the catalog has a short one.
    rows: Vec<Row>,
    /// The settings among them.
    infos: Vec<SettingInfo>,
    /// Where everything was drawn last.
    layout: Option<Layout>,
    focus: usize,
    /// The focus is shown (buttons used, or no touch screen).
    keys: bool,
    /// Read anew when it shows again.
    stale: bool,
}

/// An entry to draw.
struct Entry<'a> {
    index: usize,
    row: &'a Row,
    cell: Rect,
    glyph: Option<IconGlyph>,
}

/// The fonts the bar is drawn with: the status lines' and the icons' (where
/// one is bound).
type Fonts<'a, 'f> = (&'a Font<'f>, Option<&'a Font<'f>>);

impl BarView {
    /// The bar for what it was opened with; `touch`: the focus shows only
    /// once a button is used.
    ///
    /// # Errors
    /// [`Error::NoMemory`] and the host's refusals.
    pub(crate) fn new(host: &Host<'_>, handed: Handed, touch: bool) -> Result<Self, Error> {
        let action = if bar::valid_action(&handed.action)? {
            handed.action
        } else {
            String::new()
        };
        let mut view = Self {
            menu: handed.menu,
            hide: handed.hide,
            title: handed.title,
            text: handed.text,
            page: handed.page,
            action,
            keys: !touch,
            ..Self::default()
        };
        view.read(host)?;
        Ok(view)
    }

    /// The menu's entries read anew, the focus kept on the entry it was on
    /// (else on the first that can be chosen).
    ///
    /// # Errors
    /// [`Error::NoMemory`] and the host's refusals; the view is unchanged.
    pub(crate) fn read(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let mut reader = Reader::open(host)?;
        let made = Self::rows_from(&mut reader, &self.menu, &self.hide);
        let (mut rows, infos) = reader.finish(made)?;
        shorten(host, &mut rows)?;
        let focused = self.rows.get(self.focus).map(|r| r.item.as_str());
        let focus = focused
            .and_then(|item| rows.iter().position(|r| r.item == item))
            .unwrap_or_else(|| model::first_choosable(&rows));
        self.rows = rows;
        self.infos = infos;
        self.focus = focus;
        self.stale = false;
        Ok(())
    }

    fn rows_from(
        reader: &mut Reader<'_, '_>,
        menu: &str,
        hide: &str,
    ) -> Result<(Vec<Row>, Vec<SettingInfo>), Error> {
        let rows = reader.menu(menu)?;
        let mut infos: Vec<SettingInfo> = Vec::new();
        for row in rows.iter().filter(|r| r.kind == Kind::Setting) {
            if let Some(info) = reader.setting(&row.item)? {
                push(&mut infos, info)?;
            }
        }
        let rows = bar::entries(rows, hide, &infos)?;
        infos.retain(|i| rows.iter().any(|r| r.item == i.key));
        Ok((rows, infos))
    }

    /// Read anew when it shows again.
    pub(crate) fn mark_stale(&mut self) {
        self.stale = true;
    }

    /// It shows (again): read anew if something changed meanwhile; buttons
    /// used last show the focus.
    ///
    /// # Errors
    /// As [`BarView::read`].
    pub(crate) fn shown(
        &mut self,
        host: &Host<'_>,
        last_input: Option<InputMode>,
    ) -> Result<(), Error> {
        if last_input == Some(InputMode::Buttons) {
            self.keys = true;
        }
        if self.stale {
            self.read(host)?;
        }
        Ok(())
    }

    fn info(&self, key: &str) -> Option<&SettingInfo> {
        self.infos.iter().find(|i| i.key == key)
    }

    /// Draws the bar at the top of the screen.
    ///
    /// # Errors
    /// Propagates allocation, font and drawing errors.
    #[inline(never)]
    pub(crate) fn draw(&mut self, host: &Host<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        let display = snapshot::capture_snapshot(host)?;
        let font = status_font(host)?;
        let mut style = Style::read(host, &display, &font)?;
        let icon_font = icons::font(host)?;
        let glyphs = self.glyphs(host, icon_font.is_some())?;
        let fonts = (&font, icon_font.as_ref());
        let size = (display.display_width(), display.display_height());
        let laid = self.lay_out(fonts, &glyphs, &style, size)?;
        if matches!(frame.purpose, DrawPurpose::Screen) {
            style.describing = paint::asking(frame);
            let mut body = Vec::new();
            self.prepare(&mut body, fonts, &glyphs, &style, &laid)?;
            paint::replay_icons(frame, &font, icon_font.as_ref(), body.iter())?;
        }
        self.layout = Some(laid);
        Ok(())
    }

    /// Each entry's icon, where the host has an icon font and the icon.
    fn glyphs(&self, host: &Host<'_>, icon_font: bool) -> Result<Vec<Option<IconGlyph>>, Error> {
        let mut glyphs = Vec::new();
        glyphs
            .try_reserve_exact(self.rows.len())
            .map_err(|_| Error::NoMemory)?;
        for row in &self.rows {
            glyphs.push(if icon_font && !row.icon.is_empty() {
                icons::glyph(host, &row.icon)?
            } else {
                None
            });
        }
        Ok(glyphs)
    }

    fn lay_out(
        &self,
        (font, icon_font): Fonts<'_, '_>,
        glyphs: &[Option<IconGlyph>],
        style: &Style,
        (width, height): (u16, u16),
    ) -> Result<Layout, Error> {
        let icon = style.text * bar::ICON_SCALE;
        let mut widths = Vec::new();
        widths
            .try_reserve_exact(self.rows.len())
            .map_err(|_| Error::NoMemory)?;
        for (row, glyph) in self.rows.iter().zip(glyphs) {
            let label = measure(font, style.small, &button_text(row)?)?;
            let drawn = match (icon_font, glyph) {
                (Some(icons), Some(glyph)) => icons.measure_text(icon, glyph.as_str())?,
                _ => 0.0,
            };
            widths.push(bar::entry_width(label, drawn));
        }
        let metrics = Metrics {
            width,
            height,
            line: geometry::line_height(font, style.text)?,
            small: geometry::line_height(font, style.small)?,
            icon,
            gap: style.gap,
            margin: style.margin,
            border: style.line.saturating_mul(2),
            rtl: style.rtl,
        };
        let info = !(self.title.is_empty() && self.text.is_empty() && self.page.is_empty());
        bar::layout(&widths, info, &metrics)
    }

    fn prepare(
        &self,
        body: &mut Vec<Paint>,
        fonts: Fonts<'_, '_>,
        glyphs: &[Option<IconGlyph>],
        style: &Style,
        laid: &Layout,
    ) -> Result<(), Error> {
        let r = laid.region;
        paint::push(body, Paint::Fill(r, style.paper))?;
        let border = style.line.saturating_mul(2);
        let edge = Rect::new(
            r.x,
            r.y.saturating_add(r.height).saturating_sub(border),
            r.width,
            border,
        );
        paint::push(body, Paint::Fill(edge, style.ink))?;
        if laid.info.height > 0 {
            self.draw_info(body, fonts.0, style, laid)?;
        }
        for (index, (row, cell)) in self.rows.iter().zip(&laid.cells).enumerate() {
            let glyph = glyphs.get(index).copied().flatten();
            let entry = Entry {
                index,
                row,
                cell: *cell,
                glyph,
            };
            self.draw_entry(body, fonts, style, &entry)?;
        }
        Ok(())
    }

    /// The title and the line about where one is, over the entries: where
    /// the interface runs right to left the title and the chapter start at
    /// the right, the page at the left.
    fn draw_info(
        &self,
        body: &mut Vec<Paint>,
        font: &Font<'_>,
        style: &Style,
        laid: &Layout,
    ) -> Result<(), Error> {
        let (width, across) = (laid.region.width, laid.info);
        let line = geometry::line_px(font, style.text)?;
        let title_area = Rect::new(0, across.y, width, line);
        let text_area = Rect::new(
            0,
            across.y.saturating_add(line),
            width,
            across.height.saturating_sub(line),
        );
        let margin = f32::from(style.margin);
        let room = (f32::from(width) - 2.0 * margin).max(1.0);
        if !self.title.is_empty() {
            let title = text::fit(font, style.text, &self.title, room)?;
            let x = style.flip_x(across, margin, measure(font, style.text, &title)?);
            text::label(
                body, font, &title, style.text, x, title_area, style.ink, None,
            )?;
        }
        let (chapter, page, page_width) = bar::fit_pair(
            &self.text,
            &self.page,
            (room, margin),
            |t, w| text::fit(font, style.small, t, w),
            |t| measure(font, style.small, t),
        )?;
        if !chapter.is_empty() {
            let x = style.flip_x(across, margin, measure(font, style.small, &chapter)?);
            text::label(
                body,
                font,
                &chapter,
                style.small,
                x,
                text_area,
                style.muted,
                None,
            )?;
        }
        if !page.is_empty() {
            // The page at the end side: from the margin, mirrored the other way.
            let x = paperpaw_ui_kit::style::flip_span(!style.rtl, across, margin, page_width);
            text::label(
                body,
                font,
                &page,
                style.small,
                x,
                text_area,
                style.muted,
                None,
            )?;
        }
        Ok(())
    }

    /// An entry: its icon above its label, filled dark where it is on, a
    /// frame round it where it has the focus, faint where it does nothing now.
    fn draw_entry(
        &self,
        body: &mut Vec<Paint>,
        (font, icon_font): Fonts<'_, '_>,
        style: &Style,
        entry: &Entry<'_>,
    ) -> Result<(), Error> {
        let (row, cell) = (entry.row, entry.cell);
        let on = bar::on(row, self.info(&row.item));
        let enabled = row.choosable();
        let pad = style.gap / 2;
        let frame = Rect::new(
            cell.x.saturating_sub(pad),
            cell.y,
            cell.width.saturating_add(pad.saturating_mul(2)),
            cell.height,
        );
        let focused = self.keys && entry.index == self.focus;
        if style.describing {
            // Told as a button: what it is, which has the focus, which is on.
            let mut flags = PartFlags::NONE;
            for (set, flag) in [
                (focused, PartFlags::FOCUSED),
                (on, PartFlags::ON),
                (!enabled, PartFlags::DISABLED),
            ] {
                if set {
                    flags = flags | flag;
                }
            }
            let role = PartRole::Button;
            paint::push(
                body,
                Paint::Describe {
                    role,
                    flags,
                    rect: frame,
                },
            )?;
        }
        if on {
            let back = if enabled { style.ink } else { style.note };
            paint::push(body, Paint::Focus(frame, style.corner, None, back))?;
        }
        if focused {
            let thickness = Some(u32::from(style.line).max(1));
            paint::push(
                body,
                Paint::Focus(frame, style.corner, thickness, style.ink),
            )?;
        }
        let ink = match (on, enabled) {
            (true, _) => style.paper,
            (false, true) => style.ink,
            (false, false) => style.note,
        };
        let size = style.text * bar::ICON_SCALE;
        let icon_area = Rect::new(
            cell.x,
            cell.y.saturating_add(style.gap),
            cell.width,
            geometry::clamp_u16(size as i64),
        );
        if let (Some(icons_font), Some(glyph)) = (icon_font, entry.glyph.as_ref()) {
            icons::draw_centred(body, icons_font, glyph, size, icon_area, ink)?;
        }
        let label = button_text(row)?;
        if label.is_empty() {
            return Ok(());
        }
        let label_area = Rect::new(
            cell.x,
            icon_area.y.saturating_add(icon_area.height),
            cell.width,
            geometry::line_px(font, style.small)?,
        );
        let fitted = text::fit(font, style.small, &label, f32::from(cell.width) + 1.0)?;
        let wide = measure(font, style.small, &fitted)?;
        let x = f32::from(cell.x) + (f32::from(cell.width) - wide) / 2.0;
        text::label(body, font, &fitted, style.small, x, label_area, ink, None)
    }

    /// What input does; what is to be opened is returned.
    ///
    /// # Errors
    /// Propagates allocation and command errors; an error drops what the
    /// callback staged.
    #[inline(never)]
    pub(crate) fn input(&mut self, host: &Host<'_>, input: &Input<'_>) -> Result<Outcome, Error> {
        match *input {
            Input::Action { action, .. } => self.on_action(host, action),
            Input::Gesture {
                position,
                kind: InputGestureKind::Tap,
                ..
            } => self.on_tap(host, position),
            _ => Ok(Outcome::Handled(Handled::No)),
        }
    }

    fn on_action(&mut self, host: &Host<'_>, action: &str) -> Result<Outcome, Error> {
        let rtl = self.layout.as_ref().is_some_and(|l| l.rtl);
        match bar::key(action, self.keys, rtl) {
            Key::Move(delta) => {
                if self.keys {
                    let rows = &self.rows;
                    let enabled = |i: usize| rows.get(i).is_some_and(Row::choosable);
                    self.focus = bar::step_focus(rows.len(), self.focus, delta, enabled);
                } else {
                    // The first button press shows where the focus is.
                    self.keys = true;
                }
                CommandSender::new(host, &mut [0; 64]).queue_redraw(RedrawMode::Partial)?;
            }
            Key::Confirm => return self.run(host, self.focus),
            Key::Close => CommandSender::new(host, &mut [0; 64]).back()?,
            Key::Swallow => {}
            Key::Other => return Ok(Outcome::Handled(Handled::No)),
        }
        Ok(Outcome::Handled(Handled::Yes))
    }

    fn on_tap(&mut self, host: &Host<'_>, at: Point) -> Result<Outcome, Error> {
        let Some(layout) = self.layout.as_ref() else {
            return Ok(Outcome::Handled(Handled::No));
        };
        match layout.hit(at) {
            Hit::Outside => CommandSender::new(host, &mut [0; 64]).back()?,
            Hit::Entry(index) => {
                self.focus = index;
                return self.run(host, index);
            }
            Hit::Info if !self.action.is_empty() => {
                // The bar closes first, so the action reaches the screen below.
                let mut scratch = buffer(256)?;
                let mut commands = CommandSender::new(host, &mut scratch);
                commands.back()?;
                commands.run_action(&self.action)?;
            }
            Hit::Info | Hit::Inside => {}
        }
        Ok(Outcome::Handled(Handled::Yes))
    }

    /// Chooses entry `index`.
    fn run(&self, host: &Host<'_>, index: usize) -> Result<Outcome, Error> {
        let yes = Ok(Outcome::Handled(Handled::Yes));
        let Some(row) = self.rows.get(index) else {
            return yes;
        };
        let info = self.info(&row.item);
        match bar::does(row, info)? {
            Does::Nothing | Does::Setting(Act::Nothing | Act::StartNumber) => yes,
            Does::OpenMenu(menu) => Ok(Outcome::Menu(menu)),
            Does::Run(action) => {
                let mut scratch = buffer(256)?;
                let mut commands = CommandSender::new(host, &mut scratch);
                commands.back()?;
                commands.run_action(&action)?;
                yes
            }
            Does::Setting(Act::Step) => {
                let Some(info) = info else {
                    return yes;
                };
                if let Some(next) = next_value(host, info, 1)? {
                    let mut scratch = buffer(1024)?;
                    let mut commands = CommandSender::new(host, &mut scratch);
                    set_value(&mut commands, &info.key, &next)?;
                    commands.queue_redraw(RedrawMode::Partial)?;
                }
                yes
            }
            Does::Setting(act) => Ok(Outcome::Setting {
                act,
                key: copy(&row.item)?,
            }),
        }
    }
}

/// The status lines' font, else the interface's.
fn status_font<'h>(host: &'h Host<'h>) -> Result<Font<'h>, Error> {
    match Font::acquire_role(host, FontRole::Status) {
        Err(Error::NoMemory) => Err(Error::NoMemory),
        Err(_) => Font::acquire_role(host, FontRole::Ui),
        found => found,
    }
}

/// The width of `text`; nothing for none.
fn measure(font: &Font<'_>, size: f32, text: &str) -> Result<f32, Error> {
    if text.is_empty() {
        return Ok(0.0);
    }
    font.measure_text(size, text)
}

/// Each entry's title made short where the catalog has a short one (the
/// entry's id followed by `.short`: `reader.read-aloud.short` = "Listen"), as
/// Beta 3 labelled the bar; the descriptions, which the bar does not show, are
/// let go.
fn shorten(host: &Host<'_>, rows: &mut [Row]) -> Result<(), Error> {
    let mut messages = Messages::new()?;
    for row in rows.iter_mut() {
        row.describe = String::new();
        let key = text::concat(&[&row.item, ".short"])?;
        match messages.copy(host, &key) {
            Ok(short) if !short.is_empty() => row.title = short,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            // None, or a key the catalog cannot have: the title stays.
            _ => {}
        }
    }
    Ok(())
}
