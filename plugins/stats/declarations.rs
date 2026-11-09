//! Stats' declarations as borrowed builders, shared by the build script (which
//! writes them into the plugin image) and the tests.
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EditorDecl, EventKind, ExtraDecl, GroupDecl, Kind,
    MenuEntryDecl, ScreenDecl, SpecDecl, StatusItemDecl, SubscribeDecl, WidgetDecl,
};
use paperpaw_ui_kit::widget::options::{Opt, declare_all};

/// Callback tokens are stable within this plugin image.
pub const EVENTS_TOKEN: u32 = 1;

/// The screens' tokens.
pub const OVERVIEW_TOKEN: u32 = 20;
pub const BOOKS_TOKEN: u32 = 21;
pub const BOOK_TOKEN: u32 = 22;
pub const CALENDAR_TOKEN: u32 = 23;
pub const SYNC_TOKEN: u32 = 24;
/// The action that opens the overview.
pub const OPEN_TOKEN: u32 = 30;
/// The minute tick, for the widgets that must show a new day.
pub const TICK_TOKEN: u32 = 2;
/// The end of a request of the sync.
pub const HTTP_TOKEN: u32 = 3;
/// The widgets' tokens.
pub const TODAY_TOKEN: u32 = 40;
pub const WEEK_TOKEN: u32 = 41;
pub const RING_TOKEN: u32 = 42;
pub const STREAK_TOKEN: u32 = 43;
pub const CALENDAR_WIDGET_TOKEN: u32 = 44;
pub const YEAR_TOKEN: u32 = 45;
/// The stage of Reading goal the statistics answer, and its token.
pub const READ_TODAY: &str = "reading-goal.read-today";
pub const READ_TODAY_TOKEN: u32 = 5;
/// The status items "Time left in chapter" and "Time left in book".
pub const STATUS_TOKEN: u32 = 3;

pub const GROUP: &str = "stats.group";

/// The status items: their names (relative to the plugin) and titles.
pub const STATUS_ITEMS: [(&str, &str); 2] = [
    ("status.left-chapter", "stats.status.left-chapter"),
    ("status.left-book", "stats.status.left-book"),
];
/// How much they matter where a line has no room for all of its items (what
/// interface 6 gave an item by default).
pub const STATUS_PRIORITY: u8 = 128;

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
    // The WebDAV folder and its user, both set on the sync page (the password
    // is kept with the other passwords, not here).
    for (name, title, help, advanced) in [
        (
            "webdav-url",
            "stats.setting.sync",
            "stats.setting.sync.help",
            false,
        ),
        (
            "webdav-user",
            "stats.sync.user",
            "stats.sync.user.help",
            true,
        ),
    ] {
        let mut spec = SpecDecl::new(name, Kind::Text(""), GROUP, title);
        spec.help = help;
        spec.kept_on_reset = true;
        spec.advanced = advanced;
        declarations.spec(spec)?;
        let mut extra = ExtraDecl::new(name);
        extra.editor = Some(EditorDecl::Screen("sync"));
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
    declarations.screen(ScreenDecl::new("sync", "stats.sync.title", SYNC_TOKEN))?;
    declarations.action(ActionDecl::new("open", "stats.title", OPEN_TOKEN))?;
    for (menu, order) in [("core.tools", 35), ("core.reader-more", 30)] {
        let mut entry = MenuEntryDecl::new(menu, "stats.open");
        entry.order = order;
        declarations.menu_entry(entry)?;
    }
    declarations.icon("stats.open", "chart-bar")?;
    // The status items of the reader's lines: the time left, estimated from the
    // reading speed (names relative to the plugin: `stats.status.left-chapter`).
    for (name, title) in STATUS_ITEMS {
        declarations.status_item(
            StatusItemDecl::new(name, title, STATUS_TOKEN)
                .map_err(|_| DeclareError::Empty("status item"))?
                .priority(STATUS_PRIORITY),
        )?;
    }
    declarations.submit()
}

/// What makes the plugin record: the book's session and pages, the device's
/// sleep and start, the clock becoming known, a book moved; and the status
/// stage, which asks for the time left.
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
    declarations.subscribe_event(EventKind::Tick, TICK_TOKEN)?;
    declarations.subscribe_event(EventKind::HttpDone, HTTP_TOKEN)?;
    declarations.subscribe_event(EventKind::SettingChanged, EVENTS_TOKEN)?;
    // The reader asks for the values of the status items.
    declarations.subscribe(SubscribeDecl::new("core.status", STATUS_TOKEN))?;
    declarations.submit()
}

/// The stage `reading-goal.read-today` (schema 1, owned by Reading goal): the
/// statistics answer it. Asked with nothing, or with today's day (an `i32`,
/// local days since 1970), it answers the day and the seconds read on it as an
/// `i32` and a `u32`, little-endian; for another day it answers not found. It
/// answers nothing where the device does not know the date yet.
pub fn stages(sink: impl FnMut(&[u8]) -> Result<(), i32>) -> Result<(), DeclareError> {
    let mut declarations = Declarations::new(sink);
    declarations.subscribe(SubscribeDecl::new(READ_TODAY, READ_TODAY_TOKEN))?;
    declarations.submit()
}

