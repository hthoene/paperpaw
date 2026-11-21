//! `command.set_menu` and `command.bind_key` (`registry.md` section 4.2): the
//! user's menus and key bindings, changed by any plugin, also a plugin file on
//! the card, with no feature (owner decision 291: the menu editor and the keys
//! plugin are such files on the X4s), and saved in `paperpaw.ini` by the
//! kernel as in interface 6 (`Ask::SetMenu`, `Ask::Bind`). What protects the
//! user is the host's: no submenu cycle, and the global layer keeps an input
//! to the main menu (beside the host-fixed escape).
use super::*;
use crate::Place;
use crate::keymap::{self, DeviceInputs};
use paperpaw_sdk::builtin::GLOBAL_KEYMAP_LAYER;

/// `flags` bit 0 of both commands: back to what plugins declare (and, for a
/// key, the presets bind).
const RESET: u32 = 1;

/// The ids of a `set_menu` item list, one per line.
fn listed(items: &str) -> impl Iterator<Item = &str> {
    items.split('\n').filter(|id| !id.is_empty())
}

impl CommandStage<'_> {
    /// A `set_menu` or a `bind_key` of a batch, checked on its own.
    pub(super) fn validate_user_config(
        &self,
        env: &Place<'_>,
        command: WireCommand<'_>,
    ) -> Result<usize, i32> {
        match command {
            WireCommand::SetMenu { menu, items, flags } => {
                Self::validate_set_menu(env, menu, items, flags)
            }
            WireCommand::BindKey {
                layer,
                input,
                action,
                flags,
            } => Self::validate_bind_key(layer, input, action, flags),
            _ => Err(ERR_UNSUPPORTED),
        }
    }

    /// One `set_menu`: `ERR_INVALID` for other flags, a reset with items, a
    /// menu or an item the device does not know.
    fn validate_set_menu(
        env: &Place<'_>,
        menu: &str,
        items: &str,
        flags: u32,
    ) -> Result<usize, i32> {
        if flags & !RESET != 0 || (flags == RESET && !items.is_empty()) {
            return Err(ERR_INVALID);
        }
        let menus = env.menus();
        let menu_ids = menus.menu_ids();
        let is_menu = |id: &str| menu_ids.iter().any(|menu| menu.as_str() == id);
        if !is_menu(menu) {
            return Err(ERR_INVALID);
        }
        let mut current = None;
        for item in listed(items) {
            // An item is known by its label (a heading, an action, a
            // setting), as a menu, or as one the menu already lists (its
            // plugin may be switched off for now).
            let known = menus.title(item).is_some()
                || is_menu(item)
                || current
                    .get_or_insert_with(|| menus.ids(menu))
                    .iter()
                    .any(|id| id.as_str() == item);
            if !known {
                return Err(ERR_INVALID);
            }
        }
        Ok(menu.len() + items.len())
    }

    /// One `bind_key`: `ERR_INVALID` for other flags, a reset with an action,
    /// or a layer, input or action a key map cannot hold.
    fn validate_bind_key(layer: &str, input: &str, action: &str, flags: u32) -> Result<usize, i32> {
        if flags & !RESET != 0
            || (flags == RESET && !action.is_empty())
            || !keymap::native_binding_ok(layer, input, action)
        {
            return Err(ERR_INVALID);
        }
        Ok(layer.len() + input.len() + action.len())
    }

    /// The batch's `set_menu`s together: none makes a menu its own submenu,
    /// directly or through others (`ERR_INVALID` for the batch). A menu reset
    /// takes the rows plugins declare.
    pub(super) fn validate_menu_cycles(&self, batch: &CommandBatch<'_>) -> Result<(), i32> {
        let Some(ui) = self.ui.as_ref() else {
            return Ok(());
        };
        let mut proposed: Vec<(&str, Option<&str>)> = Vec::new();
        for command in batch.iter() {
            if let WireCommand::SetMenu { menu, items, flags } = command {
                proposed.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
                proposed.retain(|(changed, _)| *changed != menu);
                proposed.push((menu, (flags & RESET == 0).then_some(items)));
            }
        }
        // A batch without a `set_menu` makes no cycle: the menus are not read
        // (their lists are allocated, which a submit must not do unasked).
        if proposed.is_empty() {
            return Ok(());
        }
        let menus = ui.env().menus();
        let menu_ids = menus.menu_ids();
        // The rows of `menu` once the batch is applied, as far as they are menus.
        let submenus = |menu: &str| -> Vec<&str> {
            let rows: Vec<&str> = match proposed.iter().find(|(changed, _)| *changed == menu) {
                Some((_, Some(items))) => listed(items).collect(),
                Some((_, None)) => menus
                    .default_ids(menu)
                    .into_iter()
                    .map(Id::as_str)
                    .collect(),
                None => menus.ids(menu).into_iter().map(Id::as_str).collect(),
            };
            rows.into_iter()
                .filter(|row| menu_ids.iter().any(|id| id.as_str() == *row))
                .collect()
        };
        for (menu, items) in &proposed {
            if items.is_none() {
                continue;
            }
            // Depth first from the menu's rows; each menu is walked once.
            let mut seen: Vec<&str> = Vec::new();
            let mut pending = submenus(menu);
            while let Some(next) = pending.pop() {
                if next == *menu {
                    return Err(ERR_INVALID);
                }
                if !seen.contains(&next) {
                    seen.push(next);
                    pending.extend(submenus(next));
                }
            }
        }
        Ok(())
    }

    /// The batch's global `bind_key`s together: they leave the global layer an
    /// input to the main menu the device has, where it had one (`ERR_DENIED`
    /// for the batch otherwise). A reset returns an input to the declared and
    /// preset bindings, which the key map keeps the menu among.
    pub(super) fn validate_menu_input(&self, batch: &CommandBatch<'_>) -> Result<(), i32> {
        let Some(ui) = self.ui.as_ref() else {
            return Ok(());
        };
        if !batch.iter().any(|command| global(command).is_some()) {
            return Ok(());
        }
        let env = ui.env();
        let has = DeviceInputs::new(&env.device().input);
        if keymap::binds_away_the_menu(env.keys(), &has, batch.iter().filter_map(global)) {
            return Err(ERR_DENIED);
        }
        Ok(())
    }
}

