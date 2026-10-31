//! Buttons drawn in rows and dialogs.

#[allow(
    clippy::wildcard_imports,
    reason = "a part of the module, which shares its imports"
)]
use super::*;

/// The height of buttons in a row of height `row.height`: the `−` and `+`
/// of numbers, the buttons of panels. One size everywhere: seven tenths
/// of the row, and on touch screens at least as large as a fingertip
/// ([`UiStyle::finger`]), no higher than the row (rows made with
/// [`UiStyle::touch_row`] keep room around them).
pub fn button_height(style: &UiStyle, row: Rect) -> u16 {
    // Rows grown for two lines of a label do not make buttons larger.
    let height = u32::from(row.height).min(style.touch_row(style.control_row));
    (height * 7 / 10).max(style.finger).min(height) as u16
}

/// The height of a bar of buttons along the top or bottom edge of the
/// screen: its buttons as in a row made with [`UiStyle::touch_row`], the
/// room of such a row toward the content, and toward the edge the margin
/// the screen's sides have, so the buttons keep it from every edge.
pub fn edge_bar_height(style: &UiStyle) -> u16 {
    let row = style.touch_row(style.control_row) as u16;
    let button = button_height(style, Rect::new(0, 0, 0, row));
    let room = (row - button) / 2;
    button + room + style.margin as u16
}

/// Where the buttons of `bar`, a bar along the screen's bottom edge (or
/// its top, `at_top`) made [`edge_bar_height`] high, go: a band as high as
/// they are, the side margin from the edge (see [`edge_bar_height`]).
pub fn edge_bar_band(style: &UiStyle, bar: Rect, at_top: bool) -> Rect {
    let row = style.touch_row(style.control_row) as u16;
    let button = button_height(style, Rect::new(0, 0, 0, row));
    let margin = style.margin as u16;
    let y = if at_top {
        bar.y + margin
    } else {
        (bar.y + bar.height).saturating_sub(margin + button)
    };
    Rect::new(bar.x, y, bar.width, button)
}

/// The room to leave between a row of buttons `row` high and the edge of
/// the screen it lies along, so that its buttons, centred in it, keep the
/// side margin from that edge as well.
pub fn edge_room(style: &UiStyle, row: u16) -> u16 {
    let button = button_height(style, Rect::new(0, 0, 0, row));
    (style.margin as u16).saturating_sub((row - button) / 2)
}

/// The width of buttons' and boxes' borders.
pub fn button_border(style: &UiStyle) -> u32 {
    style.line.max(2)
}

/// Where a button at `rect` draws: a focus goes round it, a little
/// outside. A screen moving a focus between buttons asks for these drawn
/// again (see [`ListChange::focus`](super::ListChange::focus)).
pub fn button_reach(style: &UiStyle, rect: Rect) -> Rect {
    rect.grow((button_border(style) * 3) as u16)
}

/// A button: outlined, or filled when `on`; `focused` (chosen with
/// buttons) draws a frame around it. Labels of one character (e.g. `+`)
/// are set larger.
#[allow(
    clippy::too_many_arguments,
    reason = "its inputs are separate things; a struct of them would serve this call alone"
)]
pub(crate) fn draw_button(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    rect: Rect,
    label: &str,
    on: bool,
    focused: bool,
) {
    draw_button_said(canvas, env, style, rect, label, on, on, focused);
}

/// [`draw_button`], filled where `filled`, said to be on where `on`: a
/// key filled to show the focus is not on.
#[expect(
    clippy::too_many_arguments,
    reason = "its inputs are separate things; a struct of them would serve this call alone"
)]
pub(crate) fn draw_button_said(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    rect: Rect,
    label: &str,
    filled: bool,
    on: bool,
    focused: bool,
) {
    canvas
        .describe(Role::Button, label, rect)
        .focused(focused)
        .on(on.then_some(true));
    let on = filled;
    let Some(ink) = button_frame(canvas, style, rect, on, focused) else {
        return;
    };
    let Some(font) = style.font(env) else {
        return;
    };
    button_label(canvas, style, &font, rect, label, ink);
}

