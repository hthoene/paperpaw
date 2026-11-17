//! Buttons drawn in rows and dialogs.

#[allow(
    clippy::wildcard_imports,
    reason = "a part of the module, which shares its imports"
)]
use super::*;

/// The width of buttons' and boxes' borders.
pub(crate) fn button_border(style: &UiStyle) -> u32 {
    style.line.max(2)
}
