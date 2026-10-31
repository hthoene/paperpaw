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
/// below it ([`draw_number_note`]).
pub fn number_title_area(style: &UiStyle, rect: Rect) -> Rect {
    let band = (style.touch_row(style.title_band()) as u16).min(rect.height);
    Rect::new(rect.x, rect.y + style.gap as u16 / 2, rect.width, band)
}

/// Draws the note of a noted row of a number, below its
/// [`number_title_area`].
pub fn draw_number_note(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    rect: Rect,
    focused: bool,
    note: &str,
) {
    let below = number_title_area(style, rect).bottom() as u16;
    draw_note_below(canvas, env, style, rect, below, focused, note);
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
pub(super) fn title_lines(
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

/// The height of a row `width` wide with `title` and `value` (as
/// [`draw_value_row`] draws it): a row ([`UiStyle::row`]), taller where
/// the title takes more lines beside the value. Its title is never
/// shortened in a row as high.
pub(crate) fn value_row_height(
    env: &Env<'_>,
    style: &UiStyle,
    width: u16,
    title: &str,
    value: &str,
) -> u16 {
    let row = style.row as u16;
    let Some(font) = style.font(env) else {
        return row;
    };
    let lines = ValueLines::new(&font, style, width, title, value);
    row.max(lines.height() + style.margin as u16)
}

/// The height of a noted row `width` wide with `title`, `value` and
/// `note` (as [`draw_noted_row`] draws it): a noted row as high as its
/// note needs ([`UiStyle::noted_row_for`]), taller where the title takes
/// more lines beside the value; without a note, [`value_row_height`].
pub(crate) fn noted_row_height(
    env: &Env<'_>,
    style: &UiStyle,
    width: u16,
    title: &str,
    value: &str,
    note: &str,
) -> u16 {
    if note.is_empty() {
        return value_row_height(env, style, width, title, value);
    }
    let base = style.noted_row_for(env, width, [note]) as u16;
    let Some(font) = style.font(env) else {
        return base;
    };
    let lines = ValueLines::new(&font, style, width, title, value);
    let band = title_area(style, Rect::new(0, 0, width, base)).height;
    base + lines.height().saturating_sub(band)
}

/// The height of a row `width` wide with a switch, `title` and `note` (as
/// [`draw_switch_row`] draws it): a row ([`UiStyle::row`]) or a noted one
/// ([`UiStyle::noted_row_for`]), taller where the title takes more lines
/// beside the switch. Its title is never shortened in a row as high.
pub(crate) fn switch_row_height(
    env: &Env<'_>,
    style: &UiStyle,
    width: u16,
    title: &str,
    note: &str,
) -> u16 {
    let plain = Rect::new(0, 0, width, style.control_row as u16);
    let base = if note.is_empty() {
        style.row
    } else {
        style.noted_row_for(env, width, [note])
    } as u16;
    let Some(font) = style.font(env) else {
        return base;
    };
    // The switch and the gap the title keeps before it.
    let beside = f32::from(switch_rect(style, plain).width + style.gap as u16);
    let (lines, line) = title_lines(&font, style, width, beside, title);
    let titles = line * lines.len() as u16;
    if note.is_empty() {
        return base.max(titles + style.margin as u16);
    }
    let band = title_area(style, Rect::new(0, 0, width, base)).height;
    base + titles.saturating_sub(band)
}

/// Draws the note of a noted row below its [`title_area`].
pub fn draw_note(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    rect: Rect,
    focused: bool,
    note: &str,
) {
    let below = title_area(style, rect).bottom() as u16;
    draw_note_below(canvas, env, style, rect, below, focused, note);
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
pub(super) fn draw_note_below(
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
    let color = style.focus_faint(focused, Ink::Detail);
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
        font.draw(
            canvas,
            style.small,
            f32::from(rect.x) + margin,
            baseline as i32,
            &text,
            color,
        );
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
            draw_label_in(
                canvas,
                &font,
                style,
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
        draw_label_in(
            canvas,
            &font,
            style,
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
        draw_label_in(
            canvas,
            &font,
            style,
            TextRole::BODY,
            style.text,
            value_at(value_width),
            rect,
            &shown,
            color,
        );
        draw_label_in(
            canvas,
            &font,
            style,
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
        draw_label_in(
            canvas,
            &font,
            style,
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
            draw_label_in(
                canvas,
                &font,
                style,
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
    draw_label_in(
        canvas,
        &font,
        style,
        TextRole::BODY,
        style.text,
        value_at(value_width),
        rect,
        &shown,
        color,
    );
    draw_label_in(
        canvas,
        &font,
        style,
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

/// The line of a row in `rect` that holds its title and its control side
/// by side, as they are drawn: a number's as high as its − and + need, a
/// switch's the whole row where it has no note (as [`draw_switch_row`]
/// draws it).
fn first_line(style: &UiStyle, spec: &SettingSpec, rect: Rect, noted: bool) -> Rect {
    let number = matches!(spec.kind, SettingKind::Int { .. });
    let line = match (number, noted) {
        (true, true) => return number_title_area(style, rect),
        (false, true) => return title_area(style, rect),
        (true, false) => style.touch_row(style.row),
        (false, false) => style.row,
    };
    Rect::new(rect.x, rect.y, rect.width, (line as u16).min(rect.height))
}

/// Whether setting `spec` shows a switch (else a value or a number).
fn is_switch(spec: &SettingSpec) -> bool {
    matches!(spec.kind, SettingKind::Bool) && spec.extra().editor.is_none()
}

/// The widest a setting's value at the right of its row is drawn, of
/// all it may show in place: a choice's widest option, the widest
/// language, the wider of on and off; else (a text, a list, changed on a
/// screen of their own) the value it has. With the mark of a value set
/// for a scope where it may have one, and "›" where a tap opens a screen
/// for it.
fn widest_value(env: &Env<'_>, style: &UiStyle, font: &FontHandle<'_>, spec: &SettingSpec) -> f32 {
    let size = style.text;
    let widest = |names: &mut dyn Iterator<Item = &str>| {
        names
            .map(|name| font.measure(size, name))
            .fold(0.0, f32::max)
    };
    let value = match &spec.kind {
        SettingKind::Choice(choices) => widest(&mut choices.iter().map(|o| env.i18n().tr(o))),
        SettingKind::Language => widest(&mut env.i18n().languages().into_iter().map(|(_, n)| n)),
        SettingKind::Bool => widest(
            &mut ["core.on", "core.off"]
                .into_iter()
                .map(|k| env.i18n().tr(k)),
        ),
        _ => font.measure(size, &plain_setting_value(env, spec)),
    };
    let scoped = if spec.scoped {
        font.measure(size, "• ")
    } else {
        0.0
    };
    let opens = if opens_screen(env, spec) {
        font.measure(size, " ›")
    } else {
        0.0
    };
    value + scoped + opens
}

/// Whether setting `spec`'s title fits beside its control on `line`
/// (`room` for a number's value, see [`number_room`]): `−` value `+`, a
/// switch or a value.
fn fits_beside(env: &Env<'_>, style: &UiStyle, spec: &SettingSpec, line: Rect, room: &str) -> bool {
    let Some(font) = style.font(env) else {
        return true;
    };
    let title = env.i18n().tr(&spec.title);
    let margin = style.margin as f32;
    let room_for_title = match spec.kind {
        SettingKind::Int { .. } => {
            // Its least and greatest: the − and + stay where they are
            // while it changes.
            let own = number_room(env, style, core::iter::once(spec));
            let room = wider(&font, style.text, room, &own);
            let Some((minus, _)) = stepper(env, style, line, room) else {
                return true;
            };
            f32::from(minus.x) - f32::from(line.x) - 2.0 * margin
        }
        _ if is_switch(spec) => {
            // The title ends a gap before the switch.
            let switch = switch_rect(style, line);
            f32::from(switch.x) - f32::from(line.x) - margin - style.gap as f32
        }
        _ => f32::from(line.width) - 3.0 * margin - widest_value(env, style, &font, spec),
    };
    // A pixel of slack for rounding, as in [`draw_label`].
    font.measure(style.text, title) <= room_for_title + 1.0
}

/// The most lines a title takes on a line of its own (see [`RowLines`]).
const TITLE_LINES: usize = 2;

/// How high the line of `title` is where it stands on its own, `width`
/// wide: a title's band, and a line more where it wraps (at most
/// [`TITLE_LINES`]).
fn own_title_height(env: &Env<'_>, style: &UiStyle, width: u16, title: &str) -> u16 {
    let band = style.title_band() as u16;
    let Some(font) = style.font(env) else {
        return band;
    };
    let room = f32::from(width) - 2.0 * style.margin as f32;
    if font.measure(style.text, title) <= room + 1.0 {
        return band;
    }
    let lines = font
        .wrap(style.text, title, room)
        .len()
        .clamp(1, TITLE_LINES);
    let line = font.line_metrics(style.text).line_height as u16;
    band + line * (lines as u16 - 1)
}

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
        draw_label_in(
            canvas,
            font,
            style,
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
        font.draw(canvas, style.text, left, baseline as i32, &text, ink);
    }
}

/// How setting `spec`'s row `width` wide is laid out in a list of rows
/// `row` high (`room` for a number's value, see [`number_room`]; `noted`
/// rows or not): its title and its control side by side, or the control
/// below the title where they do not fit, for every value it may show,
/// and how high it is. At the top left: drawn at its place with
/// [`RowLines::at`]. Made once with the list (it measures, and wraps a
/// long title), not each time the row is drawn.
#[allow(
    clippy::too_many_arguments,
    reason = "its inputs are separate things; a struct of them would serve this call alone"
)]
pub fn row_lines(
    env: &Env<'_>,
    style: &UiStyle,
    spec: &SettingSpec,
    width: u16,
    room: &str,
    row: u32,
    noted: bool,
) -> RowLines {
    let height = setting_row(style, spec, row, noted) as u16;
    let rect = Rect::new(0, 0, width, height);
    let line = first_line(style, spec, rect, noted);
    // A switch without a note takes the whole row, as it is drawn.
    let beside = if is_switch(spec) && !(noted && spec.description.is_some()) {
        rect
    } else {
        line
    };
    if fits_beside(env, style, spec, beside, room) {
        return RowLines {
            height,
            ..RowLines::one(line)
        };
    }
    let high = own_title_height(env, style, width, env.i18n().tr(&spec.title));
    let title = Rect::new(line.x, line.y, line.width, high);
    let control = Rect::new(line.x, title.bottom() as u16, line.width, line.height);
    RowLines {
        title,
        control,
        height: height + high,
    }
}

/// The height of the row of setting `spec` `width` pixels wide in a list
/// of rows `row` high: as [`setting_row`], and a title's line more where
/// the title does not fit beside its control (see [`row_lines`], which
/// tells both).
#[allow(
    clippy::too_many_arguments,
    reason = "its inputs are separate things; a struct of them would serve this call alone"
)]
pub fn setting_height(
    env: &Env<'_>,
    style: &UiStyle,
    spec: &SettingSpec,
    width: u16,
    room: &str,
    row: u32,
    noted: bool,
) -> u32 {
    u32::from(row_lines(env, style, spec, width, room, row, noted).height)
}

/// A setting's row with a value: `title` and `value` on [`RowLines`]
/// (side by side, or the value below at the right), `note` below them.
#[allow(
    clippy::too_many_arguments,
    reason = "its inputs are separate things; a struct of them would serve this call alone"
)]
pub fn draw_setting_row(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    rect: Rect,
    lines: RowLines,
    focused: bool,
    title: &str,
    value: &str,
    note: &str,
) {
    if !lines.stacked() {
        return draw_noted_row(canvas, env, style, rect, focused, title, value, note);
    }
    if let Some(font) = style.font(env) {
        draw_own_title(
            canvas,
            style,
            &font,
            lines.title,
            title,
            style.focus_ink(focused),
        );
    }
    draw_value_row(canvas, env, style, lines.control, focused, "", value);
    if !note.is_empty() {
        draw_note_below(canvas, env, style, rect, lines.bottom(), focused, note);
    }
}

/// A setting's row with a switch: as [`Row::switch`](super::Row::switch), on
/// [`RowLines`] (the switch below the title, at the right, where the
/// title would not fit beside it).
#[allow(
    clippy::too_many_arguments,
    reason = "its inputs are separate things; a struct of them would serve this call alone"
)]
pub fn draw_switch_lines(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    rect: Rect,
    lines: RowLines,
    focused: bool,
    title: &str,
    note: &str,
    on: bool,
) {
    if !lines.stacked() {
        return draw_switch_row(canvas, env, style, rect, focused, title, note, on);
    }
    if let Some(font) = style.font(env) {
        draw_own_title(
            canvas,
            style,
            &font,
            lines.title,
            title,
            style.focus_ink(focused),
        );
    }
    if !note.is_empty() {
        draw_note_below(canvas, env, style, rect, lines.bottom(), focused, note);
    }
    draw_switch(
        canvas,
        style,
        lines.control,
        on,
        focused && style.dark_focus(),
    );
}

/// The note of a number's row below its [`RowLines`].
pub fn draw_lines_note(
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

/// The − and + of a number row showing `value`, exactly where a
/// [`Row::number`](super::Row::number) and [`draw_stepper`] (and their
/// `_lines` forms) draw them with `room`: for the hit areas of a screen of
/// its own.
pub fn value_stepper(
    env: &Env<'_>,
    style: &UiStyle,
    rect: Rect,
    room: &str,
    value: &str,
) -> Option<(Rect, Rect)> {
    let font = style.font(env)?;
    stepper(env, style, rect, wider(&font, style.text, room, value))
}

/// The − and + buttons of a number row, with room for `room` between
/// them (at least for "000 %"): the value, or the widest of the
/// numbers in rows above each other (see [`number_room`]), so their
/// buttons line up.
pub fn stepper(env: &Env<'_>, style: &UiStyle, rect: Rect, room: &str) -> Option<(Rect, Rect)> {
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

/// The widest value the numbers among `specs` show, from their least to
/// their greatest, with their units (and the mark of a value set for a
/// scope): the room of their steppers (see [`stepper`]) where they are
/// listed above each other.
pub fn number_room<'a>(
    env: &Env<'_>,
    style: &UiStyle,
    specs: impl IntoIterator<Item = &'a SettingSpec>,
) -> String {
    let Some(font) = style.font(env) else {
        return String::new();
    };
    let mut widest = (0.0, String::new());
    for spec in specs {
        let SettingKind::Int { min, max } = spec.kind else {
            continue;
        };
        for v in [min, max] {
            let text = match &spec.extra().unit {
                Some(unit) => env.i18n().plural(unit, v, &[]),
                None => format!("{v}"),
            };
            let text = if spec.scoped {
                format!("• {text}")
            } else {
                text
            };
            let width = font.measure(style.text, &text);
            if width > widest.0 {
                widest = (width, text);
            }
        }
    }
    widest.1
}

/// A number on a light row on [`RowLines`]: its title, then `[−] value
/// [+]` exactly as in menus, with `room` for the value (see [`stepper`]),
/// below the title where they are stacked; `focus` frames the `−`
/// (`Some(false)`) or the `+` (`Some(true)`).
#[allow(
    clippy::too_many_arguments,
    reason = "its inputs are separate things; a struct of them would serve this call alone"
)]
pub fn draw_number_lines(
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
    draw_number_title(canvas, style, &font, lines, minus, title, ink);
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
    let left = f32::from(minus.x + minus.width);
    let room = f32::from(plus.x) - left;
    left + (room - font.measure(size, value)) / 2.0
}

