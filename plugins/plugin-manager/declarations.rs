//! The plugin manager's native declarations: its screens, actions, menu
//! entries and the two events it listens to.
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EventKind, Instances, MenuEntryDecl, ScreenDecl,
};

/// The callback token of the events (start, plugins changed, answers).
pub const EVENTS_TOKEN: u32 = 1;
/// The action that opens the list of plugins.
pub const OPEN_TOKEN: u32 = 2;
/// The action that opens *Screens*.
pub const SCREENS_OPEN_TOKEN: u32 = 3;
/// The list of plugins; one instance at a time.
pub const LIST_TOKEN: u32 = 10;
/// *Screens*: the places with several screens, or one place's candidates.
pub const SCREENS_TOKEN: u32 = 11;

/// The menu of places to get more plugins (a store): plugins add their
/// actions with an entry of their own, each a row above the plugins.
pub const MORE: &str = "plugin-manager.more";

pub fn screens<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.screen(
        ScreenDecl::new("list", "plugin-manager.title", LIST_TOKEN).instances(Instances::Single),
    )?;
    // One instance per place: the slots, and the candidates of each.
    d.screen(ScreenDecl::new(
        "screens",
        "plugin-manager.screens",
        SCREENS_TOKEN,
    ))?;
    d.submit()?;
    Ok(())
}

/// The actions, the manager in the settings (last but a few) and *Screens*
/// first among the places to get more, and their icons.
pub fn navigation<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.action(ActionDecl::new("open", "plugin-manager.open", OPEN_TOKEN))?;
    d.action(ActionDecl::new(
        "screens.open",
        "plugin-manager.screens",
        SCREENS_OPEN_TOKEN,
    ))?;
    let mut settings = MenuEntryDecl::new("core.settings-pages", "plugin-manager.open");
    settings.order = 220;
    d.menu_entry(settings)?;
    let mut screens = MenuEntryDecl::new(MORE, "plugin-manager.screens.open");
    screens.order = -10;
    d.menu_entry(screens)?;
    d.icon("plugin-manager.open", "puzzle")?;
    d.icon("plugin-manager.screens.open", "layout")?;
    d.submit()?;
    Ok(())
}

/// The start (what happened to the plugins with code), plugins coming and
/// going (a screen to offer for a place), and the answer to that offer.
pub fn events<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.subscribe_event(EventKind::Started, EVENTS_TOKEN)?;
    d.subscribe_event(EventKind::PluginsChanged, EVENTS_TOKEN)?;
    d.submit()?;
    Ok(())
}
