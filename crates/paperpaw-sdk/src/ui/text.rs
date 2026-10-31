//! Labels, icons, headings, messages and empty screens: text drawn in
//! its room, shortened where it does not fit.

#[allow(
    clippy::wildcard_imports,
    reason = "a part of the module, which shares its imports"
)]
use super::look::TextPart;
use super::*;

/// Lines of a row's note, e.g. a setting's description.
pub const NOTE_LINES: usize = 2;

/// The lines a row's note takes at most, in a row made higher for it
/// ([`UiStyle::noted_row_for`]).
pub const NOTE_LINES_MOST: usize = 4;

/// Line height of notes, relative to the small text size.
pub(super) const NOTE_LINE_HEIGHT: f32 = 1.3;

/// Draws icon `name` ([`icons`](crate::icons)) `size` pixels high,
/// vertically centred in `area`, starting at `x`; returns its width, 0 if
/// there is no icon font or no such icon.
pub fn draw_icon(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    name: &str,
    size: f32,
    x: f32,
    area: Rect,
    color: Color,
) -> f32 {
    let Some((font, ch)) = icon(env, name) else {
        return 0.0;
    };
    let mut text = [0u8; 4];
    let text = ch.encode_utf8(&mut text);
    draw_text(canvas, &font, None, size, x, area, text, color)
}

/// The width of icon `name` at `size` pixels, 0 if it cannot be drawn.
pub fn icon_width(env: &Env<'_>, name: &str, size: f32) -> f32 {
    icon(env, name).map_or(0.0, |(font, ch)| font.advance(size, ch))
}

/// Where icon `name`'s ink begins and ends across at `size`, from where it
/// is drawn (see [`FontHandle::ink_across`]); `None` without a drawing.
pub fn icon_ink(env: &Env<'_>, name: &str, size: f32) -> Option<(i32, i32)> {
    let (font, ch) = icon(env, name)?;
    let mut text = [0u8; 4];
    font.ink_across(size, ch.encode_utf8(&mut text))
}

/// The icon font and the character of icon `name`, where it has a drawing
/// (its own, or an icon pack's). A semantic icon
/// ([`icons::BACK`](crate::icons::BACK) and the others) is the icon a
/// plugin named for it, else its default.
pub(super) fn icon<'f>(env: &Env<'f>, name: &str) -> Option<(FontHandle<'f>, char)> {
    let name = if name.starts_with("core.icon.") {
        env.menus()
            .icon(name)
            .or_else(|| crate::icons::default_of(name))?
    } else {
        name
    };
    let font = env.fonts().bound(crate::builtin::fonts::ICONS.as_str())?;
    let ch = crate::icons::char(name)?;
    font.has_char(ch).then_some((font, ch))
}

/// Whether icon `name` has a drawing here (see [`draw_icon`]), e.g. a
/// semantic icon a theme gave one.
pub fn has_icon(env: &Env<'_>, name: &str) -> bool {
    icon(env, name).is_some()
}

/// Draws icon `name` `size` pixels high with its ink centred at
/// `(x, y)`; `false`, drawing nothing, where it has no drawing.
pub fn draw_icon_at(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    name: &str,
    size: f32,
    (x, y): (i32, i32),
    color: Color,
) -> bool {
    let Some((first, last)) = icon_ink(env, name, size) else {
        return false;
    };
    let height = (size * 2.0) as u16;
    let area = Rect::new(
        0,
        (y - i32::from(height) / 2).max(0) as u16,
        u16::MAX,
        height,
    );
    let start = x as f32 - (first + last) as f32 / 2.0;
    draw_icon(canvas, env, name, size, start, area, color);
    true
}

