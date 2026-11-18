//! The Bluetooth plugin's native declarations: its settings page and the
//! setting on it, its two screens, the action that opens the page, the menu
//! need that hides it where there is no radio, the slot it fills and the
//! events it listens to.
use paperpaw_sdk::MenuNeed;
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EventKind, GroupDecl, Instances, Kind, ScreenDecl,
    SlotOfferDecl, SpecDecl,
};

/// The callback token of the events (start, waking, books, settings).
pub const EVENTS_TOKEN: u32 = 1;
/// The action that opens the page.
pub const OPEN_TOKEN: u32 = 2;
/// The page: the devices bonded with this one; one instance at a time.
pub const PAGE_TOKEN: u32 = 10;
/// Adding a device: a search, then pairing; one instance at a time.
pub const SEARCH_TOKEN: u32 = 11;

/// The setting group, shown as the Bluetooth page.
pub const GROUP: &str = "bluetooth.group";
/// The page.
pub const PAGE: &str = "bluetooth.page";
/// The action that opens the page.
pub const OPEN: &str = "bluetooth.open";
/// The setting: when a remote's or a keyboard's keys go to the key maps.
pub const REMOTES: &str = "bluetooth.remotes";
/// Only when chosen on the page.
pub const BY_HAND: &str = "bluetooth.remotes.hand";
/// While a book is open.
pub const WHILE_READING: &str = "bluetooth.remotes.reading";
/// Whenever the device is awake.
pub const WHILE_AWAKE: &str = "bluetooth.remotes.awake";
/// The choices of [`REMOTES`], each its name's key.
pub const REMOTES_CHOICES: [&str; 3] = [BY_HAND, WHILE_READING, WHILE_AWAKE];
/// The host slot of the Bluetooth manager: the plugin bound to it sees every
/// bond and gets the Bluetooth events (`docs/abi/device.md`).
pub const SLOT: &str = "core.bluetooth";

pub fn settings<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    let mut group = GroupDecl::new("group", "bluetooth.title");
    group.order = 86;
    group.page = "page";
    d.group(group)?;
    let mut remotes = SpecDecl::new(
        "remotes",
        Kind::Choice {
            default: BY_HAND,
            choices: &REMOTES_CHOICES,
        },
        GROUP,
        "bluetooth.remotes",
    );
    remotes.help = "bluetooth.remotes.help";
    d.spec(remotes)?;
    d.submit()?;
    Ok(())
}

pub fn screens<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.screen(ScreenDecl::new("page", "bluetooth.title", PAGE_TOKEN).instances(Instances::Single))?;
    d.screen(
        ScreenDecl::new("search", "bluetooth.add", SEARCH_TOKEN).instances(Instances::Single),
    )?;
    d.submit()?;
    Ok(())
}

/// The action, its icons, where the radio is needed, and the slot.
pub fn navigation<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.action(ActionDecl::new("open", "bluetooth.title", OPEN_TOKEN))?;
    d.icon(GROUP, "bluetooth")?;
    d.icon(OPEN, "bluetooth")?;
    d.menu_need(GROUP, MenuNeed::Bluetooth)?;
    d.menu_need(REMOTES, MenuNeed::Bluetooth)?;
    d.slot_offer(SlotOfferDecl::new(SLOT, PAGE))?;
    d.submit()?;
    Ok(())
}

/// Starting and waking, a book opened or closed, and a setting changed.
/// Event 98 (Bluetooth) needs no subscription: the host sends it to the
/// plugin bound to [`SLOT`].
pub fn events<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    for kind in [
        EventKind::Started,
        EventKind::Resume,
        EventKind::BookOpened,
        EventKind::BookClosed,
        EventKind::SettingChanged,
    ] {
        d.subscribe_event(kind, EVENTS_TOKEN)?;
    }
    d.submit()?;
    Ok(())
}
