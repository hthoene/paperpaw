//! The world clock: the time in up to three other places (of the time zones
//! PaperPaw knows), each with its name and whether it is already tomorrow or
//! still yesterday there ("Tokyo, tomorrow 18:41"); as rows one below the
//! other, or side by side in a wide row.
use crate::native_cx::Cx;
use crate::native_dates::days_from_civil;
use crate::native_draw::{message_in, minute, no_time};
use crate::native_names::chosen_name;
use alloc::{format, string::String, vec::Vec};
use paperpaw_sdk::{
    Color, Rect,
    native::{Error, formatting::Arg},
    time::{LocalTime, TIME_ZONES, TimeZone},
};
use paperpaw_ui_kit::{
    Paint, paint,
    text::{concat, copy},
    widget::{
        Align,
        layout::{label, line, share},
        text::largest_size,
    },
};

/// The settings of the places.
const PLACES: [&str; 3] = ["place-1", "place-2", "place-3"];

/// What it shows, to tell when it changed: the minute.
///
/// # Errors
/// Host refusals.
pub fn shows(cx: &Cx<'_>) -> Result<String, Error> {
    minute(cx)
}

/// A place shown: its name, its time, how many days it is ahead of here, and
/// how many minutes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub name: String,
    pub time: LocalTime,
    pub days: i64,
    pub minutes: i64,
}

/// The place of time zone `rule` at `unix` seconds, here being `here` seconds
/// east of UTC.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn place(name: &str, rule: &str, unix: i64, here: i64) -> Result<Option<Place>, Error> {
    let Some(zone) = TimeZone::parse(rule) else {
        return Ok(None);
    };
    let offset = i64::from(zone.offset_at(unix));
    let there = LocalTime::from_seconds(unix + offset);
    let local = LocalTime::from_seconds(unix + here);
    let day = |t: &LocalTime| days_from_civil(t.year, t.month, t.day);
    Ok(Some(Place {
        // The first city of "Berlin, Vienna, Zurich, Paris, Rome".
        name: copy(name.split(',').next().unwrap_or(name).trim())?,
        time: there,
        days: day(&there) - day(&local),
        minutes: (offset - here) / 60,
    }))
}

/// The places chosen, here first if wished.
fn places(cx: &mut Cx<'_>) -> Result<Vec<Place>, Error> {
    let Some(unix) = cx.snapshot.unix_seconds() else {
        return Ok(Vec::new());
    };
    let here = i64::from(cx.zone_offset(unix)?);
    let mut places: Vec<Place> = Vec::new();
    places.try_reserve(4).map_err(|_| Error::NoMemory)?;
    if cx.settings.shows("here")?
        && let Some(now) = cx.local_time()?
    {
        places.push(Place {
            name: cx.tr("widgets.world-clock.here")?,
            time: now,
            days: 0,
            minutes: 0,
        });
    }
    for key in PLACES {
        let id = cx.settings.with_text(key, |id| {
            // A zone id that does not fit is none we know.
            let mut stack = paperpaw_ui_kit::widget::settings::Stack::<64>::new();
            core::fmt::Write::write_str(&mut stack, id)
                .is_ok()
                .then_some(stack)
        })?;
        let Some(id) = id else { continue };
        let Some((zone, rule)) = TIME_ZONES.iter().find(|(z, _)| *z == id.as_str()) else {
            continue;
        };
        let name = cx.tr(zone)?;
        if let Some(place) = place(&name, rule, unix, here)? {
            places.push(place);
        }
    }
    Ok(places)
}

/// "tomorrow", "yesterday" and "+7 h", as the settings want them.
fn note(cx: &mut Cx<'_>, place: &Place) -> Result<String, Error> {
    let mut parts: Vec<String> = Vec::new();
    parts.try_reserve(2).map_err(|_| Error::NoMemory)?;
    if cx.settings.shows("day")? {
        let key = match place.days {
            1.. => Some("widgets.world-clock.tomorrow"),
            ..=-1 => Some("widgets.world-clock.yesterday"),
            0 => None,
        };
        if let Some(key) = key {
            parts.push(cx.tr(key)?);
        }
    }
    if cx.settings.shows("offset")? && place.name != cx.tr("widgets.world-clock.here")? {
        parts.push(offset_text(cx, place.minutes)?);
    }
    let mut joined = String::new();
    for (n, part) in parts.iter().enumerate() {
        joined
            .try_reserve(part.len() + 4)
            .map_err(|_| Error::NoMemory)?;
        if n > 0 {
            joined.push_str(" · ");
        }
        joined.push_str(part);
    }
    Ok(joined)
}

