//! Static declarations: screen identity, the action and its menu place, events.
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EventKind, MenuEntryDecl, ScreenDecl,
};

/// Screen tokens and the action's token are stable within this plugin image.
pub const OPEN_TOKEN: u32 = 1;
pub const EVENTS_TOKEN: u32 = 2;

pub fn screens<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.screen(ScreenDecl::new("menus", "menu-editor.title", 10))?;
    d.screen(ScreenDecl::new("entries", "menu-editor.title", 11))?;
    // The options of one entry are drawn over the list below.
    d.screen(ScreenDecl::new("entry", "menu-editor.title", 12).overlay())?;
    d.screen(ScreenDecl::new("add", "menu-editor.title", 13))?;
    d.submit()?;
    Ok(())
}

/// Actions and menu placement.
pub fn navigation<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.action(ActionDecl::new("open", "menu-editor.title", OPEN_TOKEN))?;
    let mut entry = MenuEntryDecl::new("core.settings-pages", "menu-editor.open");
    entry.order = 210;
    d.menu_entry(entry)?;
    d.icon("menu-editor.open", "menu-2")?;
    d.submit()?;
    Ok(())
}

/// The lists follow menu changes.
pub fn events<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.subscribe_event(EventKind::RegistryChanged, EVENTS_TOKEN)?;
    d.submit()?;
    Ok(())
}
