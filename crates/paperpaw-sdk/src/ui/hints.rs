//! The row of hints below the screen: what the buttons under it do.

#[allow(
    clippy::wildcard_imports,
    reason = "a part of the module, which shares its imports"
)]
use super::*;

/// The height of the row of hints below the screen (see
/// [`draw_button_hints`]).
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub fn button_hints_height(env: &Env<'_>) -> u16 {
    hints_height(env)
}

/// [`button_hints_height`], for the SDK's own parts (a dialog keeps
/// above the row).
pub(crate) fn hints_height(env: &Env<'_>) -> u16 {
    let style = UiStyle::new(env);
    let line = style.font(env).map_or(style.small * 1.3, |f| {
        f.line_metrics(style.small).line_height
    });
    ((line * 1.5) as u32 + style.line).max(env.metrics().mm(4)) as u16
}

/// A line of a hint's words as it is drawn: its size, its words, where
/// it starts and the line it lies in.
struct Placed {
    size: f32,
    text: String,
    x: f32,
    at: Rect,
}

/// Where a hint's words go below its button, in `cell` (its share of the
/// row), within `span` (its share and the room beside it it may take):
/// too long for `room` (its share less a gap at each side), over the room
/// beside it, then smaller, down to six tenths, then on two lines; only
/// then shortened. Always as near its button (`middle`) as it can be.
fn place_hint_text(
    font: &crate::fonts::FontHandle<'_>,
    style: &UiStyle,
    (cell, span, middle): (Rect, Rect, f32),
    room: f32,
    text: &str,
) -> Vec<Placed> {
    let near = |w: f32| {
        let least = f32::from(span.x);
        let most = (f32::from(span.x) + f32::from(span.width) - w).max(least);
        (middle - w / 2.0).clamp(least, most)
    };
    let placed = |size: f32, text: &str, at: Rect| Placed {
        size,
        x: near(font.measure(size, text)),
        text: String::from(text),
        at,
    };
    let width = font.measure(style.small, text);
    if width <= room + 1.0 || width <= f32::from(span.width) {
        return alloc::vec![placed(style.small, text, span)];
    }
    let room = f32::from(span.width);
    let least = style.small * 0.6;
    let size =
        crate::widgets::largest_size(style.small, least, |size| font.measure(size, text) <= room)
            .unwrap_or(least);
    // Two lines set close: the row is a line and a half high.
    let metrics = font.line_metrics(size);
    let line = (metrics.ascent + metrics.descent).min(metrics.line_height);
    let lines = font.wrap(size, text, room.max(1.0));
    if lines.len() == 2 && 2.0 * line <= f32::from(cell.height) {
        let top = f32::from(cell.y) + (f32::from(cell.height) - 2.0 * line) / 2.0;
        return lines
            .into_iter()
            .enumerate()
            .map(|(n, range)| {
                let y = top + line * n as f32;
                let at = Rect::new(span.x, y as u16, span.width, line as u16);
                placed(size, text[range].trim(), at)
            })
            .collect();
    }
    alloc::vec![placed(size, &fit(font, size, text, room), span)]
}

/// What the row's hints take of it, each from its start to its end and
/// whether that is all it may use (none for those that show nothing):
/// their own ink where it fits in their share less a gap at each side;
/// else their share and the shares beside it that show nothing, which
/// they may use.
fn reaches(
    env: &Env<'_>,
    style: &UiStyle,
    area: Rect,
    hints: &[Hint],
) -> Vec<Option<(u32, u32, bool)>> {
    let count = hints.len() as u32;
    let font = style.font(env);
    let share = |i: usize| {
        let width = u32::from(area.width);
        let left = u32::from(area.x) + width * i as u32 / count;
        (left, u32::from(area.x) + width * (i as u32 + 1) / count)
    };
    let empty = |j: usize| {
        matches!(
            hints.get(j),
            Some(Hint::Default | Hint::Hidden | Hint::Other(_))
        )
    };
    hints
        .iter()
        .enumerate()
        .map(|(i, hint)| {
            let (left, right) = share(i);
            let room = (right - left).saturating_sub(2 * style.gap) as f32;
            let middle = (left + right) as f32 / 2.0;
            let ink = match hint {
                Hint::Text(text) => font.as_ref()?.measure(style.small, text),
                Hint::Icon(name) => icon_width(env, name, style.text),
                Hint::Default | Hint::Hidden | Hint::Other(_) => return None,
            };
            if ink <= room + 1.0 || matches!(hint, Hint::Icon(_)) {
                return Some((
                    (middle - ink / 2.0) as u32,
                    (middle + ink / 2.0) as u32,
                    false,
                ));
            }
            let mut from = i;
            while from > 0 && empty(from - 1) {
                from -= 1;
            }
            let mut to = i;
            while empty(to + 1) {
                to += 1;
            }
            Some((share(from).0, share(to).1, true))
        })
        .collect()
}

