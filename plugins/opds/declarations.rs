//! What the catalogs declare: the two settings in the library's part named
//! after the catalogs (covers in lists, how many books a sync fetches at
//! most), the three screens (the list of catalogs, browsing one, editing one),
//! the actions (open the list, sync the catalogs marked for it), the place in
//! the tools and the icons, and the events the plugin runs on (the end of a
//! request and its progress, work coming due, a picture ready, the plugin
//! stopping).
use paperpaw_sdk::builtin::groups;
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EventKind, ExtraDecl, Instances, Kind, MenuEntryDecl,
    ScreenDecl, SpecDecl,
};

/// Callback tokens: stable within this plugin.
pub const OPEN_TOKEN: u32 = 1;
pub const SYNC_TOKEN: u32 = 2;
/// The end of a request, and its progress.
pub const HTTP_TOKEN: u32 = 3;
pub const PROGRESS_TOKEN: u32 = 4;
/// Work (a sync a rule asked for) comes due.
pub const WORK_TOKEN: u32 = 5;
/// The plugin stops: what is under way is cancelled and its files go.
pub const STOP_TOKEN: u32 = 6;
/// A cover is decoded.
pub const IMAGE_TOKEN: u32 = 7;
/// The screens: the list of catalogs, browsing one, editing one.
pub const CATALOGS_SCREEN: u32 = 10;
pub const BROWSE_SCREEN: u32 = 11;
pub const EDIT_SCREEN: u32 = 12;

/// The part of the library settings the catalogs' settings are in.
const SECTION: &str = "opds.section";

/// Covers in lists, and how many books a sync fetches at most.
pub fn settings<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    let group = groups::LIBRARY;
    let mut covers = SpecDecl::new(
        "covers",
        Kind::Bool(true),
        group.as_str(),
        "opds.setting.covers",
    );
    covers.help = "opds.setting.covers.help";
    d.spec(covers)?;
    let mut in_catalogs = ExtraDecl::new("covers");
    in_catalogs.section = SECTION;
    d.extra(in_catalogs)?;
    let mut limit = SpecDecl::new(
        "sync-limit",
        Kind::Int {
            default: 50,
            min: 10,
            max: 1000,
        },
        group.as_str(),
        "opds.setting.limit",
    );
    limit.help = "opds.setting.limit.help";
    d.spec(limit)?;
    let mut in_catalogs = ExtraDecl::new("sync-limit");
    in_catalogs.section = SECTION;
    in_catalogs.step = 10;
    d.extra(in_catalogs)?;
    d.submit()?;
    Ok(())
}

/// The three screens: the list of catalogs one at a time; browsing and
/// editing as many as are opened (each is a catalog's).
pub fn screens<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.screen(
        ScreenDecl::new("catalogs", "opds.title", CATALOGS_SCREEN).instances(Instances::Single),
    )?;
    d.screen(ScreenDecl::new("browse", "opds.title", BROWSE_SCREEN).instances(Instances::Many))?;
    d.screen(ScreenDecl::new("edit", "opds.title", EDIT_SCREEN).instances(Instances::Many))?;
    d.submit()?;
    Ok(())
}

/// The actions, the place in the tools and the icons.
pub fn navigation<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.action(ActionDecl::new("open", "opds.title", OPEN_TOKEN))?;
    d.action(ActionDecl::new("sync", "opds.sync-all", SYNC_TOKEN))?;
    let mut entry = MenuEntryDecl::new("core.tools", "opds.open");
    entry.order = 45;
    d.menu_entry(entry)?;
    d.icon("opds.open", "book-download")?;
    d.icon("opds.sync", "refresh")?;
    d.submit()?;
    Ok(())
}

/// The events the plugin runs on: the end of a request (a request is refused
/// without the subscription) and its progress, work coming due (declaring
/// work is refused without it), a picture ready and the plugin stopping.
pub fn events<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    for (kind, token) in [
        (EventKind::HttpDone, HTTP_TOKEN),
        (EventKind::HttpProgress, PROGRESS_TOKEN),
        (EventKind::WorkDue, WORK_TOKEN),
        (EventKind::ImageReady, IMAGE_TOKEN),
        (EventKind::Stopping, STOP_TOKEN),
    ] {
        d.subscribe_event(kind, token)?;
        d.submit()?;
    }
    Ok(())
}