/// A focused number: title, then `[−] value [+]`, with `room` for the
/// value (see [`stepper`]). While editing with buttons the boxes are
/// filled.
#[allow(
    clippy::too_many_arguments,
    reason = "its inputs are separate things; a struct of them would serve this call alone"
)]
pub fn draw_stepper(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    rect: Rect,
    title: &str,
    value: &str,
    room: &str,
    editing: bool,
) {
    let lines = RowLines::one(rect);
    draw_stepper_lines(canvas, env, style, lines, title, value, room, editing);
}

/// Where a stepper's title and its `−` value `+` go in a row `width`
/// wide and `row` high, at the top left (move them with
/// [`RowLines::at`]): side by side where the whole title fits beside
/// them, else the title above, on as many lines as it needs, and the
/// stepper on a line of its own, as setting rows do. The title is never
/// shortened; draw with [`draw_stepper_lines`], the row
/// [`RowLines::height`] high.
pub fn stepper_lines(
    env: &Env<'_>,
    style: &UiStyle,
    width: u16,
    row: u16,
    title: &str,
    room: &str,
) -> RowLines {
    let line = Rect::new(0, 0, width, row);
    let Some(font) = style.font(env) else {
        return RowLines::one(line);
    };
    let room = wider(&font, style.text, room, room);
    let beside = stepper(env, style, line, room).is_some_and(|(minus, _)| {
        let margin = style.margin as f32;
        font.measure(style.text, title) <= f32::from(minus.x) - 2.0 * margin + 1.0
    });
    if beside {
        return RowLines::one(line);
    }
    let high = own_title_height(env, style, width, title);
    RowLines {
        title: Rect::new(0, 0, width, high),
        control: Rect::new(0, high, width, row),
        height: high + row,
    }
}

