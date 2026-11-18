//! A greeting by the time of day: "Good morning", "Good afternoon", "Good
//! evening", "Good night", with the reader's name if set ("Good evening,
//! Anna"), the date below if wanted, and a sun or a moon before it.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Color, Point, Rect,
    native::{Error, Host, fonts::Font, formatting::Arg},
    round::point_at,
};
use paperpaw_ui_kit::{
    Paint, mem,
    widget::{
        Align, Cx, TextSize,
        cx::{face_font, local_time_now},
        layout::{break_words, label},
        names::{chosen_name, date_words},
        shape::{fill_circle, line as stroke_line},
        text::largest_size,
    },
};

/// The part of the day an hour is in: morning from 5, afternoon from 12,
/// evening from 18, night from 22.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Morning,
    Afternoon,
    Evening,
    Night,
}

impl Part {
    #[must_use]
    pub const fn of(hour: u8) -> Self {
        match hour {
            5..=11 => Self::Morning,
            12..=17 => Self::Afternoon,
            18..=21 => Self::Evening,
            _ => Self::Night,
        }
    }

    /// The translation key of its greeting.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Morning => "widgets.greeting.morning",
            Self::Afternoon => "widgets.greeting.afternoon",
            Self::Evening => "widgets.greeting.evening",
            Self::Night => "widgets.greeting.night",
        }
    }

    /// The translation key of its greeting with a name.
    #[must_use]
    pub const fn named_key(self) -> &'static str {
        match self {
            Self::Morning => "widgets.greeting.morning.named",
            Self::Afternoon => "widgets.greeting.afternoon.named",
            Self::Evening => "widgets.greeting.evening.named",
            Self::Night => "widgets.greeting.night.named",
        }
    }

    /// The sun by day, the moon by night.
    #[must_use]
    pub const fn sun(self) -> bool {
        matches!(self, Self::Morning | Self::Afternoon)
    }
}

/// `date` as it fits: whole, or without its weekday ("Wednesday, 14 October"
/// gives "14 October").
pub fn short_date(date: &str, fits: impl Fn(&str) -> bool) -> &str {
    match date.split_once(", ") {
        Some((_, short)) if !fits(date) => short,
        _ => date,
    }
}

/// The part of the day now; the afternoon where the time is not known.
fn part(cx: &Cx<'_>) -> Result<Part, Error> {
    Ok(cx
        .local_time()?
        .map_or(Part::Afternoon, |t| Part::of(t.hour)))
}

/// "Good evening", or "Good evening, Anna" with a name.
fn greeting(cx: &mut Cx<'_>, part: Part, named: bool) -> Result<String, Error> {
    let name = if named {
        cx.settings.string("name", crate::native_draw::TEXT_MAX)?
    } else {
        String::new()
    };
    let name = name.trim();
    if name.is_empty() {
        cx.tr(part.key())
    } else {
        cx.format(
            part.named_key(),
            &[Arg::Text {
                name: "name",
                value: name,
            }],
        )
    }
}

/// The line below it: the date, or nothing.
fn below(cx: &mut Cx<'_>) -> Result<Option<String>, Error> {
    if chosen_name(cx, "line")?.as_str() == "none" {
        return Ok(None);
    }
    let Some(t) = cx.local_time()? else {
        return Ok(None);
    };
    date_words(cx, t.month, t.day, t.weekday).map(Some)
}

/// The settings whose text a change of shows (a hash of each is kept in the
/// mark).
const MARKED: [&str; 4] = ["name", "line", GLYPH, "face"];
/// The setting that names the glyph beside the greeting.
const GLYPH: &str = "icon";

/// What it shows, to tell whether it changed: the part of the day and the
/// date.
///
/// # Errors
/// Host refusals.
pub fn shows(host: &Host<'_>, widget: &str) -> Result<u64, Error> {
    let Some(t) = local_time_now(host)? else {
        return Ok(0);
    };
    let settings = paperpaw_ui_kit::widget::Settings::new(host, widget);
    let mut mark = crate::native_draw::day_mark(&t) * 4 + part_index(Part::of(t.hour));
    for key in MARKED {
        mark = crate::native_draw::fnv(
            mark,
            settings
                .string(key, crate::native_draw::TEXT_MAX)?
                .as_bytes(),
        );
    }
    Ok(mark)
}

