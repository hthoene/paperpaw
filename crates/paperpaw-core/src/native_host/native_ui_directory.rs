//! Guarded widget/status lookup over the registry's retained declaration store.
use super::{
    declaration_store::{DeclarationStore, Kind},
    declarations::full_id,
    native_declarations::NativeDeclarations,
    registrations::Record,
    ui_service::{Declared, UiDirectory},
};
use paperpaw_native::host_glue::{ERR_BUSY, ERR_NOT_FOUND};

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
}
impl NativeDeclarations {
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
