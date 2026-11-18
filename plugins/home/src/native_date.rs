//! The date in words, as the home screen's date widget draws it: as large as
//! its size setting says, smaller down to the body's size rather than cut short,
//! then in short words ("Thu 15 Oct"), and where not even those fit a line, in
//! lines ("Thu" above "15 Oct") down to the small size.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Rect,
    native::{Error, formatting::Arg},
    time::LocalTime,
};
use paperpaw_ui_kit::widget::{
    Cx,
    names::{date_words, month_short, weekday_short},
    text::{self, Wrapped, largest_size},
};
use paperpaw_ui_kit::{Paint, text as plain};

/// Today in short words ("Thu 15 Oct"): the language's `core.date.short`.
///
/// # Errors
/// The host's refusal and [`Error::NoMemory`].
pub fn short_date(cx: &mut Cx<'_>, t: &LocalTime) -> Result<String, Error> {
    let weekday = weekday_short(cx, t.weekday)?;
    let month = month_short(cx, t.month)?;
    cx.format(
        "core.date.short",
        &[
            Arg::Text {
                name: "weekday",
                value: &weekday,
            },
            Arg::Int {
                name: "day",
                value: i64::from(t.day),
            },
            Arg::Text {
                name: "month",
                value: &month,
            },
        ],
    )
}

/// The height of the widget: a line of the size set and the frame; nothing
/// where the date is not known.
///
/// # Errors
/// Font and host refusals.
pub fn height(cx: &mut Cx<'_>, _width: u16) -> Result<u16, Error> {
    if cx.local_time()?.is_none() {
        return Ok(0);
    }
    let size = cx.settings.size()?.text(cx.style());
    let line = paperpaw_ui_kit::geometry::line_px(&cx.font, size)?;
    Ok(line.saturating_add(cx.shape.frame_height()))
}

/// What the widget shows, to tell when it changed: the day (0 where the date
/// is not known).
///
/// # Errors
/// Host refusals.
pub fn shows(host: &paperpaw_sdk::native::Host<'_>) -> Result<u64, Error> {
    Ok(
        paperpaw_ui_kit::widget::cx::local_time_now(host)?.map_or(0, |t| {
            u64::from(t.year.unsigned_abs()) * 10_000
                + u64::from(t.month) * 100
                + u64::from(t.day)
                + 1
        }),
    )
}

/// Draws the widget in `area`.
///
/// # Errors
/// Font and host refusals, [`Error::NoMemory`].
pub fn draw(cx: &mut Cx<'_>, out: &mut Vec<Paint>, area: Rect) -> Result<(), Error> {
    // The snapshot does not tell the gray levels: a card is taken to be drawn
    // on a panel with grays.
    let look = cx.shape.paint(out, area, 256)?;
    let Some(t) = cx.local_time()? else {
        return Ok(());
    };
    let area = look.area;
    let align = cx.settings.align()?;
    let (max, small, body) = {
        let style = cx.style();
        let max = cx.settings.size()?.text(style);
        (max, style.small, style.text.min(max))
    };
    let width = f32::from(area.width);
    let long = date_words(cx, t.month, t.day, t.weekday)?;
    let short = short_date(cx, &t)?;
    for words in [&long, &short] {
        let fits = |size: f32| Ok(cx.font.measure_text(size, words.as_str())? <= width);
        if let Some(size) = largest_size(max, body, fits)? {
            text::line(out, &cx.font, area, align, (size, size), words, look.ink)?;
            return Ok(());
        }
    }
    // The weekday above the day and the month.
    let lines = match short.split_once(' ') {
        Some((weekday, rest)) => plain::concat(&[weekday.trim_end_matches(','), "\n", rest])?,
        None => plain::copy(&short)?,
    };
    let wrapped = Wrapped::fitting(
        &cx.font,
        &lines,
        (max, small),
        (area.width, area.height),
        false,
    )?;
    wrapped.draw(out, &cx.font, area, align, look.ink)?;
    Ok(())
}
