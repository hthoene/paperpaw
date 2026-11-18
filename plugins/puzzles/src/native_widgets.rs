//! The home widget *Puzzle of the day*: its name, and under it how far the
//! sudoku of the day is (to play, squares left, solved) with the count of
//! puzzles solved. It draws from [`Shown`] only and asks for no redraw: the
//! plugin says when it changed (a move on the puzzle of the day, a new day).

use crate::state::Stats;
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Rect,
    native::{Error, formatting::Arg},
};
use paperpaw_ui_kit::{
    Paint,
    geometry::line_px,
    widget::{Cx, text::line as one_line},
};

/// What the widget shows of the puzzle of the day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shown {
    /// 0 not begun today, 1 on the way, 2 solved.
    pub state: u8,
    pub left: u8,
    pub solved: u32,
}

impl Shown {
    /// What `stats` say for `today`.
    #[must_use]
    pub fn of(stats: &Stats, today: Option<i32>) -> Self {
        let state = if !stats.daily_is(today) {
            0
        } else if stats.daily_done {
            2
        } else {
            1
        };
        Self {
            state,
            left: stats.daily_left,
            solved: stats.total(),
        }
    }
}

/// How high the widget is in a place `width` wide: three lines.
///
/// # Errors
/// Font refusals.
pub fn height(cx: &Cx<'_>) -> Result<u16, Error> {
    let line = line_px(&cx.font, cx.style().small)?;
    Ok(cx
        .shape
        .frame_height()
        .saturating_add(line.saturating_mul(3)))
}

/// Draws the widget in `area`.
///
/// # Errors
/// Font refusals, [`Error::NoMemory`].
pub fn draw(cx: &mut Cx<'_>, out: &mut Vec<Paint>, area: Rect, shown: &Shown) -> Result<(), Error> {
    let look = cx.shape.paint(out, area, 256)?;
    let inner = look.area;
    let (text, small) = (cx.style().text, cx.style().small);
    let line = line_px(&cx.font, small)?;
    let align = cx.settings.align()?;
    let two = inner.height >= line.saturating_mul(3);
    let (head, rest) = if two {
        let h = inner.height.saturating_sub(line);
        (
            Rect::new(inner.x, inner.y, inner.width, h),
            Some(Rect::new(inner.x, inner.y + h, inner.width, line)),
        )
    } else {
        (inner, None)
    };
    let title = cx.tr("puzzles.widget.title")?;
    let state: String = match shown.state {
        0 => cx.tr("puzzles.widget.play")?,
        2 => cx.tr("puzzles.widget.done")?,
        _ => cx.format("puzzles.widget.left", &[Arg::Count(i64::from(shown.left))])?,
    };
    // The state is the large line where there is one line only.
    let big = if two { title } else { state.clone() };
    one_line(
        out,
        &cx.font,
        head,
        align,
        (text * 1.3, small),
        &big,
        look.ink,
    )?;
    if let Some(band) = rest {
        one_line(
            out,
            &cx.font,
            band,
            align,
            (small, small * 0.6),
            &state,
            look.faint,
        )?;
    }
    Ok(())
}