/// A difference of `minutes` in hours: "+7 h", "−5:30 h", "±0 h".
///
/// # Errors
/// Host refusals and [`Error::NoMemory`].
pub fn offset_text(cx: &mut Cx<'_>, minutes: i64) -> Result<String, Error> {
    let sign = match minutes {
        1.. => "+",
        0 => "±",
        _ => "−",
    };
    let (h, m) = (minutes.abs() / 60, minutes.abs() % 60);
    let number = if m == 0 {
        format!("{sign}{h}")
    } else {
        format!("{sign}{h}:{m:02}")
    };
    let digits = cx.messages.digits(cx.host, &number)?;
    cx.format(
        "widgets.world-clock.offset",
        &[Arg::Text {
            name: "offset",
            value: &digits,
        }],
    )
}

/// The height: a row for each place, with the gaps, and the frame.
///
/// # Errors
/// Host and font refusals.
pub fn height(cx: &mut Cx<'_>, _: u16) -> Result<u16, Error> {
    let (title, small, gap) = (cx.style().title, cx.style().small, cx.style().gap);
    let count = places(cx)?.len().max(1) as u16;
    let row = paperpaw_ui_kit::geometry::line_px(&cx.font, title * 1.3)?
        .saturating_add(paperpaw_ui_kit::geometry::line_px(&cx.font, small)?);
    let _ = small;
    Ok(row
        .saturating_mul(count)
        .saturating_add(gap.saturating_mul(count - 1))
        .saturating_add(cx.shape.frame_height()))
}

/// Draws the widget in `area`.
///
/// # Errors
/// Font and host refusals, [`Error::NoMemory`].
#[allow(clippy::too_many_lines, reason = "one layout")]
pub fn draw(cx: &mut Cx<'_>, out: &mut Vec<Paint>, area: Rect) -> Result<(), Error> {
    let look = cx.shape.paint(out, area, 256)?;
    if cx.snapshot.unix_seconds().is_none() {
        return no_time(cx, out, &look);
    }
    let mut places = places(cx)?;
    if places.is_empty() {
        let message = cx.tr("widgets.world-clock.empty")?;
        return message_in(cx, out, &look, &message);
    }
    let area = look.area;
    let (small_size, text_size, gap) = (cx.style().small, cx.style().text, cx.style().gap);
    let columns = match chosen_name(cx, "layout")?.as_str() {
        "rows" => false,
        "columns" => true,
        _ => area.width / places.len() as u16 > area.height * 9 / 10 && places.len() > 1,
    };
    // Clocks of every place, once.
    let mut clocks: Vec<String> = Vec::new();
    clocks
        .try_reserve(places.len())
        .map_err(|_| Error::NoMemory)?;
    for p in &places {
        clocks.push(cx.clock_of(p.time.hour, p.time.minute)?);
    }
    // Rows with the name beside the time where every name has room there,
    // else with the name above it.
    let stacked_in =
        |cx: &Cx<'_>, places: &[Place], clocks: &[String], row_h: u16| -> Result<bool, Error> {
            if columns {
                return Ok(false);
            }
            for (place, clock) in places.iter().zip(clocks) {
                let size =
                    time_size(cx, clock, f32::from(row_h), f32::from(area.width))?.max(small_size);
                let name = name_size(text_size, small_size, row_h);
                if cx.font.measure_text(size, clock)?
                    + f32::from(gap)
                    + cx.font.measure_text(name, &place.name)?
                    > f32::from(area.width)
                {
                    return Ok(true);
                }
            }
            Ok(false)
        };
    // Where not all have room, those first chosen: each needs a line of the
    // small size, two where its name goes above its time.
    let small = line(&cx.font, small_size)?;
    let room = if columns { area.width } else { area.height };
    let mut n = places.len();
    while n > 1 {
        let each = share(room, gap, n as u16);
        let least = if stacked_in(cx, &places[..n], &clocks[..n], each)? {
            2.0 * small
        } else {
            small
        };
        if f32::from(each) >= least {
            break;
        }
        n -= 1;
    }
    places.truncate(n);
    clocks.truncate(n);
    let n = n as u16;
    let row_h = share(area.height, gap, n);
    let stacked = stacked_in(cx, &places, &clocks, row_h)?;
    // Places side by side or stacked alike: the times as large as the widest
    // allows, room for the name and the notes where any has one; where the
    // height has no room for them, the notes are left out, then the names.
    let mut notes_of: Vec<String> = Vec::new();
    notes_of
        .try_reserve(places.len())
        .map_err(|_| Error::NoMemory)?;
    for p in &places {
        notes_of.push(note(cx, p)?);
    }
    let common = {
        let any_note = !stacked && notes_of.iter().any(|n| !n.is_empty());
        let (w, h) = if columns {
            (share(area.width, gap, n), area.height)
        } else {
            (area.width, row_h)
        };
        let mut notes: f32 = if any_note { 2.0 } else { 1.0 };
        while notes > 0.0 && small * notes + small > f32::from(h) {
            notes -= 1.0;
        }
        let room = (f32::from(h) - small * notes).max(small);
        let fits = |size: f32| -> Result<bool, Error> {
            for clock in &clocks {
                if cx.font.measure_text(size, clock)? > f32::from(w) * 0.96 {
                    return Ok(false);
                }
            }
            Ok(true)
        };
        let size =
            largest_size((room * 0.8).max(small_size), small_size, fits)?.unwrap_or(small_size);
        (size, notes)
    };
    // Too flat for the name above the time: beside it, where it has room
    // there.
    let mut beside = columns && common.1 == 0.0;
    if beside {
        for (place, clock) in places.iter().zip(&clocks) {
            let w = f32::from(share(area.width, gap, n));
            let time = time_size(cx, clock, f32::from(area.height), w)?.max(small_size);
            let name = name_size(text_size, small_size, area.height);
            if cx.font.measure_text(time, clock)?
                + f32::from(gap)
                + cx.font.measure_text(name, &place.name)?
                > w
            {
                beside = false;
                break;
            }
        }
    }
    for (i, place) in places.iter().enumerate() {
        let cell = if columns {
            let w = share(area.width, gap, n);
            Rect::new(area.x + i as u16 * (w + gap), area.y, w, area.height)
        } else {
            Rect::new(area.x, area.y + i as u16 * (row_h + gap), area.width, row_h)
        };
        let (clock, note) = (&clocks[i], &notes_of[i]);
        if beside || !(columns || stacked) {
            draw_row(cx, out, &look, cell, (place, clock, note))?;
        } else {
            draw_column(cx, out, &look, cell, (place, clock, note), stacked, common)?;
        }
        // A fine line between places.
        if i > 0 && look.paper.luma() >= 128 {
            let thick = cx.style().line;
            let back = (gap / 2 + thick / 2).min(gap);
            let between = if columns {
                Rect::new(cell.x - back, cell.y, thick.min(gap), cell.height)
            } else {
                Rect::new(cell.x, cell.y - back, cell.width, thick.min(gap))
            };
            // Light gray; on a panel of black and white alone the interface-6
            // widget dotted it (the snapshot does not tell the grays yet).
            paint::push(out, Paint::Fill(between, Color::gray(200)))?;
        }
    }
    Ok(())
}