/// From where to where hint `index` may show its words: its share, and
/// beside it the room the hints beside it leave: a gap before their ink,
/// half a gap before what they may take (see [`reaches`]); within the
/// row, half a gap from its ends.
fn free_span(
    reach: &[Option<(u32, u32, bool)>],
    index: usize,
    area: Rect,
    style: &UiStyle,
) -> (u32, u32) {
    let gap = style.gap;
    let apart = |taken: bool| if taken { gap / 2 } else { gap };
    let start = reach[..index]
        .iter()
        .rev()
        .find_map(|r| r.map(|(_, end, taken)| end + apart(taken)))
        .unwrap_or(u32::from(area.x) + gap / 2);
    let end = reach[index + 1..]
        .iter()
        .find_map(|r| r.map(|(start, _, taken)| start.saturating_sub(apart(taken))))
        .unwrap_or(u32::from(area.x) + u32::from(area.width) - gap / 2);
    (start, end.max(start))
}

/// The row of hints below the screen: a line above it, then what each
/// button under it does, centred in an equal share of the width, as the
/// buttons lie. The hint of the button `pressed` is drawn inverted, as a
/// sign that its press is being worked on.
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub fn draw_button_hints(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    area: Rect,
    hints: &[Hint],
    pressed: Option<usize>,
) {
    let style = UiStyle::new(env);
    let line = style.line.min(u32::from(area.height));
    let painted = style
        .look
        .hints(canvas, &style, super::look::HintRowPart { area });
    if !painted {
        canvas.fill(area, Color::WHITE);
    }
    match style.separators {
        _ if painted => {}
        Separators::Line => canvas.fill_rect(
            i32::from(area.x),
            i32::from(area.y),
            u32::from(area.width),
            line,
            Color::BLACK,
        ),
        Separators::Inset => canvas.fill_tint(
            Rect::new(
                area.x + style.margin as u16,
                area.y,
                area.width.saturating_sub(2 * style.margin as u16),
                line as u16,
            ),
            style.ink(Ink::Rule),
        ),
        Separators::None => {}
    }
    let Some(font) = style.font(env) else {
        return;
    };
    let count = hints.len() as u32;
    let row = Rect::new(
        area.x,
        area.y + line as u16,
        area.width,
        area.height - line as u16,
    );
    let reach = reaches(env, &style, area, hints);
    for (i, hint) in hints.iter().enumerate() {
        let (i, width) = (i as u32, u32::from(area.width));
        let left = u32::from(area.x) + width * i / count;
        let right = u32::from(area.x) + width * (i + 1) / count;
        let cell = Rect::new(left as u16, row.y, (right - left) as u16, row.height);
        let room = (right - left).saturating_sub(2 * style.gap) as f32;
        let middle = left as f32 + (right - left) as f32 / 2.0;
        let is_pressed = pressed == Some(i as usize);
        let ink = if is_pressed {
            Color::WHITE
        } else {
            Color::BLACK
        };
        let mut boxed = cell.inset(style.gap as u16 / 2, style.gap as u16 / 4);
        // The label alone, in the middle: no mark beside it.
        match hint {
            Hint::Text(text) => {
                let (from, to) = free_span(&reach, i as usize, area, &style);
                let span = Rect::new(from as u16, row.y, (to - from) as u16, row.height);
                let placed = place_hint_text(&font, &style, (cell, span, middle), room, text);
                if is_pressed {
                    // The box round all of its words, a little room
                    // round them.
                    let pad = style.gap as f32 / 4.0;
                    for p in &placed {
                        let end = p.x + font.measure(p.size, &p.text) + pad;
                        let start = (p.x - pad).max(0.0) as u16;
                        let x = boxed.x.min(start);
                        let right = (u32::from(boxed.x) + u32::from(boxed.width)).max(end as u32);
                        boxed = Rect::new(x, boxed.y, (right - u32::from(x)) as u16, boxed.height);
                    }
                    let radius = style.radius(u32::from(boxed.height));
                    canvas.fill_round(boxed, radius, Color::BLACK);
                }
                for p in placed {
                    draw_label(canvas, &font, p.size, p.x, p.at, &p.text, ink);
                }
            }
            Hint::Icon(name) => {
                if is_pressed {
                    let radius = style.radius(u32::from(boxed.height));
                    canvas.fill_round(boxed, radius, Color::BLACK);
                }
                let width = icon_width(env, name, style.text);
                draw_icon(
                    canvas,
                    env,
                    name,
                    style.text,
                    middle - width / 2.0,
                    cell,
                    ink,
                );
            }
            Hint::Default | Hint::Hidden | Hint::Other(_) if is_pressed => {
                let radius = style.radius(u32::from(boxed.height));
                canvas.fill_round(boxed, radius, Color::BLACK);
            }
            Hint::Default | Hint::Hidden | Hint::Other(_) => {}
        }
    }
}
