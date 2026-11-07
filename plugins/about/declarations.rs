//! The About page's native declarations: its screen, action and menu entry.
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, Instances, MenuEntryDecl, ScreenDecl,
};

/// The page's screen token; one instance at a time, as before.
pub const SCREEN_TOKEN: u32 = 10;
/// The action that opens the page.
pub const OPEN_TOKEN: u32 = 1;

pub fn screens<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.screen(ScreenDecl::new("screen", "about.title", SCREEN_TOKEN).instances(Instances::Single))?;
    d.submit()?;
    Ok(())
}

/// The action, its place last in the device's settings, and its icon.
pub fn navigation<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.action(ActionDecl::new("open", "about.title", OPEN_TOKEN))?;
    let mut entry = MenuEntryDecl::new("core.settings-pages", "about.open");
    entry.order = 99;
    d.menu_entry(entry)?;
    d.icon("about.open", "info-circle")?;
    d.submit()?;
    Ok(())
}