const fn part_index(part: Part) -> u64 {
    match part {
        Part::Morning => 0,
        Part::Afternoon => 1,
        Part::Evening => 2,
        Part::Night => 3,
    }
}

fn big_size(cx: &Cx<'_>) -> Result<f32, Error> {
    let style = cx.style();
    Ok(match cx.settings.size()? {
        TextSize::Fit => style.title * 1.6,
        size => size.text(style),
    })
}

/// The height: the greeting's line, the date's line, the frame.
///
/// # Errors
/// Font and host refusals.
pub fn height(cx: &mut Cx<'_>, _width: u16) -> Result<u16, Error> {
    let size = big_size(cx)?;
    let date = if below(cx)?.is_some() {
        paperpaw_ui_kit::geometry::line_px(&cx.font, cx.style().small)?
    } else {
        0
    };
    Ok(paperpaw_ui_kit::geometry::line_px(&cx.font, size)?
        .saturating_add(date)
        .saturating_add(cx.shape.frame_height()))
}

/// A sun (a disc with rays) or a moon (a disc with a bite out of it) about
/// `centre`, `size` pixels high.
fn draw_icon(
    out: &mut Vec<Paint>,
    sun: bool,
    centre: Point,
    size: i32,
    ink: Color,
    paper: Color,
) -> Result<(), Error> {
    let radius = u32::try_from(size / 2).unwrap_or(0);
    if sun {
        let disc = (radius * 5 / 10).max(1);
        fill_circle(out, centre, disc, ink)?;
        let width = u32::from(size >= 24) + 1;
        for n in 0..8 {
            let from = point_at(centre, (disc as i32) + 2, n * 45);
            let to = point_at(centre, radius as i32, n * 45);
            stroke_line(out, from, to, width, ink)?;
        }
    } else {
        fill_circle(out, centre, radius, ink)?;
        let bite = Point::new(
            centre.x.saturating_add(size / 4),
            centre.y.saturating_sub(size / 6),
        );
        fill_circle(out, bite, (radius * 8 / 10).max(1), paper)?;
    }
    Ok(())
}

/// The size and lines of the greeting: on one line as large as the width and
/// the room allow; where it has no room, in two lines broken between words,
/// without the icon; else shortened on one; `None` where not even that has
/// room.
fn choose(
    cx: &Cx<'_>,
    font: &Font<'_>,
    text: String,
    (icon, room, width): (bool, f32, f32),
    height: u16,
) -> Result<Option<(f32, Vec<String>, bool)>, Error> {
    let small = cx.style().small;
    let (max, min) = match cx.settings.size()? {
        TextSize::Fit => (room.max(small), small),
        size => (size.text(cx.style()), size.text(cx.style()).min(small)),
    };
    let line_of = |size: f32| paperpaw_ui_kit::geometry::line_px(font, size);
    let one = largest_size(max, min, |size| {
        Ok(
            font.measure_text(size, &text)? + icon_width(icon, size) <= width
                && f32::from(line_of(size)?) <= room,
        )
    })?;
    if let Some(size) = one {
        return Ok(Some((size, mem::one(text)?, icon)));
    }
    let two = largest_size(max, min, |size| {
        let (lines, too_wide) = break_words(font, size, &text, width)?;
        Ok(!too_wide && lines.len() <= 2 && f32::from(line_of(size)?) * lines.len() as f32 <= room)
    })?;
    if let Some(size) = two {
        return Ok(Some((
            size,
            break_words(font, size, &text, width)?.0,
            false,
        )));
    }
    if f32::from(line_of(min)?) > f32::from(height) {
        return Ok(None);
    }
    let shown = paperpaw_ui_kit::text::fit(font, min, &text, width)?;
    if font.measure_text(min, &shown)? > width {
        return Ok(None);
    }
    Ok(Some((min, mem::one(shown)?, false)))
}

/// The room the icon takes before text `size` pixels large.
fn icon_width(on: bool, size: f32) -> f32 {
    if on { size * 1.3 } else { 0.0 }
}

