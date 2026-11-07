//! Native screen identity preparation; runtime adapters remain open.
use paperpaw_sdk::native::declare::{Declarations, DeclareError, ScreenDecl};
pub fn screens<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.screen(ScreenDecl::new("list", "profiles.title", 10))?;
    d.submit()?;
    Ok(())
}