/// [`draw_stepper`] on [`RowLines`]: the `−` value `+` below the title
/// where they are stacked.
#[allow(
    clippy::too_many_arguments,
    reason = "its inputs are separate things; a struct of them would serve this call alone"
)]
pub fn draw_stepper_lines(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    lines: RowLines,
    title: &str,
    value: &str,
    room: &str,
    editing: bool,
) {
    let Some(font) = style.font(env) else {
        return;
    };
    let room = wider(&font, style.text, room, value);
    let Some((minus, plus)) = stepper(env, style, lines.control, room) else {
        return;
    };
    let ink = style.focus_ink(true);
    draw_number_title(canvas, style, &font, lines, minus, title, ink);
    let value_x = centred(&font, style.text, value, minus, plus);
    draw_label_in(
        canvas,
        &font,
        style,
        TextRole::BODY,
        style.text,
        value_x,
        lines.control,
        value,
        ink,
    );
    let steps = (minus, plus);
    draw_focused_steps(canvas, style, &font, lines.control, steps, editing);
}

/// A number's title on its line: left of its `−` (`minus`), or the
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
    draw_label_in(
        canvas,
        font,
        style,
        TextRole::BODY,
        style.text,
        left,
        line,
        &title,
        ink,
    );
}

/// The `−` and `+` of a focused number, on the focus bar in `row`:
/// outlined in white, or filled white while the buttons change it
/// (`editing`).
pub(super) fn draw_focused_steps(
    canvas: &mut Canvas<'_>,
    style: &UiStyle,
    font: &FontHandle<'_>,
    row: Rect,
    (minus, plus): (Rect, Rect),
    editing: bool,
) {
    for (button, glyph) in [(minus, "−"), (plus, "+")] {
        let (x, y, w, h) = (
            i32::from(button.x),
            i32::from(button.y),
            u32::from(button.width),
            u32::from(button.height),
        );
        let ink = style.focus_ink(true);
        let paper = style.focus_paper(true);
        let r = style.radius(h);
        let glyph_color = if editing {
            canvas.fill_round_rect(x, y, w, h, r, ink);
            paper
        } else {
            canvas.stroke_round_rect(x, y, w, h, button_border(style), r, ink);
            ink
        };
        let glyph_width = font.measure(style.text, glyph);
        let glyph_x = f32::from(button.x) + (f32::from(button.width) - glyph_width) / 2.0;
        draw_label_in(
            canvas,
            font,
            style,
            TextRole::BODY,
            style.text,
            glyph_x,
            row,
            glyph,
            glyph_color,
        );
    }
}

