//! What the browser declares (slice 1): its screen, the slot it fills (the
//! reader opens links out of books there), the action that opens it, its
//! place in the tools menu, its icon and the event that brings a page. The
//! network need (`uses.network`) belongs to the plugin's metadata, made with
//! the native entry (`PORTING-7.md`). Compared with the interface-6
//! registration by `tests/abi7_declarations.rs`.
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EventKind, Instances, MenuEntryDecl, ScreenDecl,
    SlotOfferDecl,
};

/// Callback tokens: stable within this plugin.
pub const SCREEN: u32 = 10;
pub const OPEN: u32 = 1;
pub const LOADED: u32 = 2;

/// The browser's one screen: the start screen with its lists.
pub fn screens<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.screen(ScreenDecl::new("screen", "browser.title", SCREEN).instances(Instances::PerParams))?;
    d.submit()?;
    Ok(())
}

/// The slot, the action, its place in the tools menu and its icon.
pub fn navigation<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.slot_offer(SlotOfferDecl::new("core.browser", "browser.screen"))?;
    d.action(ActionDecl::new("open", "browser.title", OPEN))?;
    let mut entry = MenuEntryDecl::new("core.tools", "browser.open");
    entry.order = 43;
    d.menu_entry(entry)?;
    d.icon("browser.open", "world")?;
    d.submit()?;
    Ok(())
}

/// The answer to a request (`http_done`).
pub fn events<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.subscribe_event(EventKind::HttpDone, LOADED)?;
    d.submit()?;
    Ok(())
}
