//! The translation panel on interface 7: what it shows (`Body`), what a key or a
//! tap means to it (`Act`), and its drawing as prepared operations
//! (`Panel::draw`). The state is host-free and tested without a host; the
//! drawing needs the font and the style, so it runs in the draw callback only.
//!
//! The panel is an overlay over the lower 11 twentieths of the page it was
//! opened over (as interface 6 drew in a `region`): a bar along its top, the
//! original in one line at the left of the header and the service's name at the
//! right, then the translation (or why there is none) wrapped, scrolled by the
//! page buttons or taps; *Select* or a tap at the end closes it.
//!
//! Where it differs from interface 6, on purpose: a text wraps to at most
//! [`MAX_LINES`] lines (the last ends in "…"; 3,000 characters of any script
//! come to fewer); the offer to go online is this panel's own two buttons.
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::{
    Point, Rect,
    native::{
        Error, Host,
        callbacks::{DrawFrame, DrawPurpose},
        fonts::{Font, FontRole},
        snapshot,
    },
};
use paperpaw_ui_kit::{Messages, Paint, Style, geometry, paint, text};

/// The most lines a body wraps to.
pub const MAX_LINES: usize = 400;

/// How much of the original the header's line is made from, in bytes.
const HEADING_BYTES: usize = 512;

/// A message that stands where the translation would.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Note {
    /// `translate.loading`.
    Loading,
    /// `translate.needs-key`: a tap asks for the key.
    NeedsKey,
    /// `translate.bad-key`: a tap asks for it again.
    BadKey,
    /// `translate.quota`.
    Quota,
    /// `translate.no-network`.
    NoNetwork,
    /// `translate.unreachable`.
    Unreachable,
    /// `translate.failed`.
    Failed,
}

impl Note {
    /// The translation key of its text.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Loading => "translate.loading",
            Self::NeedsKey => "translate.needs-key",
            Self::BadKey => "translate.bad-key",
            Self::Quota => "translate.quota",
            Self::NoNetwork => "translate.no-network",
            Self::Unreachable => "translate.unreachable",
            Self::Failed => "translate.failed",
        }
    }
}

/// What stands in the panel's body.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Body {
    /// A message.
    Note(Note),
    /// The translation.
    Text(String),
    /// The offer to go online (offline power mode), once.
    Offer,
}

/// What an input means to the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Act {
    /// Not for the panel.
    Ignored,
    /// Taken, nothing to do.
    Held,
    /// The text moved: draw again.
    Moved,
    /// Close the panel.
    Close,
    /// Ask for the key on the keyboard.
    AskKey,
    /// *Go online*: the network, then the request again.
    GoOnline,
    /// *Not now*: no network, said so.
    NotNow,
    /// The focus of the offer moved: draw again.
    Focus,
}

/// The two buttons of the offer, *Go online* first.
const BUTTONS: [&str; 2] = ["core.offline.go-online", "core.offline.not-now"];

/// The translation panel of one instance.
#[derive(Debug)]
pub struct Panel {
    /// The selected text, as it came.
    original: String,
    /// It in one line (from its first bytes), to be fitted to the header.
    heading: String,
    body: Body,
    /// No key (or one that was refused): a tap asks for it.
    needs_key: bool,
    /// The offer was made: not again on this panel.
    offered: bool,
    /// The focused button of the offer.
    focus: usize,
    top: usize,
    /// The body wrapped for the last drawing.
    lines: Option<Vec<String>>,
    /// Lines the last drawing showed.
    rows: usize,
    /// Where the panel and the offer's buttons were drawn.
    region: Rect,
    buttons: [Rect; 2],
    /// The request was started (once, when first shown).
    started: bool,
}

