//! What the checks look at, shown: every screen a plugin adds and every
//! widget in every size, each drawn on a session's device with what was
//! drawn (picture, described parts) and, for widgets, the cell it was
//! given. Galleries, visual checks and budgets share these.

use paperpaw_core::sdk::builtin::slots;
use paperpaw_core::sdk::semantics::{Described, Part};
use paperpaw_core::sdk::{Params, widgets};
use paperpaw_platform::Rect;

use crate::image::Image;
use crate::probe;
use crate::session::Session;
use crate::target::Target;

/// The home screen's grid setting widgets are placed with.
pub const PAGE: &str = "home.page-1";

/// A screen as it was drawn.
#[derive(Debug, Clone)]
pub struct Shown {
    /// The screen asked for.
    pub id: String,
    /// The screen shown after it was opened: another one if it closed at
    /// once or opened something else (e.g. it needs parameters).
    pub shown: Option<String>,
    pub image: Image,
    pub parts: Vec<Part>,
    pub described: Vec<Described>,
}

impl Shown {
    /// Whether the screen asked for is the one shown.
    pub fn opened(&self) -> bool {
        self.shown.as_deref() == Some(self.id.as_str())
    }
}

/// A widget as it was drawn on the home screen, alone.
#[derive(Debug, Clone)]
pub struct ShownWidget {
    pub id: String,
    /// Columns and rows of the grid it takes.
    pub size: (u8, u8),
    /// The area the grid gave it (`None` if it was not placed, e.g. too
    /// small for the grid).
    pub cell: Option<Rect>,
    /// The whole screen.
    pub image: Image,
    pub parts: Vec<Part>,
    pub described: Vec<Described>,
}

impl ShownWidget {
    /// Its cell cut out of the picture.
    pub fn picture(&self) -> Option<Image> {
        self.cell.map(|cell| self.image.crop(cell))
    }
}

/// The screens the target's plugins add, sorted by id; for data plugins
/// alone, the standard screens they are seen on.
pub fn screens(session: &Session, target: &Target) -> Vec<String> {
    let known = || {
        session
            .device
            .kernel
            .known_screens()
            .map(ToString::to_string)
    };
    let mut ids: Vec<String> = known()
        .filter(|id| target.ours(id) && !id.starts_with(probe::PLUGIN))
        .collect();
    // Data plugins add no screens: theirs are those they change (a theme,
    // a layout), the standard app's main ones.
    let data_only = !target.data.is_empty()
        && target
            .plugins
            .iter()
            .all(|p| target.data.iter().any(|d| &d.id == p));
    if ids.is_empty() && data_only {
        ids = known()
            .filter(|id| STANDARD_SCREENS.contains(&id.as_str()))
            .collect();
    }
    ids.sort();
    ids
}

/// The standard app's main screens, on which data plugins are seen: the
/// home screen, the library and the settings.
pub const STANDARD_SCREENS: [&str; 3] = ["home.dashboard", "files.browser", "settings.list"];

/// The widgets the target's plugins add, with their titles (translated),
/// sorted by id; those that are only room between others left out.
pub fn widgets(session: &Session, target: &Target) -> Vec<(String, String)> {
    let kernel = &session.device.kernel;
    let mut list: Vec<(String, String)> = kernel
        .widgets()
        .everywhere()
        .into_iter()
        .filter(|(id, _, _)| {
            target.ours(id.as_str())
                && !id.as_str().starts_with(probe::PLUGIN)
                && !widgets::repeatable(id.as_str())
        })
        .map(|(id, title, _)| (id.to_string(), kernel.translator().tr(&title).to_string()))
        .collect();
    list.sort();
    list
}

/// The home screen's grid: columns and rows of a page.
pub fn grid(session: &Session) -> (u8, u8) {
    let settings = session.device.kernel.settings();
    let read = |id: &str, default: u8| {
        settings
            .text(id)
            .parse::<u8>()
            .ok()
            .filter(|n| *n > 0)
            .unwrap_or(default)
    };
    (read("home.columns", 4), read("home.rows", 6))
}

/// Every size a widget can take on the grid: one cell to the whole page.
pub fn sizes(grid: (u8, u8)) -> Vec<(u8, u8)> {
    let mut sizes = Vec::new();
    for h in 1..=grid.1 {
        for w in 1..=grid.0 {
            sizes.push((w, h));
        }
    }
    sizes
}

/// A few sizes, for quick runs: one cell, a row, a square, a column
/// and the whole page (those the grid has).
pub fn some_sizes(grid: (u8, u8)) -> Vec<(u8, u8)> {
    let (c, r) = grid;
    let mut sizes = vec![
        (1, 1),
        (2, 1),
        (c, 1),
        (2, 2),
        (c, 2),
        (1, r.min(3)),
        (c, r),
    ];
    sizes.retain(|&(w, h)| w <= c && h <= r);
    sizes.dedup();
    sizes
}

/// The parameters `screen` is opened with to show it with data: with the
/// emulator's fixtures, those their plugin opens it with where the
/// fixtures give its data ([`paperpaw_emulator::offline::SCREENS`]);
/// else none.
pub fn params(session: &Session, screen: &str) -> &'static [(&'static str, &'static str)] {
    if !session.fixtures() {
        return &[];
    }
    paperpaw_emulator::offline::SCREENS
        .iter()
        .find(|(id, _)| *id == screen)
        .map_or(&[], |(_, params)| params)
}

/// Opens `screen` (with `params`) on the session and shows what is drawn.
pub fn show_screen(
    session: &mut Session,
    screen: &str,
    params: &[(&str, &str)],
) -> Result<Shown, String> {
    session.open(screen, params)?;
    Ok(Shown {
        id: screen.to_string(),
        shown: session.screen(),
        image: session.image(),
        parts: session.parts(),
        described: session.described(),
    })
}

/// Goes back to the home screen.
pub fn home(session: &mut Session) {
    session
        .device
        .call(|kernel, now| kernel.open_slot(now, slots::HOME, Params::new()));
    session.settle();
}

/// Places `widget` alone on the home screen's first page in `size` and
/// shows what is drawn; the cell is measured with the probe, in the same
/// place, first.
pub fn show_widget(
    session: &mut Session,
    widget: &str,
    size: (u8, u8),
) -> Result<ShownWidget, String> {
    let place = |id: &str| format!("{id} 0/0 {}x{}", size.0, size.1);
    home(session);
    probe::forget();
    session.set(PAGE, &place(probe::WIDGET.as_str()))?;
    session.device.draw_again();
    let cell = probe::area();
    session.set(PAGE, &place(widget))?;
    session.device.draw_again();
    Ok(ShownWidget {
        id: widget.to_string(),
        size,
        cell,
        image: session.image(),
        parts: session.parts(),
        described: session.described(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_cover_the_grid() {
        assert_eq!(sizes((4, 6)).len(), 24);
        let some = some_sizes((4, 6));
        assert!(some.contains(&(1, 1)) && some.contains(&(4, 6)));
        assert!(some_sizes((2, 2)).iter().all(|&(w, h)| w <= 2 && h <= 2));
    }
}
