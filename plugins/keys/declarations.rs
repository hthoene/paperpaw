//! Native screen identity preparation; runtime adapters remain open.
use paperpaw_sdk::native::declare::{Declarations, DeclareError, ScreenDecl};
pub fn screens<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.screen(ScreenDecl::new("pick", "keys.title", 10))?;
    d.screen(ScreenDecl::new("place", "keys.place", 11))?;
    d.screen(ScreenDecl::new("list", "keys.title", 12))?;
    d.submit()?;
    Ok(())
}