/// Draws icon `name` `size` pixels high from `x` with its ink centred
/// on the line at `twice_middle` / 2 (twice the y, so that a centre
/// between two pixels is said exactly): to the pixel where the ink's
/// height and the line allow, else half a pixel lower. Its width, 0
/// where it has no drawing.
pub(crate) fn draw_icon_centred(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    name: &str,
    size: f32,
    x: f32,
    twice_middle: i32,
    color: Color,
) -> f32 {
    let Some((font, ch)) = icon(env, name) else {
        return 0.0;
    };
    let mut text = [0u8; 4];
    let text = ch.encode_utf8(&mut text);
    let Some((top, bottom)) = font.ink(size, text) else {
        return font.advance(size, ch);
    };
    let baseline = (twice_middle - top - bottom + 1).div_euclid(2);
    font.draw(canvas, size, x, baseline, text, color)
}

/// The height of a section's heading in a list (see [`Rows`]): small
/// text with a little room above it.
pub fn heading_height(env: &Env<'_>, style: &UiStyle) -> u16 {
    let line = style.font(env).map_or(style.small * 1.3, |f| {
        f.line_metrics(style.small).line_height
    });
    (line * 1.25) as u16 + style.gap as u16
}

/// Draws a section's heading in `rect`: small and gray, low in its room,
/// so it reads as belonging to the entries below; between the margins, a
/// long one shortened with "…" as the rows' titles are.
pub(crate) fn draw_heading(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    rect: Rect,
    text: &str,
) {
    let Some(font) = style.font(env) else {
        return;
    };
    let line = font.line_metrics(style.small).line_height as u16;
    let area = Rect::new(
        rect.x,
        rect.y + rect.height.saturating_sub(line + style.gap as u16 / 2),
        rect.width,
        line,
    );
    canvas.describe(Role::Heading, text, area);
    // Ending at the right margin, where the rows' values end: what a last
    // letter reaches past its width stays in the row.
    let margin = style.margin as u16;
    let text_area = Rect::new(
        area.x,
        area.y,
        area.width.saturating_sub(margin),
        area.height,
    );
    let x = f32::from(rect.x) + style.margin as f32;
    draw_label(
        canvas,
        &font,
        style.small,
        x,
        text_area,
        text,
        style.ink(Ink::MUTED),
    );
}

/// The switch of an on/off setting at the right end of `row`.
pub fn switch_rect(style: &UiStyle, row: Rect) -> Rect {
    // A switch is tapped with its whole row: drawn as large as in a
    // plain row, also in one a long title makes higher.
    let plain = row.height.min(style.control_row as u16);
    let height = (plain * 7 / 10 * 3 / 4).max(8);
    let width = height * 2;
    Rect::new(
        (row.x + row.width).saturating_sub(width + style.margin as u16),
        row.y + (row.height - height) / 2,
        width,
        height,
    )
}

/// Draws a switch at the right end of `row`: filled with the knob on the
/// right when `on`, outlined with the knob on the left when off. `inverted`
/// for a focused (black) row.
pub(crate) fn draw_switch(
    canvas: &mut Canvas<'_>,
    style: &UiStyle,
    row: Rect,
    on: bool,
    inverted: bool,
) {
    let r = switch_rect(style, row);
    canvas.describe(Role::Switch, "", r).on(Some(on));
    let part = super::look::SwitchPart {
        rect: r,
        on,
        inverted,
    };
    if style.look.switch(canvas, style, part) {
        return;
    }
    let (ink, paper) = if inverted {
        (Color::WHITE, Color::BLACK)
    } else {
        (Color::BLACK, Color::WHITE)
    };
    let border = button_border(style);
    let radius = style.radius(u32::from(r.height));
    if on {
        canvas.fill_round(r, radius, ink);
    } else {
        canvas.fill_round(r, radius, paper);
        canvas.stroke_round(r, border, radius, ink);
    }
    let pad = (border as u16 * 2).max(3);
    let knob = r.height.saturating_sub(2 * pad);
    let x = if on {
        r.x + r.width - pad - knob
    } else {
        r.x + pad
    };
    let knob_rect = Rect::new(x, r.y + pad, knob, knob);
    let knob_radius = if style.pill {
        u32::from(knob) / 2
    } else {
        radius.saturating_sub(u32::from(pad))
    };
    canvas.fill_round(knob_rect, knob_radius, if on { paper } else { ink });
}

