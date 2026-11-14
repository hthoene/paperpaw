//! Guarded widget/status lookup over the registry's retained declaration store.
use super::{
    declaration_store::{DeclarationStore, Kind},
    declarations::full_id,
    native_declarations::NativeDeclarations,
    registrations::Record,
    ui_service::{Declared, UiDirectory},
};
use crate::abi7::context::ui_views::HintView;
use alloc::vec::Vec;
use core::ops::ControlFlow;
use paperpaw_native::host_glue::{ERR_BUSY, ERR_NO_MEMORY, ERR_NOT_FOUND};

/// `screen.spec` flags bit 3: help waits until the plugin says it is ready.
const WAITS_READY: u32 = 1 << 3;

impl UiDirectory for NativeDeclarations {
    fn widget(&self, widget: &str) -> Result<Declared, i32> {
        self.lookup(widget, Kind::Widget)
    }
    fn status_item(&self, item: &str) -> Result<Declared, i32> {
        self.lookup(item, Kind::StatusItem)
    }
    fn status_line(&self, line: &str) -> Result<Declared, i32> {
        self.lookup(line, Kind::StatusLine)
    }
    fn hints(
        &self,
        scope: &str,
        visit: &mut dyn FnMut(HintView<'_>) -> ControlFlow<()>,
    ) -> Result<(), i32> {
        let store = self.store.try_borrow().map_err(|_| ERR_BUSY)?;
        // The declared order: the keys of the screen's hints, by plugin and
        // then record (the store keeps a kind in name order). One fallible
        // allocation of the matches only.
        let mut keys = Vec::new();
        let mut failed = false;
        store.keys(Kind::HelpHint, &mut |key| {
            if failed {
                return;
            }
            let mut hit = false;
            store.visit_record(key, &mut |record| {
                hit = matches!(record, Record::HelpHint(hint) if hint.screen == scope);
            });
            if hit {
                failed = keys.try_reserve(1).is_err();
                if !failed {
                    keys.push(key);
                }
            }
        });
        if failed {
            return Err(ERR_NO_MEMORY);
        }
        paperpaw_sort::sort_unstable_by_key(&mut keys, |key| key.order());
        for key in keys {
            let Some(owner) = store.plugin_of(key) else {
                continue;
            };
            let mut flow = ControlFlow::Continue(());
            store.visit_record(key, &mut |record| {
                if let Record::HelpHint(hint) = record {
                    flow = visit(HintView {
                        input: hint.input,
                        x: hint.x,
                        y: hint.y,
                        width: hint.width,
                        height: hint.height,
                        text: hint.text,
                        owner,
                    });
                }
            });
            if flow.is_break() {
                break;
            }
        }
        Ok(())
    }
}
impl NativeDeclarations {
    /// Whether the screen `screen` (a full id) is declared `waits_ready`
    /// (`screen.spec` flags bit 3): its plugin says when it is ready. A screen
    /// nobody declared (an interface 6 or a host screen) is not.
    pub(crate) fn screen_waits_ready(&self, screen: &str) -> bool {
        let Some((plugin, name)) = screen.split_once('.') else {
            return false;
        };
        let Ok(store) = self.store.try_borrow() else {
            return false;
        };
        let mut waits = false;
        store.find(plugin, Kind::Screen, name, &mut |record| {
            if let Record::ScreenSpec(spec) = record {
                waits = spec.flags & WAITS_READY != 0;
            }
        });
        waits
    }

    fn lookup(&self, id: &str, kind: Kind) -> Result<Declared, i32> {
        full_id(id)?;
        let (plugin, name) = id.split_once('.').ok_or(ERR_NOT_FOUND)?;
        let store = self.store.try_borrow().map_err(|_| ERR_BUSY)?;
        lookup(&store, plugin, name, kind)
    }
}
fn lookup(store: &DeclarationStore, plugin: &str, name: &str, kind: Kind) -> Result<Declared, i32> {
    let owner = store.host_owner(plugin)?;
    let mut declaration = Err(ERR_NOT_FOUND);
    store.find(plugin, kind, name, &mut |record| {
        let (token, flags) = match record {
            Record::WidgetSpec(row) if kind == Kind::Widget => (row.token, row.flags),
            Record::StatusItem(row) if kind == Kind::StatusItem => (row.token, row.flags),
            Record::StatusLine(row) if kind == Kind::StatusLine => (row.token, row.flags),
            _ => return,
        };
        declaration = Ok(Declared {
            owner,
            token,
            private: flags & 2 != 0,
        });
    });
    declaration
}

#[cfg(test)]
#[path = "native_ui_directory_tests.rs"]
mod tests;