/// The size of a place's name beside its time in a row `height` high.
fn name_size(text: f32, small: f32, height: u16) -> f32 {
    text.min(f32::from(height) * 0.4).max(small)
}

/// The size of the time in a row `height` high and `width` wide: up to half
/// the width, and most of the height.
fn time_size(cx: &Cx<'_>, clock: &str, height: f32, width: f32) -> Result<f32, Error> {
    Ok(largest_size(height * 0.62, 1.0, |size| {
        Ok(cx.font.measure_text(size, clock)? <= width * 0.5)
    })?
    .unwrap_or(1.0))
}

/// A place as a row: its name and note on the left, the time large on the
/// right.
fn draw_row(
    cx: &Cx<'_>,
    out: &mut Vec<Paint>,
    look: &paperpaw_ui_kit::widget::Look,
    cell: Rect,
    (place, clock, note): (&Place, &str, &str),
) -> Result<(), Error> {
    let (small_size, text_size, gap) =
        (cx.style().small, cx.style().text, f32::from(cx.style().gap));
    let width = f32::from(cell.width);
    let size = time_size(cx, clock, f32::from(cell.height), width)?.max(small_size);
    let time_width = cx.font.measure_text(size, clock)?;
    let big = line(&cx.font, size)?.min(f32::from(cell.height));
    let time_row = Rect::new(
        cell.x,
        cell.y + ((f32::from(cell.height) - big) / 2.0) as u16,
        cell.width,
        big as u16,
    );
    let flipped = cx.settings.align()? == Align::Right;
    let (time_align, name_x) = if flipped {
        (Align::Left, cell.x + (time_width + gap) as u16)
    } else {
        (Align::Right, cell.x)
    };
    label(out, &cx.font, (size, time_align), time_row, clock, look.ink)?;
    // The name and its note, together in the middle of the row.
    let name_room = Rect::new(
        name_x,
        cell.y,
        (width - time_width - gap).max(0.0) as u16,
        cell.height,
    );
    let name_size = name_size(text_size, small_size, cell.height);
    let name_line = line(&cx.font, name_size)?;
    let note_line = if note.is_empty() {
        0.0
    } else {
        line(&cx.font, small_size)?
    };
    let two = name_line + note_line <= f32::from(cell.height);
    let name_align = if flipped { Align::Right } else { Align::Left };
    if two {
        let top = f32::from(cell.y) + (f32::from(cell.height) - name_line - note_line) / 2.0;
        let row = Rect::new(name_room.x, top as u16, name_room.width, name_line as u16);
        label(
            out,
            &cx.font,
            (name_size, name_align),
            row,
            &place.name,
            look.ink,
        )?;
        if !note.is_empty() {
            let row = Rect::new(
                name_room.x,
                (top + name_line) as u16,
                name_room.width,
                note_line as u16,
            );
            label(
                out,
                &cx.font,
                (small_size, name_align),
                row,
                note,
                look.faint,
            )?;
        }
    } else {
        let text = if note.is_empty() {
            copy(&place.name)?
        } else {
            concat(&[&place.name, ", ", note])?
        };
        label(
            out,
            &cx.font,
            (small_size, name_align),
            name_room,
            &text,
            look.ink,
        )?;
    }
    Ok(())
}