/// The input and action of a `bind_key` that binds in the global layer (no
/// reset).
fn global<'b>(command: WireCommand<'b>) -> Option<(&'b str, &'b str)> {
    match command {
        WireCommand::BindKey {
            layer,
            input,
            action,
            flags,
        } if layer == GLOBAL_KEYMAP_LAYER && flags & RESET == 0 => Some((input, action)),
        _ => None,
    }
}

/// The kernel's command of a checked `set_menu` or `bind_key`.
pub(super) fn owned(command: WireCommand<'_>) -> Result<Ask, i32> {
    match command {
        WireCommand::SetMenu { menu, items, flags } => set_menu(menu, items, flags),
        WireCommand::BindKey {
            layer,
            input,
            action,
            flags,
        } => bind_key(layer, input, action, flags),
        _ => Err(ERR_UNSUPPORTED),
    }
}

/// The kernel's `Ask::SetMenu` of a checked `set_menu`.
fn set_menu(menu: &str, items: &str, flags: u32) -> Result<Ask, i32> {
    let menu = ui::copy_id(menu)?;
    if flags & RESET != 0 {
        return Ok(Ask::SetMenu(menu, None));
    }
    let mut ids = Vec::new();
    ids.try_reserve_exact(listed(items).count())
        .map_err(|_| ERR_NO_MEMORY)?;
    for item in listed(items) {
        ids.push(ui::copy_id(item)?);
    }
    Ok(Ask::SetMenu(menu, Some(ids)))
}

/// The kernel's `Ask::Bind` of a checked `bind_key`: an empty action binds
/// nothing (interface 6 `NOTHING`), a reset none.
fn bind_key(layer: &str, input: &str, action: &str, flags: u32) -> Result<Ask, i32> {
    let action = match (flags & RESET != 0, action) {
        (true, _) => None,
        (false, "") => Some(paperpaw_sdk::builtin::actions::NOTHING),
        (false, action) => Some(ui::copy_id(action)?),
    };
    Ok(Ask::Bind {
        layer: copy_text(layer)?,
        input: copy_text(input)?,
        action,
    })
}

/// `text`, copied fallibly.
fn copy_text(text: &str) -> Result<String, i32> {
    let mut owned = String::new();
    owned
        .try_reserve_exact(text.len())
        .map_err(|_| ERR_NO_MEMORY)?;
    owned.push_str(text);
    Ok(owned)
}
