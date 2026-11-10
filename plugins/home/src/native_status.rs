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
    geometry, mem, paint,
    status_line::{self, Face, Lists},
    widget::{Cx, Frame},
};

/// The three lists and the chosen whole line of the widget `cx` is open on.
fn lists(cx: &Cx<'_>) -> Result<Lists, Error> {
    Ok(Lists {
        left: cx.settings.string("left", 1024)?,
        center: cx.settings.string("center", 1024)?,
        right: cx.settings.string("right", 1024)?,
        line: cx.settings.string("line", 256)?,
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
pub fn targets(cx: &Cx<'_>, ui: &Ui, area: Rect) -> Result<Vec<(u32, Rect)>, Error> {
    status_line::targets((&face(cx), &lists(cx)?), ui, area)
}

/// The action the tap on the item at `target` (from 1, as [`targets`]) runs.
///
/// # Errors
/// Font and host refusals, [`Error::NoMemory`].
pub fn action(cx: &Cx<'_>, ui: &Ui, area: Rect, target: u32) -> Result<Option<String>, Error> {
    status_line::action((&face(cx), &lists(cx)?), ui, area, target)
}

/// What the line's texts show, to tell when it changed.
///
/// # Errors
/// The host's refusals and [`Error::NoMemory`].
pub fn shows(cx: &Cx<'_>, ui: &Ui) -> Result<u64, Error> {
    status_line::shows(cx.host, &lists(cx)?, ui)
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
    widget: &str,
    area: Rect,
) -> Result<(), Error> {
    // The buffers outlive the context: the drawn values borrow the host for the
    // callback and are composed in before it ends.
    let mut buffers = [
        mem::buffer(mem::SERVICE_BUFFER)?,
        mem::buffer(mem::SERVICE_BUFFER)?,
    ];
    let cx = Cx::open(host, widget)?;
    let mut out = Vec::new();
    let mut placed = Vec::new();
    let look = cx.shape.paint(&mut out, area, 256)?;
    // The widget's own ink where the reader set a look of it (a dark card, a paper,
    // a text colour); else the interface's note ink.
    let own = look.frame == Frame::Dark
        || cx.settings.is_chosen("paper")?
        || cx.settings.is_chosen("ink")?;
    let color = if own { look.faint } else { cx.style().note };
    // A host that does not answer the status calls (the compose family on the
    // X4 until its stack gate passes) leaves an empty line, not a failed draw.
    let lists = lists(&cx)?;
    match status_line::draw(
        (&face(&cx), &lists),
        ui,
        &mut out,
        &mut placed,
        &mut buffers,
        (look.area, color),
    ) {
        Err(Error::Unsupported) => {}
        other => other?,
    }
    paint::replay(frame, &cx.font, out.iter())?;
    for item in &placed {
        // An owner that cannot draw now (busy, gone) leaves its place empty.
        Ui::compose(host, item.target, item.rect).ok();
    }
    Ok(())
}
