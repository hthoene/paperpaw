//! The host's menus as the native plugins declared them (`screens-input.md`
//! section 1): the menu ids of `core.registry` op 11.
//!
//! The rows live in the declaration store, charged there, and leave with their
//! owner (`begin_native_unload` removes the owner's rows and prunes the
//! catalog); this module only reads them, with no plugin call and one small
//! seen-list of ids.

use super::{
    declaration_store::Kind, native_declarations::NativeDeclarations, registrations::Record,
};
use alloc::{string::String, vec::Vec};

use paperpaw_native::host_glue::{ERR_BUSY, ERR_NO_MEMORY};

/// `reg_item.flags` bit 5 of op 11: the user has a `command.set_menu`
/// definition for this menu, even when no plugin put anything into it.
pub(crate) const MENU_USER_DEFINED: u32 = 1 << 5;

/// The menu named by a record of the three kinds that name one, else `None`.
fn menu_of<'a>(record: Record<'a>) -> Option<&'a str> {
    match record {
        Record::MenuEntry(row) => Some(row.menu),
        Record::MenuSection(row) => Some(row.menu),
        Record::MenuHide(row) => Some(row.menu),
        _ => None,
    }
}

/// Records `menu` in `seen` unless an earlier row already named it; `true`
/// when it is new. The one allocation is reserved fallibly.
fn first_time(seen: &mut Vec<String>, menu: &str) -> Result<bool, i32> {
    if seen.iter().any(|other| other == menu) {
        return Ok(false);
    }
    seen.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
    let mut copy = String::new();
    copy.try_reserve_exact(menu.len())
        .map_err(|_| ERR_NO_MEMORY)?;
    copy.push_str(menu);
    seen.push(copy);
    Ok(true)
}

impl NativeDeclarations {
    /// Op 11: every menu id once, with `flags` bit 5 where `user_defined`
    /// says the user redefined it. `custom` are the menus the user defined
    /// (they show even when empty). One pass over the stored rows: the menus
    /// named by entry rows, then by section rows, then by hide rows (within a
    /// kind in owner order as stored), each at its first mention; then the
    /// user's own that no plugin named. `ERR_BUSY` when the store is borrowed,
    /// `ERR_NO_MEMORY` when the seen-list cannot grow (some ids were visited).
    pub(crate) fn menu_ids<'a>(
        &self,
        custom: &mut dyn Iterator<Item = &'a str>,
        user_defined: &dyn Fn(&str) -> bool,
        visit: &mut dyn FnMut(&str, u32),
    ) -> Result<(), i32> {
        let store = self.store.try_borrow().map_err(|_| ERR_BUSY)?;
        let mut seen: Vec<String> = Vec::new();
        let mut failed = None;
        for kind in [Kind::MenuEntry, Kind::MenuSection, Kind::MenuHide] {
            store.keys(kind, &mut |key| {
                if failed.is_some() {
                    return;
                }
                store.with_record(key, &mut |record| {
                    let Some(menu) = menu_of(record) else {
                        return;
                    };
                    match first_time(&mut seen, menu) {
                        Ok(true) => visit(menu, flags(menu, user_defined)),
                        Ok(false) => {}
                        Err(error) => failed = Some(error),
                    }
                });
            });
        }
        if let Some(error) = failed {
            return Err(error);
        }
        // The user's menus that no plugin named.
        for menu in custom {
            if first_time(&mut seen, menu)? {
                visit(menu, flags(menu, user_defined));
            }
        }
        Ok(())
    }
}

fn flags(menu: &str, user_defined: &dyn Fn(&str) -> bool) -> u32 {
    if user_defined(menu) {
        MENU_USER_DEFINED
    } else {
        0
    }
}

impl crate::Kernel {
    /// Op 11 as the host serves it: the menus plugins declared and those the
    /// user defined, each once, with bit 5 where the user has a
    /// `command.set_menu` definition for the menu: also an empty one, or one
    /// equal to the declared rows (interface 6 `is_customized`); a reset
    /// (`set_menu` with no entries) clears it. `ERR_STATE` before the
    /// declarations exist.
    pub(crate) fn native_menu_ids(&self, visit: &mut dyn FnMut(&str, u32)) -> Result<(), i32> {
        let menus = &self.shared.menus;
        let Some(declarations) = &self.shared.registry.native_declarations else {
            return Err(paperpaw_native::host_glue::ERR_STATE);
        };
        declarations.menu_ids(
            &mut menus.customized().map(|id| id.as_str()),
            &|menu| menus.is_customized(menu),
            visit,
        )
    }
}
