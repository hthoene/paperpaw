//! The page that opens the editors of the launcher's three menus: what the
//! home shows (the cards), what the dock holds, and what the quick panel
//! holds. The editing is the menu editor's.
use paperpaw_sdk::native::{Error, canvas::PartRole};
use paperpaw_ui_kit::widget::Align;

use crate::{
    look::{B, Ui},
    model::Target,
};

/// The menus, with the catalog keys of their name and of what they hold, and
/// an icon each.
pub const ENTRIES: [(&str, &str, &str, &str); 3] = [
    (
        crate::declarations::MENU_CARDS,
        "launcher-simple.customise.cards",
        "launcher-simple.customise.cards.note",
        "layout-grid",
    ),
    (
        crate::declarations::MENU_DOCK,
        "launcher-simple.customise.dock",
        "launcher-simple.customise.dock.note",
        "apps",
    ),
    (
        crate::declarations::MENU_PANEL,
        "launcher-simple.customise.panel",
        "launcher-simple.customise.panel.note",
        "adjustments-horizontal",
    ),
];

/// Draws the page into `ui`.
///
/// # Errors
/// [`Error::NoMemory`] and the host's refusals.
pub fn draw(ui: &mut Ui<'_, '_>) -> Result<(), Error> {
    let title = ui.t("launcher-simple.customise.title")?;
    let top = ui.header(&title, "")?;
    let area = B::new(
        ui.screen.x + ui.pad,
        top.y + ui.gap,
        ui.screen.w - 2 * ui.pad,
        top.h - ui.gap - ui.pad,
    );
    let apart = ui.gap.max(2 * ui.stroke + 8);
    let height = ((ui.u * 3.2) as i32 + 2 * ui.stroke + 16).min(area.h / 3 - apart);
    for (index, (_, name, note, icon)) in ENTRIES.iter().enumerate() {
        let b = B::new(
            area.x,
            area.y + index as i32 * (height + apart),
            area.w,
            height,
        );
        let name = ui.t(name)?;
        let note = ui.t(note)?;
        let focused = ui.focused();
        let (inner, ink) = ui.card(b, focused)?;
        let picture = inner.left(inner.h.min(inner.w / 5));
        ui.icon(icon, picture.h as f32 * 0.9, picture, ink)?;
        let words = inner.right_of(picture.w, ui.gap / 2);
        let size = (ui.u * 1.2).min(words.h as f32 * 0.5);
        let small = ui.small.min(size * 0.8);
        ui.bold(words.part(0, 55), Align::Left, size, &name, ink)?;
        ui.fit(words.part(55, 100), Align::Left, small, &note, ink)?;
        ui.zone(Target::Edit(index), b, PartRole::Row, &name)?;
    }
    Ok(())
}