impl Panel {
    /// A panel for `original`, showing that it is loading.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn new(original: String) -> Result<Self, Error> {
        let heading = heading_of(&original)?;
        Ok(Self {
            original,
            heading,
            body: Body::Note(Note::Loading),
            needs_key: false,
            offered: false,
            focus: 0,
            top: 0,
            lines: None,
            rows: 1,
            region: Rect::new(0, 0, 0, 0),
            buttons: [Rect::new(0, 0, 0, 0); 2],
            started: false,
        })
    }

    /// The text to translate.
    #[must_use]
    pub fn original(&self) -> &str {
        &self.original
    }

    /// What the panel shows.
    #[must_use]
    pub const fn body(&self) -> &Body {
        &self.body
    }

    /// Whether the first request was started: true once, then false.
    pub fn first_time(&mut self) -> bool {
        !core::mem::replace(&mut self.started, true)
    }

    /// Whether the offer to go online was made already.
    #[must_use]
    pub const fn offered(&self) -> bool {
        self.offered
    }

    /// Whether a tap asks for the key.
    #[must_use]
    pub const fn needs_key(&self) -> bool {
        self.needs_key
    }

    /// Shows a message in place of the body.
    pub fn say(&mut self, note: Note) {
        self.needs_key = matches!(note, Note::NeedsKey | Note::BadKey);
        self.set(Body::Note(note));
    }

    /// Shows the translation.
    pub fn show(&mut self, translation: String) {
        self.needs_key = false;
        self.set(Body::Text(translation));
    }

    /// Shows the offer to go online, the first time only; `false` (and
    /// nothing shown) where it was made already.
    pub fn offer(&mut self) -> bool {
        if self.offered {
            return false;
        }
        self.offered = true;
        self.focus = 0;
        self.needs_key = false;
        self.set(Body::Offer);
        true
    }

    fn set(&mut self, body: Body) {
        self.body = body;
        self.top = 0;
        self.lines = None;
    }

    const fn offering(&self) -> bool {
        matches!(self.body, Body::Offer)
    }

    fn can_scroll(&self, forward: bool) -> bool {
        let total = self.lines.as_ref().map_or(0, Vec::len);
        if forward {
            self.top.saturating_add(self.rows) < total
        } else {
            self.top > 0
        }
    }

    /// Moves the text a page; whether it moved.
    fn scroll(&mut self, forward: bool) -> bool {
        if !self.can_scroll(forward) {
            return false;
        }
        self.top = if forward {
            self.top.saturating_add(self.rows)
        } else {
            self.top.saturating_sub(self.rows)
        };
        true
    }

    /// What the action `action` (the host's name for a key) means.
    pub fn action(&mut self, action: &str) -> Act {
        if self.offering() {
            return match action {
                "core.confirm" => {
                    if self.focus == 0 {
                        Act::GoOnline
                    } else {
                        Act::NotNow
                    }
                }
                "core.down" | "core.next" | "core.right" | "core.next-page" => self.focused(1),
                "core.up" | "core.previous" | "core.left" | "core.prev-page" => self.focused(0),
                _ => Act::Ignored,
            };
        }
        match action {
            "core.next-page" | "core.down" | "core.right" | "core.next" => self.scrolled(true),
            "core.prev-page" | "core.up" | "core.left" | "core.previous" => self.scrolled(false),
            "core.confirm" if self.needs_key => Act::AskKey,
            "core.confirm" => Act::Close,
            _ => Act::Ignored,
        }
    }

    fn focused(&mut self, to: usize) -> Act {
        if self.focus == to {
            Act::Held
        } else {
            self.focus = to;
            Act::Focus
        }
    }

    fn scrolled(&mut self, forward: bool) -> Act {
        if self.scroll(forward) {
            Act::Moved
        } else {
            Act::Held
        }
    }

    /// What a tap at `at` means: only a tap in the panel counts.
    pub fn tap(&mut self, at: Point) -> Act {
        if !self.region.contains(at) {
            return Act::Ignored;
        }
        if self.offering() {
            return match self.buttons.iter().position(|b| b.contains(at)) {
                Some(0) => Act::GoOnline,
                Some(_) => Act::NotNow,
                None => Act::Held,
            };
        }
        if self.needs_key {
            return Act::AskKey;
        }
        // The right two thirds go on, the left third back; at the end a tap
        // forward closes.
        let forward = at.x > i32::from(self.region.width) / 3;
        if self.scroll(forward) {
            Act::Moved
        } else if forward {
            Act::Close
        } else {
            Act::Held
        }
    }

    /// Draws the panel as the overlay it is.
    ///
    /// # Errors
    /// Allocation, font and drawing errors; the panel's geometry stays that of
    /// the last frame that was whole.
    pub fn draw(&mut self, host: &Host<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        if !matches!(frame.purpose, DrawPurpose::Screen) {
            return Ok(());
        }
        let display = snapshot::capture_snapshot(host)?;
        let font = Font::acquire_role(host, FontRole::Ui)?;
        let result = self.paint(host, frame, &display, &font);
        font.release()?;
        result
    }

    fn paint(
        &mut self,
        host: &Host<'_>,
        frame: &mut DrawFrame<'_>,
        display: &snapshot::Snapshot,
        font: &Font<'_>,
    ) -> Result<(), Error> {
        let style = Style::read(host, display, font)?;
        let (width, height) = (display.display_width(), display.display_height());
        let top = height * 9 / 20;
        let region = Rect::new(0, top, width, height.saturating_sub(top));
        let mut messages = Messages::new()?;
        let mut ops: Vec<Paint> = Vec::new();
        paint::push(&mut ops, Paint::Fill(region, style.paper))?;
        let bar = (style.line * 2).max(2);
        paint::push(
            &mut ops,
            Paint::Fill(Rect::new(0, top, width, bar), style.ink),
        )?;
        // The original at the left, the service at the right.
        let header = Rect::new(0, top, width, style.row);
        let source = messages.copy(host, "translate.source")?;
        let source_width = font.measure_text(style.small, &source)?;
        let right = f32::from(width) - f32::from(style.margin) - source_width;
        text::label(
            &mut ops,
            font,
            &source,
            style.small,
            right,
            header,
            style.muted,
            None,
        )?;
        let room = right - f32::from(style.margin) * 2.0;
        let heading = text::fit(font, style.title, &self.heading, room.max(1.0))?;
        text::label(
            &mut ops,
            font,
            &heading,
            style.title,
            f32::from(style.margin),
            header,
            style.ink,
            None,
        )?;
        let below = top.saturating_add(style.row).saturating_add(style.gap / 2);
        let area = Rect::new(0, below, width, (top + region.height).saturating_sub(below));
        let wrap_width = f32::from(width.saturating_sub(2 * style.margin)).max(1.0);
        if self.offering() {
            self.draw_offer(host, font, &style, &mut messages, &mut ops, area)?;
        } else {
            self.draw_text(
                host,
                font,
                &style,
                &mut messages,
                &mut ops,
                area,
                wrap_width,
            )?;
        }
        paint::replay(frame, font, ops.iter())?;
        self.region = region;
        Ok(())
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "one prepared drawing: the host, the font, the style and where it goes"
    )]
    fn draw_text(
        &mut self,
        host: &Host<'_>,
        font: &Font<'_>,
        style: &Style,
        messages: &mut Messages,
        ops: &mut Vec<Paint>,
        area: Rect,
        wrap_width: f32,
    ) -> Result<(), Error> {
        if self.lines.is_none() {
            let wrapped = match &self.body {
                Body::Text(translation) => wrap(font, style, translation, wrap_width)?,
                Body::Note(note) => {
                    let said = messages.copy(host, note.key())?;
                    wrap(font, style, &said, wrap_width)?
                }
                Body::Offer => Vec::new(),
            };
            self.lines = Some(wrapped);
        }
        let line = geometry::line_px(font, style.text)?.max(1);
        self.rows = usize::from((area.height / line).max(1));
        let (ascent, _, _) = geometry::line_metrics(font, style.text)?;
        let shown = self.lines.iter().flatten().skip(self.top).take(self.rows);
        for (n, one) in shown.enumerate() {
            let y = area
                .y
                .saturating_add(line.saturating_mul(u16::try_from(n).unwrap_or(u16::MAX)));
            let baseline = i32::from(y) + ascent as i32;
            paint::push(
                ops,
                Paint::Text {
                    text: text::copy(one)?,
                    size: style.text,
                    origin: paperpaw_sdk::native::fonts::TextOrigin::new(
                        f32::from(style.margin),
                        baseline,
                    ),
                    color: style.ink,
                    clip: None,
                },
            )?;
        }
        Ok(())
    }

    fn draw_offer(
        &mut self,
        host: &Host<'_>,
        font: &Font<'_>,
        style: &Style,
        messages: &mut Messages,
        ops: &mut Vec<Paint>,
        area: Rect,
    ) -> Result<(), Error> {
        let said = messages.copy(host, "core.offline.text")?;
        let room = f32::from(area.width.saturating_sub(4 * style.margin)).max(1.0);
        let lines = text::wrap_with(|l| font.measure_text(style.text, l), &said, room, 6)?;
        let line = geometry::line_px(font, style.text)?.max(1);
        let mut y = area.y;
        for one in &lines {
            text::label(
                ops,
                font,
                one,
                style.text,
                f32::from(2 * style.margin),
                Rect::new(0, y, area.width, line),
                style.ink,
                None,
            )?;
            y = y.saturating_add(line);
        }
        y = y.saturating_add(style.gap);
        let button = style.row;
        for (n, key) in BUTTONS.iter().enumerate() {
            let rect = Rect::new(
                2 * style.margin,
                y,
                area.width.saturating_sub(4 * style.margin),
                button,
            );
            let focused = n == self.focus;
            let label = messages.copy(host, key)?;
            if focused {
                paint::push(ops, Paint::Focus(rect, style.corner, None, style.ink))?;
            } else {
                paint::push(
                    ops,
                    Paint::Focus(
                        rect,
                        style.corner,
                        Some(u32::from(style.line.max(1))),
                        style.ink,
                    ),
                )?;
            }
            let color = if focused { style.paper } else { style.ink };
            let inner = Rect::new(rect.x, rect.y, rect.width, rect.height);
            let x = f32::from(rect.x)
                + (f32::from(rect.width) - font.measure_text(style.text, &label)?).max(0.0) / 2.0;
            text::label(ops, font, &label, style.text, x, inner, color, None)?;
            if let Some(slot) = self.buttons.get_mut(n) {
                *slot = rect;
            }
            y = y.saturating_add(button).saturating_add(style.gap);
        }
        Ok(())
    }
}

