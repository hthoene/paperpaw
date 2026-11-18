//! The three widgets: *Pomodoro timer* (phase, minutes left, a bar and the dots
//! of the set, a hint what a tap does), *Pomodoro start/pause* (a button and the
//! time beneath it) and *Pomodoro today* (the day's rounds, the goal, the week).
//!
//! Each draws from one [`Shown`] in whatever room its place has, measured in
//! pixels, not in cells: a figure when small, with the phase, bar, dots and hint
//! as the room allows. Nothing here asks for a redraw: the plugin tells the host
//! that a widget changed each minute while the timer runs, which is as often as
//! the minutes they show change. The interface-6 code was `widget.rs`.

use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Color, Point, Rect,
    native::{Error, formatting::Arg},
};
use paperpaw_ui_kit::{
    Paint,
    geometry::{clamp_u16, line_px},
    paint,
    widget::{
        Cx,
        shape::{fill, fill_circle, spans, stroke_circle},
        text::line as one_line,
    },
};

use crate::declarations::{BUTTON_TOKEN, TIMER_TOKEN, TODAY_TOKEN};
use crate::native_shown::{Shown, count_line, week_line};
use crate::phase::Phase;

/// The key of a phase's name, and of its short name for small places.
#[must_use]
pub const fn phase_key(phase: Phase, short: bool) -> &'static str {
    match (phase, short) {
        (Phase::Focus, false) => "pomodoro.phase.focus",
        (Phase::Short, false) => "pomodoro.phase.short",
        (Phase::Long, false) => "pomodoro.phase.long",
        (Phase::Focus, true) => "pomodoro.phase.focus.short",
        (Phase::Short, true) => "pomodoro.phase.short.short",
        (Phase::Long, true) => "pomodoro.phase.long.short",
    }
}

/// The key of the main button: Start, Pause, or Resume after a pause.
#[must_use]
pub const fn main_key(shown: &Shown) -> &'static str {
    if shown.running {
        "pomodoro.pause"
    } else if shown.fresh() {
        "pomodoro.start"
    } else {
        "pomodoro.resume"
    }
}

/// How high the widget with `token` is in a place `width` wide: the timer a
/// half square (at least four lines), the button and the count three lines.
///
/// # Errors
/// Font refusals.
pub fn height(cx: &Cx<'_>, token: u32, width: u16) -> Result<u16, Error> {
    let line = line_px(&cx.font, cx.style().small)?;
    let inner = match token {
        TIMER_TOKEN => (width / 2).max(line.saturating_mul(4)),
        _ => line.saturating_mul(3),
    };
    Ok(cx.shape.frame_height().saturating_add(inner))
}

/// Draws the widget with `token` in `area`.
///
/// # Errors
/// Font refusals, [`Error::NoMemory`].
pub fn draw(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    area: Rect,
    token: u32,
    shown: &Shown,
) -> Result<(), Error> {
    match token {
        TIMER_TOKEN => timer(cx, out, area, shown),
        BUTTON_TOKEN => button(cx, out, area, shown),
        TODAY_TOKEN => today(cx, out, area, shown),
        _ => Err(Error::NotFound),
    }
}

/// The pieces of a widget, top to bottom, and the room they ask for.
struct Bands {
    next: u16,
    end: u16,
}

impl Bands {
    fn new(area: Rect) -> Self {
        Self {
            next: area.y,
            end: area.y.saturating_add(area.height),
        }
    }

    fn left(&self) -> u16 {
        self.end.saturating_sub(self.next)
    }

    /// The next `height` pixels of `area`.
    fn take(&mut self, area: Rect, height: u16) -> Rect {
        let band = Rect::new(area.x, self.next, area.width, height.min(self.left()));
        self.next = self.next.saturating_add(band.height);
        band
    }
}

/// Whether `text` fits `width` pixels on one line at `px`.
fn fits(cx: &Cx<'_>, px: f32, text: &str, width: u16) -> Result<bool, Error> {
    Ok(cx.font.measure_text(px, text)? <= f32::from(width))
}

/// "24 min" (just "24" where that does not fit `width`): the last minute
/// reads "1 min", never "0".
fn minutes_text(cx: &mut Cx<'_>, shown: &Shown, width: u16) -> Result<String, Error> {
    let n = i64::try_from(shown.minutes).unwrap_or(0);
    let long = cx.format("pomodoro.widget.time", &[Arg::Count(n)])?;
    if fits(cx, cx.style().small, &long, width)? {
        Ok(long)
    } else {
        cx.number(n)
    }
}

