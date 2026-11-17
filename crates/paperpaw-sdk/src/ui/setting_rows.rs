//! Settings drawn as rows: values, notes, numbers and steppers, and
//! stepping a setting's value.

#[allow(
    clippy::wildcard_imports,
    reason = "a part of the module, which shares its imports"
)]
use super::*;

// ---- Settings as rows --------------------------------------------------

/// The part of a noted row (see [`UiStyle::noted_row`]) that holds the
/// title and the value, or a stepper; the note goes below it.
pub fn title_area(style: &UiStyle, rect: Rect) -> Rect {
    let band = (style.title_band() as u16).min(rect.height);
    Rect::new(rect.x, rect.y + style.gap as u16 / 2, rect.width, band)
}

/// The part of a noted row of a number that holds its title and its
/// `−` and `+`: as [`title_area`], on touch screens as much higher as the
/// buttons grow to a fingertip ([`UiStyle::touch_row`]); the note goes
/// below it (`draw_number_note`).
pub fn number_title_area(style: &UiStyle, rect: Rect) -> Rect {
    let band = (style.touch_row(style.title_band()) as u16).min(rect.height);
    Rect::new(rect.x, rect.y + style.gap as u16 / 2, rect.width, band)
}

/// Draws a row with `title` and `value` on its first line and `note` in
/// small text below, wrapped to [`NOTE_LINES`] lines and shortened with
/// "…" if longer. Rows are always fully readable before they are tapped,
/// so a description never needs a tap that would change a setting.
/// Without a note, the row looks like [`draw_value_row`].
#[allow(
    clippy::too_many_arguments,
    reason = "its inputs are separate things; a struct of them would serve this call alone"
)]
pub(crate) fn draw_noted_row(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    rect: Rect,
    focused: bool,
    title: &str,
    value: &str,
    note: &str,
) {
    if note.is_empty() {
        return draw_value_row(canvas, env, style, rect, focused, title, value);
    }
    let band = title_area(style, rect);
    let band = match style.font(env) {
        Some(font) => {
            let high = ValueLines::new(&font, style, rect.width, title, value).height();
            title_band_of(style, rect, band, high)
        }
        None => band,
    };
    draw_value_row(canvas, env, style, band, focused, title, value);
    let below = band.bottom() as u16;
    draw_note_below(canvas, env, style, rect, below, focused, note);
}

/// The band of a noted row's title: as [`title_area`], taller where the
/// title, beside a control `beside` pixels wide, takes more lines.
pub(super) fn titled_band(
    env: &Env<'_>,
    style: &UiStyle,
    rect: Rect,
    beside: f32,
    title: &str,
) -> Rect {
    let band = title_area(style, rect);
    let Some(font) = style.font(env) else {
        return band;
    };
    let (lines, line) = title_lines(&font, style, rect.width, beside, title);
    title_band_of(style, rect, band, line * lines.len() as u16)
}

/// A noted row's title band, `band` as [`title_area`] gives it, `high`
/// where its title takes that: as much as the row holds while its note
/// keeps the room it has in a row [`UiStyle::noted_row`] high.
fn title_band_of(style: &UiStyle, rect: Rect, band: Rect, high: u16) -> Rect {
    let noted = style.noted_row() as u16;
    let note = noted.saturating_sub(title_area(style, Rect::new(0, 0, rect.width, noted)).height);
    let most = rect.height.saturating_sub(note).max(band.height);
    Rect::new(band.x, band.y, band.width, high.clamp(band.height, most))
}

/// How a row `width` wide lays out its `title` and `value` (as
/// [`draw_value_row`] draws them): side by side where both fit; the title
/// wrapped beside the whole value where the value takes no more than 60 %
/// of the row and no word of the title is too long for what is left; else
/// the title wrapped across the row and the value on a line of its own
/// below it, at the right.
pub(super) struct ValueLines {
    /// The title's lines, and how wide they are from the row's left: a
    /// margin and the width they were wrapped to.
    pub title: Vec<core::ops::Range<usize>>,
    pub title_width: u16,
    /// The value: whole, unless longer than the whole row.
    pub value: String,
    pub value_width: f32,
    /// Whether the value is on a line of its own below the title.
    pub below: bool,
    /// The size of both, and a line's height.
    pub size: f32,
    pub line: u16,
}