/// The choices of the daily goal: minutes, or none.
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

/// Which days the week widget shows: the last 7, this week, the last 14 or 28.
pub const PERIODS: [&str; 4] = [
    "stats.period.7",
    "stats.period.week",
    "stats.period.14",
    "stats.period.28",
];
/// The bars' ends: round or square.
pub const SHAPES: [&str; 2] = ["stats.shape.round", "stats.shape.square"];

/// The ring's look: a whole ring, or three quarters of one open at the bottom.
pub const RING_STYLES: [&str; 2] = ["stats.ring.full", "stats.ring.gauge"];
/// How thick the ring is.
pub const RING_LINES: [&str; 3] = ["stats.line.thin", "stats.line.medium", "stats.line.thick"];

/// Which days count for the streak: any reading, or the daily goal reached.
pub const STREAK_COUNTS: [&str; 2] = ["stats.streak.any", "stats.streak.goal"];
/// How the marks show: dots, or dots with a check.
pub const STREAK_MARKS: [&str; 2] = ["stats.marks.dots", "stats.marks.checks"];
/// How many days the marks show.
pub const STREAK_DAYS: [&str; 2] = ["stats.marks.7", "stats.marks.14"];

/// This month as a calendar, or the last weeks as a heat map.
pub const CALENDAR_STYLES: [&str; 2] = ["stats.calendar.month", "stats.calendar.weeks"];
/// The weeks of the heat map (fewer where they do not fit).
pub const CALENDAR_WEEKS: [&str; 4] = [
    "stats.weeks.8",
    "stats.weeks.12",
    "stats.weeks.16",
    "stats.weeks.26",
];
/// The days' shape.
pub const CALENDAR_CELLS: [&str; 2] = ["stats.cells.square", "stats.cells.round"];

/// Books a year to read, or none.
pub const YEAR_GOALS: [&str; 6] = [
    "stats.books.goal.off",
    "stats.books.goal.6",
    "stats.books.goal.12",
    "stats.books.goal.24",
    "stats.books.goal.36",
    "stats.books.goal.52",
];
/// How the way to the yearly goal shows: a bar, a ring, a dot a book.
pub const YEAR_STYLES: [&str; 3] = ["stats.year.bar", "stats.year.ring", "stats.year.dots"];

/// A widget of the plugin.
#[derive(Debug, Clone, Copy)]
pub struct Def {
    /// Relative to the plugin: `widget.today` is `stats.widget.today`.
    pub name: &'static str,
    pub token: u32,
    /// The widget's own settings, in order: its look first, as interface 6
    /// listed them.
    pub options: &'static [Opt],
}

/// The look the widgets share, in the order interface 6 listed it, up to and
/// without the size and the alignment (which follow, as each widget has them).
const fn look() -> [Opt; 7] {
    [
        Opt::Frame("core.frame.none"),
        Opt::Padding,
        Opt::Paper,
        Opt::Ink,
        Opt::Corners,
        Opt::FrameLine,
        Opt::Shadow,
    ]
}

const TODAY_OWN: &[Opt] = &[
    Opt::Size("core.size.medium", true),
    Opt::Align("core.align.left"),
    Opt::Part("caption", "stats.part.caption", true),
    Opt::Part("pages", "stats.part.pages", true),
    Opt::Part("bar", "stats.part.goal-bar", true),
    Opt::Part("note", "stats.part.note", true),
];

const WEEK_OWN: &[Opt] = &[
    Opt::Align("core.align.left"),
    Opt::Choice(
        "period",
        "stats.setting.period",
        &PERIODS,
        "stats.period.7",
        "stats.setting.period.help",
    ),
    Opt::Choice(
        "shape",
        "stats.setting.bars",
        &SHAPES,
        "stats.shape.round",
        "stats.setting.bars.help",
    ),
    Opt::Part("caption", "stats.part.caption", true),
    Opt::Part("labels", "stats.part.weekdays", true),
    Opt::Part("goal", "stats.part.goal-line", true),
    Opt::Part("today", "stats.part.today", true),
];

const RING_OWN: &[Opt] = &[
    Opt::Size("core.size.fit", true),
    Opt::Align("core.align.center"),
    Opt::Choice(
        "style",
        "stats.setting.ring",
        &RING_STYLES,
        "stats.ring.full",
        "stats.setting.ring.help",
    ),
    Opt::Choice(
        "line",
        "stats.setting.line",
        &RING_LINES,
        "stats.line.medium",
        "stats.setting.line.help",
    ),
    Opt::Part("caption", "stats.part.caption", true),
    Opt::Part("unit", "stats.part.unit", true),
    Opt::PartHelp(
        "note",
        "stats.part.streak-left",
        true,
        "stats.part.streak-left.help",
    ),
];