/// Draws the widget in `area`.
///
/// # Errors
/// Font and host refusals, [`Error::NoMemory`].
pub fn draw(cx: &mut Cx<'_>, out: &mut Vec<Paint>, area: Rect) -> Result<(), Error> {
    let look = cx.shape.paint(out, area, 256)?;
    let area = look.area;
    let face = cx.settings.string("face", 96)?;
    let font = face_font(cx.host, &face)?;
    let part = part(cx)?;
    let small = cx.style().small;
    let width = f32::from(area.width);
    // The date below, where it fits whole (without its weekday if need be)
    // and leaves a line for the greeting.
    let small_line = paperpaw_ui_kit::geometry::line_px(&cx.font, small)?;
    let mut date: Option<String> = None;
    if let Some(full) = below(cx)? {
        let fits = |text: &str| -> bool {
            cx.font
                .measure_text(small, text)
                .is_ok_and(|wide| wide <= width)
        };
        let shown = short_date(&full, fits);
        let room = area.height.saturating_sub(small_line);
        if fits(shown) && room >= paperpaw_ui_kit::geometry::line_px(&font, small)? {
            date = Some(paperpaw_ui_kit::text::copy(shown)?);
        }
    }
    let date_line = date.as_ref().map_or(0, |_| small_line);
    let room = f32::from(area.height.saturating_sub(date_line));
    // Where it is narrow, without the name, then without the icon.
    let mut icon = cx.settings.shows("icon")?;
    let mut text = greeting(cx, part, true)?;
    let fits = |text: &str, icon: bool| -> bool {
        font.measure_text(small, text)
            .is_ok_and(|wide| wide + icon_width(icon, small) <= width)
    };
    if !fits(&text, icon) {
        text = greeting(cx, part, false)?;
    }
    if !fits(&text, icon) {
        icon = false;
    }
    let Some((size, lines, icon)) = choose(cx, &font, text, (icon, room, width), area.height)?
    else {
        return Ok(());
    };
    let align: Align = cx.settings.align()?;
    let big = paperpaw_ui_kit::geometry::line_px(&font, size)?;
    let count = u16::try_from(lines.len()).unwrap_or(u16::MAX);
    let used = big.saturating_mul(count).saturating_add(date_line);
    let top = area.y + area.height.saturating_sub(used) / 2;
    for (n, shown) in lines.iter().enumerate() {
        let row = Rect::new(
            area.x,
            top + big * u16::try_from(n).unwrap_or(0),
            area.width,
            big,
        );
        let total = font.measure_text(size, shown)? + icon_width(icon, size);
        let x = align.x(area, total);
        let mut text_row = row;
        if icon {
            let side = (size * 0.9) as i32;
            let middle = Point::new(
                (x + size * 0.45) as i32,
                i32::from(row.y) + i32::from(row.height) / 2,
            );
            draw_icon(out, part.sun(), middle, side, look.ink, look.paper)?;
            let shift = icon_width(true, size) as i32;
            text_row.x = (x as i32 + shift).clamp(0, i32::from(u16::MAX)) as u16;
            text_row.width = row.width.saturating_sub(u16::try_from(shift).unwrap_or(0));
            label(out, &font, (size, Align::Left), text_row, shown, look.ink)?;
        } else {
            label(out, &font, (size, align), row, shown, look.ink)?;
        }
    }
    if let Some(date) = date {
        let row = Rect::new(area.x, top + big * count, area.width, small_line);
        label(out, &cx.font, (small, align), row, &date, look.faint)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greetings_follow_the_hours() {
        let parts: Vec<Part> = (0..24).map(Part::of).collect();
        assert_eq!(parts[4], Part::Night);
        assert_eq!(parts[5], Part::Morning);
        assert_eq!(parts[11], Part::Morning);
        assert_eq!(parts[12], Part::Afternoon);
        assert_eq!(parts[17], Part::Afternoon);
        assert_eq!(parts[18], Part::Evening);
        assert_eq!(parts[21], Part::Evening);
        assert_eq!(parts[22], Part::Night);
        assert_eq!(parts[0], Part::Night);
        assert_eq!(Part::Evening.key(), "widgets.greeting.evening");
        assert_eq!(Part::Evening.named_key(), "widgets.greeting.evening.named");
        assert!(Part::Morning.sun() && !Part::Night.sun());
    }

    #[test]
    fn dates_are_shortened_to_fit() {
        let date = "Wednesday, 14 October";
        assert_eq!(short_date(date, |_| true), date);
        assert_eq!(short_date(date, |_| false), "14 October");
        assert_eq!(short_date("14 October", |_| false), "14 October");
    }
}