impl ValueLines {
    /// In the text's size.
    pub fn new(
        font: &FontHandle<'_>,
        style: &UiStyle,
        width: u16,
        title: &str,
        value: &str,
    ) -> Self {
        Self::sized(font, style, style.text, width, title, value)
    }

    /// In `size`.
    pub fn sized(
        font: &FontHandle<'_>,
        style: &UiStyle,
        size: f32,
        width: u16,
        title: &str,
        value: &str,
    ) -> Self {
        let margin = style.margin as f32;
        let inner = (f32::from(width) - 2.0 * margin).max(1.0);
        let line = font.line_metrics(size).line_height as u16;
        let value_width = font.measure(size, value);
        let gap = if value.is_empty() { 0.0 } else { margin };
        let room = inner - gap - value_width;
        let beside = |title: Vec<core::ops::Range<usize>>| Self {
            title,
            title_width: (margin + room) as u16,
            value: value.to_string(),
            value_width,
            below: false,
            size,
            line,
        };
        if font.measure(size, title) <= room + 1.0 {
            return beside(alloc::vec![0..title.len()]);
        }
        let words_fit = title
            .split_whitespace()
            .all(|word| font.measure(size, word) <= room + 1.0);
        if value_width <= (inner - gap) * 0.6 && words_fit {
            return beside(font.wrap(size, title, room.max(1.0)));
        }
        let value = fit(font, size, value, inner + 1.0);
        Self {
            title: font.wrap(size, title, inner),
            title_width: (margin + inner) as u16,
            value_width: font.measure(size, &value),
            below: !value.is_empty(),
            value,
            size,
            line,
        }
    }

    /// The height of its lines.
    pub fn height(&self) -> u16 {
        self.line * (self.title.len() + usize::from(self.below)) as u16
    }
}

/// The lines of a row's title beside a control `beside` pixels wide (a
/// value, a switch; 0 for none) in a row `width` wide: one where it fits,
/// else its words wrapped, in the text size; and a line's height.
pub(crate) fn title_lines(
    font: &FontHandle<'_>,
    style: &UiStyle,
    width: u16,
    beside: f32,
    title: &str,
) -> (Vec<core::ops::Range<usize>>, u16) {
    let margin = style.margin as f32;
    let control = if beside > 0.0 { beside + margin } else { 0.0 };
    let room = (f32::from(width) - 2.0 * margin - control).max(1.0);
    let line = font.line_metrics(style.text).line_height as u16;
    if font.measure(style.text, title) <= room + 1.0 {
        return (alloc::vec![0..title.len()], line);
    }
    (font.wrap(style.text, title, room), line)
}

/// Whether `note`, a setting's description, shows whole in a row `width`
/// wide: on the [`NOTE_LINES`] lines a row gives it. A longer one is
/// shortened with "…"; descriptions are kept short to fit (detail goes in
/// the plugin's How it works).
pub fn note_fits(env: &Env<'_>, style: &UiStyle, width: u16, note: &str) -> bool {
    let Some(font) = style.font(env) else {
        return true;
    };
    let width = f32::from(width) - 2.0 * style.margin as f32;
    font.wrap(style.small, note, width).len() <= NOTE_LINES
}

