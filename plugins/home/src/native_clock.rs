//! The clock, as the home screen draws it: the time large (the digits as large
//! as the size setting says or the place holds), what follows them ("PM")
//! beside them small, and the date below in proportion, left out only where
//! the time at the body's size and the date at the small size do not fit
//! together.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Rect,
    native::{Error, Host},
    time::LocalTime,
};
use paperpaw_ui_kit::widget::{
    Cx, TextSize,
    text::{self as widget_text, largest_size},
};
use paperpaw_ui_kit::{Paint, text as plain};

use crate::native_date::short_date;
use paperpaw_ui_kit::widget::names::date_words;

/// How wide the time is by the clock's size setting, as a share of the
/// screen's width: small, medium (the first home screen's, on every device)
/// and large clearly apart, the same wherever it is placed (as far as the place
/// holds it); as large as fits ([`TextSize::Fit`]) fills the place instead.
const fn set_share(size: TextSize) -> f32 {
    match size {
        TextSize::Small => 0.15,
        TextSize::Large => 0.32,
        _ => 0.21,
    }
}

/// The date's share of the time's size: it grows with the time, never smaller
/// than small text.
const DATE_SHARE: f32 = 0.24;

/// Where the time's digits end and what follows them begins ("PM"): the digits
/// (or the dashes of a clock not yet set) are drawn large, the rest beside
/// them at the date's size.
fn split_time(clock: &str) -> (&str, &str) {
    let end = clock
        .find(|c: char| !(c.is_ascii_digit() || c == ':' || c == '-' || is_digit_of_a_language(c)))
        .unwrap_or(clock.len());
    (
        clock.get(..end).unwrap_or(""),
        clock.get(end..).unwrap_or("").trim(),
    )
}

/// A digit of another script (the clock is shown in the language's digits).
fn is_digit_of_a_language(c: char) -> bool {
    c.is_numeric()
}

/// What the clock shows: the time (as the clock is set) and the date.
struct Shown {
    clock: String,
    long: Option<String>,
    short: Option<String>,
}

impl Shown {
    /// The text of the time and, where the date is wished and known, the date
    /// in full and in short words.
    fn read(cx: &mut Cx<'_>, wants_date: bool) -> Result<Self, Error> {
        let time = cx.local_time()?;
        let clock = match &time {
            Some(t) => cx.clock_of(t.hour, t.minute)?,
            None => plain::copy("--:--")?,
        };
        let (long, short) = match (&time, wants_date) {
            (Some(t), true) => (
                Some(date_words(cx, t.month, t.day, t.weekday)?),
                Some(short_date(cx, t)?),
            ),
            _ => (None, None),
        };
        Ok(Self { clock, long, short })
    }
}

/// How the clock is laid out in a width by a height: the time's size, the
/// date's with its text where it shows, and the ink of both from their
/// baselines.
struct Layout {
    size: f32,
    /// The size of the date and of what follows the digits ("PM").
    small: f32,
    date: Option<String>,
    /// The ink of the digits and of the date, from their baselines (the top
    /// negative).
    digits_ink: (i32, i32),
    date_ink: (i32, i32),
}

/// What a layout is made of.
struct Room<'a, 'c> {
    cx: &'a Cx<'c>,
    shown: &'a Shown,
    digits: &'a str,
    rest: &'a str,
    width: f32,
    height: f32,
}

/// The top and bottom of the ink of `text` at `size` from its baseline; the
/// line's where it draws nothing.
fn ink_of(cx: &Cx<'_>, size: f32, text: &str) -> Result<(i32, i32), Error> {
    match cx.font.ink(size, text)? {
        Some(ink) => paperpaw_ui_kit::geometry::check_ink(ink.top, ink.bottom),
        None => {
            let (ascent, descent, _) = paperpaw_ui_kit::geometry::line_metrics(&cx.font, size)?;
            Ok((-(ascent as i32), -(descent as i32)))
        }
    }
}

impl Layout {
    /// The ink's height, the date's and the gap between included.
    fn height(&self, gap: i32) -> i32 {
        let date = match self.date {
            Some(_) => gap + self.date_ink.1 - self.date_ink.0,
            None => 0,
        };
        self.digits_ink.1 - self.digits_ink.0 + date
    }