/// The value of a setting for display, with its unit.
pub fn setting_value(env: &Env<'_>, spec: &SettingSpec) -> String {
    let value = plain_setting_value(env, spec);
    // Set for the scope (e.g. this book only): marked.
    if env.settings().is_scoped(spec.id.as_str()) {
        format!("• {value}")
    } else {
        value
    }
}

pub(super) fn plain_setting_value(env: &Env<'_>, spec: &SettingSpec) -> String {
    match env.settings().get(spec.id.as_str()) {
        Some(SettingValue::Bool(true)) => env.i18n().tr("core.on").to_string(),
        Some(SettingValue::Bool(false)) => env.i18n().tr("core.off").to_string(),
        Some(SettingValue::Int(v)) => match &spec.extra().unit {
            Some(unit) => env.i18n().plural(unit, *v, &[]),
            None => format!("{v}"),
        },
        // Kept with the secrets: dots where there is one.
        _ if spec.extra().secret => {
            if env
                .secrets
                .get(spec.id.as_str())
                .is_some_and(|s| !s.is_empty())
            {
                "••••••".to_string()
            } else {
                String::new()
            }
        }
        Some(SettingValue::Text(v)) => option_name(env, spec, v),
        None => String::new(),
    }
}

/// The display name of a choice or language value.
pub fn option_name(env: &Env<'_>, spec: &SettingSpec, value: &str) -> String {
    match spec.kind {
        SettingKind::Language => env
            .i18n
            .languages()
            .into_iter()
            .find(|(tag, _)| *tag == value)
            .map_or(value.to_string(), |(_, name)| name.to_string()),
        // Choices are ids; their translation is their display name.
        SettingKind::Choice(_) => env.i18n().tr(value).to_string(),
        SettingKind::List => {
            let names: Vec<&str> = crate::status::item_ids(value)
                .map(|id| {
                    let title = env
                        .status_items()
                        .title(id)
                        .or_else(|| env.widgets().title(id));
                    env.i18n().tr(title.unwrap_or(id))
                })
                .collect();
            if names.is_empty() {
                env.i18n().tr("core.list.none").to_string()
            } else {
                names.join(", ")
            }
        }
        _ => value.to_string(),
    }
}