/// Draws the note of a row from `below` down to the end of `rect`: in
/// small gray text (light on a focused row), wrapped to [`NOTE_LINES`],
/// or to more in a row higher than [`UiStyle::noted_row`] by them (see
/// [`UiStyle::noted_row_for`]).
pub(crate) fn draw_note_below(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    rect: Rect,
    below: u16,
    focused: bool,
    note: &str,
) {
    let Some(font) = style.font(env) else {
        return;
    };
    let at = Rect::new(
        rect.x,
        below,
        rect.width,
        rect.bottom().saturating_sub(u32::from(below)) as u16,
    );
    let margin = style.margin as f32;
    let width = f32::from(rect.width) - 2.0 * margin;
    let color = style.focus_faint(focused, Ink::DETAIL);
    let line = style.small * NOTE_LINE_HEIGHT;
    let ascent = font.line_metrics(style.small).ascent;
    let top = f32::from(below);
    let lines = font.wrap(style.small, note, width);
    // Only as many as the row holds below its title: never past its end.
    let room = (f32::from(at.height) / line) as usize;
    let more = (f32::from(rect.height) - style.noted_row() as f32).max(0.0) / line;
    let most = (NOTE_LINES + (more + 0.5) as usize).min(room);
    // What is drawn, so that what describes the screen sees a cut.
    let mut shown = String::new();
    for (n, range) in lines.iter().take(most).enumerate() {
        let last = n + 1 == most && lines.len() > most;
        let text = if last {
            fit(&font, style.small, note[range.start..].trim_end(), width)
        } else {
            note[range.clone()].to_string()
        };
        if !shown.is_empty() {
            shown.push(' ');
        }
        shown.push_str(text.trim());
        let baseline = top + line * n as f32 + ascent;
        let x = f32::from(rect.x) + margin;
        let small = style.small;
        let x = style.flip_x(rect, x, font.measure(small, &text));
        font.draw(canvas, small, x, baseline as i32, &text, color);
    }
    let said = if lines.len() > most {
        shown.as_str()
    } else {
        note
    };
    canvas.describe(Role::Text, said, at);
}

/// Draws a row: `title` on the left, `value` right-aligned.
#[allow(
    clippy::too_many_arguments,
    reason = "its inputs are separate things; a struct of them would serve this call alone"
)]
#[expect(
    clippy::too_many_lines,
    reason = "grew by the role each label now names; split it when it is next changed"
)]
pub(crate) fn draw_value_row(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    rect: Rect,
    focused: bool,
    title: &str,
    value: &str,
) {
    let Some(font) = style.font(env) else {
        return;
    };
    // The chevron of a row that opens a screen points the other way.
    let turned = turn_chevron(style, value);
    let value = turned.as_ref();
    let color = style.focus_ink(focused);
    let margin = style.margin as f32;
    let width = f32::from(rect.width) - 2.0 * margin;
    // At least a margin between them: the value ends at the margin, as
    // the title begins at it.
    let (title_text, shown, value_width) = fit_pair(&font, style.text, title, value, width, margin);
    let title_x = f32::from(rect.x) + margin;
    let value_at = |width: f32| f32::from(rect.x) + f32::from(rect.width) - margin - width;
    let lines = (title_text != title || shown != value)
        .then(|| ValueLines::new(&font, style, rect.width, title, value))
        .filter(|lines| lines.height() > lines.line && lines.height() <= rect.height);
    let draw_lines = |canvas: &mut Canvas<'_>, lines: ValueLines| {
        let (height, line, size) = (lines.height(), lines.line, lines.size);
        let top = rect.y + (rect.height - height) / 2;
        for (i, range) in lines.title.iter().cloned().enumerate() {
            let at = Rect::new(rect.x, top + i as u16 * line, lines.title_width, line);
            draw_label_out(
                canvas,
                &font,
                style,
                rect,
                TextRole::BODY,
                size,
                title_x,
                at,
                title[range].trim(),
                color,
            );
        }
        let at = if lines.below {
            let count = lines.title.len() as u16;
            Rect::new(rect.x, top + count * line, rect.width, line)
        } else {
            rect
        };
        let x = value_at(lines.value_width);
        draw_label_out(
            canvas,
            &font,
            style,
            rect,
            TextRole::BODY,
            size,
            x,
            at,
            &lines.value,
            color,
        );
    };
    // Too long side by side: the title's words on as many lines as the
    // row holds (as [`value_row_height`] made it), the value whole beside
    // them.
    let (beside, below) = match lines {
        Some(lines) if !lines.below => return draw_lines(canvas, lines),
        lines => (shown == value, lines),
    };
    // Else, beside a whole value, the title smaller, down to the small
    // size; then the value on a line of its own below the title.
    let room = width - margin - value_width;
    if title_text != title
        && beside
        && let Some(size) = crate::widgets::largest_size(style.text, style.small, |size| {
            font.measure(size, title) <= room + 1.0
        })
    {
        draw_label_out(
            canvas,
            &font,
            style,
            rect,
            TextRole::BODY,
            style.text,
            value_at(value_width),
            rect,
            &shown,
            color,
        );
        draw_label_out(
            canvas,
            &font,
            style,
            rect,
            TextRole::BODY,
            size,
            title_x,
            rect,
            title,
            color,
        );
        return;
    }
    if let Some(lines) = below {
        return draw_lines(canvas, lines);
    }
    // Still too long: the title on two lines in the small size where the
    // row holds them, the value as it is, rather than shortened.
    let lines = font.wrap(style.small, title, room.max(1.0));
    let line = font.line_metrics(style.small).line_height as u16;
    if title_text != title && lines.len() == 2 && 2 * line <= rect.height {
        draw_label_out(
            canvas,
            &font,
            style,
            rect,
            TextRole::BODY,
            style.text,
            value_at(value_width),
            rect,
            &shown,
            color,
        );
        let top = rect.y + (rect.height - 2 * line) / 2;
        let title_width = up_to(rect, title_x + room);
        for (i, range) in lines.into_iter().enumerate() {
            let at = Rect::new(rect.x, top + i as u16 * line, title_width, line);
            draw_label_out(
                canvas,
                &font,
                style,
                rect,
                TextRole::SMALL,
                style.small,
                title_x,
                at,
                &title[range],
                color,
            );
        }
        return;
    }
    // A row too short even for that (a list's at the largest sizes, whose
    // fonts come in steps): both smaller, from the small size down to
    // seven tenths of it, on as many lines as the row holds.
    if (title_text != title || shown != value)
        && let Some(size) = crate::widgets::largest_size(style.small, style.small * 0.7, |size| {
            ValueLines::sized(&font, style, size, rect.width, title, value).height() <= rect.height
        })
    {
        let lines = ValueLines::sized(&font, style, size, rect.width, title, value);
        return draw_lines(canvas, lines);
    }
    draw_label_out(
        canvas,
        &font,
        style,
        rect,
        TextRole::BODY,
        style.text,
        value_at(value_width),
        rect,
        &shown,
        color,
    );
    draw_label_out(
        canvas,
        &font,
        style,
        rect,
        TextRole::BODY,
        style.text,
        title_x,
        rect,
        &title_text,
        color,
    );
}

