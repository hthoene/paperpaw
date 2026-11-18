//! The four widgets: *Tasks today* (what is due today or before, each with a
//! box to tick), *Task list* (one list), *Open tasks* (how many, and how many
//! are due) and *Next task* (the one to do next; a tap ticks it off).
//!
//! Each draws from the tasks in memory in whatever room its place has,
//! measured in pixels: as many rows as fit, then how many more there are.
//! Nothing here reads the card or asks for a redraw: the plugin tells the
//! host that a widget changed when a task did.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Color, Point, Rect,
    native::{Error, Host, callbacks::FrameRect, formatting::Arg, ui::WidgetTarget},
};
use paperpaw_ui_kit::{
    Messages, Paint,
    geometry::{line_metrics, line_px},
    mem, paint,
    text::{concat, fit},
    widget::{
        Align, Cx, Look,
        layout::{caption, caption_height, label},
        shape::{fill, fill_circle, line as stroke_line, stroke_circle},
        text::line as one_line,
    },
};

use crate::declarations::{
    COUNT_TOKEN, LIST_TOKEN, NEXT_TOKEN, ROWS_DEFAULT, ROWS_MOST, TODAY_TOKEN,
};
use crate::model::{Store, Task, View};

/// The target of a tap on the heading, "more" or an empty widget: it opens
/// the lists.
pub const OPEN_TARGET: u32 = 2;
/// Targets of tasks have the top bit set, the rest is from the task's id.
const TASK_TARGET: u32 = 0x8000_0000;

/// The target of a task: from its id, so that a tap ticks the task that was
/// shown even if the list changed since.
#[must_use]
pub fn target_of(task: &Task) -> u32 {
    let mut hash = 0x811c_9dc5_u32;
    for byte in task.uid().bytes() {
        hash = (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193);
    }
    TASK_TARGET | (hash & 0x7fff_ffff)
}

/// The open task a tap on `target` meant, if it is still there.
#[must_use]
pub fn task_of(store: &Store, target: u32) -> Option<usize> {
    if target & TASK_TARGET == 0 {
        return None;
    }
    store
        .tasks
        .iter()
        .position(|task| task.shows() && target_of(task) == target)
}

/// A day as a row or a widget says it: *Today*, *Tomorrow*, *Yesterday*, or
/// the day and the month; a day that is past says so.
///
/// # Errors
/// Host refusals, `NoMemory`.
pub fn due_text(
    host: &Host<'_>,
    messages: &mut Messages,
    due: i32,
    today: i32,
) -> Result<String, Error> {
    match i64::from(due) - i64::from(today) {
        0 => return messages.copy(host, "tasks.due.today"),
        1 => return messages.copy(host, "tasks.due.tomorrow"),
        -1 => return messages.copy(host, "tasks.due.yesterday"),
        _ => {}
    }
    let (_, month, day) = paperpaw_caldav::date::civil_from_days(i64::from(due));
    let mut digits = [0u8; 20];
    let key = concat(&[
        "core.month.short.",
        paperpaw_ui_kit::text::decimal(usize::from(month), &mut digits),
    ])?;
    let month = messages.copy(host, &key)?;
    let date = messages.format(
        host,
        "tasks.due.date",
        &[
            Arg::Int {
                name: "day",
                value: i64::from(day),
            },
            Arg::Text {
                name: "month",
                value: &month,
            },
        ],
    )?;
    if due < today {
        return messages.format(
            host,
            "tasks.due.late",
            &[Arg::Text {
                name: "date",
                value: &date,
            }],
        );
    }
    Ok(date)
}

/// What a list widget shows.
struct Listing {
    /// The tasks shown, as places in the store.
    items: Vec<u16>,
    /// Tasks not shown for want of count.
    more: usize,
    /// Open ones in all.
    open: usize,
    /// The heading.
    name: String,
    /// What to say instead of rows (a key).
    empty: &'static str,
}

fn listing(cx: &mut Cx<'_>, token: u32, store: &Store, today: i32) -> Result<Listing, Error> {
    let count = cx
        .settings
        .int("count", ROWS_DEFAULT)?
        .clamp(1, i64::from(ROWS_MOST));
    let count = usize::try_from(count).unwrap_or(1);
    let (view, name, empty) = if token == TODAY_TOKEN {
        (
            Some(View::Today),
            cx.tr("tasks.widget.today.caption")?,
            "tasks.widget.today.empty",
        )
    } else {
        let number = usize::try_from(cx.settings.int("list", 1)?.max(1)).unwrap_or(1);
        match store.lists.get(number - 1) {
            Some(list) => (
                Some(View::List(list.id)),
                paperpaw_ui_kit::text::copy(&list.name)?,
                "tasks.widget.list.empty",
            ),
            None => (None, cx.tr("tasks.title")?, "tasks.widget.list.none"),
        }
    };
    let Some(view) = view else {
        return Ok(Listing {
            items: Vec::new(),
            more: 0,
            open: 0,
            name,
            empty,
        });
    };
    let mut items = store.order(view, today)?;
    if token == LIST_TOKEN && !cx.settings.shows("done")? {
        items.retain(|at| store.tasks.get(usize::from(*at)).is_some_and(Task::open));
    }
    let more = items.len().saturating_sub(count);
    items.truncate(count);
    Ok(Listing {
        items,
        more,
        open: store.count_open(view, today),
        name,
        empty,
    })
}

