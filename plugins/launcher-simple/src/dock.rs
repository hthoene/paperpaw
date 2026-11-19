//! The dock at the bottom of the home screen, the quick panel and the library:
//! the icons of the user's own menu, the one of the screen shown filled.
use paperpaw_sdk::native::{Error, canvas::PartRole};

use crate::{
    data::Row,
    look::{B, Ui},
    model::Target,
};

/// The icon of a row without one of its own.
const FALLBACK: &str = "apps";

/// The most icons a dock holds.
pub const MOST: usize = 5;

/// The dock's height for `ui`.
pub fn height(ui: &Ui<'_, '_>) -> i32 {
    ((ui.u * 3.4) as i32).min(ui.screen.h / 5).max(64)
}

/// The room under the dock to the screen's edge.
pub fn edge(ui: &Ui<'_, '_>) -> i32 {
    i32::from(ui.style.margin) + 12
}

/// Draws the dock of `rows` (the first [`MOST`]); `here` names the item of the
/// screen shown. Returns what is left of the screen above it.
///
/// # Errors
/// [`Error::NoMemory`] and the host's refusals.
pub fn draw(ui: &mut Ui<'_, '_>, rows: &[Row], here: &str) -> Result<B, Error> {
    let h = height(ui);
    let edge = edge(ui);
    let band = ui.screen.bottom(h + edge).top(h);
    let shown = &rows[..rows.len().min(MOST)];
    if !shown.is_empty() {
        let count = shown.len() as i32;
        // Few icons keep together in the middle, many use the width.
        let room = if count <= 3 {
            band.inset_xy(ui.screen.w / 8, 0)
        } else {
            band.inset_xy(ui.pad / 2, 0)
        };
        let radius = (h * 42 / 100).min(room.w / count / 2 - 2 * ui.stroke - 2);
        for (index, row) in shown.iter().enumerate() {
            let cell = room.col(index as i32, count, 0);
            let centre = cell.middle();
            let around = B::new(centre.x - radius, centre.y - radius, 2 * radius, 2 * radius);
            let zone = around.inset(-ui.stroke);
            let on = row.item == here;
            let icon = if row.icon.is_empty() {
                FALLBACK
            } else {
                &row.icon
            };
            let ink = ui.ink;
            if on {
                ui.disc(centre, radius, icon, true)?;
            } else {
                ui.icon(icon, radius as f32 * 1.4, around, ink)?;
            }
            if ui.focused() {
                ui.ring(around, radius)?;
            }
            ui.zone(Target::Dock(index), zone, PartRole::Button, &row.title)?;
        }
    }
    Ok(ui.screen.above(h + edge, ui.gap / 2))
}
