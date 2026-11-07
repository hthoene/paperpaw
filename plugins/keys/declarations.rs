//! Native screen identity preparation; runtime adapters remain open.
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, MenuEntryDecl, ScreenDecl,
};
pub fn screens<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.screen(ScreenDecl::new("pick", "keys.title", 10))?;
    d.screen(ScreenDecl::new("place", "keys.place", 11))?;
    d.screen(ScreenDecl::new("list", "keys.title", 12))?;
    d.submit()?;
    Ok(())
}

/// Actions and menu placement; callback tokens are stable within this plugin.
pub fn navigation<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.action(ActionDecl::new("open", "keys.title", 1))?;
    let mut entry = MenuEntryDecl::new("core.settings-pages", "keys.open");
    entry.order = 75;
    d.menu_entry(entry)?;
    d.icon("keys.open", "hand-finger")?;
    d.submit()?;
    Ok(())
}
