//! A to-do list: a few things to do, kept in `todo.txt` in the plugin's data
//! folder (see [`crate::native_todo_list`]). The widget shows the open ones
//! with a box each; a tap on one ticks it off: it shows struck through for the
//! rest of the day (or goes at once), then it goes.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Point, Rect,
    native::{Error, Host, callbacks::FrameRect, ui::WidgetTarget},
};
use paperpaw_ui_kit::{
    Paint,
    geometry::{line_metrics, line_px},
    mem, paint,
    text::{concat, fit},
    widget::{
        Align, Cx, Look, Settings, TextSize,
        layout::{caption, caption_height, label},
        shape::{fill, fill_circle, line as stroke_line, stroke_circle},
        text::fit_line,
    },
};

use crate::native_todo_list::{Item, Loaded, load, save};

/// The token the widget is declared with.
pub const TOKEN: u32 = 112;

/// The target of a tap on the heading, "more" or an empty list: it opens the
/// list's own screen.
pub const OPEN_TARGET: u32 = 2;
/// Targets of items begin here: the top bit set, the rest from the item.
const ITEM_TARGET: u32 = 0x8000_0000;

/// The boxes before the items.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Boxes {
    Circle,
    Square,
    None,
}

/// The items to show and how many there are.
#[derive(Debug, Default, PartialEq, Eq)]
struct View {
    /// The items shown.
    items: Vec<Item>,
    /// Open ones in all.
    open: usize,
    /// Items not shown for want of count.
    more: usize,
    /// The list could not be read.
    unreadable: bool,
}

/// The items to show on `today`, as the settings say.
fn view(host: &Host<'_>, settings: &Settings<'_, '_>, today: i64) -> Result<View, Error> {
    let hide = settings.with_text("done", |value| value.ends_with(".hidden"))?;
    let count = settings.int("count", 5)?.clamp(1, 12) as usize;
    let loaded = load(host, today)?;
    let Loaded::List(list) = loaded else {
        return Ok(View {
            unreadable: true,
            ..View::default()
        });
    };
    let mut all = list.items;
    if hide {
        all.retain(|item| item.done.is_none());
    }
    let open = all.iter().filter(|item| item.done.is_none()).count();
    let more = all.len().saturating_sub(count);
    all.truncate(count);
    Ok(View {
        items: all,
        open,
        more,
        unreadable: false,
    })
}

fn boxes(settings: &Settings<'_, '_>) -> Result<Boxes, Error> {
    settings.with_text("box", |value| match value.rsplit('.').next() {
        Some("square") => Boxes::Square,
        Some("none") => Boxes::None,
        _ => Boxes::Circle,
    })
}

/// What it shows, as a number that changes with it: the items and how they
/// are done, the day (those done go with it), what is set.
///
/// # Errors
/// Host refusals.
pub fn shows(host: &Host<'_>, widget: &str) -> Result<u64, Error> {
    let settings = Settings::new(host, widget);
    let today = crate::native_draw::epoch_day(host)?;
    let view = view(host, &settings, today)?;
    let mut mark = crate::native_draw::fnv(
        1,
        &[
            u8::from(view.unreadable),
            boxes(&settings)? as u8,
            u8::from(settings.shows("caption")?),
        ],
    );
    mark = crate::native_draw::fnv(mark, &(view.open as u64).to_le_bytes());
    mark = crate::native_draw::fnv(mark, &(view.more as u64).to_le_bytes());
    for item in &view.items {
        mark = crate::native_draw::fnv(mark, item.text.as_bytes());
        mark = crate::native_draw::fnv(mark, &[u8::from(item.done.is_some())]);
    }
    Ok(mark)
}

/// The size of the items' text: a list is denser than a widget's one line, so
/// a step smaller than the body (small is the size of notes, medium the
/// body's).
fn text_size(cx: &Cx<'_>) -> Result<f32, Error> {
    let style = cx.style();
    Ok(match cx.settings.size()? {
        TextSize::Small => style.small,
        TextSize::Large => style.title,
        _ => style.text,
    })
}

