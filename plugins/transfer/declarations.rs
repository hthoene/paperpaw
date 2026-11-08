//! What the transfer declares, compared with the interface-6 registration by
//! the declaration tests: its three screens, the action that opens the
//! transfer, its place in the tools and icon, and the two slots its screens
//! are offered for (typing on a phone, the cable).
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, Instances, MenuEntryDecl, ScreenDecl, SlotOfferDecl,
};

/// Callback tokens: stable within this plugin.
pub const OPEN_TOKEN: u32 = 1;
pub const SCREEN: u32 = 10;
pub const PHONE: u32 = 11;
pub const CABLE: u32 = 12;

/// The transfer screen and the cable's, one at a time; the phone's keyboard
/// screen is reused for equal parameters.
pub fn screens<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.screen(ScreenDecl::new("phone", "transfer.phone.title", PHONE))?;
    d.screen(ScreenDecl::new("screen", "transfer.title", SCREEN).instances(Instances::Single))?;
    d.screen(ScreenDecl::new("cable", "transfer.cable.title", CABLE).instances(Instances::Single))?;
    d.submit()?;
    Ok(())
}

/// The action, its place in the tools, its icon and the two slots.
pub fn navigation<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.action(ActionDecl::new("open", "transfer.title", OPEN_TOKEN))?;
    let mut entry = MenuEntryDecl::new("core.tools", "transfer.open");
    entry.order = 46;
    d.menu_entry(entry)?;
    d.icon("transfer.open", "transfer")?;
    d.slot_offer(SlotOfferDecl::new("core.remote-keyboard", "transfer.phone"))?;
    d.slot_offer(SlotOfferDecl::new("core.cable", "transfer.cable"))?;
    d.submit()?;
    Ok(())
}
