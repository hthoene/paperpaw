//! Text of the interface, one builder for all of it: [`Label`].

use alloc::borrow::Cow;
use alloc::format;

use paperpaw_graphics::{Canvas, Role};
use paperpaw_platform::{Color, Rect};

use crate::env::Env;
use crate::fonts::FontHandle;
use crate::widgets::Align;

use super::{Ink, TextRole, Ui, UiStyle, fit};

/// A piece of the interface's text, set as the builder says: its size (a
/// [`TextRole`] of the style, or pixels), its ink, where it sits in its
/// area and on how many lines. One line by default, shortened with "…"
/// where it does not fit; wrapped on more where [`Label::lines`] or
/// [`Label::wrap`] allows, the last line shortened where the area holds
/// fewer than the text needs.
///
/// ```
/// use paperpaw_sdk::ui::{Ink, Label, TextRole};
/// use paperpaw_sdk::widgets::Align;
///
/// let title = Label::new("Settings").role(TextRole::TITLE).align(Align::Center);
/// let note = Label::new("Changes are kept at once.").role(TextRole::SMALL).ink(Ink::NOTE).wrap();
/// # let _ = (title, note);
/// ```
///
/// It borrows its text and is drawn without allocating, but for the
/// words of a line that is shortened.
#[derive(Clone, Copy)]
#[must_use = "a label does nothing until it is drawn or measured"]
pub struct Label<'t> {
    text: &'t str,
    /// A font of its own; the interface's otherwise.
    font: Option<FontHandle<'t>>,
    size: Size,
    min: Option<f32>,
    ink: Paint,
    align: Align,
    lines: usize,
}

/// How large: a role of the style, or pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Size {
    Role(TextRole),
    Px(f32),
}

/// What colour: an ink of the style, or a colour of one's own.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Paint {
    Ink(Ink),
    Color(Color),
}

impl<'t> Label<'t> {
    /// `text` as body text in the text's ink, at the left, on one line.
    pub const fn new(text: &'t str) -> Self {
        Self {
            text,
            font: None,
            size: Size::Role(TextRole::BODY),
            min: None,
            ink: Paint::Ink(Ink::TEXT),
            align: Align::Left,
            lines: 1,
        }
    }

    /// In `font` rather than the interface's (e.g. a book's, or a bold
    /// one).
    pub const fn font(mut self, font: FontHandle<'t>) -> Self {
        self.font = Some(font);
        self
    }

    /// As large as the style sets text of `role`.
    pub const fn role(mut self, role: TextRole) -> Self {
        self.size = Size::Role(role);
        self
    }

    /// `size` pixels large.
    pub const fn px(mut self, size: f32) -> Self {
        self.size = Size::Px(size);
        self
    }

    /// On one line too wide for its area, smaller down to `size` pixels
    /// before it is shortened.
    pub const fn min(mut self, size: f32) -> Self {
        self.min = Some(size);
        self
    }

    /// In the style's `ink`.
    pub const fn ink(mut self, ink: Ink) -> Self {
        self.ink = Paint::Ink(ink);
        self
    }

    /// In `color`, e.g. [`UiStyle::focus_ink`] on a focused row.
    pub const fn color(mut self, color: Color) -> Self {
        self.ink = Paint::Color(color);
        self
    }

    /// Where its lines sit across its area.
    pub const fn align(mut self, align: Align) -> Self {
        self.align = align;
        self
    }

    /// On up to `lines` lines (at least one), as many as its area holds.
    pub const fn lines(mut self, lines: usize) -> Self {
        self.lines = if lines == 0 { 1 } else { lines };
        self
    }

    /// On as many lines as its area holds.
    pub const fn wrap(self) -> Self {
        self.lines(usize::MAX)
    }

    /// Its size in pixels with `style` (before [`Label::min`] makes it
    /// smaller).
    pub fn size(&self, style: &UiStyle) -> f32 {
        match self.size {
            Size::Role(role) => style.size(role),
            Size::Px(px) => px,
        }
    }

    /// How wide its text is on one line.
    pub fn width(&self, env: &Env<'_>, style: &UiStyle) -> f32 {
        self.font
            .or_else(|| style.font(env))
            .map_or(0.0, |font| font.measure(self.size(style), self.text))
    }

    /// How high it is in an area `width` wide: a line, or the lines its
    /// text takes there (up to [`Label::lines`]).
    pub fn height(&self, env: &Env<'_>, style: &UiStyle, width: u16) -> u16 {
        let Some(font) = self.font.or_else(|| style.font(env)) else {
            return 0;
        };
        let size = self.size(style);
        let line = font.line_metrics(size).line_height;
        let lines = if self.lines == 1 {
            1
        } else {
            font.wrap(size, self.text, f32::from(width).max(1.0))
                .len()
                .clamp(1, self.lines)
        };
        (line * lines as f32) as u16 + 1
    }

    /// Draws it in `area`, its lines in the middle of the area's height;
    /// where its last line ends.
    pub fn draw(&self, ui: &mut Ui<'_>, area: Rect) -> f32 {
        if self.lines == 1 {
            return self.draw_from(ui, f32::from(area.x), area);
        }
        let style = ui.style();
        let Some(font) = self.font.or_else(|| ui.font()) else {
            return f32::from(area.x);
        };
        let color = self.paint(style);
        self.draw_lines(ui.canvas(), &font, style, area, color)
    }

    /// Draws it on one line from `x` (between pixels, as a line goes on
    /// after another's words) to the right of `area`, in the middle of
    /// its height; where it ends.
    pub fn draw_from(&self, ui: &mut Ui<'_>, x: f32, area: Rect) -> f32 {
        let style = ui.style();
        let Some(font) = self.font.or_else(|| ui.font()) else {
            return x;
        };
        let color = self.paint(style);
        self.draw_line(ui.canvas(), &font, style, x, area, color)
    }