/// The height of a row of the widget: its line with room around, at least as
/// high as a finger needs on touch screens.
fn row_height(cx: &Cx<'_>) -> Result<u16, Error> {
    let line = line_px(&cx.font, text_size(cx)?)?;
    let least = if cx.snapshot.has_touch() {
        // 6 mm.
        u16::try_from((60 * u32::from(cx.snapshot.display_dpi()) + 127) / 254).unwrap_or(u16::MAX)
    } else {
        0
    };
    Ok(line.saturating_add(line / 3).max(least))
}

/// The height: the heading, a row for each item (one for the message of an
/// empty list), the row of "more", the frame.
///
/// # Errors
/// Font and host refusals.
pub fn height(cx: &mut Cx<'_>, _width: u16) -> Result<u16, Error> {
    let today = crate::native_draw::epoch_day(cx.host)?;
    let view = view(cx.host, &cx.settings, today)?;
    let rows = u16::try_from(view.items.len().max(1))
        .unwrap_or(u16::MAX)
        .saturating_add(u16::from(view.more > 0));
    let caption = if cx.settings.shows("caption")? {
        let style = cx.style();
        caption_height(&cx.font, style.small, style.gap)?
    } else {
        0
    };
    Ok(caption
        .saturating_add(row_height(cx)?.saturating_mul(rows))
        .saturating_add(cx.shape.frame_height()))
}

/// Where the widget's parts go in an area (inside its frame).
struct Places {
    caption: Option<Rect>,
    rows: Vec<Rect>,
    /// "3 more", where not all are shown, and how many.
    more: Option<(Rect, usize)>,
}

fn places(cx: &Cx<'_>, area: Rect, view: &View) -> Result<Places, Error> {
    let mut rest = area;
    let mut caption = None;
    let row = row_height(cx)?.max(1);
    // The heading where an item fits below it.
    let style = cx.style();
    let height = caption_height(&cx.font, style.small, style.gap)?;
    if cx.settings.shows("caption")? && rest.height >= height.saturating_add(row) {
        caption = Some(Rect::new(rest.x, rest.y, rest.width, height));
        rest = Rect::new(
            rest.x,
            rest.y.saturating_add(height),
            rest.width,
            rest.height - height,
        );
    }
    let fits = usize::from(rest.height / row).max(1);
    let row = row.min(rest.height.max(1));
    let mut shown = view.items.len().min(fits);
    let mut more = view.more;
    // Not all fit: the last row says how many more there are (where two items
    // show at least).
    if shown < view.items.len() && shown > 2 {
        more += view.items.len() - shown + 1;
        shown -= 1;
    }
    let mut rows = Vec::new();
    for n in 0..shown {
        let top = rest
            .y
            .saturating_add(row.saturating_mul(u16::try_from(n).unwrap_or(0)));
        mem::push(&mut rows, Rect::new(rest.x, top, rest.width, row))?;
    }
    let more = (more > 0 && rows.len() < fits).then(|| {
        let top = rest
            .y
            .saturating_add(row.saturating_mul(u16::try_from(rows.len()).unwrap_or(0)));
        (Rect::new(rest.x, top, rest.width, row), more)
    });
    Ok(Places {
        caption,
        rows,
        more,
    })
}

/// How far an item's text begins from its row's edge at `size`: the box and a
/// space after it, where there are boxes.
fn lead(size: f32, boxes: Boxes) -> u16 {
    if boxes == Boxes::None {
        0
    } else {
        (size * 0.8) as u16 + (size * 0.55) as u16
    }
}

/// The size of the items in `rows`: the list's own, or smaller (down to the
/// small size) where an item would be shortened at it, the same for all so
/// that they line up.
fn items_size(cx: &Cx<'_>, rows: &[Rect], items: &[Item], boxes: Boxes) -> Result<f32, Error> {
    let size = text_size(cx)?;
    let least = cx.style().small.min(size);
    let mut smallest = size;
    for (row, item) in rows.iter().zip(items) {
        let room = f32::from(row.width.saturating_sub(lead(size, boxes)));
        smallest = smallest.min(fit_line(&cx.font, size, least, &item.text, room)?.0);
    }
    Ok(smallest)
}