/// A button that cannot be used now (e.g. a menu entry shown faint): its
/// border and label faint; `focused` as [`draw_button`].
pub(crate) fn draw_faint_button(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    rect: Rect,
    label: &str,
    focused: bool,
) {
    canvas
        .describe(Role::Button, label, rect)
        .focused(focused)
        .disabled(true);
    let faint = style.faint(Ink::Note);
    framed(canvas, style, rect, (Color::WHITE, faint), focused);
    let Some(font) = style.font(env) else {
        return;
    };
    button_label(canvas, style, &font, rect, label, faint);
}

/// A button with a drawing of its own instead of a word (an arrow, a
/// sign), drawn empty for the caller to draw on with the ink it returns;
/// `name` says what it does, for screen readers and remote control.
pub(crate) fn draw_icon_button(
    canvas: &mut Canvas<'_>,
    style: &UiStyle,
    rect: Rect,
    name: &str,
    on: bool,
    focused: bool,
) -> Color {
    draw_icon_button_said(canvas, style, rect, name, on, on, focused)
}

/// [`draw_icon_button`], filled where `filled`, said to be on where `on`.
pub(crate) fn draw_icon_button_said(
    canvas: &mut Canvas<'_>,
    style: &UiStyle,
    rect: Rect,
    name: &str,
    filled: bool,
    on: bool,
    focused: bool,
) -> Color {
    canvas
        .describe(Role::Button, name, rect)
        .focused(focused)
        .on(on.then_some(true));
    button_frame(canvas, style, rect, filled, focused).unwrap_or(Color::BLACK)
}

/// A button's background, border and focus frame; the ink for what goes
/// on it.
pub(super) fn button_frame(
    canvas: &mut Canvas<'_>,
    style: &UiStyle,
    rect: Rect,
    on: bool,
    focused: bool,
) -> Option<Color> {
    let part = super::look::ButtonPart { rect, on, focused };
    if let Some(ink) = style.look.button(canvas, style, part) {
        return Some(ink);
    }
    let (back, ink) = if on {
        (Color::BLACK, Color::WHITE)
    } else {
        (Color::WHITE, Color::BLACK)
    };
    framed(canvas, style, rect, (back, Color::BLACK), focused);
    Some(ink)
}

/// A button's background and border in `colors`, and its focus frame.
fn framed(
    canvas: &mut Canvas<'_>,
    style: &UiStyle,
    rect: Rect,
    (back, line): (Color, Color),
    focused: bool,
) {
    let border = button_border(style);
    let r = style.radius(u32::from(rect.height.min(rect.width)));
    let (x, y, w, h) = (
        i32::from(rect.x),
        i32::from(rect.y),
        u32::from(rect.width),
        u32::from(rect.height),
    );
    if r > 0 {
        // Round: the corners outside keep what is behind.
        canvas.fill_round_rect(x, y, w, h, r, back);
    } else {
        canvas.fill(rect, back);
    }
    canvas.stroke_round_rect(x, y, w, h, border, r, line);
    if focused {
        let out = (border * 2) as i32;
        canvas.stroke_round_rect(
            x - out,
            y - out,
            w + 2 * out as u32,
            h + 2 * out as u32,
            border,
            r + out as u32,
            Color::BLACK,
        );
    }
}

/// The room a button `width` wide keeps at each side of its word: a gap;
/// narrow buttons (a keyboard's keys) keep less, so short words fit.
pub(super) fn label_pad(style: &UiStyle, width: f32) -> f32 {
    (style.gap as f32).min(width / 12.0)
}

/// How wide a button must be for its word, `label` wide, not to be
/// shortened ([`Ui::button`](super::Ui::button) keeps room at its sides): for laying out
/// buttons as wide as their words.
pub fn button_width(style: &UiStyle, label: f32) -> f32 {
    // A gap at each side, or a twelfth of the width where that is less.
    let gap = style.gap as f32;
    if label >= 10.0 * gap {
        label + 2.0 * gap
    } else {
        label * 1.2
    }
}

/// The lines of a button's `label` in a button `width` wide: one where it
/// fits there (a single character always), else its words wrapped, in the
/// small size; and a line's height.
fn label_lines(
    font: &FontHandle<'_>,
    style: &UiStyle,
    width: f32,
    label: &str,
) -> (Vec<core::ops::Range<usize>>, u16) {
    let line = font.line_metrics(style.small).line_height as u16;
    let room = width - 2.0 * label_pad(style, width);
    if label.chars().count() <= 1 || font.measure(style.small, label) <= room + 1.0 {
        return (alloc::vec![0..label.len()], line);
    }
    (font.wrap(style.small, label, room.max(1.0)), line)
}

