//! The pages: each a grid of items in a setting of its own,
//! `picture-home.page-1` to `picture-home.page-4`, kept as
//! `paperpaw_sdk::widgets::grid` writes a layout. The first page always
//! shows, the others where they have items. A page added and not filled
//! yet keeps a space ([`widgets::SPACE`], which the grid leaves out) so
//! that it shows until it is removed.
//!
//! One layout serves the screen upright and on its side: on its side the
//! items of a page are placed anew in the order they have (the grid puts
//! each into the first free cells), nothing of that is kept, so that
//! turning the screen back and forth changes no page.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, menus};
use paperpaw_sdk::widgets;
use paperpaw_sdk::widgets::grid::{self, Item, What};
use paperpaw_sdk::{Command, Env, Id, SettingValue};

use crate::settings::{self, MAX_PAGES};

/// On a page: every tool not on it otherwise, each in a cell.
pub(crate) const TOOLS: &str = "home.tools";
/// The shortcuts at the head of the tools (the built-in home screen's).
const SHORTCUTS: Id = Id::new_static("home.actions");

/// A page shown: which setting keeps it, and its items.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Page {
    /// The setting it is kept in: 0 for `picture-home.page-1`.
    pub(crate) slot: usize,
    pub(crate) items: Vec<Item>,
}

/// Every page shown, in the order of their settings.
pub(crate) fn pages(env: &Env<'_>) -> Vec<Page> {
    settings::pages(env)
        .into_iter()
        .map(|(slot, items)| Page { slot, items })
        .collect()
}

/// The setting `id` is, if it keeps a page: 0 for `picture-home.page-1`.
pub(crate) fn slot_of(id: &str) -> Option<usize> {
    (0..MAX_PAGES).find(|&i| settings::page_setting(i).as_str() == id)
}

/// A page's items as the grid shows them: the tools for `home.tools`,
/// without what the grid has no use for (spaces: empty cells are) or
/// cannot show (a widget of a plugin not loaded now).
pub(crate) fn expand(env: &Env<'_>, items: &[Item]) -> Vec<Item> {
    let mut out: Vec<Item> = Vec::new();
    for item in items {
        match &item.what {
            What::Widget(id) if id == TOOLS => {
                for (action, _) in tools(env) {
                    let shown = items.iter().any(|i| i.what == What::Tool(action.clone()));
                    if !shown {
                        out.push(Item::new(What::Tool(action)));
                    }
                }
            }
            What::Widget(id) if id == widgets::SPACE || id == widgets::FILL => {}
            What::Widget(id) if !env.widgets().has(id) => {}
            _ => out.push(item.clone()),
        }
    }
    out
}

/// Whether the page keeps `item` without showing it ([`expand`] leaves
/// it out): a widget of a plugin not loaded now (switched off, say).
fn hidden(env: &Env<'_>, item: &Item) -> bool {
    match &item.what {
        What::Widget(id) if id == TOOLS || id == widgets::SPACE => false,
        What::Widget(id) => id == widgets::FILL || !env.widgets().has(id),
        _ => false,
    }
}

/// The commands that keep `items` (as the page shows them) as the page in
/// `slot`, with what the page keeps there without showing it; an empty
/// page keeps a space.
pub(crate) fn store(env: &Env<'_>, slot: usize, items: &[Item]) -> Vec<Command> {
    let setting = settings::page_setting(slot);
    let mut all: Vec<Item> = items.to_vec();
    for kept in grid::parse(env.settings().text(setting.as_str())) {
        if hidden(env, &kept) && !all.iter().any(|i| i.what == kept.what) {
            all.push(kept);
        }
    }
    let text = if all.is_empty() && slot > 0 {
        String::from(widgets::SPACE)
    } else {
        grid::write(&all)
    };
    Vec::from([Command::SetSetting(
        setting,
        SettingValue::Text(text.into()),
    )])
}

/// The commands that keep `pages` as the pages: the others emptied.
pub(crate) fn store_all(env: &Env<'_>, pages: &[Page]) -> Vec<Command> {
    let mut out = Vec::new();
    for (position, page) in pages.iter().enumerate() {
        // The pages move up when one went: each into the setting of its
        // place.
        out.extend(store(env, position, &page.items));
    }
    for slot in pages.len()..MAX_PAGES {
        out.push(Command::SetSetting(
            settings::page_setting(slot),
            SettingValue::Text(String::new().into()),
        ));
    }
    out
}

/// Whether any page holds widget `id` (a placement `id@n`).
pub(crate) fn holds(env: &Env<'_>, id: &str) -> bool {
    pages(env).iter().any(|p| {
        p.items
            .iter()
            .any(|i| matches!(&i.what, What::Widget(w) if w == id))
    })
}

/// The tools a page may show: the shortcuts, the tools, what there is to
/// open ([`menus::APPS`]), then the settings; their action and the
/// translation key of their name.
pub(crate) fn tools(env: &Env<'_>) -> Vec<(Id, String)> {
    let mut items: Vec<(Id, String)> = Vec::new();
    for menu in [SHORTCUTS, menus::TOOLS, menus::APPS] {
        for item in env.menus().items(menu.as_str()) {
            // A screenshot from here shows only the home screen: it is
            // for the tools opened over a book (unless listed above).
            let here_useless = menu == menus::TOOLS && *item.id == actions::SCREENSHOT;
            // Hidden by its plugin now (e.g. *Apps* with nothing in it).
            let hidden = !env.menus().state(env, item.id.as_str()).shown;
            if !item.section
                && !here_useless
                && !hidden
                && !items.iter().any(|(id, _)| id == item.id)
            {
                items.push((item.id.clone(), item.title.to_string()));
            }
        }
    }
    if !items.iter().any(|(id, _)| *id == actions::SETTINGS) {
        items.push((actions::SETTINGS, String::from("core.action.settings")));
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_are_found_by_their_settings() {
        assert_eq!(slot_of("picture-home.page-1"), Some(0));
        assert_eq!(slot_of("picture-home.page-4"), Some(3));
        assert_eq!(slot_of("picture-home.page-5"), None);
        assert_eq!(slot_of("home.page-1"), None);
    }
}
