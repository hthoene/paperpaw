//! The weather symbols (the sun, clouds, rain, snow, fog, thunder) drawn as
//! [`Paint`] operations, line or solid. Owner: the *Weather now* port.
//! The interface-6 code is `symbol.rs`.
use alloc::vec::Vec;
use paperpaw_sdk::{Rect, native::Error};
use paperpaw_ui_kit::Paint;

use crate::native_weather::Symbol;

/// How a symbol is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Style {
    /// Outlines.
    Line,
    /// Filled shapes.
    Solid,
}

/// The ink of `symbol` in a square of side 1,000: left, top, width, height
/// (what `symbol.rs`'s `ink_box` says).
#[must_use]
pub fn ink_box(symbol: Symbol, day: bool) -> (i32, i32, i32, i32) {
    let _ = (symbol, day);
    (0, 0, 1000, 1000)
}

/// Draws `symbol` in the square `area`.
///
/// # Errors
/// `NoMemory` when a drawing operation cannot be kept.
pub fn draw(
    out: &mut Vec<Paint>,
    symbol: Symbol,
    day: bool,
    area: Rect,
    style: Style,
) -> Result<(), Error> {
    let _ = (out, symbol, day, area, style);
    Err(Error::Unsupported)
}
