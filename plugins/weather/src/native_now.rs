//! The widget *now* of the weather plugin (interface-6 code: `now.rs`).
use alloc::vec::Vec;
use paperpaw_sdk::{Rect, native::Error};
use paperpaw_ui_kit::{Paint, widget::Cx};

use crate::native_shown::Shown;

/// How high the widget is, in a place `width` wide.
///
/// # Errors
/// `NoMemory`, or what the host says.
pub fn height(cx: &mut Cx<'_>, shown: &Shown<'_>, width: u16) -> Result<u16, Error> {
    let _ = (cx, shown, width);
    Err(Error::Unsupported)
}

/// Lays the widget out in `area` as drawing operations.
///
/// # Errors
/// `NoMemory`, or what the host says.
pub fn draw(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    area: Rect,
    shown: &Shown<'_>,
) -> Result<(), Error> {
    let _ = (cx, out, area, shown);
    Err(Error::Unsupported)
}
