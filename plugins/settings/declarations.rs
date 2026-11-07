//! What the settings plugin declares, built into its image by `build.rs` and
//! compared with the interface-6 registration by the declaration tests.
use paperpaw_sdk::native::declare::{
    Declarations, DeclareError, EventKind, Kind, ScreenDecl, SlotOfferDecl, SpecDecl,
};

/// Callback tokens: stable within this plugin.
pub const LIST: u32 = 10;
pub const GROUP: u32 = 11;
pub const CHOOSE: u32 = 12;
pub const THEMES: u32 = 13;
pub const SEARCH: u32 = 14;
/// The event subscription's token.
pub const EVENTS: u32 = 3;

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
    d.submit()
}

/// The slots the main page and the chooser fill.
pub fn navigation<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.slot_offer(SlotOfferDecl::new("core.settings", "settings.list"))?;
    d.slot_offer(SlotOfferDecl::new("core.choose-value", "settings.choose"))?;
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