/// The height of a row: its line with room around, at least as high as a
/// finger needs on touch screens.
fn row_height(cx: &Cx<'_>) -> Result<u16, Error> {
    let line = line_px(&cx.font, cx.style().text)?;
    let least = if cx.snapshot.has_touch() {
        // 6 mm.
        u16::try_from((60 * u32::from(cx.snapshot.display_dpi()) + 127) / 254).unwrap_or(u16::MAX)
    } else {
        0
    };
    Ok(line.saturating_add(line / 3).max(least))
}

/// How high the widget with `token` is in a place `width` wide.
///
/// # Errors
/// Font and host refusals.
pub fn height(cx: &mut Cx<'_>, token: u32, store: &Store, today: i32) -> Result<u16, Error> {
    let style = cx.style();
    let (small, gap) = (style.small, style.gap);
    let inner = match token {
        TODAY_TOKEN | LIST_TOKEN => {
            let listing = listing(cx, token, store, today)?;
            let rows = u16::try_from(listing.items.len().max(1))
                .unwrap_or(u16::MAX)
                .saturating_add(u16::from(listing.more > 0));
            let heading = if cx.settings.shows("caption")? {
                caption_height(&cx.font, small, gap)?
            } else {
                0
            };
            heading.saturating_add(row_height(cx)?.saturating_mul(rows))
        }
        _ => line_px(&cx.font, small)?.saturating_mul(3),
    };
    Ok(cx.shape.frame_height().saturating_add(inner))
}

/// Where a list widget's parts go in an area (inside its frame).
struct Places {
    caption: Option<Rect>,
    rows: Vec<Rect>,
    /// "3 more", where not all are shown, and how many.
    more: Option<(Rect, usize)>,
}

fn places(cx: &Cx<'_>, area: Rect, listing: &Listing) -> Result<Places, Error> {
    let mut rest = area;
    let mut heading = None;
    let row = row_height(cx)?.max(1);
    let style = cx.style();
    let height = caption_height(&cx.font, style.small, style.gap)?;
    // The heading where a task fits below it.
    if cx.settings.shows("caption")? && rest.height >= height.saturating_add(row) {
        heading = Some(Rect::new(rest.x, rest.y, rest.width, height));
        rest = Rect::new(
            rest.x,
            rest.y.saturating_add(height),
            rest.width,
            rest.height - height,
        );
    }
    let fits = usize::from(rest.height / row).max(1);
    let row = row.min(rest.height.max(1));
    let mut shown = listing.items.len().min(fits);
    let mut more = listing.more;
    // Not all fit: the last row says how many more there are (where two
    // tasks show at least).
    if shown < listing.items.len() && shown > 2 {
        more += listing.items.len() - shown + 1;
        shown -= 1;
    } else {
        more += listing.items.len() - shown;
    }
    let at = |n: usize| {
        let top = rest
            .y
            .saturating_add(row.saturating_mul(u16::try_from(n).unwrap_or(0)));
        Rect::new(rest.x, top, rest.width, row)
    };
    let mut rows = Vec::new();
    for n in 0..shown {
        mem::push(&mut rows, at(n))?;
    }
    let more = (more > 0 && rows.len() < fits).then(|| (at(rows.len()), more));
    Ok(Places {
        caption: heading,
        rows,
        more,
    })
}

/// How far a task's text begins from its row's edge: the box and a space.
fn lead(size: f32) -> u16 {
    (size * 0.8) as u16 + (size * 0.55) as u16
}

/// A round box in `place`, filled and ticked when done.
fn draw_box(
    cx: &Cx<'_>,
    out: &mut Vec<Paint>,
    place: Rect,
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
    if !done {
        return stroke_circle(out, centre, r, line, look.ink);
    }
    fill_circle(out, centre, r, look.faint)?;
    // A tick in the paper's colour.
    let s = i32::from(place.width);
    let (x, y) = (i32::from(place.x), i32::from(place.y));
    let width = (s.unsigned_abs() / 9).max(2);
    let a = Point::new(x + s * 28 / 100, y + s * 50 / 100);
    let b = Point::new(x + s * 44 / 100, y + s * 66 / 100);
    let c = Point::new(x + s * 72 / 100, y + s * 36 / 100);
    stroke_line(out, a, b, width, look.paper)?;
    stroke_line(out, b, c, width, look.paper)
}