/// Icons in lists are this much larger than the text beside them.
pub const ICON_SCALE: f32 = 1.25;

/// The room icons take at the start of list rows (icon and gap), or 0
/// without an icon font or when `any` row has none.
pub fn icon_room(env: &Env<'_>, style: &UiStyle, any: bool) -> u16 {
    if any && has_icons(env) {
        (style.text * ICON_SCALE) as u16 + style.gap as u16
    } else {
        0
    }
}

/// Draws a list row's icon at the margin, centred on the row's first
/// line (`rect`), and returns the rest of the row for its label: the
/// label starts where it would after any icon, so rows line up.
pub(crate) fn draw_row_icon(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    rect: Rect,
    room: u16,
    icon: Option<&str>,
    ink: Color,
) -> Rect {
    // Looked up once, for its ink and its drawing.
    if let Some((font, ch)) = icon
        .filter(|_| room > 0)
        .and_then(|name| self::icon(env, name))
    {
        let size = style.text * ICON_SCALE;
        let mut text = [0u8; 4];
        let text = ch.encode_utf8(&mut text);
        // Centred by its ink in a column as wide as the icons are high:
        // icons drawn with more room on one side line up with the others.
        let column = f32::from(rect.x) + style.margin as f32;
        let x = font.ink_across(size, text).map_or(column, |(left, right)| {
            column + (size - (right - left) as f32) / 2.0 - left as f32
        });
        draw_text(canvas, &font, None, size, x, rect, text, ink);
    }
    Rect::new(
        rect.x + room,
        rect.y,
        rect.width.saturating_sub(room),
        rect.height,
    )
}

/// Draws a row with `title` and a switch for `on` at its end, with
/// `note` below when `noted` (see [`draw_noted_row`]).
#[allow(
    clippy::too_many_arguments,
    reason = "its inputs are separate things; a struct of them would serve this call alone"
)]
pub(crate) fn draw_switch_row(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    rect: Rect,
    focused: bool,
    title: &str,
    note: &str,
    on: bool,
) {
    let plain = Rect::new(
        rect.x,
        rect.y,
        rect.width,
        (style.control_row as u16).min(rect.height),
    );
    let beside = f32::from(switch_rect(style, plain).width);
    let band = if note.is_empty() {
        rect
    } else {
        // As high as the title takes beside the switch.
        super::setting_rows::titled_band(env, style, rect, beside, title)
    };
    let switch = switch_rect(style, band);
    // The title ends a gap before the switch.
    let room = (switch.x + style.margin as u16).saturating_sub(band.x + style.gap as u16);
    let title_rect = Rect::new(band.x, band.y, room, band.height);
    draw_value_row(canvas, env, style, title_rect, focused, title, "");
    if !note.is_empty() {
        let below = band.bottom() as u16;
        super::setting_rows::draw_note_below(canvas, env, style, rect, below, focused, note);
    }
    draw_switch(canvas, style, band, on, focused && style.dark_focus());
}

/// Whether icons can be drawn (an icon font is there).
pub fn has_icons(env: &Env<'_>) -> bool {
    env.fonts()
        .bound(crate::builtin::fonts::ICONS.as_str())
        .is_some()
}

/// The translation of `key` for how the device is used: where there is
/// no touch screen, `<key>.buttons` if there is one (e.g. "Select a rule
/// to pause it" for "Tap a rule to pause it"), else `key`'s own. Notes
/// that tell what a tap or a hold does use it, so they never speak of
/// taps on a device without a touch screen.
pub fn tr_input<'a>(env: &Env<'a>, key: &'a str) -> &'a str {
    if !env.device().input.touch
        && let Some(text) = env.i18n().get(&format!("{key}.buttons"))
    {
        return text;
    }
    env.i18n().tr(key)
}