/// Whether a choice (or the language) has just two options: choosing it
/// switches to the other at once, as a switch does, instead of opening a
/// list that would only offer that one (fewer presses).
pub fn two_options(env: &Env<'_>, spec: &SettingSpec) -> bool {
    // More ways to choose are listed with the options.
    if !env.menus().items(spec.id.as_str()).is_empty() {
        return false;
    }
    match &spec.kind {
        SettingKind::Choice(options) => options.len() == 2,
        SettingKind::Language => env.i18n().languages().len() == 2,
        _ => false,
    }
}

/// What choosing a setting (confirm or a tap) opens, the same on every
/// screen that lists settings: its editor ([`SettingExtra::editor`](crate::SettingExtra::editor)),
/// the keyboard for text (a secret entered afresh), the values of a
/// choice of more than two on the screen of slot
/// [`CHOOSE_VALUE`](crate::builtin::slots::CHOOSE_VALUE). `None` where
/// the screen changes it itself: a switch, a number, a choice of two
/// (see [`next_value`]), or a choice where no screen fills that slot.
pub fn setting_opens(env: &Env<'_>, spec: &SettingSpec) -> Option<crate::Command> {
    if let Some(editor) = &spec.extra().editor {
        return Some(editor.open(&spec.id));
    }
    match spec.kind {
        SettingKind::Text => {
            let question = crate::ask::text(
                env.i18n().tr(&spec.title),
                &crate::builtin::events::SETTING_TYPED,
            )
            .with(crate::builtin::params::ID, spec.id.as_str().to_string());
            let question = if spec.extra().secret {
                question.password()
            } else {
                question.initial(setting_value(env, spec))
            };
            Some(question.command())
        }
        SettingKind::Choice(_) | SettingKind::Language
            if !two_options(env, spec) && env.has_slot(&crate::builtin::slots::CHOOSE_VALUE) =>
        {
            Some(crate::Editor::Slot(crate::builtin::slots::CHOOSE_VALUE).open(&spec.id))
        }
        _ => None,
    }
}

