//! The five widgets: *Today* (the date and what the day holds), *Next event*,
//! *Agenda* (the next days), *Month* (a grid with a dot under the days that
//! have events) and *Birthdays*. A calendar's shade marks each event: a dot,
//! a ring, a gray dot or a square.
//!
//! Each draws from the events in memory in whatever room its place has,
//! measured in pixels: as many rows as fit, then how many more there are.
//! Nothing here reads the card or asks for a redraw: the plugin tells the
//! host that a widget changed when the events did.
use alloc::{string::String, vec::Vec};
use paperpaw_caldav::date::civil_from_days;
use paperpaw_sdk::{
    Color, Point, Rect,
    native::{Error, callbacks::FrameRect, formatting::Arg, ui::WidgetTarget},
};
use paperpaw_ui_kit::{
    Paint,
    geometry::line_px,
    mem, paint,
    text::{concat, copy},
    widget::{
        Align, Cx, Look,
        layout::{caption, caption_height, label},
        shape::{fill, fill_circle, stroke_circle},
        text::line as one_line,
    },
};

use crate::declarations::{
    AGENDA_TOKEN, BIRTHDAYS_TOKEN, MONTH_TOKEN, NEXT_TOKEN, ROWS_MOST, TODAY_TOKEN,
};
use crate::model::{Mark, NO_TIME, Occ, Show, Store, WINDOW_DAYS, hash};
use crate::native_month::{self, Month, Palette};
use crate::native_screens::Pick;
use crate::native_text::{
    aged, day_text, long_date, month_name, time_text, weekday_name, when_text,
};

/// The target of a tap on the heading, "more" or an empty widget: it opens
/// the agenda.
pub const OPEN_TARGET: u32 = 2;
/// A target with the top bit set is an event (the rest from its identity),
/// one with the next bit a day (the rest its number).
const EVENT_TARGET: u32 = 0x8000_0000;
const DAY_TARGET: u32 = 0x4000_0000;

/// What the widgets are drawn with: the day, the time, the clock's style.
#[derive(Debug, Clone, Copy)]
pub struct Env {
    pub today: i32,
    pub secs: u32,
    pub twelve: bool,
    pub week_start: u8,
    /// The whole screen is the widget's (the sleep screen): all it can hold.
    pub full: bool,
}

/// The target of an event on a day: from what it is, so that a tap opens the
/// event that was shown even if the list changed since.
#[must_use]
pub fn event_target(store: &Store, occ: &Occ) -> u32 {
    let entry = store.entries.get(usize::from(occ.entry));
    let who = entry.map_or(0, |e| hash(&e.title) ^ e.uid);
    EVENT_TARGET | ((who ^ (occ.day as u32).wrapping_mul(0x9e37_79b1)) & 0x3fff_ffff)
}

/// The event a tap on `target` meant, if it is still there.
///
/// # Errors
/// `NoMemory`.
pub fn pick_of(store: &Store, target: u32, env: &Env) -> Result<Option<Pick>, Error> {
    if target & EVENT_TARGET == 0 {
        return Ok(None);
    }
    let occs = store.occurrences(env.today, env.today + WINDOW_DAYS, Show::All)?;
    let birthdays = store.occurrences(env.today, env.today + 90, Show::Birthdays)?;
    Ok(occs
        .iter()
        .chain(&birthdays)
        .find(|occ| event_target(store, occ) == target)
        .map(|occ| Pick {
            entry: occ.entry,
            day: occ.day,
            age: occ.age,
        }))
}

/// The day a tap on `target` meant, if it is a day's.
#[must_use]
pub const fn day_of(target: u32) -> Option<i32> {
    if target & EVENT_TARGET == 0 && target & DAY_TARGET != 0 {
        Some((target & 0x3fff_ffff) as i32)
    } else {
        None
    }
}

/// A line of a list widget.
enum Line {
    Heading(String),
    Event {
        target: u32,
        title: String,
        when: String,
        shade: u8,
    },
}

/// What a list widget shows.
struct Listing {
    lines: Vec<Line>,
    /// Events not shown for want of count.
    more: usize,
    caption: String,
    /// What to say instead of rows (a key).
    empty: &'static str,
}

/// Whether an occurrence of today is over.
fn over(occ: &Occ, env: &Env) -> bool {
    if occ.day != env.today || occ.whole_day() {
        return false;
    }
    let ends = if occ.end == NO_TIME {
        occ.secs
    } else {
        occ.end
    };
    ends < env.secs
}

