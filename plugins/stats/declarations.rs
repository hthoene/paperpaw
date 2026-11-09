//! Stats' declarations as borrowed builders, shared by the build script (which
//! writes them into the plugin image) and the tests.
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EventKind, ExtraDecl, GroupDecl, Kind, MenuEntryDecl,
    ScreenDecl, SpecDecl, StageDecl, WidgetDecl,
};
use paperpaw_ui_kit::widget::options::{Opt, declare_all};

/// Callback tokens are stable within this plugin image.
pub const EVENTS_TOKEN: u32 = 1;

/// The screens' tokens.
pub const OVERVIEW_TOKEN: u32 = 20;
pub const BOOKS_TOKEN: u32 = 21;
pub const BOOK_TOKEN: u32 = 22;
pub const CALENDAR_TOKEN: u32 = 23;
/// The action that opens the overview.
pub const OPEN_TOKEN: u32 = 30;
/// The minute tick, for the widgets that must show a new day.
pub const TICK_TOKEN: u32 = 2;
/// The widgets' tokens.
pub const TODAY_TOKEN: u32 = 40;
pub const WEEK_TOKEN: u32 = 41;
pub const RING_TOKEN: u32 = 42;

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
    declarations.subscribe_event(EventKind::Tick, TICK_TOKEN)?;
    declarations.subscribe_event(EventKind::SettingChanged, EVENTS_TOKEN)?;
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

/// The widgets, with their own settings (the shared look comes first).
pub const WIDGETS: [Def; 3] = [
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
