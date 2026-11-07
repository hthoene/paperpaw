//! Native screen identity preparation; runtime adapters remain open.
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, Instances, MenuEntryDecl, ScreenDecl,
};
pub fn screens<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.screen(ScreenDecl::new("list", "profiles.title", 10).instances(Instances::Single))?;
    d.submit()?;
    Ok(())
}

/// Actions and menu placement; callback tokens are stable within this plugin.
pub fn navigation<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.action(ActionDecl::new("open", "profiles.title", 1))?;
    d.action(ActionDecl::new("next", "profiles.next", 2))?;
    let mut entry = MenuEntryDecl::new("core.reader-more", "profiles.open");
    entry.order = 15;
    d.menu_entry(entry)?;
    let mut entry = MenuEntryDecl::new("core.settings-pages", "profiles.open");
    entry.order = 205;
    d.menu_entry(entry)?;
    d.icon("profiles.open", "stack-2")?;
    d.submit()?;
    Ok(())
}