/// Draws `text` vertically centred in `area`, starting at `x`; text
/// longer than the rest of the area is shortened with "…", so a label
/// never runs into what is next to it.
///
/// This draws the stock look: a theme's text painter
/// ([`Painters::text`](super::look::Painters::text)) does not see it. Use
/// [`draw_label_in`] where the text should follow the theme.
pub fn draw_label(
    canvas: &mut Canvas<'_>,
    font: &FontHandle<'_>,
    size: f32,
    x: f32,
    area: Rect,
    text: &str,
    color: Color,
) -> f32 {
    let room = f32::from(area.x) + f32::from(area.width) - x;
    let at = Rect::new(x.max(0.0) as u16, area.y, room.max(0.0) as u16, area.height);
    canvas.describe(Role::Text, text, at);
    draw_text(canvas, font, None, size, x, area, text, color)
}

/// [`draw_label`] for text that follows the look: `role` says what the
/// text is ([`TextRole::BUTTON`], [`TextRole::HINT`], [`TextRole::TITLE`]
/// …) and the look's text painter, if it has one and draws it, paints it
/// (its width stays the stock one). Same placing, shortening and result as
/// [`draw_label`].
#[allow(clippy::too_many_arguments, reason = "draw_label's, and the look")]
pub fn draw_label_in(
    canvas: &mut Canvas<'_>,
    font: &FontHandle<'_>,
    style: &UiStyle,
    role: TextRole,
    size: f32,
    x: f32,
    area: Rect,
    text: &str,
    color: Color,
) -> f32 {
    let room = f32::from(area.x) + f32::from(area.width) - x;
    let at = Rect::new(x.max(0.0) as u16, area.y, room.max(0.0) as u16, area.height);
    canvas.describe(Role::Text, text, at);
    draw_text(
        canvas,
        font,
        Some((style, role)),
        size,
        x,
        area,
        text,
        color,
    )
}

/// [`draw_label`] without telling what it draws (icons say nothing), and
/// by the look's text painter where it has `look` (style and role).
#[allow(clippy::too_many_arguments, reason = "one run of text")]
pub(super) fn draw_text(
    canvas: &mut Canvas<'_>,
    font: &FontHandle<'_>,
    look: Option<(&UiStyle, TextRole)>,
    size: f32,
    x: f32,
    area: Rect,
    text: &str,
    color: Color,
) -> f32 {
    let metrics = font.line_metrics(size);
    let baseline = i32::from(area.y)
        + ((f32::from(area.height) + metrics.ascent - metrics.descent) / 2.0) as i32;
    let room = f32::from(area.x) + f32::from(area.width) - x;
    // A pixel of slack for rounding: callers often measured to fit.
    if room > 0.0 && font.measure(size, text) > room + 1.0 {
        let text = fit(font, size, text, room);
        return put_run(canvas, font, look, size, x, baseline, area, &text, color);
    }
    put_run(canvas, font, look, size, x, baseline, area, text, color)
}

/// One run of text: by the look's text painter, if there is a `look`, it
/// has one and draws it, else as usual; its width.
#[allow(clippy::too_many_arguments, reason = "one run of text")]
pub(super) fn put_run(
    canvas: &mut Canvas<'_>,
    font: &FontHandle<'_>,
    look: Option<(&UiStyle, TextRole)>,
    size: f32,
    x: f32,
    baseline: i32,
    area: Rect,
    text: &str,
    color: Color,
) -> f32 {
    if let Some((style, role)) = look
        && style.look().painters().is_some()
    {
        let part = TextPart::new(text, x, baseline, size, color, area, font, role);
        if style.look().text(canvas, style, &part) {
            // Looks, not metrics: the width is the stock one.
            return font.measure(size, text);
        }
    }
    font.draw(canvas, size, x, baseline, text, color)
}