/// The dots of a set, `done` of `every` filled, centred in `area`.
pub fn dots(
    out: &mut Vec<Paint>,
    area: Rect,
    (every, done): (u32, u32),
    ink: Color,
) -> Result<(), Error> {
    let every = i32::try_from(every.max(1)).unwrap_or(1);
    let radius = i32::from((area.height * 3 / 10).clamp(3, 14));
    let most = i32::from(area.width) / every;
    let step = (radius * 3).min(most.max(radius * 2 + 2));
    let width = step * (every - 1);
    let x0 = i32::from(area.x) + i32::from(area.width) / 2 - width / 2;
    let y = i32::from(area.y) + i32::from(area.height) / 2;
    let radius = u32::try_from(radius).unwrap_or(3);
    for n in 0..every {
        let centre = Point::new(x0 + step * n, y);
        if u32::try_from(n).unwrap_or(0) < done {
            fill_circle(out, centre, radius, ink)?;
        } else {
            stroke_circle(out, centre, radius, 2, ink)?;
        }
    }
    Ok(())
}

/// The bar of how much of the phase is gone: an outline, filled from the left.
pub fn bar(out: &mut Vec<Paint>, area: Rect, gone: u16, ink: Color) -> Result<(), Error> {
    if area.width < 4 || area.height < 2 {
        return Ok(());
    }
    let radius = u32::from(area.height / 2);
    paint::push(out, Paint::Focus(area, radius, Some(2), ink))?;
    let inner = Rect::new(
        area.x.saturating_add(3),
        area.y.saturating_add(3),
        area.width.saturating_sub(6),
        area.height.saturating_sub(6),
    );
    let filled = u32::from(inner.width) * u32::from(gone.min(1000)) / 1000;
    if filled > 0 && inner.height > 0 {
        let filled = clamp_u16(i64::from(filled));
        paint::push(
            out,
            Paint::Focus(
                Rect::new(inner.x, inner.y, filled, inner.height),
                u32::from(inner.height / 2),
                None,
                ink,
            ),
        )?;
    }
    Ok(())
}

/// Phase, minutes left, the bar and dots, and a hint what a tap does: as many
/// as the place has room for.
fn timer(cx: &mut Cx<'_>, out: &mut Vec<Paint>, area: Rect, shown: &Shown) -> Result<(), Error> {
    let look = cx.shape.paint(out, area, 256)?;
    let inner = look.area;
    let small = cx.style().small;
    let line = line_px(&cx.font, small)?;
    let align = cx.settings.align()?;
    let hint_key = if shown.fresh() {
        "pomodoro.hint.start"
    } else {
        "pomodoro.hint.resume"
    };
    let hint_text = cx.tr(hint_key)?;

    // What the place has room for, most wanted first; the time keeps at least
    // two lines.
    let mut room = inner.height.saturating_sub(line.saturating_mul(2));
    let mut want = |height: u16, wanted: bool| {
        let fits = wanted && room >= height;
        if fits {
            room -= height;
        }
        fits
    };
    let name = want(line, true);
    let rounds_wide =
        usize::from(inner.width) >= (small as usize).saturating_mul(shown.rounds as usize);
    let dots_on = want(line, rounds_wide && cx.settings.shows("dots")?);
    let bar_h = (line / 3).max(6);
    let tall = inner.height >= line.saturating_mul(6);
    let bar_on = want(bar_h, tall && cx.settings.shows("bar")?);
    let hint = want(
        line,
        !shown.running && fits(cx, small, &hint_text, inner.width)?,
    );

    let mut bands = Bands::new(inner);
    if name {
        let long = cx.tr(phase_key(shown.phase, false))?;
        let text = if fits(cx, small, &long, inner.width)? {
            long
        } else {
            cx.tr(phase_key(shown.phase, true))?
        };
        let band = bands.take(inner, line);
        one_line(
            out,
            &cx.font,
            band,
            align,
            (small, small * 0.6),
            &text,
            look.faint,
        )?;
    }
    let below = u16::from(dots_on) * line + u16::from(bar_on) * bar_h + u16::from(hint) * line;
    let time = bands.take(inner, bands.left().saturating_sub(below));
    let text = minutes_text(cx, shown, inner.width)?;
    one_line(
        out,
        &cx.font,
        time,
        align,
        (f32::from(time.height), small),
        &text,
        look.ink,
    )?;
    if bar_on {
        let band = bands.take(inner, bar_h);
        bar(out, band, shown.gone, look.ink)?;
    }
    if dots_on {
        let band = bands.take(inner, line);
        dots(out, band, (shown.rounds, shown.done), look.ink)?;
    }
    if hint {
        let band = bands.take(inner, line);
        one_line(
            out,
            &cx.font,
            band,
            align,
            (small, small * 0.6),
            &hint_text,
            look.faint,
        )?;
    }
    Ok(())
}

