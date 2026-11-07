//! Native identity of Setup: the guide screen, the action that opens it, its
//! place in the settings menu, its offer for the setup slot and the start it
//! follows. Callback tokens are stable within this plugin.
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EventKind, Instances, MenuEntryDecl, ScreenDecl,
    SlotOfferDecl,
};

/// The guide's screen token, as the plugin's callbacks take it.
pub const GUIDE: u32 = 20;
/// The action that opens the guide.
pub const OPEN: u32 = 1;
/// The start event's token.
pub const STARTED: u32 = 2;

/// The guide (one at a time).
pub fn screens<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.screen(ScreenDecl::new("guide", "setup.title", GUIDE).instances(Instances::Single))?;
    d.submit()?;
    Ok(())
}

/// The guide as the screen of the setup slot, which another plugin can replace.
pub fn slots<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.slot_offer(SlotOfferDecl::new("core.setup", "setup.guide"))?;
    d.submit()?;
    Ok(())
}

/// The action that opens the setup slot, in the device's part of the settings,
/// before *About*.
pub fn navigation<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.action(ActionDecl::new("open", "setup.open", OPEN))?;
    let mut entry = MenuEntryDecl::new("core.settings-pages", "setup.open");
    entry.order = 97;
    d.menu_entry(entry)?;
    d.icon("setup.open", "wand")?;
    d.submit()?;
    Ok(())
}

/// The first start: the guide opens once.
pub fn events<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.subscribe_event(EventKind::Started, STARTED)?;
    d.submit()?;
    Ok(())
}
