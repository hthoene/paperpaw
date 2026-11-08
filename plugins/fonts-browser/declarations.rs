//! What the fonts browser declares, built into its image by `build.rs` and
//! compared with the interface-6 registration by the declaration tests.
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EventKind, Instances, Kind, MenuEntryDecl, ScreenDecl,
    SpecDecl,
};

/// Callback tokens: stable within this plugin.
pub const FONT: u32 = 10;
pub const LIST: u32 = 11;
pub const OPEN: u32 = 1;
pub const OWN: u32 = 2;
pub const DOWNLOADED: u32 = 3;
pub const STARTED: u32 = 4;

pub fn screens<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.screen(ScreenDecl::new("font", "fonts-browser.title", FONT))?;
    d.screen(ScreenDecl::new("list", "fonts-browser.title", LIST).instances(Instances::Single))?;
    d.submit()?;
    Ok(())
}

/// The font library's address, for developers and libraries of one's own.
pub fn settings<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    let mut catalog = SpecDecl::new(
        "catalog",
        Kind::Text("https://paperpaw.org/api/v1/fonts.json"),
        "core.group.developer",
        "fonts-browser.setting.catalog",
    );
    catalog.help = "fonts-browser.setting.catalog.help";
    d.spec(catalog)?;
    d.submit()?;
    Ok(())
}

/// Actions, where they are offered (after the fonts to choose from) and
/// their icons.
pub fn navigation<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.action(ActionDecl::new("open", "fonts-browser.more", OPEN))?;
    d.action(ActionDecl::new("own", "fonts-browser.own", OWN))?;
    for menu in ["core.reading-font", "core.status-font"] {
        d.menu_entry(MenuEntryDecl::new(menu, "fonts-browser.open"))?;
        d.menu_entry(MenuEntryDecl::new(menu, "fonts-browser.own"))?;
    }
    d.icon("fonts-browser.open", "typography")?;
    d.icon("fonts-browser.own", "folder-open")?;
    d.submit()?;
    Ok(())
}

/// A download goes on while no screen of the plugin is open; one cut off by
/// a restart is begun anew from the font's page.
pub fn events<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.subscribe_event(EventKind::HttpDone, DOWNLOADED)?;
    d.subscribe_event(EventKind::Started, STARTED)?;
    d.submit()?;
    Ok(())
}