/// The original in one line: whitespace made single, from its first
/// [`HEADING_BYTES`] bytes.
fn heading_of(original: &str) -> Result<String, Error> {
    let mut end = original.len().min(HEADING_BYTES);
    while !original.is_char_boundary(end) {
        end -= 1;
    }
    let head = original.get(..end).unwrap_or_default();
    let mut out = String::new();
    out.try_reserve_exact(head.len())
        .map_err(|_| Error::NoMemory)?;
    for word in head.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    Ok(out)
}

/// `body` wrapped to `width`, a paragraph (a line break of the text) at a
/// time, at most [`MAX_LINES`] lines; the last ends in "…" where there was more.
fn wrap(font: &Font<'_>, style: &Style, body: &str, width: f32) -> Result<Vec<String>, Error> {
    wrap_with(|line| font.measure_text(style.text, line), body, width)
}

/// As [`wrap`], with `measure` giving a text's width.
pub fn wrap_with(
    mut measure: impl FnMut(&str) -> Result<f32, Error>,
    body: &str,
    width: f32,
) -> Result<Vec<String>, Error> {
    let mut lines: Vec<String> = Vec::new();
    for paragraph in body.split('\n') {
        let room = MAX_LINES - lines.len();
        if room == 0 {
            if let Some(last) = lines.last_mut() {
                if !last.ends_with('…') {
                    last.try_reserve(3).map_err(|_| Error::NoMemory)?;
                    last.push('…');
                }
            }
            break;
        }
        let wrapped = text::wrap_with(&mut measure, paragraph, width, room)?;
        lines
            .try_reserve(wrapped.len().max(1))
            .map_err(|_| Error::NoMemory)?;
        if wrapped.is_empty() {
            // An empty paragraph is an empty line.
            lines.push(String::new());
        } else {
            lines.extend(wrapped);
        }
    }
    Ok(lines)
}

#[cfg(test)]
mod tests;
