//! What the lock screen draws, as prepared operations: the clock (unlocking) or
//! the header (setting up), what is asked for, the dots of a PIN and a message,
//! and the keypad, the pattern or the password's button. The geometry is
//! `native_pad`; the steps are `native_flow`; the kinds list is the kit's.
use alloc::{string::String, vec::Vec};

use paperpaw_sdk::{
    Color, Point, Rect,
    native::{Error, Host, fonts::Font, formatting::Arg, snapshot::Snapshot, ui::RequestId},
};
use paperpaw_ui_kit::{
    ItemList, Messages, Page, Paint, Style, geometry, header,
    mem::push,
    text::{self, label},
};

use crate::native_flow::{Flow, Message, Step};
use crate::native_model::{DOTS, KEYS, MIN_LENGTH};
use crate::native_pad::{Layout, Room};

/// The key that deletes, among [`KEYS`].
const DELETE: &str = "delete";
const OK: &str = "OK";

/// One lock screen: where it is, the kinds it lists, the question of the
/// password while it is open, and where things were drawn last (for touches).
#[derive(Debug)]
pub struct View {
    pub flow: Flow,
    pub list: ItemList,
    /// The password's question, while the keyboard is open.
    pub asked: Option<RequestId>,
    /// The geometry of the last frame.
    pub drawn: Option<Drawn>,
}

impl View {
    #[must_use]
    pub fn new(flow: Flow) -> Self {
        Self {
            flow,
            list: ItemList::default(),
            asked: None,
            drawn: None,
        }
    }
}

/// Where the last frame put things.
#[derive(Debug, Clone)]
pub struct Drawn {
    pub layout: Layout,
    /// The back arrow's region (setting up, touch screens).
    pub back: Option<Rect>,
}

/// The clock, as the user wants it, and today in words.
#[derive(Debug, Default)]
pub struct Clock {
    pub time: String,
    pub date: String,
}

/// A filled disc.
fn disc(paint: &mut Vec<Paint>, at: Point, r: i32, color: Color) -> Result<(), Error> {
    let r = r.max(0);
    push(
        paint,
        Paint::Focus(
            Rect::new(
                geometry::clamp_u16(i64::from(at.x - r)),
                geometry::clamp_u16(i64::from(at.y - r)),
                geometry::clamp_u16(i64::from(r) * 2),
                geometry::clamp_u16(i64::from(r) * 2),
            ),
            u32::try_from(r).unwrap_or(0),
            None,
            color,
        ),
    )
}

/// A circle's outline.
fn ring(paint: &mut Vec<Paint>, at: Point, r: i32, width: u32) -> Result<(), Error> {
    let r = r.max(0);
    push(
        paint,
        Paint::Focus(
            Rect::new(
                geometry::clamp_u16(i64::from(at.x - r)),
                geometry::clamp_u16(i64::from(at.y - r)),
                geometry::clamp_u16(i64::from(r) * 2),
                geometry::clamp_u16(i64::from(r) * 2),
            ),
            u32::try_from(r).unwrap_or(0),
            Some(width),
            Color::BLACK,
        ),
    )
}

fn line(
    paint: &mut Vec<Paint>,
    from: Point,
    to: Point,
    width: u32,
    color: Color,
) -> Result<(), Error> {
    push(
        paint,
        Paint::Line {
            from,
            to,
            width,
            color,
        },
    )
}

/// The size of the clock's digits.
#[must_use]
pub fn clock_size(style: &Style) -> f32 {
    style.title * 2.6
}

/// The room the layout is made in.
///
/// # Errors
/// A refused font reply.
pub fn room(
    font: &Font<'_>,
    style: &Style,
    display: &Snapshot,
    setting_up: bool,
) -> Result<Room, Error> {
    Ok(Room {
        width: display.display_width(),
        height: display.display_height(),
        dpi: display.display_dpi(),
        margin: style.margin,
        gap: style.gap,
        row: style.row,
        header: setting_up.then_some(style.header),
        clock_lines: (
            geometry::line_px(font, clock_size(style))?,
            geometry::line_px(font, style.small)?,
        ),
        button: style.row,
        touch: display.has_touch(),
    })
}

/// A button: outlined, or filled when the focus is on it.
fn button(
    paint: &mut Vec<Paint>,
    font: &Font<'_>,
    style: &Style,
    area: Rect,
    text: &str,
    focused: bool,
) -> Result<(), Error> {
    let (ink, on) = if focused {
        (style.ink, style.paper)
    } else {
        (style.ink, style.ink)
    };
    push(
        paint,
        Paint::Focus(
            area,
            style.corner,
            if focused {
                None
            } else {
                Some(u32::from(style.line).max(1))
            },
            ink,
        ),
    )?;
    let wide = font.measure_text(style.text, text)?;
    let x = f32::from(area.x) + (f32::from(area.width) - wide).max(0.0) / 2.0;
    label(paint, font, text, style.text, x, area, on, None)
}

