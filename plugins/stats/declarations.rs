//! Stats' declarations as borrowed builders, shared by the build script (which
//! writes them into the plugin image) and the tests.
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EventKind, ExtraDecl, GroupDecl, Kind, MenuEntryDecl,
    ScreenDecl, SpecDecl, StageDecl,
};

/// Callback tokens are stable within this plugin image.
pub const EVENTS_TOKEN: u32 = 1;

/// The screens' tokens.
pub const OVERVIEW_TOKEN: u32 = 20;
pub const BOOKS_TOKEN: u32 = 21;
pub const BOOK_TOKEN: u32 = 22;
/// The action that opens the overview.
pub const OPEN_TOKEN: u32 = 30;

pub const GROUP: &str = "stats.group";

/// The group, and the three settings: recording on or off, and the shortest and
/// longest stay on a page that count (KOReader's rule).
pub fn settings(sink: impl FnMut(&[u8]) -> Result<(), i32>) -> Result<(), DeclareError> {
    let mut declarations = Declarations::new(sink);
    let mut group = GroupDecl::new("group", "stats.group");
    group.order = 25;
    declarations.group(group)?;
    let mut enabled = SpecDecl::new("enabled", Kind::Bool(true), GROUP, "stats.setting.enabled");
    enabled.help = "stats.setting.enabled.help";
    declarations.spec(enabled)?;
    for (name, default, min, max, step, title) in [
        ("min-seconds", 5, 1, 60, 1, "stats.setting.min"),
        ("max-seconds", 120, 30, 600, 10, "stats.setting.max"),
    ] {
        let mut spec = SpecDecl::new(name, Kind::Int { default, min, max }, GROUP, title);
        spec.help = if name == "min-seconds" {
            "stats.setting.min.help"
        } else {
            "stats.setting.max.help"
        };
        spec.advanced = true;
        declarations.spec(spec)?;
        let mut extra = ExtraDecl::new(name);
        extra.unit = "stats.unit.seconds";
        extra.step = step;
        declarations.extra(extra)?;
    }
    declarations.submit()
}

/// The chart icon of the settings group and the screens (the overview, the
/// calendar, the action that opens the overview and its menu entries follow).
pub fn ui(sink: impl FnMut(&[u8]) -> Result<(), i32>) -> Result<(), DeclareError> {
    let mut declarations = Declarations::new(sink);
    declarations.icon(GROUP, "chart-bar")?;
    declarations.screen(ScreenDecl::new("overview", "stats.title", OVERVIEW_TOKEN))?;
    declarations.screen(ScreenDecl::new("books", "stats.books", BOOKS_TOKEN))?;
    declarations.screen(ScreenDecl::new("book", "stats.title", BOOK_TOKEN))?;
    declarations.action(ActionDecl::new("open", "stats.title", OPEN_TOKEN))?;
    for (menu, order) in [("core.tools", 35), ("core.reader-more", 30)] {
        let mut entry = MenuEntryDecl::new(menu, "stats.open");
        entry.order = order;
        declarations.menu_entry(entry)?;
    }
    declarations.icon("stats.open", "chart-bar")?;
    declarations.submit()
}

/// What makes the plugin record: the book's session and pages, the device's
/// sleep and start, the clock becoming known, a book moved.
pub fn events(sink: impl FnMut(&[u8]) -> Result<(), i32>) -> Result<(), DeclareError> {
    let mut declarations = Declarations::new(sink);
    for kind in [
        EventKind::Started,
        EventKind::Resume,
        EventKind::Suspend,
        EventKind::Stopping,
        EventKind::PageTurned,
        EventKind::BookOpened,
        EventKind::BookClosed,
        EventKind::BookFinished,
        EventKind::BookMoved,
        EventKind::StatusChanged,
    ] {
        declarations.subscribe_event(kind, EVENTS_TOKEN)?;
    }
    declarations.submit()
}

/// The stage `stats.read-today` (schema 1): asked with nothing, it answers the
/// day (days since 1970, local) as an `i32` and the seconds read on it as a
/// `u32`, both little-endian, for those who show the reading goal. It answers
/// nothing where the device does not know the date yet.
pub fn stages(sink: impl FnMut(&[u8]) -> Result<(), i32>) -> Result<(), DeclareError> {
    let mut declarations = Declarations::new(sink);
    declarations.stage(StageDecl::new("read-today", 1, 1, 8, 1))?;
    declarations.submit()
}