/// How wide a part of `rect` from its left to `end` is: the place a
/// title's line takes where it ends before what stands beside it.
fn up_to(rect: Rect, end: f32) -> u16 {
    (end - f32::from(rect.x)).max(0.0) as u16
}

/// Room a number's value always gets between its − and +, so the
/// buttons of rows above each other line up.
pub(super) const STEPPER_VALUE: &str = "000 %";

/// The height of the row of setting `spec` in a list of rows `row` high,
/// `noted` ones ([`UiStyle::noted_row`]) or not: a number's row, which
/// holds its − and +, grows on touch screens with its buttons
/// ([`UiStyle::touch_row`]; in a noted row its first line,
/// [`number_title_area`]), the others stay as they are.
pub fn setting_row(style: &UiStyle, spec: &SettingSpec, row: u32, noted: bool) -> u32 {
    match spec.kind {
        SettingKind::Int { .. } if noted => {
            let band = style.title_band();
            row + style.touch_row(band) - band
        }
        SettingKind::Int { .. } => style.touch_row(row),
        _ => row,
    }
}

/// Where a setting's row puts its title and its control (a value, a
/// switch or `−` value `+`): side by side, or, where the title would be
/// cut, the control on a line of its own below the title, at the right. A
/// title is never cut for a control. Decided once, when the row is laid
/// out ([`row_lines`]), for every value it may show (a choice's widest
/// option), and moved to where the row is drawn ([`RowLines::at`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct RowLines {
    /// The line of the title.
    pub title: Rect,
    /// The line of the control: the title's where both fit side by side.
    pub control: Rect,
    /// The whole row's height, its note included.
    pub height: u16,
}

impl RowLines {
    /// Title and control side by side in `line`, a row as high.
    pub const fn one(line: Rect) -> Self {
        Self {
            title: line,
            control: line,
            height: line.height,
        }
    }

