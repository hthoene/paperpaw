//! The lookup's native declarations (slice 1): the summary screen, and the
//! provider that offers Wikipedia beside the dictionaries (declared but not
//! submitted until the host admits providers). Binding the
//! screen to the slot `core.lookup-online`, the line in *About*, drawing and
//! fetching follow with their host pieces (`PORTING-7.md`).
use paperpaw_sdk::native::declare::{Declarations, DeclareError, LookupProviderDecl, ScreenDecl};

/// The token of the summary screen's callbacks.
pub const SCREEN_TOKEN: u32 = 1;
/// The token the host calls the provider with.
pub const PROVIDER_TOKEN: u32 = 2;

/// The summary screen.
pub fn screen<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    // The summary is drawn over the page below it, in the lower part.
    d.screen(ScreenDecl::new("lookup", "wikipedia.title", SCREEN_TOKEN).overlay())?;
    d.submit()?;
    Ok(())
}

/// The provider. Not submitted yet: the host refuses a lookup provider
/// (`ERR_UNSUPPORTED`) until the stage `core.lookup` is live, and a plugin
/// whose `make` fails is not admitted at all (`PORTING-7.md`).
pub fn provider<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.lookup_provider(
        LookupProviderDecl::new("summary", "wikipedia.title", PROVIDER_TOKEN).online(),
    )?;
    d.submit()?;
    Ok(())
}