/// The height of a button `width` wide for `label`, its word whole: a
/// button's in a row that can be touched ([`Segments::height`]), a line
/// higher for each more line a word too long for one takes.
pub fn button_height_for(env: &Env<'_>, style: &UiStyle, width: u16, label: &str) -> u16 {
    let base = Segments::height(style);
    let Some(font) = style.font(env) else {
        return base;
    };
    let (lines, line) = label_lines(&font, style, f32::from(width), label);
    base + line * (lines.len().max(1) as u16 - 1)
}

/// The height of a button's slot in a column of buttons `width` wide, one
/// below the other: a row that can be touched ([`UiStyle::touch_row`]),
/// higher only by the lines more a long `label` takes
/// ([`button_height_for`]). [`button_in_slot`] places the button in it.
pub fn button_slot_height(env: &Env<'_>, style: &UiStyle, width: u16, label: &str) -> u16 {
    let row = style.touch_row(style.control_row) as u16;
    row + button_height_for(env, style, width, label).saturating_sub(Segments::height(style))
}

/// The button in `slot` ([`button_slot_height`] high): as high as a
/// button in a row that can be touched, and as much higher as the slot
/// is, centred in the row's room.
pub fn button_in_slot(style: &UiStyle, slot: Rect) -> Rect {
    let row = (style.touch_row(style.control_row) as u16).min(slot.height);
    let base = Segments::height(style).min(row);
    let extra = slot.height - row;
    Rect::new(slot.x, slot.y + (row - base) / 2, slot.width, base + extra)
}

/// A button's word, centred: on more lines where it is too long for one
/// and the button is high enough ([`button_height_for`]), else shortened
/// to fit.
pub(super) fn button_label(
    canvas: &mut Canvas<'_>,
    style: &UiStyle,
    font: &FontHandle<'_>,
    rect: Rect,
    label: &str,
    ink: Color,
) {
    let single = label.chars().count() == 1;
    let mut size = if single { style.text } else { style.small };
    // A single character wider than the button (an `m` on a narrow key)
    // is set smaller rather than shortened to nothing.
    if single {
        let width = font.measure(size, label);
        let room = f32::from(rect.width) * 0.8;
        if width > room && width > 0.0 {
            size *= room / width;
        }
    }
    // A single character (− or +) is never shortened.
    let pad = label_pad(style, f32::from(rect.width));
    let room = if single {
        f32::from(rect.width)
    } else {
        f32::from(rect.width) - 2.0 * pad
    };
    if !single {
        let (lines, line) = label_lines(font, style, f32::from(rect.width), label);
        let height = line * lines.len() as u16;
        if lines.len() > 1 && height <= rect.height {
            let mut y = rect.y + (rect.height - height) / 2;
            for range in lines {
                let part = label[range].trim();
                let x =
                    f32::from(rect.x) + (f32::from(rect.width) - font.measure(size, part)) / 2.0;
                draw_label_in(
                    canvas,
                    font,
                    style,
                    TextRole::BUTTON,
                    size,
                    x,
                    Rect::new(rect.x, y, rect.width, line),
                    part,
                    ink,
                );
                y += line;
            }
            return;
        }
    }
    let label = fit(font, size, label, room);
    let label_x = f32::from(rect.x) + (f32::from(rect.width) - font.measure(size, &label)) / 2.0;
    draw_label_in(
        canvas,
        font,
        style,
        TextRole::BUTTON,
        size,
        label_x,
        rect,
        &label,
        ink,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A button as wide as `button_width` says keeps the room its word
    /// needs, at every gap and for words short and long.
    #[test]
    fn a_button_as_wide_as_its_word_needs_keeps_it_whole() {
        for gap in [0, 8, 19, 30] {
            let style = UiStyle {
                gap,
                ..UiStyle::PLAIN
            };
            for label in [0.0f32, 5.0, 40.0, 190.0, 300.0, 600.0] {
                let width = button_width(&style, label);
                let room = width - 2.0 * label_pad(&style, width);
                assert!(room + 0.01 >= label, "gap {gap}, {label}: {room}");
                // Not much wider than that.
                assert!(width <= label * 1.2 + 2.0 * gap as f32 + 0.01);
            }
        }
    }
}
