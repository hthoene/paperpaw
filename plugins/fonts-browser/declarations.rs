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
pub const INSTALLED: u32 = 4;
pub const SETTING: u32 = 5;
pub const PLUGINS: u32 = 6;

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
/// a restart is begun anew from the font's page. The pages show a font that
/// was chosen, installed or removed (the reading font and the status font are
/// settings of the host, the fonts plugins of the registry).
pub fn events<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.subscribe_event(EventKind::HttpDone, DOWNLOADED)?;
    d.subscribe_event(EventKind::SettingChanged, SETTING)?;
    d.subscribe_event(EventKind::PluginsChanged, PLUGINS)?;
    d.submit()?;
    Ok(())
}

/// The end of an install or a removal the host was asked for (the font
/// downloaded or removed). Not in interface 6, which had the kernel say it.
pub fn installs<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.subscribe_event(EventKind::InstallDone, INSTALLED)?;
    d.submit()?;
    Ok(())
}
