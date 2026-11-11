//! The plugin store's native declarations (slice 1): its two settings, the
//! library's address (for developers and libraries of one's own) and whether
//! it lists community plugins (guarded: no other plugin's defaults, presets or
//! commands switch it on), the action that opens the store with its menu
//! entries and icon, and the end of a request. The screens, the downloads, the
//! install and the first-run offer follow (`PORTING-7.md`).
use paperpaw_sdk::builtin::groups;
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EventKind, Instances, Kind, MenuEntryDecl, ScreenDecl,
    SetupPage, SetupStepDecl, SpecDecl,
};

/// PaperPaw's own plugin library.
pub const OFFICIAL_LIST: &str = "https://paperpaw.org/api/v1/plugins.json";

/// Callback tokens: stable within this plugin.
pub const OPEN_TOKEN: u32 = 1;
/// The end of a request (`core.net`).
pub const HTTP_TOKEN: u32 = 2;
/// The end of an install or a removal (`event.install_done`).
pub const INSTALL_TOKEN: u32 = 3;
/// The plugins of the device changed, a download made progress, a picture is
/// ready: the screen on top reads what it shows anew.
pub const CHANGED_TOKEN: u32 = 4;
pub const PROGRESS_TOKEN: u32 = 5;
pub const IMAGE_TOKEN: u32 = 6;
/// PaperPaw started: what a restart cut off goes on.
pub const STARTED_TOKEN: u32 = 7;
/// Other plugins asking for plugins (event 77).
pub const PLUGINS_TOKEN: u32 = 8;
/// The screens' tokens: the start, the page of a plugin, a view, the letters,
/// the source, the pictures and the progress of a fetch.
pub const LIST_SCREEN: u32 = 20;
pub const PLUGIN_SCREEN: u32 = 21;
pub const VIEW_SCREEN: u32 = 22;
pub const LETTERS_SCREEN: u32 = 23;
pub const SOURCE_SCREEN: u32 = 24;
pub const SHOTS_SCREEN: u32 = 25;
pub const FETCH_SCREEN: u32 = 26;
/// The first-run offer's page of the setup.
pub const OFFER_SCREEN: u32 = 27;

/// The eight screens of the store: the start and the progress of a fetch are
/// single, as in Beta 3; the others come once for each set of parameters.
pub fn screens<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.screen(ScreenDecl::new("list", "store.title", LIST_SCREEN).instances(Instances::Single))?;
    d.screen(ScreenDecl::new("plugin", "store.title", PLUGIN_SCREEN))?;
    d.screen(ScreenDecl::new("view", "store.title", VIEW_SCREEN))?;
    d.screen(ScreenDecl::new("letters", "store.letter", LETTERS_SCREEN))?;
    d.screen(ScreenDecl::new("source", "store.source", SOURCE_SCREEN))?;
    d.screen(ScreenDecl::new("shots", "store.shots", SHOTS_SCREEN))?;
    d.screen(
        ScreenDecl::new("fetch", "store.fetch.title", FETCH_SCREEN).instances(Instances::Single),
    )?;
    d.screen(ScreenDecl::new("offer", "store.offer.title", OFFER_SCREEN))?;
    d.submit()?;
    Ok(())
}

/// The setup's step that offers a few plugins (`store.offer`), after the
/// keyboard's and before Wi-Fi's. It applies until the plugin says it does not
/// (`command.set_item_state`, where no plugin of the offer can be added here;
/// `native_screen_offer::applies`), and the item state carries the badge of
/// what became of those chosen. Opened all the same with nothing to offer,
/// the page shows only *Done*, as in Beta 3.
pub fn setup<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    let mut step = SetupStepDecl::new(
        "setup",
        "store.offer.title",
        SetupPage::Screen("store.offer"),
    );
    step.help = "store.offer.help";
    step.order = 55;
    d.setup_step(step)?;
    d.submit()?;
    Ok(())
}

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

/// The events the plugin watches: the end of a request and its progress, the end
/// of an install, a change of the plugins, a picture ready.
pub fn events<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.subscribe_event(EventKind::HttpDone, HTTP_TOKEN)?;
    d.subscribe_event(EventKind::InstallDone, INSTALL_TOKEN)?;
    d.subscribe_event(EventKind::PluginsChanged, CHANGED_TOKEN)?;
    d.subscribe_event(EventKind::HttpProgress, PROGRESS_TOKEN)?;
    d.subscribe_event(EventKind::ImageReady, IMAGE_TOKEN)?;
    d.subscribe_event(EventKind::Started, STARTED_TOKEN)?;
    d.subscribe_event(EventKind::PluginsRequested, PLUGINS_TOKEN)?;
    d.submit()?;
    Ok(())
}