/// The baseline that centres the capitals of `size` in `band` (from the
/// baseline to the top of an *H*), rather than the line, which leaves
/// more room above the letters than below them, or the ink of the text
/// itself, which puts a word with a *g* higher than one without: what
/// headers use, so every title stands on one line, its capitals in the
/// middle as the arrows beside them are.
pub(super) fn cap_baseline(font: &FontHandle<'_>, band: Rect, size: f32) -> i32 {
    let (top, bottom) = font.ink(size, "H").unwrap_or_else(|| {
        // No ink at all: as a line would be.
        let metrics = font.line_metrics(size);
        (-(metrics.ascent as i32), -(metrics.descent as i32))
    });
    let room = i32::from(band.height) - (bottom - top);
    // Rounded down: an odd pixel goes below, where the eye expects it.
    i32::from(band.y) + room.div_euclid(2) - top
}

/// [`draw_label`] on `baseline` rather than centred in `area`, by the
/// look's text painter where it has `look` (style and role).
#[allow(
    clippy::too_many_arguments,
    reason = "its inputs are separate things; a struct of them would serve this call alone"
)]
pub(super) fn draw_label_on(
    canvas: &mut Canvas<'_>,
    font: &FontHandle<'_>,
    look: Option<(&UiStyle, TextRole)>,
    size: f32,
    x: f32,
    area: Rect,
    baseline: i32,
    text: &str,
    color: Color,
) -> f32 {
    let room = f32::from(area.x) + f32::from(area.width) - x;
    let at = Rect::new(x.max(0.0) as u16, area.y, room.max(0.0) as u16, area.height);
    canvas.describe(Role::Text, text, at);
    if room > 0.0 && font.measure(size, text) > room + 1.0 {
        let text = fit(font, size, text, room);
        return put_run(canvas, font, look, size, x, baseline, area, &text, color);
    }
    put_run(canvas, font, look, size, x, baseline, area, text, color)
}

/// A title and a value side by side in `width` pixels: both as they are
/// if they fit, else shortened with "…". The title says what the row is,
/// so it keeps up to 60 % of the width; the value gets the rest. Returns
/// the texts and the value's width.
pub fn fit_pair(
    font: &FontHandle<'_>,
    size: f32,
    title: &str,
    value: &str,
    width: f32,
    gap: f32,
) -> (String, String, f32) {
    let room = (width - gap).max(0.0);
    let title_width = font.measure(size, title).min(room * 0.6);
    let value_room = room - title_width;
    // A pixel of slack for rounding, as in [`draw_label`].
    let value = fit(font, size, value, value_room + 1.0);
    let value_width = font.measure(size, &value);
    let title = fit(font, size, title, (room - value_width).max(0.0) + 1.0);
    (title, value, value_width)
}

/// The height [`Ui::message`](super::Ui::message) needs for all of `text` in an area
/// `width` wide.
pub fn message_height(env: &Env<'_>, style: &UiStyle, width: u16, text: &str) -> u16 {
    let Some(font) = style.font(env) else {
        return 0;
    };
    let room = (f32::from(width) - 2.0 * style.margin as f32).max(1.0);
    let lines = font.wrap(style.text, text, room).len().max(1);
    (font.line_metrics(style.text).line_height * lines as f32) as u16 + 1
}

