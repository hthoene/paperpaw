//! The three widgets: *Last sync* (when, and what it did), *Pending changes*
//! (how many files of the card are not sent yet) and *Sync now* (a button).
//! Each draws from one [`Shown`] in whatever room its place has.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Rect,
    native::{Error, formatting::Arg},
};
use paperpaw_ui_kit::{
    Paint,
    geometry::line_px,
    widget::{Cx, layout::label},
};

use crate::declarations::{BUTTON_TOKEN, LAST_TOKEN, PENDING_TOKEN};

/// What the widgets show.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Shown {
    /// When the last sync ended, as text; empty for never.
    pub when: String,
    /// What it did, as a line; empty for nothing to say.
    pub line: String,
    pub pending: u32,
    pub running: bool,
}

/// How high the widget is in a place `width` wide: three lines and the frame.
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
    let (title_key, value) = match token {
        LAST_TOKEN => (
            "file-sync.widget.last",
            if shown.when.is_empty() {
                cx.tr("file-sync.never")?
            } else {
                paperpaw_ui_kit::text::copy(&shown.when)?
            },
        ),
        PENDING_TOKEN => (
            "file-sync.widget.pending",
            cx.format(
                "file-sync.pending.count",
                &[Arg::Count(i64::from(shown.pending))],
            )?,
        ),
        BUTTON_TOKEN => (
            "",
            cx.tr(if shown.running {
                "file-sync.syncing.short"
            } else {
                "file-sync.sync"
            })?,
        ),
        _ => return Err(Error::NotFound),
    };
    let look = cx.shape.paint(out, area, 256)?;
    let inner = look.area;
    let (text, small) = (cx.style().text, cx.style().small);
    let align = cx.settings.align()?;
    if title_key.is_empty() {
        return label(out, &cx.font, (text, align), inner, &value, look.ink);
    }
    let line = line_px(&cx.font, small)?;
    let title = cx.tr(title_key)?;
    let head = Rect::new(inner.x, inner.y, inner.width, line.min(inner.height));
    label(out, &cx.font, (small, align), head, &title, look.faint)?;
    let rest = inner.height.saturating_sub(head.height);
    let body = Rect::new(
        inner.x,
        inner.y.saturating_add(head.height),
        inner.width,
        rest,
    );
    label(out, &cx.font, (text, align), body, &value, look.ink)
}
