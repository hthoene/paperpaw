//! What the settings plugin declares, built into its image by `build.rs` and
//! compared with the interface-6 registration by the declaration tests.
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EventKind, Kind, MenuEntryDecl, ScreenDecl,
    SlotOfferDecl, SpecDecl,
};

/// Callback tokens: stable within this plugin.
pub const LIST: u32 = 10;
pub const GROUP: u32 = 11;
pub const CHOOSE: u32 = 12;
pub const THEMES: u32 = 13;
pub const SEARCH: u32 = 14;
/// Passwords and keys: the list of them.
pub const SECRETS: u32 = 15;
/// What can be done with one of them.
pub const SECRET: u32 = 16;
/// The event subscription's token.
pub const EVENTS: u32 = 3;
/// The action that opens the list of passwords and keys.
pub const OPEN_SECRETS: u32 = 4;
/// Where the entry of the passwords and keys stands in the device's part of
/// the main page, after the system.
pub const SECRETS_ORDER: i16 = 95;

/// The switch for advanced settings.
pub fn settings<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    let mut spec = SpecDecl::new(
        "advanced",
        Kind::Bool(false),
        "core.group.system",
        "settings.advanced",
    );
    spec.help = "settings.advanced.help";
    d.spec(spec)?;
    d.submit()
}

/// The screens of the settings.
pub fn screens<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.screen(ScreenDecl::new("list", "settings.title", LIST))?;
    d.screen(ScreenDecl::new("group", "settings.title", GROUP))?;
    d.screen(ScreenDecl::new("themes", "settings.theme", THEMES))?;
    d.screen(ScreenDecl::new("choose", "settings.title", CHOOSE))?;
    d.screen(ScreenDecl::new("search", "settings.search", SEARCH))?;
    d.screen(ScreenDecl::new("secrets", "settings.secrets", SECRETS))?;
    d.screen(ScreenDecl::new("secret", "settings.secrets", SECRET))?;
    d.submit()
}

/// The slots the main page and the chooser fill, and the entry of the
/// passwords and keys with the action and icon it has.
pub fn navigation<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.slot_offer(SlotOfferDecl::new("core.settings", "settings.list"))?;
    d.slot_offer(SlotOfferDecl::new("core.choose-value", "settings.choose"))?;
    d.action(ActionDecl::new(
        "secrets.open",
        "settings.secrets",
        OPEN_SECRETS,
    ))?;
    let mut entry = MenuEntryDecl::new("core.settings-pages", "settings.secrets.open");
    entry.order = SECRETS_ORDER;
    d.menu_entry(entry)?;
    d.icon("settings.secrets.open", "key")?;
    d.submit()
}

/// Values and plugins change what the pages show.
pub fn events<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    for kind in [
        EventKind::SettingChanged,
        EventKind::PluginsChanged,
        EventKind::RegistryChanged,
        EventKind::PreviewChanged,
    ] {
        d.subscribe_event(kind, EVENTS)?;
    }
    d.submit()
}