/// A box in `place`: round or square, filled and ticked when done.
fn draw_box(
    cx: &Cx<'_>,
    out: &mut Vec<Paint>,
    place: Rect,
    square: bool,
    (done, look): (bool, &Look),
) -> Result<(), Error> {
    let line = (u32::from(place.width) / 10)
        .max(u32::from(cx.style().line))
        .max(1);
    let r = u32::from(place.width / 2);
    let radius = i32::try_from(r).unwrap_or(0);
    let centre = Point::new(
        i32::from(place.x).saturating_add(radius),
        i32::from(place.y).saturating_add(radius),
    );
    match (square, done) {
        (false, false) => stroke_circle(out, centre, r, line, look.ink)?,
        (false, true) => fill_circle(out, centre, r, look.faint)?,
        (true, false) => paint::push(out, Paint::Focus(place, r / 3, Some(line), look.ink))?,
        (true, true) => paint::push(out, Paint::Focus(place, r / 3, None, look.faint))?,
    }
    if done {
        // A tick in the paper's colour.
        let s = i32::from(place.width);
        let (x, y) = (i32::from(place.x), i32::from(place.y));
        let width = (s as u32 / 9).max(2);
        let a = Point::new(x + s * 28 / 100, y + s * 50 / 100);
        let b = Point::new(x + s * 44 / 100, y + s * 66 / 100);
        let c = Point::new(x + s * 72 / 100, y + s * 36 / 100);
        stroke_line(out, a, b, width, look.paper)?;
        stroke_line(out, b, c, width, look.paper)?;
    }
    Ok(())
}

/// Draws an item in `row`: its box, ticked if done, and its text, struck
/// through if done.
fn draw_item(
    cx: &Cx<'_>,
    out: &mut Vec<Paint>,
    (size, boxes): (f32, Boxes),
    row: Rect,
    item: &Item,
    (align, look): (Align, &Look),
) -> Result<(), Error> {
    let done = item.done.is_some();
    let side = (size * 0.8) as u16;
    let lead = lead(size, boxes);
    let room = f32::from(row.width.saturating_sub(lead));
    let text = fit(&cx.font, size, &item.text, room)?;
    let width = cx.font.measure_text(size, &text)?;
    let x = align.x(row, f32::from(lead) + width) as u16;
    let color = if done { look.faint } else { look.ink };
    if boxes != Boxes::None {
        let mid = row.y.saturating_add(row.height / 2);
        let place = Rect::new(x, mid.saturating_sub(side / 2), side, side);
        draw_box(cx, out, place, boxes == Boxes::Square, (done, look))?;
    }
    let place = Rect::new(
        x.saturating_add(lead),
        row.y,
        row.width.saturating_sub(lead),
        row.height,
    );
    label(out, &cx.font, (size, Align::Left), place, &text, color)?;
    if done {
        // A line through the middle of the small letters.
        let (ascent, descent, _) = line_metrics(&cx.font, size)?;
        let baseline = f32::from(row.y) + (f32::from(row.height) + ascent - descent) / 2.0;
        let y = (baseline - size * 0.27) as u16;
        let thick = cx.style().line.max(1);
        fill(out, Rect::new(place.x, y, width as u16, thick), color)?;
    }
    Ok(())
}

/// "To do", or "To do · 3" with open items.
fn caption_text(cx: &mut Cx<'_>, open: usize) -> Result<String, Error> {
    let title = cx.tr("widgets.todo")?;
    if open == 0 {
        return Ok(title);
    }
    let number = cx.number(i64::try_from(open).unwrap_or(i64::MAX))?;
    concat(&[&title, " · ", &number])
}