    /// Its colour with `style`.
    fn paint(&self, style: &UiStyle) -> Color {
        match self.ink {
            Paint::Ink(ink) => style.ink(ink),
            Paint::Color(color) => color,
        }
    }

    /// One line from `from`, shortened where it does not fit.
    fn draw_line(
        &self,
        canvas: &mut Canvas<'_>,
        font: &FontHandle<'_>,
        style: &UiStyle,
        from: f32,
        area: Rect,
        color: Color,
    ) -> f32 {
        let room = f32::from(area.x) + f32::from(area.width) - from;
        let told = Rect::new(
            from.max(0.0) as u16,
            area.y,
            room.max(0.0) as u16,
            area.height,
        );
        let mut size = self.size(style);
        let mut width = font.measure(size, self.text);
        if let Some(min) = self.min
            && width > room
            && width > 0.0
        {
            size = (size * room / width).max(min.min(size));
            width = font.measure(size, self.text);
        }
        canvas.describe(Role::Text, self.text, told);
        // A pixel of slack for rounding: callers often measured to fit.
        // No room at all (it starts past its area): drawn whole, the
        // canvas clips it, as the parts' functions did.
        let text: Cow<'_, str> = if room > 0.0 && width > room + 1.0 {
            let short = fit(font, size, self.text, room);
            width = font.measure(size, &short);
            Cow::Owned(short)
        } else {
            Cow::Borrowed(self.text)
        };
        let x = from + self.free(room, width);
        self.put(
            canvas,
            font,
            style,
            size,
            x,
            baseline(font, size, area),
            area,
            &text,
            color,
        ) + x
    }

    /// One run of text at `x` on `baseline`: by the look's text painter,
    /// if it has one and draws it, else as usual; its width.
    #[allow(clippy::too_many_arguments, reason = "one run of text")]
    fn put(
        &self,
        canvas: &mut Canvas<'_>,
        font: &FontHandle<'_>,
        style: &UiStyle,
        size: f32,
        x: f32,
        baseline: i32,
        area: Rect,
        text: &str,
        color: Color,
    ) -> f32 {
        let role = match self.size {
            Size::Role(role) => role,
            Size::Px(_) => TextRole::BODY,
        };
        super::text::put_run(
            canvas,
            font,
            Some((style, role)),
            size,
            x,
            baseline,
            area,
            text,
            color,
        )
    }

    /// Wrapped lines, the block in the middle of the area's height, the
    /// last line shortened where the area holds fewer than it needs.
    fn draw_lines(
        &self,
        canvas: &mut Canvas<'_>,
        font: &FontHandle<'_>,
        style: &UiStyle,
        area: Rect,
        color: Color,
    ) -> f32 {
        let size = self.size(style);
        let line = font.line_metrics(size).line_height;
        let room = f32::from(area.width).max(1.0);
        let mut lines = font.wrap(size, self.text, room);
        let fits = ((f32::from(area.height) / line) as usize).clamp(1, self.lines);
        let cut = lines.len() > fits;
        lines.truncate(fits);
        let count = lines.len().max(1);
        let block = line * count as f32;
        let top = f32::from(area.y) + (f32::from(area.height) - block).max(0.0) / 2.0;
        let shown = Rect::new(area.x, top as u16, area.width, block as u16);
        let mut end = f32::from(area.x);
        for (n, range) in lines.iter().enumerate() {
            let row = Rect::new(
                area.x,
                (top + line * n as f32) as u16,
                area.width,
                line as u16,
            );
            // A line before the last outside the band drawn now (a line
            // more round it, for letters that reach past theirs): only
            // the last says where the label ends.
            if n + 1 < count && !super::shows(canvas, row.grow(line as u16)) {
                continue;
            }
            let at = baseline(font, size, row);
            if cut && n + 1 == count {
                let rest = self.text[range.start..].trim_end();
                let last = fit(font, size, &format!("{rest} …"), room);
                let x = self.x(area, font.measure(size, &last));
                end = x + self.put(canvas, font, style, size, x, at, area, &last, color);
                let told = format!("{}{last}", &self.text[..range.start]);
                canvas.describe(Role::Text, &told, shown);
                return end;
            }
            let text = &self.text[range.clone()];
            let x = self.x(area, font.measure(size, text));
            end = x + self.put(canvas, font, style, size, x, at, area, text, color);
        }
        canvas.describe(Role::Text, self.text, shown);
        end
    }

    /// Where a line `width` wide starts in `area`.
    fn x(&self, area: Rect, width: f32) -> f32 {
        f32::from(area.x) + self.free(f32::from(area.width), width)
    }

    /// How far a line `width` wide goes in from the start of `room`.
    fn free(&self, room: f32, width: f32) -> f32 {
        let free = (room - width).max(0.0);
        match self.align.resolved(false) {
            Align::Center => free / 2.0,
            Align::Right => free,
            _ => 0.0,
        }
    }
}

impl core::fmt::Debug for Label<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Label")
            .field("text", &self.text)
            .field("size", &self.size)
            .field("align", &self.align)
            .field("lines", &self.lines)
            .finish_non_exhaustive()
    }
}

/// The baseline that puts a line of `size` in the middle of `row`, as the
/// interface's labels sit.
fn baseline(font: &FontHandle<'_>, size: f32, row: Rect) -> i32 {
    let metrics = font.line_metrics(size);
    i32::from(row.y) + ((f32::from(row.height) + metrics.ascent - metrics.descent) / 2.0) as i32
}
