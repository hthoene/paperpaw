//! Native screen identity preparation; runtime adapters remain open.
use paperpaw_sdk::native::declare::{Declarations, DeclareError, ScreenDecl};
pub fn screens<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.screen(ScreenDecl::new("menus", "menu-editor.title", 10))?;
    d.screen(ScreenDecl::new("entries", "menu-editor.title", 11))?;
    d.screen(ScreenDecl::new("entry", "menu-editor.title", 12))?;
    d.screen(ScreenDecl::new("add", "menu-editor.title", 13))?;
    d.submit()?;
    Ok(())
}