/// Draws the widget in `area`.
///
/// # Errors
/// Font and host refusals, [`Error::NoMemory`].
pub fn draw(cx: &mut Cx<'_>, out: &mut Vec<Paint>, area: Rect) -> Result<(), Error> {
    let look = cx.shape.paint(out, area, 256)?;
    let today = crate::native_draw::epoch_day(cx.host)?;
    let view = view(cx.host, &cx.settings, today)?;
    let align = cx.settings.align()?;
    let small = cx.style().small;
    let message = if view.unreadable {
        Some(cx.tr("widgets.todo.unreadable")?)
    } else if view.items.is_empty() {
        // The whole place to say how to add some, small.
        let key = if cx.snapshot.has_touch() {
            "widgets.todo.none.touch"
        } else {
            "widgets.todo.none"
        };
        Some(cx.tr(key)?)
    } else {
        None
    };
    if let Some(message) = message {
        if line_px(&cx.font, small)? <= look.area.height {
            crate::native_draw::message_in(cx, out, &look, &message)?;
        }
        return Ok(());
    }
    let places = places(cx, look.area, &view)?;
    if places.caption.is_some() {
        let text = caption_text(cx, view.open)?;
        let style = cx.style();
        caption(
            out,
            &cx.font,
            (style.small, style.gap),
            (look.area, align, look.faint),
            &text,
        )?;
    }
    let boxes = boxes(&cx.settings)?;
    let size = items_size(cx, &places.rows, &view.items, boxes)?;
    for (row, item) in places.rows.iter().zip(&view.items) {
        draw_item(cx, out, (size, boxes), *row, item, (align, &look))?;
    }
    if let Some((row, more)) = places.more {
        let text = cx.format(
            "widgets.todo.more",
            &[paperpaw_sdk::native::formatting::Arg::Int {
                name: "count",
                value: i64::try_from(more).unwrap_or(i64::MAX),
            }],
        )?;
        label(out, &cx.font, (small, align), row, &text, look.faint)?;
    }
    Ok(())
}

/// The target number of an item: what it says and whether it is done, so that
/// a tap ticks the item that was shown even if the list changed since.
fn target_of(item: &Item) -> u32 {
    let hash = crate::native_draw::fnv(1, item.text.as_bytes());
    let hash = crate::native_draw::fnv(hash, &[u8::from(item.done.is_some())]);
    // The top bit marks an item; the rest is the hash folded to 31 bits.
    ITEM_TARGET | ((hash ^ (hash >> 31)) as u32 & 0x7fff_ffff)
}

/// A rectangle of the widget as a target's.
fn frame_of(rect: Rect) -> FrameRect {
    paint::frame_rect(rect)
}

/// The targets of the widget placed in `area`: an item ticks, the heading,
/// "more" and an empty list open the list's screen.
///
/// # Errors
/// Font and host refusals, [`Error::NoMemory`].
pub fn targets(cx: &Cx<'_>, area: Rect) -> Result<Vec<WidgetTarget>, Error> {
    let today = crate::native_draw::epoch_day(cx.host)?;
    let view = view(cx.host, &cx.settings, today)?;
    let mut targets = Vec::new();
    if view.unreadable || view.items.is_empty() {
        mem::push(&mut targets, WidgetTarget::new(OPEN_TARGET, frame_of(area)))?;
        return Ok(targets);
    }
    let look = cx.shape.look(area);
    let places = places(cx, look.area, &view)?;
    if let Some(caption) = places.caption {
        mem::push(
            &mut targets,
            WidgetTarget::new(OPEN_TARGET, frame_of(caption)),
        )?;
    }
    for (row, item) in places.rows.iter().zip(&view.items) {
        mem::push(
            &mut targets,
            WidgetTarget::new(target_of(item), frame_of(*row)),
        )?;
    }
    if let Some((row, _)) = places.more {
        mem::push(&mut targets, WidgetTarget::new(OPEN_TARGET, frame_of(row)))?;
    }
    Ok(targets)
}

/// Ticks off (or on again) the item a tap on `target` meant, in the list kept:
/// the item that was shown, found by what it says; nothing if it is not there
/// any more.
///
/// # Errors
/// The host's refusals, [`Error::NoMemory`], [`Error::Unsupported`] for a list
/// that cannot be read (it is not written over).
pub fn tick(host: &Host<'_>, target: u32) -> Result<(), Error> {
    let today = crate::native_draw::epoch_day(host)?;
    let Loaded::List(mut list) = load(host, today)? else {
        return Err(Error::Unsupported);
    };
    let Some(item) = list.items.iter_mut().find(|item| target_of(item) == target) else {
        return Ok(());
    };
    item.done = if item.done.is_some() {
        None
    } else {
        Some(today)
    };
    save(host, &list)
}
