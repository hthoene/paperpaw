//! The lookup's native declarations: the summary screen (an overlay that
//! fills the slot `core.lookup-online`, with the subscription to the end of
//! its request), and the provider that offers Wikipedia beside the
//! dictionaries (declared but not submitted until the host admits and
//! answers providers that fetch). The line in *About* follows with its host
//! piece (`PORTING-7.md`).
use paperpaw_sdk::native::declare::{
    Declarations, DeclareError, EventKind, LookupProviderDecl, ScreenDecl, SlotOfferDecl,
};

/// The token of the summary screen's callbacks.
pub const SCREEN_TOKEN: u32 = 1;
/// The token of the end of a request for a summary (`event.http_done`).
pub const FETCHED_TOKEN: u32 = 3;
/// The token the host calls the provider with.
pub const PROVIDER_TOKEN: u32 = 2;

/// The summary screen.
pub fn screen<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    // The summary is drawn over the page below it, in the lower part.
    d.screen(ScreenDecl::new("lookup", "wikipedia.title", SCREEN_TOKEN).overlay())?;
    // It fills the slot the reader's online lookup opens.
    d.slot_offer(SlotOfferDecl::new("core.lookup-online", "wikipedia.lookup"))?;
    d.subscribe_event(EventKind::HttpDone, FETCHED_TOKEN)?;
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