/// The dots that stand for the digits of a PIN, and the keys below them.
fn pin(
    paint: &mut Vec<Paint>,
    font: &Font<'_>,
    style: &Style,
    layout: &Layout,
    flow: &Flow,
    shown: bool,
) -> Result<(), Error> {
    // A dot for every digit, rings for the least still to come.
    let r = i32::from(layout.dots.height) / 7;
    let count = flow.pin_len().max(MIN_LENGTH);
    let gap = r * 4;
    let count_i = i32::try_from(count).unwrap_or(0);
    let mut x = i32::from(layout.dots.width) / 2 - (count_i - 1) * gap / 2;
    let y = i32::from(layout.dots.y) + i32::from(layout.dots.height) / 2;
    for i in 0..count {
        if i < flow.pin_len() {
            disc(paint, Point::new(x, y), r, Color::BLACK)?;
        } else {
            ring(paint, Point::new(x, y), r, u32::from(style.line).max(2))?;
        }
        x += gap;
    }
    for (i, key) in KEYS.iter().enumerate() {
        let rect = layout.keys[i];
        let focused = shown && flow.focus == i;
        let centre = Point::new(
            i32::from(rect.x) + i32::from(rect.width) / 2,
            i32::from(rect.y) + i32::from(rect.height) / 2,
        );
        let radius = i32::from(rect.width) / 2;
        // Digits in a thin ring; delete and OK as plain signs. Chosen with
        // the buttons, a key is filled.
        let digit = *key != DELETE && *key != OK;
        let ink = if focused {
            disc(paint, centre, radius, Color::BLACK)?;
            Color::WHITE
        } else {
            if digit {
                ring(paint, centre, radius, u32::from(style.line).max(1))?;
            }
            Color::BLACK
        };
        if digit {
            let size = f32::from(rect.height) * 0.42;
            let x =
                f32::from(rect.x) + (f32::from(rect.width) - font.measure_text(size, key)?) / 2.0;
            label(paint, font, key, size, x, rect, ink, None)?;
        } else {
            sign(
                paint,
                style,
                centre,
                i32::from(rect.height),
                *key == DELETE,
                ink,
            )?;
        }
    }
    Ok(())
}

/// The sign of the delete key (an arrow to the left) or of *OK* (a check
/// mark), drawn with lines at `centre`, `size` pixels across the key.
fn sign(
    paint: &mut Vec<Paint>,
    style: &Style,
    centre: Point,
    size: i32,
    delete: bool,
    ink: Color,
) -> Result<(), Error> {
    let reach = size * 4 / 20;
    let width = u32::from(style.line).max(2);
    let (x, y) = (centre.x, centre.y);
    if delete {
        let tail = Point::new(x + reach, y);
        let head = Point::new(x - reach, y);
        line(paint, tail, head, width, ink)?;
        line(
            paint,
            head,
            Point::new(x - reach / 2, y - reach / 2),
            width,
            ink,
        )?;
        line(
            paint,
            head,
            Point::new(x - reach / 2, y + reach / 2),
            width,
            ink,
        )
    } else {
        let bend = Point::new(x - reach / 3, y + reach / 2);
        line(paint, Point::new(x - reach, y), bend, width, ink)?;
        line(
            paint,
            bend,
            Point::new(x + reach, y - reach / 2),
            width,
            ink,
        )
    }
}

/// The pattern: the lines between the dots drawn, the one to the finger, and
/// the nine dots.
fn pattern(
    paint: &mut Vec<Paint>,
    font: &Font<'_>,
    style: &Style,
    layout: &Layout,
    flow: &Flow,
    touch: bool,
    ok: &str,
) -> Result<(), Error> {
    let shown = flow.shown;
    let width = u32::try_from((layout.radius / 2).max(2)).unwrap_or(2);
    let mut chosen = [false; DOTS];
    let mut last: Option<usize> = None;
    for dot in flow.pattern() {
        if let Some(from) = last {
            line(
                paint,
                layout.points[from],
                layout.points[dot],
                width,
                Color::BLACK,
            )?;
        }
        chosen[dot] = true;
        last = Some(dot);
    }
    if let (Some(last), Some(at)) = (last, flow.finger()) {
        line(paint, layout.points[last], at, width, Color::gray(110))?;
    }
    for (i, point) in layout.points.iter().enumerate() {
        let on = chosen[i];
        disc(
            paint,
            *point,
            if on { layout.radius } else { layout.radius / 2 },
            Color::BLACK,
        )?;
        if on {
            ring(
                paint,
                *point,
                layout.radius * 2,
                u32::from(style.line).max(1),
            )?;
        }
        if shown && flow.focus == i {
            ring(paint, *point, layout.radius * 3, u32::from(style.line) * 2)?;
        }
    }
    if !touch {
        let focused = shown && flow.focus == crate::native_flow::PATTERN_OK;
        button(paint, font, style, layout.ok, ok, focused)?;
    }
    Ok(())
}

