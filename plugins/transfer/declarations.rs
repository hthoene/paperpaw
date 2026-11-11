//! What the transfer declares, compared with the interface-6 registration by
//! the declaration tests: its three screens, the action that opens the
//! transfer, its place in the tools and icon, and the two slots its screens
//! are offered for (typing on a phone, the cable). The native plugin submits
//! the transfer screen, the phone's screen with its slot, the cable's screen
//! with its slot, the navigation and the events.
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EventKind, Instances, MenuEntryDecl, ScreenDecl,
    SlotOfferDecl,
};

/// Callback tokens: stable within this plugin.
pub const OPEN_TOKEN: u32 = 1;
pub const SCREEN: u32 = 10;
pub const PHONE: u32 = 11;
pub const CABLE: u32 = 12;
/// The events it hears: a server of its own started, failed or stopped, and
/// a request to it (the page's server and the phone's).
pub const SERVER_EVENT: u32 = 2;
pub const SERVED_REQUEST: u32 = 3;
/// The end of an install of a plugin sent from the computer.
pub const INSTALL_DONE: u32 = 4;
/// The end of hosting the reader's own network, and the idle timer that closes
/// it.
pub const WIFI_DONE: u32 = 5;
pub const WORK: u32 = 6;

/// The transfer screen, one at a time, and the phone's keyboard screen (kept
/// per parameters) with the slot it is offered for.
pub fn screens<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.screen(ScreenDecl::new("screen", "transfer.title", SCREEN).instances(Instances::Single))?;
    d.screen(
        ScreenDecl::new("phone", "transfer.phone.title", PHONE).instances(Instances::PerParams),
    )?;
    d.slot_offer(SlotOfferDecl::new("core.remote-keyboard", "transfer.phone"))?;
    d.submit()?;
    Ok(())
}

/// The cable's screen (one at a time) and the slot it is offered for: the host
/// opens it when a computer talks over the USB cable and no server runs
/// (`docs/abi/ui.md`, slot `core.cable`).
pub fn cable<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.screen(ScreenDecl::new("cable", "transfer.cable.title", CABLE).instances(Instances::Single))?;
    d.slot_offer(SlotOfferDecl::new("core.cable", "transfer.cable"))?;
    d.submit()?;
    Ok(())
}

/// The action, its place in the tools and its icon.
pub fn navigation<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.action(ActionDecl::new("open", "transfer.title", OPEN_TOKEN))?;
    let mut entry = MenuEntryDecl::new("core.tools", "transfer.open");
    entry.order = 46;
    d.menu_entry(entry)?;
    d.icon("transfer.open", "transfer")?;
    d.submit()?;
    Ok(())
}

/// What the served page's server tells and the requests it gets.
pub fn events<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.subscribe_event(EventKind::Server, SERVER_EVENT)?;
    d.subscribe_event(EventKind::ServedRequest, SERVED_REQUEST)?;
    d.subscribe_event(EventKind::InstallDone, INSTALL_DONE)?;
    d.subscribe_event(EventKind::WifiDone, WIFI_DONE)?;
    d.subscribe_event(EventKind::WorkDue, WORK)?;
    d.submit()?;
    Ok(())
}