/// Whether choosing the setting opens another screen (its own editor, or
/// a list to choose from): its row shows a `›`.
pub fn opens_screen(env: &Env<'_>, spec: &SettingSpec) -> bool {
    spec.extra().editor.is_some()
        || (matches!(spec.kind, SettingKind::Choice(_) | SettingKind::Language)
            && !two_options(env, spec))
}

/// The sizes of book text and how the reading font draws them: sizes
/// that look alike are drawn at the same pixel size of a font pack.
struct FontSizes<'e> {
    font: crate::FontHandle<'e>,
    metrics: crate::Metrics,
    min: i64,
    max: i64,
}

impl<'e> FontSizes<'e> {
    /// Those `spec` sets, where it is the size of book text and a reading
    /// font is there.
    fn of(env: &Env<'e>, spec: &SettingSpec) -> Option<Self> {
        let SettingKind::Int { min, max } = spec.kind else {
            return None;
        };
        if spec.id.as_str() != crate::builtin::settings::FONT_SIZE.as_str() {
            return None;
        }
        Some(Self {
            font: env.fonts().role(crate::builtin::fonts::READING.as_str())?,
            metrics: env.metrics(),
            min,
            max: max.max(min),
        })
    }

    /// What `points` looks like: alike for sizes drawn alike.
    fn looks(&self, points: i64) -> (u32, u32) {
        let px = self.metrics.pt(points as f32);
        let height = self.font.line_metrics(px).line_height;
        (
            height.to_bits(),
            self.font.measure(px, "Hamburgefonstiv").to_bits(),
        )
    }

