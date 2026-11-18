//! What the wireless library declares: Calibre's address (a setting that says
//! where Calibre is when it is not found by itself), the connection screen,
//! the action, the place in the tools and the icon, and the socket and
//! broadcast events the connection runs on.
use paperpaw_sdk::builtin::groups;
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EventKind, ExtraDecl, Instances, Kind, MenuEntryDecl,
    ScreenDecl, SpecDecl,
};

/// Callback tokens: stable within this plugin.
pub const OPEN_TOKEN: u32 = 1;
/// The events of a socket and of a broadcast.
pub const SOCKET_TOKEN: u32 = 2;
pub const BROADCAST_TOKEN: u32 = 3;
/// The connection screen.
pub const SCREEN: u32 = 10;

/// Calibre's address, where it is not found by itself.
pub fn settings<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    let group = groups::LIBRARY;
    let mut address = SpecDecl::new(
        "address",
        Kind::Text(""),
        group.as_str(),
        "calibre.setting.address",
    );
    address.help = "calibre.setting.address.help";
    address.advanced = true;
    d.spec(address)?;
    // In a part named after the plugin, as interface 6 showed it.
    let mut extra = ExtraDecl::new("address");
    extra.section = "calibre.plugin.name";
    d.extra(extra)?;
    d.submit()?;
    Ok(())
}

/// The connection screen, one at a time.
pub fn screens<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.screen(ScreenDecl::new("screen", "calibre.title", SCREEN).instances(Instances::Single))?;
    d.submit()?;
    Ok(())
}

/// The action that opens the screen, its place in the tools and its icon.
pub fn navigation<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.action(ActionDecl::new("open", "calibre.title", OPEN_TOKEN))?;
    let mut entry = MenuEntryDecl::new("core.tools", "calibre.open");
    entry.order = 47;
    d.menu_entry(entry)?;
    d.icon("calibre.open", "device-desktop")?;
    d.submit()?;
    Ok(())
}

/// The events of the connection: a socket (a connect is refused without the
/// subscription) and the answers to a broadcast.
pub fn events<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.subscribe_event(EventKind::Socket, SOCKET_TOKEN)?;
    d.submit()?;
    d.subscribe_event(EventKind::Broadcast, BROADCAST_TOKEN)?;
    d.submit()?;
    Ok(())
}
