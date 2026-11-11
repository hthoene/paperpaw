//! The plugin store's native declarations (slice 1): its two settings, the
//! library's address (for developers and libraries of one's own) and whether
//! it lists community plugins (guarded: no other plugin's defaults, presets or
//! commands switch it on), the action that opens the store with its menu
//! entries and icon, and the end of a request. The screens, the downloads, the
//! install and the first-run offer follow (`PORTING-7.md`).
use paperpaw_sdk::builtin::groups;
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EventKind, Kind, MenuEntryDecl, SpecDecl,
};

/// PaperPaw's own plugin library.
pub const OFFICIAL_LIST: &str = "https://paperpaw.org/api/v1/plugins.json";

/// Callback tokens: stable within this plugin.
pub const OPEN_TOKEN: u32 = 1;
/// The end of a request (`core.net`).
pub const HTTP_TOKEN: u32 = 2;

/// The library's address, and the switch for community plugins.
pub fn settings<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    let developer = groups::DEVELOPER;
    let system = groups::SYSTEM;
    let mut catalog = SpecDecl::new(
        "catalog",
        Kind::Text(OFFICIAL_LIST),
        developer.as_str(),
        "store.setting.catalog",
    );
    catalog.help = "store.setting.catalog.help";
    d.spec(catalog)?;
    let mut community = SpecDecl::new(
        "community",
        Kind::Bool(false),
        system.as_str(),
        "store.setting.community",
    );
    community.help = "store.setting.community.help";
    community.guarded = true;
    d.spec(community)?;
    d.submit()?;
    Ok(())
}

/// The action that opens the store, its place in the tools and in the plugin
/// manager's places to get more, and the icon. (The screens it opens are
/// declared with them, `PORTING-7.md` slice 9.)
pub fn navigation<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.action(ActionDecl::new("open", "store.title", OPEN_TOKEN))?;
    let mut tools = MenuEntryDecl::new("core.tools", "store.open");
    tools.order = 90;
    d.menu_entry(tools)?;
    let mut more = MenuEntryDecl::new("plugin-manager.more", "store.open");
    more.order = 10;
    d.menu_entry(more)?;
    d.icon("store.open", "cloud-download")?;
    d.submit()?;
    Ok(())
}

/// The events the plugin watches: the end of a request.
pub fn events<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.subscribe_event(EventKind::HttpDone, HTTP_TOKEN)?;
    d.submit()?;
    Ok(())
}