const STREAK_OWN: &[Opt] = &[
    Opt::Size("core.size.medium", true),
    Opt::Align("core.align.center"),
    Opt::Choice(
        "counts",
        "stats.setting.counts",
        &STREAK_COUNTS,
        "stats.streak.any",
        "stats.setting.counts.help",
    ),
    Opt::Choice(
        "style",
        "stats.setting.marks",
        &STREAK_MARKS,
        "stats.marks.checks",
        "stats.setting.marks.help",
    ),
    Opt::Choice(
        "days",
        "stats.setting.days",
        &STREAK_DAYS,
        "stats.marks.7",
        "stats.setting.days.help",
    ),
    Opt::Part("caption", "stats.part.caption", true),
    Opt::Part("longest", "stats.part.longest", true),
    Opt::PartHelp("marks", "stats.part.marks", true, "stats.part.marks.help"),
    Opt::Part("weekdays", "stats.part.weekdays", true),
];

const CALENDAR_OWN: &[Opt] = &[
    Opt::Align("core.align.center"),
    Opt::Choice(
        "style",
        "stats.setting.calendar",
        &CALENDAR_STYLES,
        "stats.calendar.month",
        "stats.setting.calendar.help",
    ),
    Opt::Choice(
        "weeks",
        "stats.setting.weeks",
        &CALENDAR_WEEKS,
        "stats.weeks.12",
        "stats.setting.weeks.help",
    ),
    Opt::Choice(
        "cells",
        "stats.setting.cells",
        &CALENDAR_CELLS,
        "stats.cells.square",
        "stats.setting.cells.help",
    ),
    Opt::Part("caption", "stats.part.caption", true),
    Opt::Part("weekdays", "stats.part.weekdays", true),
    Opt::Part("numbers", "stats.part.numbers", true),
    Opt::Part("streak", "stats.part.streak", true),
    Opt::Part("total", "stats.part.total", true),
];

const YEAR_OWN: &[Opt] = &[
    Opt::Size("core.size.medium", true),
    Opt::Align("core.align.left"),
    Opt::Choice(
        "goal",
        "stats.setting.books-goal",
        &YEAR_GOALS,
        "stats.books.goal.12",
        "stats.setting.books-goal.help",
    ),
    Opt::Choice(
        "style",
        "stats.setting.year-style",
        &YEAR_STYLES,
        "stats.year.bar",
        "stats.setting.year-style.help",
    ),
    Opt::Part("caption", "stats.part.caption", true),
    Opt::PartHelp("plan", "stats.part.plan", true, "stats.part.plan.help"),
    Opt::Part("last", "stats.part.last-book", true),
];

/// The widgets, with their own settings (the shared look comes first).
pub const WIDGETS: [Def; 6] = [
    Def {
        name: "widget.today",
        token: TODAY_TOKEN,
        options: TODAY_OWN,
    },
    Def {
        name: "widget.week",
        token: WEEK_TOKEN,
        options: WEEK_OWN,
    },
    Def {
        name: "widget.goal",
        token: RING_TOKEN,
        options: RING_OWN,
    },
    Def {
        name: "widget.streak",
        token: STREAK_TOKEN,
        options: STREAK_OWN,
    },
    Def {
        name: "widget.calendar",
        token: CALENDAR_WIDGET_TOKEN,
        options: CALENDAR_OWN,
    },
    Def {
        name: "widget.year",
        token: YEAR_TOKEN,
        options: YEAR_OWN,
    },
];

impl Def {
    /// Every setting of the widget in order: the shared look, then its own.
    #[allow(
        dead_code,
        reason = "the tests read it; the build script reads the same file and does not"
    )]
    pub fn all(&self) -> impl Iterator<Item = Opt> + '_ {
        look().into_iter().chain(self.options.iter().copied())
    }
}

/// The daily goal, and the widgets with the group of settings each: *Read
/// today* and *Reading per day* (the others follow).
pub fn widgets(sink: impl FnMut(&[u8]) -> Result<(), i32>) -> Result<(), DeclareError> {
    let mut d = Declarations::new(sink);
    let mut goal = SpecDecl::new(
        "goal",
        Kind::Choice {
            default: GOALS[0],
            choices: &GOALS,
        },
        GROUP,
        "stats.setting.goal",
    );
    goal.help = "stats.setting.goal.help";
    d.spec(goal)?;
    d.submit()?;
    for widget in WIDGETS {
        let group = ["stats.", widget.name].concat();
        d.group(GroupDecl::new(widget.name, &group))?;
        let spec = WidgetDecl::new(widget.name, &group, "core.widgets.reading", widget.token)
            .map_err(|_| DeclareError::TooLong("widget"))?
            .live()
            .previews(&group)
            .map_err(|_| DeclareError::TooLong("widget preview"))?;
        d.widget(spec)?;
        declare_all(&mut d, widget.name, &group, widget.all())?;
        d.submit()?;
    }
    Ok(())
}