/// The clock, large, and the date below it.
fn draw_clock(
    paint: &mut Vec<Paint>,
    font: &Font<'_>,
    style: &Style,
    layout: &Layout,
    clock: &Clock,
) -> Result<(), Error> {
    let size = clock_size(style);
    let big = geometry::line_px(font, size)?;
    let top = Rect::new(layout.clock.x, layout.clock.y, layout.clock.width, big);
    let x = (f32::from(top.width) - font.measure_text(size, &clock.time)?) / 2.0;
    label(paint, font, &clock.time, size, x, top, style.ink, None)?;
    if !clock.date.is_empty() {
        let below = Rect::new(
            top.x,
            top.y.saturating_add(big),
            top.width,
            geometry::line_px(font, style.small)?,
        );
        let x = (f32::from(below.width) - font.measure_text(style.small, &clock.date)?) / 2.0;
        label(
            paint,
            font,
            &clock.date,
            style.small,
            x,
            below,
            style.muted,
            None,
        )?;
    }
    Ok(())
}

/// The message under the code: the wait, what a new code needs, or what was
/// wrong.
///
/// # Errors
/// The host's refusal and `NoMemory`.
pub fn message_text(
    host: &Host<'_>,
    messages: &mut Messages,
    flow: &Flow,
    wait_left: u64,
) -> Result<Option<String>, Error> {
    let count = |name, value: u64| Arg::Int {
        name,
        value: i64::try_from(value).unwrap_or(i64::MAX),
    };
    let min = MIN_LENGTH as u64;
    Ok(if wait_left > 0 {
        Some(messages.format(host, "lock.wait", &[count("count", wait_left)])?)
    } else {
        match (flow.message(), flow.step()) {
            (Message::Wrong, _) => Some(messages.copy(host, "lock.wrong")?),
            (Message::Mismatch, _) => Some(messages.copy(host, "lock.mismatch")?),
            // What a new code needs.
            (Message::TooShort, _) | (Message::None, Step::New) => {
                Some(messages.format(host, "lock.too-short", &[count("count", min)])?)
            }
            _ => None,
        }
    })
}

/// Everything but the kinds list, as operations, and where it put things.
///
/// # Errors
/// The host's refusal, `NoMemory`.
#[allow(
    clippy::too_many_arguments,
    reason = "the host, the font and style it is drawn in, and what is drawn"
)]
pub fn prepare(
    host: &Host<'_>,
    font: &Font<'_>,
    style: &Style,
    display: &Snapshot,
    messages: &mut Messages,
    view: &View,
    wait_left: u64,
    clock: &Clock,
) -> Result<(Vec<Paint>, Drawn), Error> {
    let flow = &view.flow;
    let setting_up = flow.step() != Step::Unlock;
    let layout = Layout::new(&room(font, style, display, setting_up)?);
    let (width, height) = (display.display_width(), display.display_height());
    let mut paint = Vec::new();
    push(
        &mut paint,
        Paint::Fill(Rect::new(0, 0, width, height), style.paper),
    )?;
    let title = messages.copy(host, flow.title_key())?;
    let mut back = None;
    if setting_up {
        let mut page = Page::default();
        header::prepare(
            &mut page,
            host,
            font,
            style,
            messages,
            &title,
            width,
            display.has_touch(),
            0,
            1,
        )?;
        for op in page.paint {
            push(&mut paint, op)?;
        }
        back = page.back;
    } else {
        draw_clock(&mut paint, font, style, &layout, clock)?;
        text::message(&mut paint, font, style, layout.title, &title)?;
    }
    if let Some(message) = message_text(host, messages, flow, wait_left)? {
        text::message(&mut paint, font, style, layout.message, &message)?;
    }
    let shown = flow.shown;
    match flow.kind() {
        "pattern" => {
            let ok = messages.copy(host, "lock.ok")?;
            pattern(
                &mut paint,
                font,
                style,
                &layout,
                flow,
                display.has_touch(),
                &ok,
            )?;
        }
        "password" => {
            let label = messages.copy(host, "lock.type-password")?;
            button(&mut paint, font, style, layout.password, &label, shown)?;
        }
        _ => pin(&mut paint, font, style, &layout, flow, shown)?,
    }
    Ok((paint, Drawn { layout, back }))
}
