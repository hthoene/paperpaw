//! Setup's declarations as borrowed builders, shared by the build script
//! (which writes them into the plugin image) and the tests.
use paperpaw_sdk::builtin::{menus, slots};
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EventKind, Instances, MenuEntryDecl, ScreenDecl,
    SlotOfferDecl,
};

/// Callback tokens are stable within this plugin image.
pub const EVENTS_TOKEN: u32 = 1;
pub const OPEN_TOKEN: u32 = 2;
pub const GUIDE_TOKEN: u32 = 10;

/// The guide's screen (one at a time), the slot it fills, the action and the
/// entry in the device's part of the settings, before *About*, with its icon.
pub fn ui(sink: impl FnMut(&[u8]) -> Result<(), i32>) -> Result<(), DeclareError> {
    let mut declarations = Declarations::new(sink);
    declarations.screen(
        ScreenDecl::new("guide", "setup.title", GUIDE_TOKEN).instances(Instances::Single),
    )?;
    declarations.slot_offer(SlotOfferDecl::new(slots::SETUP.as_str(), "setup.guide"))?;
    declarations.action(ActionDecl::new("open", "setup.open", OPEN_TOKEN))?;
    let settings = menus::SETTINGS;
    let mut entry = MenuEntryDecl::new(settings.as_str(), "setup.open");
    entry.order = 97;
    declarations.menu_entry(entry)?;
    declarations.icon("setup.open", "wand")?;
    declarations.submit()
}

/// The first start, the answer to the skip question, the settings and the
/// radio the pages show, and the steps' coming and going.
pub fn events(sink: impl FnMut(&[u8]) -> Result<(), i32>) -> Result<(), DeclareError> {
    let mut declarations = Declarations::new(sink);
    for kind in [
        EventKind::Started,
        // EventKind::Answered (draft 22): the answers to the skip question and the list of options.
        // EventKind::Answered,
        EventKind::SettingChanged,
        EventKind::WifiChanged,
        EventKind::RegistryChanged,
    ] {
        declarations.subscribe_event(kind, EVENTS_TOKEN)?;
    }
    declarations.submit()
}
