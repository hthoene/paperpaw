//! The host's menus as the native plugins declared them (`screens-input.md`
//! section 1): the menu ids of `core.registry` op 11.
//!
//! The rows live in the declaration store, charged there, and leave with their
//! owner (`begin_native_unload` removes the owner's rows and prunes the
//! catalog); this module only reads them, with no allocation and no plugin call.

use super::{
    declaration_store::{DeclarationStore, Kind},
    native_declarations::NativeDeclarations,
    registrations::Record,
};
use paperpaw_native::host_glue::ERR_BUSY;

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

/// Calls `visit` with every menu a record names, in declaration order (owners
/// as stored, then the kinds entry, section, hide), repeats included.
fn declared(store: &DeclarationStore, visit: &mut dyn FnMut(&str) -> bool) {
    for kind in [Kind::MenuEntry, Kind::MenuSection, Kind::MenuHide] {
        let mut go = true;
        store.keys(kind, &mut |key| {
            if go {
                store.with_record(key, &mut |record| {
                    if let Some(menu) = menu_of(record) {
                        go = visit(menu);
                    }
                });
            }
        });
        if !go {
            return;
        }
    }
}

impl NativeDeclarations {
    /// Op 11: every menu id once, with `flags` bit 5 where `user_defined`
    /// says the user redefined it. `custom` are the menus the user defined
    /// (they show even when empty). Ids come in declaration order, then the
    /// user's own. `ERR_BUSY` when the store is borrowed.
    pub(crate) fn menu_ids<'a>(
        &self,
        custom: &mut dyn Iterator<Item = &'a str>,
        user_defined: &dyn Fn(&str) -> bool,
        visit: &mut dyn FnMut(&str, u32),
    ) -> Result<(), i32> {
        let store = self.store.try_borrow().map_err(|_| ERR_BUSY)?;
        let mut index = 0_usize;
        declared(&store, &mut |menu| {
            // Not twice: skip it when an earlier row named the same menu.
            let mut earlier = false;
            let mut at = 0_usize;
            declared(&store, &mut |other| {
                if at >= index {
                    return false;
                }
                at += 1;
                earlier = earlier || other == menu;
                !earlier
            });
            index += 1;
            if !earlier {
                visit(menu, flags(menu, user_defined));
            }
            true
        });
        // The user's menus that no plugin named.
        for menu in custom {
            let mut seen = false;
            declared(&store, &mut |other| {
                seen = other == menu;
                !seen
            });
            if !seen {
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
