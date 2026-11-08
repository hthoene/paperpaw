//! What the menu plugin declares, built into its image by `build.rs` and
//! compared with the interface-6 registration by the declaration tests.
use paperpaw_sdk::native::declare::{
    Declarations, DeclareError, EventKind, Kind, ScreenDecl, SlotOfferDecl, SpecDecl,
};

/// Callback tokens: stable within this plugin.
pub const LIST: u32 = 10;
/// The event subscription's token.
pub const EVENTS: u32 = 3;

/// How menus look.
pub fn settings<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    let mut style = SpecDecl::new(
        "style",
        Kind::Choice {
            default: "menu.style.list",
            choices: &["menu.style.list", "menu.style.tiles"],
        },
        "core.group.look",
        "menu.setting.style",
    );
    style.help = "menu.setting.style.help";
    d.spec(style)?;
    d.submit()
}

/// The menu: a sheet over the screen below.
pub fn screens<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.screen(ScreenDecl::new("list", "menu.title", LIST).overlay())?;
    d.submit()
}

/// The slot the menu fills.
pub fn navigation<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.slot_offer(SlotOfferDecl::new("core.menu", "menu.list"))?;
    d.submit()
}

/// Values, plugins and menus change what the sheet shows.
pub fn events<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    for kind in [
        EventKind::SettingChanged,
        EventKind::PluginsChanged,
        EventKind::RegistryChanged,
    ] {
        d.subscribe_event(kind, EVENTS)?;
    }
    d.submit()
}