    /// The control on a line of its own, below the title.
    pub fn stacked(&self) -> bool {
        self.title != self.control
    }

    /// Where a note below them begins.
    pub fn bottom(&self) -> u16 {
        self.control.bottom() as u16
    }

    /// The lines of a row laid out at the top left moved to the row's
    /// place `rect`.
    pub fn at(self, rect: Rect) -> Self {
        let moved = |r: Rect| Rect::new(r.x + rect.x, r.y + rect.y, r.width, r.height);
        Self {
            title: moved(self.title),
            control: moved(self.control),
            height: self.height,
        }
    }
}

/// `value` as a row shows it where the UI runs right to left: the "›" that
/// ends a value (it opens a screen) is a "‹" in front of it, at the end the
/// value stands at. Left to right, `value` as it is.
fn turn_chevron<'a>(style: &UiStyle, value: &'a str) -> alloc::borrow::Cow<'a, str> {
    match value.strip_suffix('›').filter(|_| style.is_rtl()) {
        Some(rest) if rest.trim_end().is_empty() => alloc::borrow::Cow::Borrowed("‹"),
        Some(rest) => alloc::borrow::Cow::Owned(alloc::format!("‹ {}", rest.trim_end())),
        None => alloc::borrow::Cow::Borrowed(value),
    }
}

/// The most lines a title takes on a line of its own (see [`RowLines`]).
const TITLE_LINES: usize = 2;

/// Draws `title` on its own `line` (see [`RowLines`]): wrapped where it
/// is too wide for one, the last line shortened with "…" past
/// [`TITLE_LINES`].
fn draw_own_title(
    canvas: &mut Canvas<'_>,
    style: &UiStyle,
    font: &FontHandle<'_>,
    line: Rect,
    title: &str,
    ink: Color,
) {
    let margin = style.margin as f32;
    let room = f32::from(line.width) - 2.0 * margin;
    let left = f32::from(line.x) + margin;
    // One line, as most are: nothing to wrap.
    if font.measure(style.text, title) <= room + 1.0 {
        draw_label_out(
            canvas,
            font,
            style,
            line,
            TextRole::BODY,
            style.text,
            left,
            line,
            title,
            ink,
        );
        return;
    }
    let wrapped = font.wrap(style.text, title, room);
    canvas.describe(Role::Text, title, line);
    let metrics = font.line_metrics(style.text);
    let count = wrapped.len().clamp(1, TITLE_LINES);
    // The lines centred in the line's height, as one line is.
    let total = metrics.line_height * count as f32;
    let top = f32::from(line.y) + (f32::from(line.height) - total) / 2.0;
    for (n, range) in wrapped.iter().take(TITLE_LINES).enumerate() {
        let text = if n + 1 == TITLE_LINES && wrapped.len() > TITLE_LINES {
            fit(font, style.text, title[range.start..].trim_end(), room)
        } else {
            String::from(title[range.clone()].trim_end())
        };
        let baseline = top + metrics.line_height * n as f32 + metrics.ascent;
        let size = style.text;
        draw_run_out(
            canvas,
            font,
            style,
            line,
            size,
            left,
            baseline as i32,
            &text,
            ink,
        );
    }
}

/// The note of a number's row below its [`RowLines`].
pub(crate) fn draw_lines_note(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    rect: Rect,
    lines: RowLines,
    focused: bool,
    note: &str,
) {
    draw_note_below(canvas, env, style, rect, lines.bottom(), focused, note);
}

/// The − and + buttons of a number row, with room for `room` between
/// them (at least for "000 %"): the value, or the widest of the
/// numbers in rows above each other (see [`number_room`]), so their
/// buttons line up.
pub(crate) fn stepper(
    env: &Env<'_>,
    style: &UiStyle,
    rect: Rect,
    room: &str,
) -> Option<(Rect, Rect)> {
    let (minus, plus) = stepper_ltr(env, style, rect, room)?;
    Some((style.flip(rect, minus), style.flip(rect, plus)))
}