/// The sign of a button in `area`: two bars (pause) while it runs, else a
/// triangle (start).
fn glyph(out: &mut Vec<Paint>, area: Rect, running: bool, ink: Color) -> Result<(), Error> {
    let side = i32::from(area.width.min(area.height)) / 2;
    if side < 4 {
        return Ok(());
    }
    let x0 = i32::from(area.x) + (i32::from(area.width) - side) / 2;
    let y0 = i32::from(area.y) + (i32::from(area.height) - side) / 2;
    if running {
        let bar = side / 3;
        let (x, y) = (clamp_u16(i64::from(x0)), clamp_u16(i64::from(y0)));
        let (bar_w, high) = (clamp_u16(i64::from(bar)), clamp_u16(i64::from(side)));
        fill(out, Rect::new(x, y, bar_w, high), ink)?;
        let right = clamp_u16(i64::from(x0 + side - bar));
        return fill(out, Rect::new(right, y, bar_w, high), ink);
    }
    // Pointing right, one pixel row at a time.
    let half = (side / 2).max(1);
    let side_u = u32::try_from(side).unwrap_or(0);
    spans(out, (x0, y0), (side_u, side_u), ink, |column, row| {
        let row = i32::try_from(row).unwrap_or(0);
        let rise = half - (row - half).abs();
        i32::try_from(column).unwrap_or(0) < (side * rise / half).max(1)
    })
}

/// Start or Pause in a button, and the time left beneath it where there is room.
fn button(cx: &mut Cx<'_>, out: &mut Vec<Paint>, area: Rect, shown: &Shown) -> Result<(), Error> {
    let look = cx.shape.paint(out, area, 256)?;
    let inner = look.area;
    let (text_px, small) = (cx.style().text, cx.style().small);
    let line = line_px(&cx.font, small)?;
    let label = cx.tr(main_key(shown))?;
    let align = cx.settings.align()?;
    // No room for the word: the sign of what a tap does.
    if !fits(cx, text_px, &label, inner.width)? {
        return glyph(out, inner, shown.running, look.ink);
    }
    let two = inner.height >= line.saturating_mul(3);
    let mut bands = Bands::new(inner);
    let main = bands.take(
        inner,
        if two {
            inner.height.saturating_sub(line)
        } else {
            inner.height
        },
    );
    one_line(
        out,
        &cx.font,
        main,
        align,
        (text_px, small),
        &label,
        look.ink,
    )?;
    if two {
        let band = bands.take(inner, line);
        let phase = cx.tr(phase_key(shown.phase, true))?;
        let minutes = minutes_text(cx, shown, inner.width)?;
        let text = [phase.as_str(), " · ", minutes.as_str()].concat();
        one_line(
            out,
            &cx.font,
            band,
            align,
            (small, small * 0.6),
            &text,
            look.faint,
        )?;
    }
    Ok(())
}

/// The day's count, large, the goal beside it, and the week below.
fn today(cx: &mut Cx<'_>, out: &mut Vec<Paint>, area: Rect, shown: &Shown) -> Result<(), Error> {
    let look = cx.shape.paint(out, area, 256)?;
    let inner = look.area;
    let small = cx.style().small;
    let line = line_px(&cx.font, small)?;
    let align = cx.settings.align()?;
    let week = if inner.height < line.saturating_mul(3) || !cx.settings.shows("week")? {
        None
    } else {
        week_line(shown, |key, args| cx.format(key, args))?
    };
    let below = if week.is_some() { line } else { 0 };
    let mut bands = Bands::new(inner);
    let main = bands.take(inner, inner.height.saturating_sub(below));
    let said = count_line(shown, |key, args| cx.format(key, args))?;
    let text = if fits(cx, small, &said, inner.width)? {
        said
    } else {
        cx.number(i64::from(shown.today))?
    };
    one_line(
        out,
        &cx.font,
        main,
        align,
        (f32::from(main.height), small),
        &text,
        look.ink,
    )?;
    if let Some(week) = week {
        let band = bands.take(inner, line);
        one_line(
            out,
            &cx.font,
            band,
            align,
            (small, small * 0.6),
            &week,
            look.faint,
        )?;
    }
    Ok(())
}