/// Draws wrapped, centred message text in `area` ([`message_height`]
/// high holds all of it).
pub(crate) fn draw_message(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    area: Rect,
    text: &str,
) {
    let Some(font) = style.font(env) else {
        return;
    };
    let width = f32::from(area.width) - 2.0 * style.margin as f32;
    let line_height = font.line_metrics(style.text).line_height;
    let mut lines = font.wrap(style.text, text, width);
    // As many lines as the area holds; the last one says there is more.
    let fits = ((f32::from(area.height) / line_height) as usize).max(1);
    let cut = lines.len() > fits;
    lines.truncate(fits);
    let total = line_height * lines.len() as f32;
    let mut baseline =
        f32::from(area.y) + (f32::from(area.height) - total).max(0.0) / 2.0 + line_height * 0.8;
    let count = lines.len();
    let mut last = String::new();
    for (n, range) in lines.iter().cloned().enumerate() {
        let line = if cut && n + 1 == count {
            let rest = text[range.start..].trim_end();
            last = fit(&font, style.text, &format!("{rest} …"), width);
            Cow::Borrowed(last.as_str())
        } else {
            Cow::Borrowed(&text[range])
        };
        let x = f32::from(area.x) + (f32::from(area.width) - font.measure(style.text, &line)) / 2.0;
        font.draw(canvas, style.text, x, baseline as i32, &line, Color::BLACK);
        baseline += line_height;
    }
    canvas.describe(Role::Text, &as_shown(text, &lines, cut, &last), area);
}

/// What a block of `text` wrapped on `lines` says as it is drawn: all of
/// it, or where its last line was cut (`last` as drawn), the lines shown,
/// so that what describes the screen (and paperpaw-dev) sees the "…".
fn as_shown<'a>(
    text: &'a str,
    lines: &[core::ops::Range<usize>],
    cut: bool,
    last: &str,
) -> Cow<'a, str> {
    match lines.last() {
        Some(range) if cut => Cow::Owned(alloc::format!("{}{last}", &text[..range.start])),
        _ => Cow::Borrowed(text),
    }
}

/// The height [`Ui::note_line`](super::Ui::note_line) needs for all of `text` in an area
/// `width` wide: its light line and the lines of the text.
pub fn note_line_height(env: &Env<'_>, style: &UiStyle, width: u16, text: &str) -> u16 {
    let Some(font) = style.font(env) else {
        return style.line as u16;
    };
    let room = f32::from(width.saturating_sub(2 * style.margin as u16)).max(1.0);
    let lines = font.wrap(style.small, text, room).len().max(1);
    let line = font.line_metrics(style.small).line_height;
    style.line as u16 + (line * lines as f32) as u16 + 1
}

/// A note at the bottom of a screen (what a tap or a hold does there, or
/// what just happened): a light line above it, then `text` in small gray
/// letters, wrapped on as many lines as `area` holds (the last one
/// shortened if need be; [`note_line_height`] holds all of it).
pub(crate) fn draw_note_line(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    area: Rect,
    text: &str,
) {
    let margin = style.margin as u16;
    let width = area.width.saturating_sub(2 * margin);
    if style.separators != Separators::None {
        canvas.fill_tint(
            Rect::new(area.x + margin, area.y, width, style.line as u16),
            style.ink(Ink::LINE),
        );
    }
    let Some(font) = style.font(env) else {
        return;
    };
    let top = area.y + style.line as u16;
    let height = area.height.saturating_sub(style.line as u16);
    let line = font.line_metrics(style.small).line_height;
    let room = f32::from(width);
    let mut lines = font.wrap(style.small, text, room);
    let fits = ((f32::from(height) / line) as usize).max(1);
    let cut = lines.len() > fits;
    lines.truncate(fits);
    let count = lines.len();
    let mut y = f32::from(top) + (f32::from(height) - line * count as f32).max(0.0) / 2.0;
    let block = Rect::new(
        area.x + margin,
        y as u16,
        width,
        (line * count as f32) as u16,
    );
    let mut last = String::new();
    for (n, range) in lines.iter().cloned().enumerate() {
        let shown = if cut && n + 1 == count {
            last = fit(
                &font,
                style.small,
                &format!("{} …", text[range.start..].trim_end()),
                room,
            );
            last.clone()
        } else {
            text[range].to_string()
        };
        let row = Rect::new(area.x + margin, y as u16, width, line as u16);
        draw_text(
            canvas,
            &font,
            Some((style, TextRole::SMALL)),
            style.small,
            f32::from(row.x),
            row,
            &shown,
            style.ink(Ink::NOTE),
        );
        y += line;
    }
    canvas.describe(Role::Text, &as_shown(text, &lines, cut, &last), block);
}

