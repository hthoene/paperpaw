//! The status line as a widget of the home screen: its settings (the items the
//! user chose for its left, centre and right, or a whole line another plugin
//! draws) are read here; the line itself is `paperpaw_ui_kit::status_line`,
//! which picture-home's bar draws with too.
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::{
    Rect,
    native::{Error, Host, callbacks::DrawFrame, ui::Ui},
};
use paperpaw_ui_kit::{
    SharedInputs, geometry, paint,
    status_line::{self, Face, Lists},
    widget::{Cx, Frame},
};

use crate::native_lines;
use crate::native_widgets::Looks;

/// The three lists and the chosen whole line of the widget `cx` is open on: of
/// the lines the setting lists, the first that is there, by its id now
/// (`native_lines::shown`); none, and the items show.
/// The registry is asked once for as long as `looks` keeps the answer (a
/// setting or registry change lets it go); `fresh` asks it in any case.
fn lists(cx: &Cx<'_>, looks: &Looks, fresh: bool) -> Result<Lists, Error> {
    let listed = cx.settings.string("line", 256)?;
    let line = looks.line(&listed, fresh, || native_lines::shown(cx.host, &listed))?;
    Ok(Lists {
        left: cx.settings.string("left", 1024)?,
        center: cx.settings.string("center", 1024)?,
        right: cx.settings.string("right", 1024)?,
        line,
    })
}

fn face<'a, 'f>(cx: &'f Cx<'a>) -> Face<'a, 'f> {
    Face {
        host: cx.host,
        font: &cx.font,
        small: cx.style().small,
    }
}

/// The height of the status line: a line of the small size and half a gap, and
/// the frame.
///
/// # Errors
/// Font refusals.
pub fn height(cx: &Cx<'_>) -> Result<u16, Error> {
    let style = cx.style();
    Ok(geometry::line_px(&cx.font, style.small)?
        .saturating_add(style.gap / 2)
        .saturating_add(cx.shape.frame_height()))
}

/// Where a tap on the line in `area` does something (see
/// [`status_line::targets`]).
///
/// # Errors
/// Font and host refusals, [`Error::NoMemory`].
pub fn targets(cx: &Cx<'_>, ui: &Ui, looks: &Looks, area: Rect) -> Result<Vec<(u32, Rect)>, Error> {
    status_line::targets((&face(cx), &lists(cx, looks, false)?), ui, area)
}

/// The action the tap on the item at `target` (from 1, as [`targets`]) runs.
///
/// # Errors
/// Font and host refusals, [`Error::NoMemory`].
pub fn action(
    cx: &Cx<'_>,
    ui: &Ui,
    looks: &Looks,
    area: Rect,
    target: u32,
) -> Result<Option<String>, Error> {
    status_line::action((&face(cx), &lists(cx, looks, false)?), ui, area, target)
}

/// What the line's texts show, to tell when it changed. The tick asks the
/// registry for the line in any case and keeps the answer for the draws.
///
/// # Errors
/// The host's refusals and [`Error::NoMemory`].
pub fn shows(cx: &Cx<'_>, ui: &Ui, looks: &Looks) -> Result<u64, Error> {
    status_line::shows(cx.host, &lists(cx, looks, true)?, ui)
}

/// Draws the status line instance `widget` in `area` (its widget rectangle):
/// the frame, the texts, then the values its owners draw, composed in.
///
/// # Errors
/// Font and host refusals, [`Error::NoMemory`].
pub fn draw_widget(
    host: &Host<'_>,
    ui: &Ui,
    frame: &mut DrawFrame<'_>,
    (widget, kept, shared): (&str, &Looks, &SharedInputs),
    area: Rect,
) -> Result<(), Error> {
    // The buffers outlive the context: the drawn values borrow the host for the
    // callback and are composed in before it ends.
    let mut buffers = [Vec::new(), Vec::new()];
    let cx = Cx::open_kept_with(host, widget, kept.take(), shared)?;
    let mut out = Vec::new();
    let mut placed = Vec::new();
    let look = cx.shape.paint(&mut out, area, 256)?;
    // The widget's own ink where the reader set a look of it (a dark card, a paper,
    // a text colour); else the interface's note ink.
    let own = look.frame == Frame::Dark
        || kept.own(|| Ok(cx.settings.is_chosen("paper")? || cx.settings.is_chosen("ink")?))?;
    let color = if own { look.faint } else { cx.style().note };
    // A host that does not answer the status calls (the compose family on the
    // X4 until its stack gate passes) leaves an empty line, not a failed draw.
    let lists = lists(&cx, kept, false)?;
    let drawn = kept.with_status(|status| {
        status_line::draw_kept(
            &mut status.items,
            (&face(&cx), &lists),
            ui,
            (&mut out, &mut placed),
            &mut buffers,
            ((look.area, color), &[]),
        )
    });
    match drawn.and_then(|drawn| drawn) {
        Err(Error::Unsupported) => {}
        other => other?,
    }
    paint::replay(frame, &cx.font, out.iter())?;
    for item in &placed {
        // An owner that cannot draw now (busy, gone) leaves its place empty.
        Ui::compose(host, item.target, item.rect).ok();
    }
    kept.keep(cx.keep());
    Ok(())
}