fn listing(cx: &mut Cx<'_>, token: u32, store: &Store, env: &Env) -> Result<Listing, Error> {
    let count = cx.settings.int("count", 5)?.clamp(1, i64::from(ROWS_MOST));
    let count = if env.full {
        usize::try_from(ROWS_MOST).unwrap_or(12)
    } else {
        usize::try_from(count).unwrap_or(1)
    };
    let days = i32::try_from(cx.settings.int("days", 1)?.clamp(1, 60)).unwrap_or(1);
    let days = if env.full { days.max(7) } else { days };
    let (from, to, show, caption_key, empty) = match token {
        TODAY_TOKEN => (
            env.today,
            env.today,
            Show::All,
            "calendar.widget.today.caption",
            "calendar.widget.today.empty",
        ),
        BIRTHDAYS_TOKEN => (
            env.today,
            env.today + days - 1,
            Show::Birthdays,
            "calendar.widget.birthdays.caption",
            "calendar.widget.birthdays.empty",
        ),
        _ => (
            env.today,
            env.today + days - 1,
            Show::All,
            "calendar.widget.agenda.caption",
            "calendar.widget.agenda.empty",
        ),
    };
    let mut occs = store.occurrences(from, to, show)?;
    occs.retain(|occ| !over(occ, env));
    let more = occs.len().saturating_sub(count);
    occs.truncate(count);
    let mut lines = Vec::new();
    let mut last = None;
    for occ in &occs {
        let Some(entry) = store.entries.get(usize::from(occ.entry)) else {
            continue;
        };
        if token == AGENDA_TOKEN && last != Some(occ.day) {
            last = Some(occ.day);
            let text = day_text(cx.host, &mut cx.messages, occ.day, env.today)?;
            mem::push(&mut lines, Line::Heading(text))?;
        }
        let title = if entry.mark == Mark::Event {
            copy(&entry.title)?
        } else {
            aged(cx.host, &mut cx.messages, &entry.title, occ.age)?
        };
        let when = if token == BIRTHDAYS_TOKEN {
            day_text(cx.host, &mut cx.messages, occ.day, env.today)?
        } else if token == AGENDA_TOKEN && occ.whole_day() {
            String::new()
        } else {
            when_text(cx.host, &mut cx.messages, env.twelve, occ, false)?
        };
        mem::push(
            &mut lines,
            Line::Event {
                target: event_target(store, occ),
                title,
                when,
                shade: store.shade_of(entry),
            },
        )?;
    }
    Ok(Listing {
        lines,
        more,
        caption: cx.tr(caption_key)?,
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

fn date_height(cx: &Cx<'_>) -> Result<u16, Error> {
    Ok(line_px(&cx.font, cx.style().text * 1.5)?.saturating_add(cx.style().gap / 2))
}

/// How high the widget with `token` is.
///
/// # Errors
/// Font and host refusals.
pub fn height(cx: &mut Cx<'_>, token: u32, store: &Store, env: &Env) -> Result<u16, Error> {
    let style = cx.style();
    let (small, gap) = (style.small, style.gap);
    let inner = match token {
        NEXT_TOKEN => line_px(&cx.font, small)?.saturating_mul(3),
        MONTH_TOKEN => {
            let cell = row_height(cx)?;
            let head = if cx.settings.shows("caption")? {
                caption_height(&cx.font, small, gap)?
            } else {
                0
            };
            head.saturating_add(cell.saturating_mul(7))
        }
        _ => {
            let listing = listing(cx, token, store, env)?;
            let rows = u16::try_from(listing.lines.len().max(1))
                .unwrap_or(u16::MAX)
                .saturating_add(u16::from(listing.more > 0));
            let head = if token == TODAY_TOKEN {
                if cx.settings.shows("date")? {
                    date_height(cx)?
                } else {
                    0
                }
            } else if cx.settings.shows("caption")? {
                caption_height(&cx.font, small, gap)?
            } else {
                0
            };
            head.saturating_add(row_height(cx)?.saturating_mul(rows))
        }
    };
    Ok(cx.shape.frame_height().saturating_add(inner))
}

/// A mark for the calendar an event is of: a dot, a ring, a gray dot, a square.
fn marker(out: &mut Vec<Paint>, place: Rect, shade: u8, look: &Look) -> Result<(), Error> {
    let r = u32::from(place.width / 2).max(1);
    let radius = i32::try_from(r).unwrap_or(1);
    let centre = Point::new(
        i32::from(place.x).saturating_add(radius),
        i32::from(place.y).saturating_add(radius),
    );
    match shade % 4 {
        0 => fill_circle(out, centre, r, look.ink),
        1 => stroke_circle(out, centre, r, (r / 3).max(1), look.ink),
        2 => fill_circle(out, centre, r, Color::gray(0x88)),
        _ => fill(
            out,
            Rect::new(place.x, place.y, place.width, place.width),
            look.ink,
        ),
    }
}

/// A line of an event: the mark, the time (where there is room) and the title.
fn draw_event(
    cx: &Cx<'_>,
    out: &mut Vec<Paint>,
    row: Rect,
    (title, when, shade): (&str, &str, u8),
    look: &Look,
) -> Result<(), Error> {
    let style = cx.style();
    let (size, small) = (style.text, style.small);
    let side = (size * 0.55) as u16;
    let lead = side + (size * 0.45) as u16;
    let mid = row.y.saturating_add(row.height / 2);
    let x = row.x;
    marker(
        out,
        Rect::new(x, mid.saturating_sub(side / 2), side, side),
        shade,
        look,
    )?;
    let mut at = x.saturating_add(lead);
    let mut room = row.width.saturating_sub(lead);
    if !when.is_empty() {
        let column = cx.font.measure_text(small, when)? as u16 + (size * 0.4) as u16;
        // The time only where the title keeps most of the row.
        if column < room / 3 {
            let cell = Rect::new(at, row.y, column, row.height);
            label(out, &cx.font, (small, Align::Left), cell, when, look.faint)?;
            at = at.saturating_add(column);
            room -= column;
        }
    }
    let place = Rect::new(at, row.y, room, row.height);
    label(out, &cx.font, (size, Align::Left), place, title, look.ink)
}

/// Where a list widget's parts go in an area (inside its frame).
struct Places {
    head: Option<Rect>,
    rows: Vec<Rect>,
    /// "3 more", where not all are shown, and how many.
    more: Option<(Rect, usize)>,
}

fn places(cx: &mut Cx<'_>, area: Rect, token: u32, listing: &Listing) -> Result<Places, Error> {
    let mut rest = area;
    let row = row_height(cx)?.max(1);
    let style = cx.style();
    let (small, gap) = (style.small, style.gap);
    let want = if token == TODAY_TOKEN {
        cx.settings
            .shows("date")?
            .then(|| date_height(cx))
            .transpose()?
    } else {
        cx.settings
            .shows("caption")?
            .then(|| caption_height(&cx.font, small, gap))
            .transpose()?
    };
    let mut head = None;
    if let Some(height) = want.filter(|h| rest.height >= h.saturating_add(row)) {
        head = Some(Rect::new(rest.x, rest.y, rest.width, height));
        rest = Rect::new(rest.x, rest.y + height, rest.width, rest.height - height);
    }
    let fits = usize::from(rest.height / row).max(1);
    let row = row.min(rest.height.max(1));
    let mut shown = listing.lines.len().min(fits);
    let mut more = listing.more;
    if shown < listing.lines.len() && shown > 2 {
        // Not all fit: the last row says how many more there are.
        more += listing.lines[shown - 1..]
            .iter()
            .filter(|line| matches!(line, Line::Event { .. }))
            .count();
        shown -= 1;
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
    Ok(Places { head, rows, more })
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

fn draw_head(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    (token, head): (u32, Rect),
    (listing, env, look): (&Listing, &Env, &Look),
) -> Result<(), Error> {
    let align = cx.settings.align()?;
    if token == TODAY_TOKEN {
        let text = long_date(cx.host, &mut cx.messages, env.today)?;
        let size = cx.style().text * 1.5;
        one_line(
            out,
            &cx.font,
            head,
            align,
            (size, cx.style().small),
            &text,
            look.ink,
        )?;
        return Ok(());
    }
    let style = cx.style();
    caption(
        out,
        &cx.font,
        (style.small, style.gap),
        (head, align, look.faint),
        &listing.caption,
    )?;
    Ok(())
}

fn draw_listing(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    area: Rect,
    token: u32,
    (store, env): (&Store, &Env),
) -> Result<(), Error> {
    let look = cx.shape.paint(out, area, 256)?;
    let listing = listing(cx, token, store, env)?;
    let places = places(cx, look.area, token, &listing)?;
    if let Some(head) = places.head {
        draw_head(cx, out, (token, head), (&listing, env, &look))?;
    }
    if listing.lines.is_empty() {
        let room = match places.head {
            Some(head) => Rect::new(
                look.area.x,
                look.area.y.saturating_add(head.height),
                look.area.width,
                look.area.height.saturating_sub(head.height),
            ),
            None => look.area,
        };
        return message(cx, out, (room, look.faint), listing.empty);
    }
    let small = cx.style().small;
    for (row, line) in places.rows.iter().zip(&listing.lines) {
        match line {
            Line::Heading(text) => {
                label(out, &cx.font, (small, Align::Left), *row, text, look.faint)?
            }
            Line::Event {
                title, when, shade, ..
            } => draw_event(cx, out, *row, (title, when, *shade), &look)?,
        }
    }
    if let Some((row, more)) = places.more {
        let text = cx.format(
            "calendar.widget.more",
            &[Arg::Int {
                name: "count",
                value: i64::try_from(more).unwrap_or(i64::MAX),
            }],
        )?;
        label(out, &cx.font, (small, Align::Left), row, &text, look.faint)?;
    }
    Ok(())
}

/// The next event that is not over, within the window kept.
fn next_event(store: &Store, env: &Env) -> Result<Option<Occ>, Error> {
    let occs = store.occurrences(env.today, env.today + WINDOW_DAYS, Show::All)?;
    Ok(occs.into_iter().find(|occ| !over(occ, env) && !occ.goes_on))
}

/// How long until an event: "in 25 min", "in 3 h", or its day and time.
fn until_text(cx: &mut Cx<'_>, occ: &Occ, env: &Env) -> Result<String, Error> {
    if occ.day == env.today && !occ.whole_day() {
        let minutes = i64::from(occ.secs.saturating_sub(env.secs)) / 60;
        if minutes < 1 {
            return cx.tr("calendar.in.now");
        }
        if minutes < 60 {
            return cx.plural("calendar.in.minutes", minutes, &[]);
        }
        if minutes < 360 {
            return cx.plural("calendar.in.hours", minutes / 60, &[]);
        }
    }
    let day = day_text(cx.host, &mut cx.messages, occ.day, env.today)?;
    if occ.whole_day() {
        return Ok(day);
    }
    let time = time_text(cx.host, &mut cx.messages, env.twelve, occ.secs)?;
    concat(&[&day, " ", &time])
}

/// The next event, large, and below it when and where.
fn draw_next(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    area: Rect,
    (store, env): (&Store, &Env),
) -> Result<(), Error> {
    let look = cx.shape.paint(out, area, 256)?;
    let inner = look.area;
    let Some(occ) = next_event(store, env)? else {
        return message(cx, out, (inner, look.faint), "calendar.widget.next.empty");
    };
    let Some(entry) = store.entries.get(usize::from(occ.entry)) else {
        return Ok(());
    };
    let (text, small) = (cx.style().text, cx.style().small);
    let line = line_px(&cx.font, small)?;
    let align = cx.settings.align()?;
    let title = if entry.mark == Mark::Event {
        copy(&entry.title)?
    } else {
        aged(cx.host, &mut cx.messages, &entry.title, occ.age)?
    };
    let mut below = String::new();
    if cx.settings.shows("until")? {
        below = until_text(cx, &occ, env)?;
    }
    if cx.settings.shows("place")? && !entry.place.is_empty() {
        below = if below.is_empty() {
            copy(&entry.place)?
        } else {
            concat(&[&below, " · ", &entry.place])?
        };
    }
    let lower = if below.is_empty() || inner.height < line.saturating_mul(3) {
        0
    } else {
        line
    };
    let main = Rect::new(
        inner.x,
        inner.y,
        inner.width,
        inner.height.saturating_sub(lower),
    );
    one_line(
        out,
        &cx.font,
        main,
        align,
        (text * 1.4, small),
        &title,
        look.ink,
    )?;
    if lower > 0 {
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

/// The month's data for a widget or the screen: its names and busy days.
fn month_of(
    cx: &mut Cx<'_>,
    store: &Store,
    env: &Env,
) -> Result<(i32, u8, Vec<u8>, Vec<String>), Error> {
    let (year, month, _) = civil_from_days(i64::from(env.today));
    let first = i32::try_from(paperpaw_caldav::date::days_from_civil(year, month, 1)).unwrap_or(0);
    let last = first + i32::from(paperpaw_caldav::date::days_in_month(year, month)) - 1;
    let busy = store.busy(first, last)?;
    let mut names = Vec::new();
    for n in 0..7 {
        let name = weekday_name(cx.host, &mut cx.messages, n, true)?;
        mem::push(&mut names, name)?;
    }
    Ok((year, month, busy, names))
}

fn draw_month(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    area: Rect,
    (store, env): (&Store, &Env),
) -> Result<Vec<(Rect, i32)>, Error> {
    let look = cx.shape.paint(out, area, 256)?;
    let (year, month, busy, names) = month_of(cx, store, env)?;
    let mut inner = look.area;
    let style = cx.style();
    let (small, gap, text, line) = (style.small, style.gap, style.text, style.line);
    if cx.settings.shows("caption")? {
        let name = month_name(cx.host, &mut cx.messages, month, false)?;
        let mut digits = [0u8; 20];
        let number =
            paperpaw_ui_kit::text::decimal(usize::try_from(year).unwrap_or(0), &mut digits);
        let title = concat(&[&name, " ", number])?;
        let align = cx.settings.align()?;
        inner = caption(
            out,
            &cx.font,
            (small, gap),
            (inner, align, look.faint),
            &title,
        )?;
    }
    let dots = cx.settings.shows("dots")?;
    let shown = Month {
        year,
        month,
        week_start: env.week_start,
        today: env.today,
        selected: None,
        busy: &busy,
        dots,
        names: &names,
    };
    let palette = Palette {
        ink: look.ink,
        faint: look.faint,
        paper: look.paper,
    };
    // Digits as large as the cells let them be.
    let cell = f32::from(inner.width / 7).min(f32::from(inner.height / 7));
    // Not `clamp`: its panic message formats floats (a float formatter and a
    // 2 KiB frame in the plugin file).
    let size = (cell * 0.55).max(small * 0.8).min(text);
    native_month::paint_month(out, &cx.font, (size, line), palette, inner, &shown)
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
    data: (&Store, &Env),
) -> Result<(), Error> {
    match token {
        TODAY_TOKEN | AGENDA_TOKEN | BIRTHDAYS_TOKEN => draw_listing(cx, out, area, token, data),
        NEXT_TOKEN => draw_next(cx, out, area, data),
        MONTH_TOKEN => draw_month(cx, out, area, data).map(|_| ()),
        _ => Err(Error::NotFound),
    }
}

/// A rectangle of the widget as a target's.
fn frame_of(rect: Rect) -> FrameRect {
    paint::frame_rect(rect)
}

/// The targets of the widget placed in `area`: an event opens, a day of the
/// month opens the day; the heading, "more" and an empty widget open the
/// agenda.
///
/// # Errors
/// Font and host refusals, `NoMemory`.
pub fn targets(
    cx: &mut Cx<'_>,
    area: Rect,
    token: u32,
    (store, env): (&Store, &Env),
) -> Result<Vec<WidgetTarget>, Error> {
    let mut targets = Vec::new();
    let whole = |id: u32| WidgetTarget::new(id, frame_of(area));
    match token {
        NEXT_TOKEN => {
            let id = next_event(store, env)?.map_or(OPEN_TARGET, |occ| event_target(store, &occ));
            mem::push(&mut targets, whole(id))?;
            return Ok(targets);
        }
        MONTH_TOKEN => {
            let mut scratch = Vec::new();
            let cells = draw_month(cx, &mut scratch, area, (store, env))?;
            for (rect, day) in cells {
                let id = DAY_TARGET | (u32::try_from(day).unwrap_or(0) & 0x3fff_ffff);
                mem::push(&mut targets, WidgetTarget::new(id, frame_of(rect)))?;
            }
            return Ok(targets);
        }
        _ => {}
    }
    let listing = listing(cx, token, store, env)?;
    if listing.lines.is_empty() {
        mem::push(&mut targets, whole(OPEN_TARGET))?;
        return Ok(targets);
    }
    let look = cx.shape.look(area);
    let places = places(cx, look.area, token, &listing)?;
    if let Some(head) = places.head {
        mem::push(&mut targets, WidgetTarget::new(OPEN_TARGET, frame_of(head)))?;
    }
    for (row, line) in places.rows.iter().zip(&listing.lines) {
        let id = match line {
            Line::Event { target, .. } => *target,
            Line::Heading(_) => OPEN_TARGET,
        };
        mem::push(&mut targets, WidgetTarget::new(id, frame_of(*row)))?;
    }
    if let Some((row, _)) = places.more {
        mem::push(&mut targets, WidgetTarget::new(OPEN_TARGET, frame_of(row)))?;
    }
    Ok(targets)
}
