//! What the wallabag set-up declares, compared with the interface-6
//! registration by the declaration tests: the three settings edited on the
//! set-up screen, the screen, the action, the place in the tools and the
//! icons.
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EditorDecl, EventKind, ExtraDecl, Instances, Kind,
    MenuEntryDecl, ScreenDecl, SpecDecl,
};

/// Callback tokens: stable within this plugin.
pub const OPEN_TOKEN: u32 = 1;
pub const FETCH_TOKEN: u32 = 2;
/// The end of a request, and the reader's page turns.
pub const HTTP_TOKEN: u32 = 3;
pub const PAGE_TOKEN: u32 = 4;
/// The plugin stops: a fetch under way is cancelled and its files go.
pub const STOP_TOKEN: u32 = 5;
pub const SCREEN: u32 = 10;

/// The server and the account, set on the set-up screen: the address, the
/// user name and the API client's ID.
pub fn settings<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    for (name, title, help, advanced) in [
        ("server", "wallabag.server", "wallabag.server.help", false),
        ("user", "wallabag.user", "wallabag.user.help", true),
        (
            "client-id",
            "wallabag.client-id",
            "wallabag.client-id.help",
            true,
        ),
    ] {
        let mut spec = SpecDecl::new(name, Kind::Text(""), "core.group.library", title);
        spec.help = help;
        spec.kept_on_reset = true;
        spec.advanced = advanced;
        d.spec(spec)?;
        let mut extra = ExtraDecl::new(name);
        extra.editor = Some(EditorDecl::Screen("setup"));
        d.extra(extra)?;
    }
    d.submit()?;
    Ok(())
}

/// The set-up screen, one at a time.
pub fn screens<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.screen(ScreenDecl::new("setup", "wallabag.title", SCREEN).instances(Instances::Single))?;
    d.submit()?;
    Ok(())
}

/// The actions, the place in the tools and the icons.
pub fn navigation<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.action(ActionDecl::new("open", "wallabag.title", OPEN_TOKEN))?;
    d.action(ActionDecl::new("fetch", "wallabag.fetch", FETCH_TOKEN))?;
    let mut entry = MenuEntryDecl::new("core.tools", "wallabag.open");
    entry.order = 48;
    d.menu_entry(entry)?;
    d.icon("wallabag.open", "article")?;
    d.icon("wallabag.fetch", "refresh")?;
    d.submit()?;
    Ok(())
}

/// The events the fetch needs: the end of a request (a request is refused
/// without the subscription), the reader's page turns (an article read to the
/// end is noted to be archived) and the plugin's stopping.
pub fn events<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.subscribe_event(EventKind::HttpDone, HTTP_TOKEN)?;
    d.submit()?;
    d.subscribe_event(EventKind::PageTurned, PAGE_TOKEN)?;
    d.submit()?;
    d.subscribe_event(EventKind::Stopping, STOP_TOKEN)?;
    d.submit()?;
    Ok(())
}