    /// The layout at `size`, and whether it fits the room.
    fn at(room: &Room<'_, '_>, size: f32, with_date: bool) -> Result<(bool, Self), Error> {
        let cx = room.cx;
        let style = cx.style();
        let gap = i32::from(style.gap);
        let small = (size * DATE_SHARE).max(style.small);
        let rest_width = if room.rest.is_empty() {
            0.0
        } else {
            cx.font.measure_text(small, " ")? + cx.font.measure_text(small, room.rest)?
        };
        let wide = cx.font.measure_text(size, room.digits)? + rest_width;
        let digits_ink = ink_of(cx, size, room.digits)?;
        let date = if with_date {
            fitting_date(room, small, style.small)?
        } else {
            None
        };
        let date_ink = match &date {
            Some((size, text)) => ink_of(cx, *size, text)?,
            None => (0, 0),
        };
        let high = digits_ink.1 - digits_ink.0
            + date.as_ref().map_or(0, |_| gap + date_ink.1 - date_ink.0);
        let layout = Self {
            size,
            small: date.as_ref().map_or(small, |(size, _)| *size),
            date: date.map(|(_, text)| text),
            digits_ink,
            date_ink,
        };
        Ok((
            wide <= room.width + 1.0 && high <= room.height as i32,
            layout,
        ))
    }

    /// The time as large as its size setting says and the place holds: as
    /// large as fits by [`TextSize::Fit`], at most that else; the date below
    /// it in proportion (in short words where the full ones do not fit), left
    /// out only where not even the time at the body's size and the date at the
    /// small size fit together.
    fn of(room: &Room<'_, '_>, setting: TextSize) -> Result<Self, Error> {
        let cx = room.cx;
        let style = cx.style();
        let size_for = |wide: f32| -> Result<f32, Error> {
            let hundred = cx.font.measure_text(100.0, room.digits)?.max(1.0);
            Ok(100.0 * wide / hundred)
        };
        let most = match setting {
            TextSize::Fit => size_for(room.width)?,
            size => size_for(set_share(size) * f32::from(cx.snapshot.display_width()))?,
        };
        let wants_date = room.shown.long.is_some();
        let fitting = |with_date: bool| -> Result<Option<Self>, Error> {
            let mut found = None;
            largest_size(most, style.text.min(most), |size| {
                let (fits, layout) = Self::at(room, size, with_date)?;
                if fits {
                    found = Some(layout);
                }
                Ok(fits)
            })?;
            Ok(found)
        };
        if let Some(layout) = fitting(wants_date)? {
            return Ok(layout);
        }
        if wants_date && let Some(layout) = fitting(false)? {
            return Ok(layout);
        }
        Ok(Self::at(room, style.text, false)?.1)
    }
}

/// The date as large as fits `room`'s width whole between `max` and `min`: in
/// full words, else in short ones; `None` where not even the short ones do.
fn fitting_date(room: &Room<'_, '_>, max: f32, min: f32) -> Result<Option<(f32, String)>, Error> {
    for text in [&room.shown.long, &room.shown.short].into_iter().flatten() {
        let fits = |size: f32| Ok(room.cx.font.measure_text(size, text)? <= room.width);
        if let Some(size) = largest_size(max, min.min(max), fits)? {
            return Ok(Some((size, plain::copy(text)?)));
        }
    }
    Ok(None)
}

/// The height of the widget: as high as the time and the date need at their
/// size (as large as fits is as wide as the place), the room a line of text
/// has above and below ink, the frame.
///
/// # Errors
/// Font and host refusals.
pub fn height(cx: &mut Cx<'_>, width: u16) -> Result<u16, Error> {
    let wants_date = cx.settings.shows("date")?;
    let shown = Shown::read(cx, wants_date)?;
    let (digits, rest) = split_time(&shown.clock);
    let setting = cx.settings.size()?;
    let inner = f32::from(width.saturating_sub(cx.shape.frame_height().min(width / 2)));
    let room = Room {
        cx: &*cx,
        shown: &shown,
        digits,
        rest,
        width: inner,
        height: f32::MAX,
    };
    let layout = Layout::of(&room, setting)?;
    let style = cx.style();
    // The room above and below the ink a line of text has.
    let around = paperpaw_ui_kit::geometry::line_px(&cx.font, style.small)? / 2;
    let ink = u16::try_from(layout.height(i32::from(style.gap)).max(0)).unwrap_or(u16::MAX);
    Ok(ink
        .saturating_add(around.saturating_mul(2))
        .saturating_add(cx.shape.frame_height()))
}

