//! Settings drawn as rows: values, notes, numbers and steppers, and
//! stepping a setting's value.

#[allow(
    clippy::wildcard_imports,
    reason = "a part of the module, which shares its imports"
)]
use super::*;

// ---- Settings as rows --------------------------------------------------

/// Whether `note`, a setting's description, shows whole in a row `width`
/// wide: on the `NOTE_LINES` lines a row gives it. A longer one is
/// shortened with "…"; descriptions are kept short to fit (detail goes in
/// the plugin's How it works).
pub fn note_fits(env: &Place<'_>, style: &UiStyle, width: u16, note: &str) -> bool {
    let Some(font) = style.font(env) else {
        return true;
    };
    let width = f32::from(width) - 2.0 * style.margin as f32;
    font.wrap(style.small, note, width).len() <= NOTE_LINES
}