/// `text` shortened with an ellipsis to fit `width` pixels, its pairs
/// kerned as [`FontHandle::measure`] sets them (a pair kerned apart, `f\`
/// in the UI font, takes more than its advances).
pub fn fit(font: &FontHandle<'_>, size: f32, text: &str, width: f32) -> String {
    if font.measure(size, text) <= width {
        return text.to_string();
    }
    let ellipsis = font.measure(size, "…");
    let mut result = String::new();
    let mut used = 0.0;
    let mut before = None;
    for c in text.chars() {
        let w = before.map_or(0.0, |b| font.kern(size, b, c)) + font.advance(size, c);
        if used + w + font.kern(size, c, '…') + ellipsis > width {
            break;
        }
        used += w;
        result.push(c);
        before = Some(c);
    }
    result.push('…');
    result
}

/// How wide an empty state's words are in an area `width` wide: twice
/// the margin at the sides, less in a small area (a card on a page with
/// wide margins), so that the text keeps room for a few words a line; at
/// least half a gap, where letters reaching past their start (an accent)
/// stay inside.
fn empty_width(style: &UiStyle, width: u16) -> f32 {
    let least = style.text * 8.0;
    let pad = style.gap as f32 / 2.0;
    let margin = (style.margin as f32 * 2.0).min(((f32::from(width) - least) / 2.0).max(pad));
    (f32::from(width) - 2.0 * margin).max(1.0)
}

/// An empty state's `title` in words `width` wide: as large as fits on a
/// line, down to the text's size, else on more lines in that size; its
/// size and lines.
fn empty_title(
    font: &FontHandle<'_>,
    style: &UiStyle,
    width: f32,
    title: &str,
) -> (f32, Vec<core::ops::Range<usize>>) {
    let size = crate::widgets::largest_size(style.title, style.text, |size| {
        font.measure(size, title) <= width
    })
    .unwrap_or(style.text);
    let lines = if title.is_empty() {
        Vec::new()
    } else if font.measure(size, title) <= width {
        alloc::vec![0..title.len()]
    } else {
        font.wrap(size, title, width)
    };
    (size, lines)
}

/// The height of an empty screen's title of `count` lines at `size` (none:
/// nothing; else a line, and a heading's line for each of the others).
fn empty_title_height(font: &FontHandle<'_>, size: f32, count: usize) -> f32 {
    if count == 0 {
        return 0.0;
    }
    let line = font.line_metrics(size).line_height;
    line + font.heading_line_height(size) * count.saturating_sub(1) as f32
}

/// The height [`Ui::empty`](super::Ui::empty) needs in an area `width` wide for all of
/// `title` and `text` in their full sizes, without its icon (which it
/// shows where there is room for it as well).
pub fn empty_height(env: &Env<'_>, style: &UiStyle, width: u16, title: &str, text: &str) -> u16 {
    let Some(font) = style.font(env) else {
        return 0;
    };
    let width = empty_width(style, width);
    let (size, titles) = empty_title(&font, style, width, title);
    let words = empty_title_height(&font, size, titles.len());
    let lines = if text.is_empty() {
        0
    } else {
        font.wrap(style.text, text, width).len()
    };
    let text = font.line_metrics(style.text).line_height * lines as f32;
    let gap = if lines > 0 { style.gap as f32 } else { 0.0 };
    (words + gap + text) as u16 + 1
}