/// [`stepper`] left to right: the block is at the row's end, its `−` left
/// of its `+`; where the UI runs right to left it is the mirror image of
/// this.
pub(super) fn stepper_ltr(
    env: &Env<'_>,
    style: &UiStyle,
    rect: Rect,
    room: &str,
) -> Option<(Rect, Rect)> {
    let font = style.font(env)?;
    let size = button_height(style, rect);
    let y = rect.y + (rect.height - size) / 2;
    let margin = style.margin as u16;
    let gap = style.gap as u16;
    let right = rect.x + rect.width - margin;
    let plus = Rect::new(right.saturating_sub(size), y, size, size);
    let value_width = font
        .measure(style.text, room)
        .max(font.measure(style.text, STEPPER_VALUE)) as u16;
    let minus_x = plus.x.saturating_sub(2 * gap + value_width + size);
    Some((Rect::new(minus_x, y, size, size), plus))
}

/// A number on a light row on [`RowLines`]: its title, then `[−] value
/// [+]` exactly as in menus, with `room` for the value (see [`stepper`]),
/// below the title where they are stacked; `focus` frames the `−`
/// (`Some(false)`) or the `+` (`Some(true)`).
#[allow(
    clippy::too_many_arguments,
    reason = "its inputs are separate things; a struct of them would serve this call alone"
)]
pub(crate) fn draw_number_lines(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    lines: RowLines,
    title: &str,
    value: &str,
    room: &str,
    focus: Option<bool>,
) {
    let Some(font) = style.font(env) else {
        return;
    };
    let room = wider(&font, style.text, room, value);
    let Some((minus, plus)) = stepper(env, style, lines.control, room) else {
        return;
    };
    let ink = Color::BLACK;
    let start = stepper_ltr(env, style, lines.control, room).map_or(minus, |(minus, _)| minus);
    draw_number_title(canvas, style, &font, lines, start, title, ink);
    let value_x = centred(&font, style.text, value, minus, plus);
    let row = lines.control;
    draw_label_in(
        canvas,
        &font,
        style,
        TextRole::BODY,
        style.text,
        value_x,
        row,
        value,
        ink,
    );
    draw_button(canvas, env, style, minus, "−", false, focus == Some(false));
    draw_button(canvas, env, style, plus, "+", false, focus == Some(true));
}

/// The wider of `room` and `value` as drawn: the room a stepper keeps for
/// its value, so that steppers of stacked rows line up while no value is
/// wider than that room (measured, not counted in bytes: "−" or "•" take
/// three bytes and are narrow).
pub(super) fn wider<'a>(
    font: &FontHandle<'_>,
    size: f32,
    room: &'a str,
    value: &'a str,
) -> &'a str {
    if font.measure(size, room) < font.measure(size, value) {
        value
    } else {
        room
    }
}

/// Where `value` starts, centred between the buttons `minus` and `plus`.
pub(super) fn centred(
    font: &FontHandle<'_>,
    size: f32,
    value: &str,
    minus: Rect,
    plus: Rect,
) -> f32 {
    // Whichever of them is at the left (the `+` where the UI runs right to
    // left).
    let (first, second) = if minus.x <= plus.x {
        (minus, plus)
    } else {
        (plus, minus)
    };
    let left = f32::from(first.x + first.width);
    let room = f32::from(second.x) - left;
    left + (room - font.measure(size, value)) / 2.0
}

/// A number's title on its line: before its `−` (`minus`, as it stands
/// left to right), or the
/// line's whole width where the buttons are below it.
fn draw_number_title(
    canvas: &mut Canvas<'_>,
    style: &UiStyle,
    font: &FontHandle<'_>,
    lines: RowLines,
    minus: Rect,
    title: &str,
    ink: Color,
) {
    if lines.stacked() {
        return draw_own_title(canvas, style, font, lines.title, title, ink);
    }
    let margin = style.margin as f32;
    let line = lines.title;
    // From the row's own start, e.g. right of an icon.
    let left = f32::from(line.x) + margin;
    let title = fit(font, style.text, title, f32::from(minus.x) - margin - left);
    draw_label_out(
        canvas,
        font,
        style,
        line,
        TextRole::BODY,
        style.text,
        left,
        line,
        &title,
        ink,
    );
}
