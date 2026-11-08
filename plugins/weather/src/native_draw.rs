//! What both widgets draw with: the style setting, the large digits and the
//! waiting message. Owner: the *Weather now* port. The interface-6 code is
//! `draw.rs`.
use alloc::vec::Vec;
use paperpaw_sdk::{Rect, native::Error};
use paperpaw_ui_kit::{Paint, widget::Cx};

use crate::native_shown::Waiting;
use crate::native_symbol::Style;

/// How the widget `cx` draws its symbols (its setting `style`).
///
/// # Errors
/// What the settings read says.
pub fn style(cx: &Cx<'_>) -> Result<Style, Error> {
    let _ = cx;
    Err(Error::Unsupported)
}

/// The message of a widget that has no weather to show, centred in `area`.
///
/// # Errors
/// `NoMemory`, or what the host says.
pub fn draw_waiting(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    area: Rect,
    waiting: Waiting,
) -> Result<(), Error> {
    let _ = (cx, out, area, waiting);
    Err(Error::Unsupported)
}