/// Draws a task in `row`: its box, ticked if done, and its title, struck
/// through if done.
fn draw_task(
    cx: &Cx<'_>,
    out: &mut Vec<Paint>,
    row: Rect,
    (title, done): (&str, bool),
    (align, look): (Align, &Look),
) -> Result<(), Error> {
    let size = cx.style().text;
    let side = (size * 0.8) as u16;
    let lead = lead(size);
    let room = f32::from(row.width.saturating_sub(lead));
    let text = fit(&cx.font, size, title, room)?;
    let width = cx.font.measure_text(size, &text)?;
    let x = align.x(row, f32::from(lead) + width) as u16;
    let color = if done { look.faint } else { look.ink };
    let mid = row.y.saturating_add(row.height / 2);
    let place = Rect::new(x, mid.saturating_sub(side / 2), side, side);
    draw_box(cx, out, place, (done, look))?;
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

/// "Today", or "Today · 3" with open tasks.
fn caption_text(cx: &mut Cx<'_>, listing: &Listing) -> Result<String, Error> {
    if listing.open == 0 {
        return paperpaw_ui_kit::text::copy(&listing.name);
    }
    let number = cx.number(i64::try_from(listing.open).unwrap_or(i64::MAX))?;
    concat(&[&listing.name, " · ", &number])
}

/// A message small in the middle of the widget.
fn message(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    (area, color): (Rect, Color),
    key: &str,
) -> Result<(), Error> {
    let small = cx.style().small;
    if line_px(&cx.font, small)? > area.height {
        return Ok(());
    }
    let text = cx.tr(key)?;
    let align = cx.settings.align()?;
    one_line(
        out,
        &cx.font,
        area,
        align,
        (small, small * 0.7),
        &text,
        color,
    )?;
    Ok(())
}

fn draw_listing(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    area: Rect,
    token: u32,
    store: &Store,
    today: i32,
) -> Result<(), Error> {
    let look = cx.shape.paint(out, area, 256)?;
    let listing = listing(cx, token, store, today)?;
    let align = cx.settings.align()?;
    let places = places(cx, look.area, &listing)?;
    if places.caption.is_some() {
        let text = caption_text(cx, &listing)?;
        let style = cx.style();
        caption(
            out,
            &cx.font,
            (style.small, style.gap),
            (look.area, align, look.faint),
            &text,
        )?;
    }
    if listing.items.is_empty() {
        let room = match places.caption {
            Some(heading) => Rect::new(
                look.area.x,
                look.area.y.saturating_add(heading.height),
                look.area.width,
                look.area.height.saturating_sub(heading.height),
            ),
            None => look.area,
        };
        return message(cx, out, (room, look.faint), listing.empty);
    }
    for (row, at) in places.rows.iter().zip(&listing.items) {
        if let Some(task) = store.tasks.get(usize::from(*at)) {
            draw_task(cx, out, *row, (task.title(), task.done()), (align, &look))?;
        }
    }
    if let Some((row, more)) = places.more {
        let text = cx.format(
            "tasks.widget.more",
            &[Arg::Int {
                name: "count",
                value: i64::try_from(more).unwrap_or(i64::MAX),
            }],
        )?;
        let small = cx.style().small;
        label(out, &cx.font, (small, align), row, &text, look.faint)?;
    }
    Ok(())
}

/// How many tasks are open, large, and below how many are due.
fn draw_count(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    area: Rect,
    store: &Store,
    today: i32,
) -> Result<(), Error> {
    let look = cx.shape.paint(out, area, 256)?;
    let inner = look.area;
    let small = cx.style().small;
    let line = line_px(&cx.font, small)?;
    let align = cx.settings.align()?;
    let open = i64::try_from(store.count_open(View::Open, today)).unwrap_or(i64::MAX);
    let due = i64::try_from(store.count_open(View::Today, today)).unwrap_or(i64::MAX);
    let said = cx.format("tasks.widget.count.open", &[Arg::Count(open)])?;
    let below = if inner.height >= line.saturating_mul(3) && cx.settings.shows("due")? {
        Some(if due == 0 {
            cx.tr("tasks.widget.count.none-due")?
        } else {
            cx.format("tasks.widget.count.due", &[Arg::Count(due)])?
        })
    } else {
        None
    };
    let lower = if below.is_some() { line } else { 0 };
    let main = Rect::new(
        inner.x,
        inner.y,
        inner.width,
        inner.height.saturating_sub(lower),
    );
    // The words where they fit, else the number alone.
    let fits = cx.font.measure_text(small, &said)? <= f32::from(inner.width);
    let text = if fits { said } else { cx.number(open)? };
    one_line(
        out,
        &cx.font,
        main,
        align,
        (f32::from(main.height), small),
        &text,
        look.ink,
    )?;
    if let Some(below) = below {
        let band = Rect::new(
            inner.x,
            main.y.saturating_add(main.height),
            inner.width,
            line,
        );
        one_line(
            out,
            &cx.font,
            band,
            align,
            (small, small * 0.6),
            &below,
            look.faint,
        )?;
    }
    Ok(())
}

/// The task to do next, and below it its day and its list.
fn draw_next(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    area: Rect,
    store: &Store,
    today: i32,
) -> Result<(), Error> {
    let look = cx.shape.paint(out, area, 256)?;
    let inner = look.area;
    let Some(task) = store.next(today).and_then(|at| store.tasks.get(at)) else {
        return message(cx, out, (look.area, look.faint), "tasks.widget.next.empty");
    };
    let small = cx.style().small;
    let line = line_px(&cx.font, small)?;
    let align = cx.settings.align()?;
    let below = if inner.height >= line.saturating_mul(3) && cx.settings.shows("due")? {
        let list = store.list(task.list).map_or("", |list| list.name.as_str());
        Some(match task.due {
            Some(due) => {
                let when = due_text(cx.host, &mut cx.messages, due, today)?;
                concat(&[&when, " · ", list])?
            }
            None => paperpaw_ui_kit::text::copy(list)?,
        })
    } else {
        None
    };
    let lower = if below.is_some() { line } else { 0 };
    let main = Rect::new(
        inner.x,
        inner.y,
        inner.width,
        inner.height.saturating_sub(lower),
    );
    draw_task(cx, out, main, (task.title(), false), (align, &look))?;
    if let Some(below) = below {
        let band = Rect::new(
            inner.x,
            main.y.saturating_add(main.height),
            inner.width,
            line,
        );
        one_line(
            out,
            &cx.font,
            band,
            align,
            (small, small * 0.6),
            &below,
            look.faint,
        )?;
    }
    Ok(())
}

/// Draws the widget with `token` in `area`.
///
/// # Errors
/// Font and host refusals, `NoMemory`.
pub fn draw(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    area: Rect,
    token: u32,
    (store, today): (&Store, i32),
) -> Result<(), Error> {
    match token {
        TODAY_TOKEN | LIST_TOKEN => draw_listing(cx, out, area, token, store, today),
        COUNT_TOKEN => draw_count(cx, out, area, store, today),
        NEXT_TOKEN => draw_next(cx, out, area, store, today),
        _ => Err(Error::NotFound),
    }
}

/// A rectangle of the widget as a target's.
fn frame_of(rect: Rect) -> FrameRect {
    paint::frame_rect(rect)
}

/// The targets of the widget placed in `area`: a task ticks; the heading,
/// "more", an empty widget and the count open the lists.
///
/// # Errors
/// Font and host refusals, `NoMemory`.
pub fn targets(
    cx: &mut Cx<'_>,
    area: Rect,
    token: u32,
    (store, today): (&Store, i32),
) -> Result<Vec<WidgetTarget>, Error> {
    let mut targets = Vec::new();
    let whole = |id: u32| WidgetTarget::new(id, frame_of(area));
    match token {
        TODAY_TOKEN | LIST_TOKEN => {}
        NEXT_TOKEN => {
            let id = store
                .next(today)
                .and_then(|at| store.tasks.get(at))
                .map_or(OPEN_TARGET, target_of);
            mem::push(&mut targets, whole(id))?;
            return Ok(targets);
        }
        _ => {
            mem::push(&mut targets, whole(OPEN_TARGET))?;
            return Ok(targets);
        }
    }
    let listing = listing(cx, token, store, today)?;
    if listing.items.is_empty() {
        mem::push(&mut targets, whole(OPEN_TARGET))?;
        return Ok(targets);
    }
    let look = cx.shape.look(area);
    let places = places(cx, look.area, &listing)?;
    if let Some(heading) = places.caption {
        mem::push(
            &mut targets,
            WidgetTarget::new(OPEN_TARGET, frame_of(heading)),
        )?;
    }
    for (row, at) in places.rows.iter().zip(&listing.items) {
        if let Some(task) = store.tasks.get(usize::from(*at)) {
            mem::push(
                &mut targets,
                WidgetTarget::new(target_of(task), frame_of(*row)),
            )?;
        }
    }
    if let Some((row, _)) = places.more {
        mem::push(&mut targets, WidgetTarget::new(OPEN_TARGET, frame_of(row)))?;
    }
    Ok(targets)
}