    /// The smallest of the sizes that look as `points` does.
    fn first_alike(&self, points: i64) -> i64 {
        let mut points = points.clamp(self.min, self.max);
        let look = self.looks(points);
        while points > self.min && self.looks(points - 1) == look {
            points -= 1;
        }
        points
    }
}

/// The value number setting `spec` shows for `value`, e.g. where a finger
/// leaves a slider: `value`, but for the size of book text the smallest
/// of the sizes the reading font draws alike (a font pack has a few pixel
/// sizes; on the X4 Pro 9 and 10 points are both 28 pixels), so that no
/// two sizes offered look the same. Values kept stay as they are.
pub fn shown_number(env: &Env<'_>, spec: &SettingSpec, value: i64) -> i64 {
    FontSizes::of(env, spec).map_or(value, |sizes| sizes.first_alike(value))
}

/// The value number setting `spec` steps to from `from` towards `to`
/// (a step of its own, a button's, a key's): `to`, but for the size of
/// book text the next size that looks different from `from`, as
/// [`shown_number`] shows it, and `from` where none that way does.
pub fn shown_step(env: &Env<'_>, spec: &SettingSpec, from: i64, to: i64) -> i64 {
    let Some(sizes) = FontSizes::of(env, spec).filter(|_| to != from) else {
        return to;
    };
    let direction = (to - from).signum();
    let before = sizes.looks(from);
    let mut points = to.clamp(sizes.min, sizes.max);
    while sizes.looks(points) == before {
        let next = (points + direction).clamp(sizes.min, sizes.max);
        if next == points {
            return from;
        }
        points = next;
    }
    sizes.first_alike(points)
}

/// The value a slider of number setting `spec` comes to, moved from
/// `before` (its value when the finger came down, or the key was
/// pressed) to `value` ([`Slider`](crate::ui::slider::Slider) steps and
/// drags): a step away, the next value that looks different
/// ([`shown_step`]); further, the value shown for `value`
/// ([`shown_number`]). Only the size of book text skips values.
pub fn shown_slider_value(env: &Env<'_>, spec: &SettingSpec, before: i64, value: i64) -> i64 {
    if value.abs_diff(before) == spec.extra().step.unsigned_abs().max(1) {
        shown_step(env, spec, before, value)
    } else {
        shown_number(env, spec, value)
    }
}

/// The value after one step of the setting: a switch toggles, a number
/// moves by `step` within its range (the size of book text to the next
/// size that looks different, see [`shown_step`]), a choice or language
/// moves to the next (or previous) option. `None` for text.
pub fn next_value(env: &Env<'_>, spec: &SettingSpec, step: i64) -> Option<SettingValue> {
    let current = env.settings().get(spec.id.as_str())?;
    let cycle = |options: &[&str], value: &str| -> SettingValue {
        let len = options.len().max(1) as i64;
        // From a value not among the options, the first step on is the
        // first option and the first step back the last.
        let index = match options.iter().position(|o| *o == value) {
            Some(index) => index as i64,
            None if step > 0 => -1,
            None => len,
        };
        let next = options
            .get(index.saturating_add(step).rem_euclid(len) as usize)
            .copied()
            .unwrap_or_default();
        SettingValue::Text(Cow::Owned(next.to_string()))
    };
    Some(match (&spec.kind, current) {
        (SettingKind::Bool, SettingValue::Bool(v)) => SettingValue::Bool(!v),
        (SettingKind::Int { min, max }, SettingValue::Int(v)) => {
            let next = v.saturating_add(step.saturating_mul(spec.extra().step));
            SettingValue::Int(shown_step(
                env,
                spec,
                *v,
                next.clamp(*min, (*max).max(*min)),
            ))
        }
        (SettingKind::Choice(options), SettingValue::Text(v)) => {
            let options: Vec<&str> = options.iter().collect();
            cycle(&options, v)
        }
        (SettingKind::Language, SettingValue::Text(v)) => {
            let tags: Vec<&str> = env.i18n().languages().into_iter().map(|(t, _)| t).collect();
            cycle(&tags, v)
        }
        _ => return None,
    })
}
