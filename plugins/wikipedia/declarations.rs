//! The lookup's native declarations: the summary screen (an overlay that
//! fills the slot `core.lookup-online`, with the subscription to the end of
//! its request), the provider that offers Wikipedia beside the dictionaries
//! (it answers *pending* and fetches, `native_provider`), and the
//! subscription that adds the line naming the source and the licence to
//! *About*.
use paperpaw_sdk::native::declare::{
    Declarations, DeclareError, EventKind, LookupProviderDecl, ScreenDecl, SlotOfferDecl,
    SubscribeDecl,
};

/// The token of the summary screen's callbacks.
pub const SCREEN_TOKEN: u32 = 1;
/// The token of the end of a request for a summary (`event.http_done`).
pub const FETCHED_TOKEN: u32 = 3;
/// The token of the answers to `about.lines`.
pub const ABOUT_TOKEN: u32 = 4;
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

/// The provider, a network one: the host calls it only while connected
/// (`document.md` 7).
pub fn provider<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.lookup_provider(
        LookupProviderDecl::new("summary", "wikipedia.title", PROVIDER_TOKEN).online(),
    )?;
    d.submit()?;
    Ok(())
}

/// The line in *About* that names the source and the licence: the plugin
/// answers `about.lines` (`stages/subscribed/about.lines@1.def`, a copy of
/// the owner's schema, held to it by a test).
pub fn about<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.subscribe(SubscribeDecl::new("about.lines", ABOUT_TOKEN))?;
    d.submit()?;
    Ok(())
}