/// A place as a column: its name small above, the time large, the note below
/// (`compact`: beside the name); the time `size` large, room left for `notes`
/// lines of small text (as for the other places, so that they line up).
fn draw_column(
    cx: &Cx<'_>,
    out: &mut Vec<Paint>,
    look: &paperpaw_ui_kit::widget::Look,
    cell: Rect,
    (place, clock, note): (&Place, &str, &str),
    compact: bool,
    (size, notes): (f32, f32),
) -> Result<(), Error> {
    let small_size = cx.style().small;
    let align = cx.settings.align()?;
    let (mut note, mut name) = (copy(note)?, copy(&place.name)?);
    if compact && !note.is_empty() {
        // One line above: "Tokyo, tomorrow".
        name = concat(&[&name, ", ", &note])?;
        note = String::new();
    }
    let small = line(&cx.font, small_size)?;
    let room = (f32::from(cell.height) - small * notes).max(small);
    let big = line(&cx.font, size)?.min(room);
    let total = small * notes + big;
    let mut y = f32::from(cell.y) + (f32::from(cell.height) - total).max(0.0) / 2.0;
    let row = |y: f32, h: f32| Rect::new(cell.x, y as u16, cell.width, h as u16);
    // The name where there is room for it (see `draw`), the note too.
    if notes >= 1.0 {
        label(
            out,
            &cx.font,
            (small_size, align),
            row(y, small),
            &name,
            look.faint,
        )?;
        y += small;
    }
    label(out, &cx.font, (size, align), row(y, big), clock, look.ink)?;
    y += big;
    if notes >= 2.0 && !note.is_empty() {
        label(
            out,
            &cx.font,
            (small_size, align),
            row(y, small),
            &note,
            look.faint,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Wednesday, 14 October 2026, 10:41 in Berlin (summer time).
    const NOW: i64 = 1_791_967_260;
    const BERLIN: i64 = 7200;

    #[test]
    fn the_places_offered_are_the_time_zones() {
        let zones: Vec<&str> = TIME_ZONES.iter().map(|(id, _)| *id).collect();
        assert_eq!(crate::declarations::WORLD_ZONES[..], zones[..]);
        assert_eq!(
            crate::declarations::WORLD_ZONES_OR_NONE[0],
            "widgets.world-clock.none"
        );
        assert_eq!(crate::declarations::WORLD_ZONES_OR_NONE[1..], zones[..]);
    }

    #[test]
    fn places_are_ahead_or_behind() {
        let here = LocalTime::from_seconds(NOW + BERLIN);
        assert_eq!((here.day, here.hour, here.minute), (14, 10, 41));
        let tokyo = place("Tokyo, Seoul", "JST-9", NOW, BERLIN)
            .unwrap()
            .unwrap();
        assert_eq!(tokyo.name, "Tokyo");
        assert_eq!(
            (tokyo.time.hour, tokyo.time.minute, tokyo.days),
            (17, 41, 0)
        );
        assert_eq!(tokyo.minutes, 7 * 60);
        let new_york = place("New York", "EST5EDT,M3.2.0,M11.1.0", NOW, BERLIN)
            .unwrap()
            .unwrap();
        assert_eq!(
            (new_york.time.hour, new_york.days, new_york.minutes),
            (4, 0, -360)
        );
        // Late in the evening, Auckland is in tomorrow already.
        let evening = NOW + 12 * 3600;
        let auckland = place("Auckland", "NZST-12NZDT,M9.5.0,M4.1.0/3", evening, BERLIN)
            .unwrap()
            .unwrap();
        assert_eq!((auckland.time.day, auckland.days), (15, 1));
        // Early in the morning, Honolulu is still in yesterday.
        let morning = NOW - 9 * 3600;
        let honolulu = place("Honolulu", "HST10", morning, BERLIN)
            .unwrap()
            .unwrap();
        assert_eq!(honolulu.days, -1);
        let india = place("India", "IST-5:30", NOW, BERLIN).unwrap().unwrap();
        assert_eq!(india.minutes, 210);
    }
}