/// An empty screen or list, saying what would be here and how it gets
/// filled: a large `icon` in gray, a short `title`, and `text` below it,
/// together a little above the middle of `area`. The icon is left out
/// where there is not room for all of it, the text shortened last.
#[allow(
    clippy::too_many_arguments,
    reason = "its inputs are separate things; a struct of them would serve this call alone"
)]
#[expect(
    clippy::too_many_lines,
    reason = "longer than the limit from before it was set; split it when it is next changed"
)]
pub(crate) fn draw_empty(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    area: Rect,
    icon: &str,
    title: &str,
    text: &str,
) {
    let Some(font) = style.font(env) else {
        return;
    };
    let width = empty_width(style, area.width);
    let icon_size = style.title * 2.4;
    let gap = style.gap as f32;
    let (title_size, mut titles) = empty_title(&font, style, width, title);
    let room = f32::from(area.height);
    // Its lines where the area holds them, else one line, shortened.
    if empty_title_height(&font, title_size, titles.len()) > room {
        titles = alloc::vec![0..title.len()];
    }
    let words = empty_title_height(&font, title_size, titles.len());
    // From one of its lines to the next: a heading's line.
    let title_line = if titles.len() > 1 {
        font.heading_line_height(title_size)
    } else {
        font.line_metrics(title_size).line_height
    };
    // The text as large as all its lines fit below the title, down to
    // the small size; only then its last line shortened.
    let below = room - words - gap;
    let text_size = crate::widgets::largest_size(style.text, style.small, |size| {
        let lines = font.wrap(size, text, width).len() as f32;
        lines * font.line_metrics(size).line_height <= below
    })
    .unwrap_or(style.small);
    let line = font.line_metrics(text_size).line_height;
    let mut lines = if text.is_empty() {
        Vec::new()
    } else {
        font.wrap(text_size, text, width)
    };
    let text_height = |n: usize| line * n as f32;
    let icon_w = icon_width(env, icon, icon_size);
    let with_icon = icon_w > 0.0
        && icon_w <= f32::from(area.width)
        && icon_size + gap * 2.0 + words + gap + text_height(lines.len()) <= room;
    let icon_room = if with_icon {
        icon_size + gap * 2.0
    } else {
        0.0
    };
    // The text whole, or not at all where it does not fit (a small
    // widget's cell): the title says enough there, and no explanation
    // shows cut.
    let fits = ((room - icon_room - words - gap) / line).max(0.0) as usize;
    if lines.len() > fits {
        lines.clear();
    }
    let total =
        icon_room + words + if lines.is_empty() { 0.0 } else { gap } + text_height(lines.len());
    // A little above the middle, where the eye looks first.
    let mut y = f32::from(area.y) + ((room - total) * 0.42).max(0.0);
    let centre = |w: f32| f32::from(area.x) + (f32::from(area.width) - w) / 2.0;
    if with_icon {
        let w = icon_w;
        let at = Rect::new(area.x, y as u16, area.width, icon_size as u16);
        draw_icon(
            canvas,
            env,
            icon,
            icon_size,
            centre(w),
            at,
            style.ink(Ink::FAINT),
        );
        y += icon_room;
    }
    // Each line in a row of its own height, a heading's line apart; the
    // text below after all of them.
    let below_title = y + words;
    let row = font.line_metrics(title_size).line_height;
    for range in &titles {
        let shown = fit(&font, title_size, title[range.clone()].trim(), width);
        let at = Rect::new(area.x, y as u16, area.width, row as u16);
        draw_label(
            canvas,
            &font,
            title_size,
            centre(font.measure(title_size, &shown)),
            at,
            &shown,
            Color::BLACK,
        );
        y += title_line;
    }
    y = y.max(below_title);
    if !titles.is_empty() {
        y += gap;
    }
    if lines.is_empty() {
        return;
    }
    let block = Rect::new(
        area.x,
        y as u16,
        area.width,
        text_height(lines.len()) as u16,
    );
    let ascent = font.line_metrics(text_size).ascent;
    for range in &lines {
        let shown = text[range.clone()].trim();
        let x = centre(font.measure(text_size, shown));
        font.draw(
            canvas,
            text_size,
            x,
            (y + ascent) as i32,
            shown,
            style.ink(Ink::NOTE),
        );
        y += line;
    }
    canvas.describe(Role::Text, text, block);
}