/// What the widget shows, to tell when it changed: the minute (0 where the
/// time is not known).
///
/// # Errors
/// Host refusals.
pub fn shows(host: &Host<'_>) -> Result<u64, Error> {
    Ok(
        paperpaw_ui_kit::widget::cx::local_time_now(host)?.map_or(0, |t: LocalTime| {
            (u64::from(t.year.unsigned_abs()) * 10_000
                + u64::from(t.month) * 100
                + u64::from(t.day)
                + 1)
                * 10_000
                + u64::from(t.hour) * 100
                + u64::from(t.minute)
        }),
    )
}

/// Draws the widget in `area`.
///
/// # Errors
/// Font and host refusals, [`Error::NoMemory`].
pub fn draw(cx: &mut Cx<'_>, out: &mut Vec<Paint>, area: Rect) -> Result<(), Error> {
    let look = cx.shape.paint(out, area, 256)?;
    let area = look.area;
    let wants_date = cx.settings.shows("date")?;
    let shown = Shown::read(cx, wants_date)?;
    let (digits, rest) = split_time(&shown.clock);
    let setting = cx.settings.size()?;
    let align = cx.settings.align()?;
    let room = Room {
        cx: &*cx,
        shown: &shown,
        digits,
        rest,
        width: f32::from(area.width),
        height: f32::from(area.height),
    };
    let layout = Layout::of(&room, setting)?;
    let gap = i32::from(cx.style().gap);
    let size = layout.size;
    let (top, bottom) = layout.digits_ink;
    // Together in the middle by their ink.
    let total = layout.height(gap);
    let y = i32::from(area.y) + (i32::from(area.height) - total) / 2;
    let rest_width = if rest.is_empty() {
        0.0
    } else {
        cx.font.measure_text(layout.small, " ")? + cx.font.measure_text(layout.small, rest)?
    };
    // Shortened only where not even the body's size fits (a sliver).
    let digits = widget_text::fit_line(
        &cx.font,
        size,
        size,
        digits,
        f32::from(area.width) - rest_width,
    )?
    .1;
    let digits_width = cx.font.measure_text(size, &digits)?;
    let x = align.x(area, digits_width + rest_width);
    let baseline = y - top;
    paperpaw_ui_kit::paint::push(
        out,
        Paint::Text {
            text: digits,
            size,
            origin: paperpaw_sdk::native::fonts::TextOrigin::new(x, baseline),
            color: look.ink,
            clip: None,
        },
    )?;
    if !rest.is_empty() {
        let x = x + digits_width + cx.font.measure_text(layout.small, " ")?;
        paperpaw_ui_kit::paint::push(
            out,
            Paint::Text {
                text: plain::copy(rest)?,
                size: layout.small,
                origin: paperpaw_sdk::native::fonts::TextOrigin::new(x, baseline),
                color: look.ink,
                clip: None,
            },
        )?;
    }
    if let Some(text) = &layout.date {
        let (date_top, _) = layout.date_ink;
        let y = y + (bottom - top) + gap;
        let x = align.x(area, cx.font.measure_text(layout.small, text)?);
        paperpaw_ui_kit::paint::push(
            out,
            Paint::Text {
                text: plain::copy(text)?,
                size: layout.small,
                origin: paperpaw_sdk::native::fonts::TextOrigin::new(x, y - date_top),
                color: look.faint,
                clip: None,
            },
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_digits_end_where_what_follows_them_begins() {
        assert_eq!(split_time("14:05"), ("14:05", ""));
        assert_eq!(split_time("2:05 PM"), ("2:05", "PM"));
        assert_eq!(split_time("--:--"), ("--:--", ""));
        assert_eq!(split_time("02:05 am"), ("02:05", "am"));
        // The digits of a language are digits too.
        assert_eq!(
            split_time("\u{661}\u{662}:\u{660}\u{665}"),
            ("\u{661}\u{662}:\u{660}\u{665}", "")
        );
        assert_eq!(split_time(""), ("", ""));
        assert_eq!(split_time("PM"), ("", "PM"));
    }

    #[test]
    fn small_medium_and_large_are_clearly_apart_and_fit_has_its_own_rule() {
        let (small, medium, large) = (
            set_share(TextSize::Small),
            set_share(TextSize::Medium),
            set_share(TextSize::Large),
        );
        assert!(small < medium && medium < large);
        assert!(large < 0.5);
    }
}
