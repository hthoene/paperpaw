//! What the simple launcher declares: its settings, the screens (home, quick
//! panel, library, the customising page), the actions, the offers of the home
//! and panel slots, the three menus the user edits (the cards, the dock, the
//! panel's switches and bars) with what they hold at first, and the events it
//! subscribes to. `build.rs` turns it into the image's batches.
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EventKind, GroupDecl, Kind, MenuEntryDecl, ScreenDecl,
    SlotOfferDecl, SpecDecl, StatusBar,
};

/// The screens.
pub const HOME: u32 = 10;
pub const PANEL: u32 = 11;
pub const LIBRARY: u32 = 12;
pub const CUSTOMISE: u32 = 13;
/// The actions.
pub const CONTINUE: u32 = 20;
pub const OPEN_CUSTOMISE: u32 = 21;
/// The events.
pub const TICK: u32 = 30;
pub const SETTING_CHANGED: u32 = 31;
pub const REGISTRY_CHANGED: u32 = 32;
pub const LIBRARY_CHANGED: u32 = 33;

/// The settings group.
const GROUP: &str = "launcher-simple.group";

/// The settings by their full ids.
#[allow(
    dead_code,
    reason = "only the plugin reads the settings; the build script sees the declarations alone"
)]
pub const SHOW_DATE: &str = "launcher-simple.show-date";
#[allow(
    dead_code,
    reason = "only the plugin reads the settings; the build script sees the declarations alone"
)]
pub const SHOW_BATTERY: &str = "launcher-simple.show-battery";

/// The menus the user edits (in Settings, *Edit menus*).
#[allow(
    dead_code,
    reason = "only the plugin reads the menus; the build script sees the declarations alone"
)]
pub const MENU_CARDS: &str = "launcher-simple.cards";
#[allow(
    dead_code,
    reason = "only the plugin reads the menus; the build script sees the declarations alone"
)]
pub const MENU_DOCK: &str = "launcher-simple.dock";
#[allow(
    dead_code,
    reason = "only the plugin reads the menus; the build script sees the declarations alone"
)]
pub const MENU_PANEL: &str = "launcher-simple.panel";

/// The action of the *Continue reading* card.
#[allow(
    dead_code,
    reason = "only the plugin reads the action; the build script sees the declarations alone"
)]
pub const ACTION_CONTINUE: &str = "launcher-simple.continue";

/// The group and its two switches.
pub fn settings<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    let mut group = GroupDecl::new("group", GROUP);
    group.order = 215;
    d.group(group)?;
    for (name, title, help) in [
        (
            "show-date",
            "launcher-simple.setting.date",
            "launcher-simple.setting.date.help",
        ),
        (
            "show-battery",
            "launcher-simple.setting.battery",
            "launcher-simple.setting.battery.help",
        ),
    ] {
        let mut spec = SpecDecl::new(name, Kind::Bool(true), GROUP, title);
        spec.help = help;
        d.spec(spec)?;
    }
    d.submit()
}

/// The screens: the home screen shows a status line of its own.
pub fn screens<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.screen(ScreenDecl::new("home", "launcher-simple.title", HOME).status_bar(StatusBar::Hide))?;
    d.screen(ScreenDecl::new(
        "panel",
        "launcher-simple.panel.title",
        PANEL,
    ))?;
    d.screen(ScreenDecl::new(
        "library",
        "launcher-simple.library.title",
        LIBRARY,
    ))?;
    d.screen(ScreenDecl::new(
        "customise",
        "launcher-simple.customise.title",
        CUSTOMISE,
    ))?;
    d.submit()
}

/// The actions, the slot offers, the three menus with their first entries and
/// the icons.
pub fn navigation<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.action(ActionDecl::new(
        "continue",
        "launcher-simple.continue",
        CONTINUE,
    ))?;
    d.action(ActionDecl::new(
        "customise",
        "launcher-simple.customise.title",
        OPEN_CUSTOMISE,
    ))?;
    d.slot_offer(SlotOfferDecl::new("core.home", "launcher-simple.home"))?;
    d.slot_offer(SlotOfferDecl::new("core.panel", "launcher-simple.panel"))?;
    d.slot_offer(SlotOfferDecl::new(
        "core.library",
        "launcher-simple.library",
    ))?;
    let entries: [(&str, &str, i16); 15] = [
        (MENU_CARDS, ACTION_CONTINUE, 10),
        (MENU_CARDS, "core.library", 20),
        (MENU_CARDS, "store.open", 30),
        (MENU_DOCK, "core.home", 10),
        (MENU_DOCK, "core.quick-panel", 20),
        (MENU_DOCK, "core.library", 30),
        (MENU_PANEL, "core.night-mode", 10),
        (MENU_PANEL, "wifi.open", 20),
        (MENU_PANEL, "bluetooth.open", 30),
        (MENU_PANEL, "lock.touch-lock", 40),
        (MENU_PANEL, "core.sleep", 50),
        (MENU_PANEL, "core.frontlight", 60),
        (MENU_PANEL, "core.font-size", 70),
        (MENU_PANEL, "core.settings", 80),
        ("core.settings-pages", "launcher-simple.customise", 215),
    ];
    for (menu, item, order) in entries {
        let mut entry = MenuEntryDecl::new(menu, item);
        entry.order = order;
        d.menu_entry(entry)?;
    }
    d.icon(GROUP, "layout-grid")?;
    d.icon("launcher-simple.continue", "book")?;
    d.icon("launcher-simple.customise", "layout-grid")?;
    d.submit()
}

/// The minute tick (the clock), a setting changed, what the registry lists
/// changed (the cards, the dock, the switches) and the library changed (the
/// book being read).
pub fn events<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.subscribe_event(EventKind::Tick, TICK)?;
    d.subscribe_event(EventKind::SettingChanged, SETTING_CHANGED)?;
    d.subscribe_event(EventKind::RegistryChanged, REGISTRY_CHANGED)?;
    d.subscribe_event(EventKind::LibraryChanged, LIBRARY_CHANGED)?;
    d.submit()
}
