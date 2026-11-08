//! Stats' declarations as borrowed builders, shared by the build script (which
//! writes them into the plugin image) and the tests.
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EventKind, ExtraDecl, GroupDecl, Kind, MenuEntryDecl,
    ScreenDecl, SpecDecl, SubscribeDecl,
};

/// Callback tokens are stable within this plugin image.
pub const EVENTS_TOKEN: u32 = 1;

/// The screens' tokens.
pub const OVERVIEW_TOKEN: u32 = 20;
pub const BOOKS_TOKEN: u32 = 21;
pub const BOOK_TOKEN: u32 = 22;
pub const CALENDAR_TOKEN: u32 = 23;
/// The action that opens the overview.
pub const OPEN_TOKEN: u32 = 30;
/// The question Reading goal asks (the stage `reading-goal.read-today`).
pub const ASKED_TOKEN: u32 = 40;

pub const GROUP: &str = "stats.group";

/// The choices of the daily goal: none, then minutes.
pub const GOALS: [&str; 12] = [
    "stats.goal.off",
    "stats.goal.10",
    "stats.goal.15",
    "stats.goal.20",
    "stats.goal.30",
    "stats.goal.45",
    "stats.goal.60",
    "stats.goal.90",
    "stats.goal.120",
    "stats.goal.180",
    "stats.goal.240",
    "stats.goal.300",
];

/// The group, and the four settings: the daily goal (first, as the widgets
/// registered it), recording on or off, and the
/// shortest and longest stay on a page that count (KOReader's rule).
pub fn settings(sink: impl FnMut(&[u8]) -> Result<(), i32>) -> Result<(), DeclareError> {
    let mut declarations = Declarations::new(sink);
    let mut group = GroupDecl::new("group", "stats.group");
    group.order = 25;
    declarations.group(group)?;
    // The daily goal the widgets, the overview and Reading goal go by: minutes,
    // or none. 20 minutes at first (interface 6: Reading goal set that default
    // where it is installed).
    let mut goal = SpecDecl::new(
        "goal",
        Kind::Choice {
            default: "stats.goal.20",
            choices: &GOALS,
        },
        GROUP,
        "stats.setting.goal",
    );
    goal.help = "stats.setting.goal.help";
    declarations.spec(goal)?;
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
/// books, a book, the calendar, the action that opens the overview and its menu entries follow).
pub fn ui(sink: impl FnMut(&[u8]) -> Result<(), i32>) -> Result<(), DeclareError> {
    let mut declarations = Declarations::new(sink);
    declarations.icon(GROUP, "chart-bar")?;
    declarations.screen(ScreenDecl::new("overview", "stats.title", OVERVIEW_TOKEN))?;
    declarations.screen(ScreenDecl::new("books", "stats.books", BOOKS_TOKEN))?;
    declarations.screen(ScreenDecl::new("book", "stats.title", BOOK_TOKEN))?;
    declarations.screen(ScreenDecl::new(
        "calendar",
        "stats.calendar",
        CALENDAR_TOKEN,
    ))?;
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

/// The subscription to the stage `reading-goal.read-today` (schema 1), which
/// Reading goal owns and runs when it needs today's reading: asked with the day
/// as an `i32` (empty: today), it answers the day and the seconds read on it as
/// a `u32`, little-endian, from memory. A day it does not know is `ERR_NOT_FOUND`.
/// Without Reading goal installed the subscription stays inert.
pub fn stages(sink: impl FnMut(&[u8]) -> Result<(), i32>) -> Result<(), DeclareError> {
    let mut declarations = Declarations::new(sink);
    declarations.subscribe(SubscribeDecl::new("reading-goal.read-today", ASKED_TOKEN))?;
    declarations.submit()
}
