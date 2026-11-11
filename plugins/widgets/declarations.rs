//! Native identity of the widget pack: its widgets, the settings group of
//! each, and the tick that keeps clocks right. Callback tokens are stable
//! within this plugin; every widget's settings are declared here, none at
//! run time.
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EventKind, GroupDecl, Instances, ScreenDecl, WidgetDecl,
};
pub use paperpaw_ui_kit::widget::options::Opt;
use paperpaw_ui_kit::widget::options::{FACES, declare_all, shared_look};

/// The callback token of the minute tick.
pub const TICK: u32 = 1;
/// The callback token of the reading events, which the reading widget records.
pub const READING: u32 = 2;
/// The callback token of the to-do list's screen.
pub const TODO_SCREEN: u32 = 10;
/// The callback token of the action that opens it.
pub const TODO_OPEN: u32 = 11;

/// The widget categories of the host's catalogue.
pub mod category {
    pub const TIME: &str = "core.widgets.time";
    pub const PERSONAL: &str = "core.widgets.personal";
    pub const WORDS: &str = "core.widgets.words";
    pub const DEVICE: &str = "core.widgets.device";
    pub const BOOKS: &str = "core.widgets.books";
    pub const OTHER: &str = "core.widgets.other";
}

/// A widget of the pack.
#[derive(Debug, Clone, Copy)]
pub struct Def {
    /// Relative to the plugin: `day` is `widgets.day`.
    pub name: &'static str,
    pub token: u32,
    pub category: &'static str,
    /// Read each minute.
    pub live: bool,
    /// Where the text sits by default: the widget's look settings (frame,
    /// padding, paper, ink, corners, line, shadow) are the shared ones, and
    /// come first.
    pub align: &'static str,
    /// Whether the shared look comes after the widget's own settings.
    pub look_last: bool,
    /// The widget's own settings.
    pub options: &'static [Opt],
}

impl Def {
    /// Every setting of the widget in order: the shared look, then its own.
    #[allow(
        dead_code,
        reason = "the tests read it; the build script reads the same file and does not"
    )]
    pub fn all(&self) -> impl Iterator<Item = Opt> + '_ {
        let look = shared_look(self.align);
        let (own, last) = (self.options, self.look_last);
        (0..look.len() + own.len()).filter_map(move |n| {
            if last {
                own.get(n).or_else(|| look.get(n - own.len())).copied()
            } else {
                look.get(n).or_else(|| own.get(n - look.len())).copied()
            }
        })
    }
}

const DAY_OPTIONS: &[Opt] = &[
    Opt::Choice(
        "style",
        "widgets.setting.style",
        &[
            "widgets.day.style.weekday",
            "widgets.day.style.number",
            "widgets.day.style.sheet",
        ],
        "widgets.day.style.weekday",
        "widgets.day.style.help",
    ),
    Opt::Size("core.size.fit", true),
    Opt::DateFormat("widgets.date-format.long"),
    Opt::Part("year", "widgets.setting.year", true),
    Opt::Part("week", "widgets.setting.week-number", false),
];

const TILES_OPTIONS: &[Opt] = &[
    Opt::Choice(
        "tiles",
        "widgets.setting.tiles",
        &[
            "widgets.tiles.dark",
            "widgets.tiles.light",
            "widgets.tiles.outline",
        ],
        "widgets.tiles.dark",
        "widgets.tile-clock.tiles.help",
    ),
    Opt::Choice(
        "group",
        "widgets.setting.tile-group",
        &["widgets.tiles.digits", "widgets.tiles.pairs"],
        "widgets.tiles.digits",
        "widgets.tile-clock.group.help",
    ),
    Opt::Part("hinge", "widgets.setting.hinge", true),
    Opt::Part("colon", "widgets.setting.colon", false),
    Opt::Part("date", "widgets.setting.show-date", false),
    Opt::DateFormat("widgets.date-format.weekday"),
];

/// The values of the week start setting.
pub const WEEK_STARTS: &[&str] = &[
    "widgets.week-start.auto",
    "widgets.week-start.monday",
    "widgets.week-start.sunday",
];

const CALENDAR_OPTIONS: &[Opt] = &[
    Opt::Choice(
        "week-start",
        "widgets.setting.week-start",
        WEEK_STARTS,
        "widgets.week-start.auto",
        "widgets.calendar.week-start.help",
    ),
    Opt::Choice(
        "mark",
        "widgets.setting.today-mark",
        &[
            "widgets.mark.circle",
            "widgets.mark.square",
            "widgets.mark.ring",
            "widgets.mark.underline",
        ],
        "widgets.mark.circle",
        "widgets.calendar.mark.help",
    ),
    Opt::Choice(
        "initials",
        "widgets.setting.initials",
        &["widgets.initials.one", "widgets.initials.two"],
        "widgets.initials.one",
        "widgets.calendar.initials.help",
    ),
    Opt::Part("heading", "widgets.setting.month-name", true),
    Opt::Part("year", "widgets.setting.year", false),
    Opt::Part("weekdays", "widgets.setting.weekdays", true),
    Opt::Part("weeks", "widgets.setting.week-numbers", false),
    Opt::Part("others", "widgets.setting.other-days", false),
    Opt::Part("weekends", "widgets.setting.weekends-faint", false),
    Opt::Part("next", "widgets.setting.next-month", false),
];

const COUNTDOWN_OPTIONS: &[Opt] = &[
    Opt::Text(
        "title",
        "widgets.setting.countdown-title",
        "",
        "widgets.setting.countdown-title.help",
    ),
    Opt::Text(
        "date",
        "widgets.setting.countdown-date",
        "12-24",
        "widgets.setting.countdown-date.help",
    ),
    Opt::Choice(
        "style",
        "widgets.setting.style",
        &[
            "widgets.countdown.style.number",
            "widgets.countdown.style.sentence",
        ],
        "widgets.countdown.style.number",
        "widgets.countdown.style.help",
    ),
    Opt::Size("core.size.fit", true),
    Opt::Part("show-date", "widgets.setting.show-date", true),
    Opt::Part("weeks", "widgets.setting.weeks", false),
    Opt::DateFormat("widgets.date-format.weekday"),
];

const YEAR_OPTIONS: &[Opt] = &[
    Opt::Choice(
        "style",
        "widgets.setting.style",
        &[
            "widgets.year.bar",
            "widgets.year.ring",
            "widgets.year.months",
        ],
        "widgets.year.bar",
        "widgets.year-progress.style.help",
    ),
    Opt::Part("year", "widgets.setting.year", true),
    Opt::Part("percent", "widgets.setting.percent", true),
    Opt::Part("day", "widgets.setting.day-of-year", true),
    Opt::Part("week", "widgets.setting.week-number", false),
    Opt::Part("left", "widgets.setting.days-left", false),
];

const ANALOGUE_OPTIONS: &[Opt] = &[
    Opt::Choice(
        "face",
        "widgets.setting.face",
        &[
            "widgets.face.minimal",
            "widgets.face.hours",
            "widgets.face.minutes",
            "widgets.face.quarters",
            "widgets.face.numerals",
        ],
        "widgets.face.hours",
        "widgets.analogue-clock.face.help",
    ),
    Opt::Choice(
        "ring",
        "widgets.setting.ring",
        &["widgets.ring.none", "widgets.ring.one", "widgets.ring.two"],
        "widgets.ring.one",
        "widgets.analogue-clock.ring.help",
    ),
    Opt::Choice(
        "hands",
        "widgets.setting.hands",
        &[
            "widgets.hands.thin",
            "widgets.hands.regular",
            "widgets.hands.bold",
        ],
        "widgets.hands.regular",
        "widgets.analogue-clock.hands.help",
    ),
    Opt::Choice(
        "date",
        "widgets.setting.clock-date",
        &[
            "widgets.clock-date.none",
            "widgets.clock-date.below",
            "widgets.clock-date.inside",
        ],
        "widgets.clock-date.none",
        "widgets.analogue-clock.date.help",
    ),
    Opt::DateFormat("widgets.date-format.weekday"),
    Opt::Part("centre", "widgets.setting.centre-dot", true),
];

const BATTERY_OPTIONS: &[Opt] = &[
    Opt::Size("core.size.fit", true),
    Opt::Choice(
        "style",
        "widgets.books.setting.style",
        &[
            "widgets.battery.lying",
            "widgets.battery.standing",
            "widgets.battery.ring",
        ],
        "widgets.battery.lying",
        "widgets.battery.style.help",
    ),
    Opt::Part("percent", "widgets.books.part.percent", true),
    Opt::Part("charging", "widgets.books.part.charging", true),
    Opt::Part("caption", "widgets.books.part.caption", false),
];

const LIBRARY_OPTIONS: &[Opt] = &[
    Opt::Size("core.size.fit", true),
    Opt::Part("books", "widgets.books.part.books", true),
    Opt::Part("finished", "widgets.books.part.finished", true),
    Opt::Part("reading", "widgets.books.part.reading", true),
    Opt::Part("unread", "widgets.books.part.unread", true),
    Opt::Part("bar", "widgets.books.part.bar", true),
    Opt::Part("caption", "widgets.books.part.caption", false),
];

const HIGHLIGHT_OPTIONS: &[Opt] = &[
    Opt::Size("core.size.fit", true),
    Opt::Choice(
        "source",
        "widgets.books.setting.highlight-source",
        &["widgets.highlight.recent", "widgets.highlight.all"],
        "widgets.highlight.recent",
        "widgets.highlight.source.help",
    ),
    Opt::Choice(
        "change",
        "widgets.books.setting.highlight-change",
        &["widgets.highlight.daily", "widgets.highlight.shown"],
        "widgets.highlight.daily",
        "widgets.books.setting.highlight-change.help",
    ),
    Opt::Part("caption", "widgets.books.part.caption", false),
    Opt::Part("marks", "widgets.books.part.marks", true),
    Opt::Part("title", "widgets.books.part.title", true),
    Opt::Part("author", "widgets.books.part.author", false),
    Opt::Part("note", "widgets.books.part.note", false),
];

const FAVOURITES_OPTIONS: &[Opt] = &[
    Opt::Size("core.size.medium", false),
    Opt::Choice(
        "layout",
        "widgets.books.setting.layout",
        &["widgets.books.layout.row", "widgets.books.layout.grid"],
        "widgets.books.layout.row",
        "widgets.favourites.layout.help",
    ),
    Opt::Number(
        "count",
        "widgets.books.setting.count",
        1,
        24,
        8,
        "widgets.setting.count.help",
    ),
    Opt::Part("caption", "widgets.books.part.caption", true),
    Opt::Part("titles", "widgets.books.part.titles", false),
    Opt::Part("read", "widgets.books.part.read", false),
    Opt::Part("current", "widgets.books.part.current", false),
];

const SERIES_OPTIONS: &[Opt] = &[
    Opt::Size("core.size.medium", false),
    Opt::Text(
        "name",
        "widgets.books.setting.series-name",
        "",
        "widgets.books.setting.series-name.help",
    ),
    Opt::Number(
        "count",
        "widgets.books.setting.count",
        2,
        12,
        6,
        "widgets.setting.count.help",
    ),
    Opt::Choice(
        "start",
        "widgets.books.setting.series-start",
        &["widgets.series.around", "widgets.series.first"],
        "widgets.series.around",
        "widgets.books.setting.series-start.help",
    ),
    Opt::Part("caption", "widgets.books.part.caption", true),
    Opt::Part("numbers", "widgets.books.part.numbers", true),
    Opt::Part("titles", "widgets.books.part.titles", false),
    Opt::Part("read", "widgets.books.part.read", true),
    Opt::Part("current", "widgets.books.part.current", true),
];

const NEXT_OPTIONS: &[Opt] = &[
    Opt::Size("core.size.medium", false),
    Opt::Choice(
        "pick",
        "widgets.books.setting.pick",
        &[
            "widgets.next.series",
            "widgets.next.newest",
            "widgets.next.random",
        ],
        "widgets.next.series",
        "widgets.books.setting.pick.help",
    ),
    Opt::Part("caption", "widgets.books.part.caption", true),
    Opt::Part("cover", "widgets.books.part.cover", true),
    Opt::Part("author", "widgets.books.part.author", true),
    Opt::Part("series", "widgets.books.part.series", true),
];

const PROGRESS_OPTIONS: &[Opt] = &[
    Opt::Size("core.size.medium", false),
    Opt::Choice(
        "style",
        "widgets.books.setting.style",
        &[
            "widgets.progress.beside",
            "widgets.progress.round",
            "widgets.progress.ring",
            "widgets.progress.bar",
        ],
        "widgets.progress.beside",
        "widgets.progress.style.help",
    ),
    Opt::Choice(
        "thickness",
        "widgets.books.setting.thickness",
        &[
            "widgets.books.thin",
            "widgets.books.medium",
            "widgets.books.thick",
        ],
        "widgets.books.medium",
        "widgets.progress.thickness.help",
    ),
    Opt::Part("cover", "widgets.books.part.cover", true),
    Opt::Part("title", "widgets.books.part.title", true),
    Opt::Part("author", "widgets.books.part.author", true),
    Opt::Part("caption", "widgets.books.part.caption", false),
];

const READING_OPTIONS: &[Opt] = &[
    Opt::Size("core.size.medium", false),
    Opt::Part("caption", "widgets.books.part.caption", true),
    Opt::Part("cover", "widgets.books.part.cover", true),
    Opt::Part("author", "widgets.books.part.author", true),
    Opt::Part("pages", "widgets.books.part.pages", true),
    Opt::Part("bar", "widgets.books.part.bar", true),
    Opt::Part("left", "widgets.books.part.left", true),
    Opt::Part("chapter", "widgets.books.part.chapter", true),
    // Shown again when the reading position carries the chapter's end (Kernel).
    Opt::Part("chapter-left", "widgets.books.part.chapter-left", true),
];

const MOON_OPTIONS: &[Opt] = &[
    Opt::Part("name", "widgets.setting.phase-name", true),
    Opt::Part("full", "widgets.setting.next-full-moon", true),
    Opt::Part("lit", "widgets.setting.lit-percent", false),
    Opt::Choice(
        "shadow",
        "widgets.setting.shadow",
        &["widgets.moon.shadow.dark", "widgets.moon.shadow.light"],
        "widgets.moon.shadow.dark",
        "widgets.moon.shadow.help",
    ),
    Opt::Choice(
        "hemisphere",
        "widgets.setting.hemisphere",
        &["widgets.moon.north", "widgets.moon.south"],
        "widgets.moon.north",
        "widgets.moon.hemisphere.help",
    ),
    Opt::DateFormat("widgets.date-format.long"),
];

/// The values of the first place's setting: the ids of the time zones the
/// SDK knows, written out to stay in the program (a test keeps them the same).
pub const WORLD_ZONES: [&str; 22] = [
    "core.tz.utc",
    "core.tz.honolulu",
    "core.tz.anchorage",
    "core.tz.los-angeles",
    "core.tz.phoenix",
    "core.tz.denver",
    "core.tz.chicago",
    "core.tz.new-york",
    "core.tz.sao-paulo",
    "core.tz.london",
    "core.tz.lisbon",
    "core.tz.berlin",
    "core.tz.helsinki",
    "core.tz.istanbul",
    "core.tz.moscow",
    "core.tz.dubai",
    "core.tz.india",
    "core.tz.bangkok",
    "core.tz.china",
    "core.tz.tokyo",
    "core.tz.sydney",
    "core.tz.auckland",
];
/// The values of the other places' settings: none, or one of the zones.
pub const WORLD_ZONES_OR_NONE: [&str; 23] = {
    let mut values = ["widgets.world-clock.none"; 23];
    let mut n = 0;
    while n < WORLD_ZONES.len() {
        values[n + 1] = WORLD_ZONES[n];
        n += 1;
    }
    values
};

const WORLD_OPTIONS: &[Opt] = &[
    Opt::Choice(
        "place-1",
        "widgets.setting.place-1",
        &WORLD_ZONES,
        "core.tz.new-york",
        "widgets.world-clock.place.help",
    ),
    Opt::Choice(
        "place-2",
        "widgets.setting.place-2",
        &WORLD_ZONES_OR_NONE,
        "core.tz.tokyo",
        "widgets.world-clock.place.help",
    ),
    Opt::Choice(
        "place-3",
        "widgets.setting.place-3",
        &WORLD_ZONES_OR_NONE,
        "widgets.world-clock.none",
        "widgets.world-clock.place.help",
    ),
    Opt::Choice(
        "layout",
        "widgets.setting.layout",
        &[
            "widgets.layout.auto",
            "widgets.layout.rows",
            "widgets.layout.columns",
        ],
        "widgets.layout.auto",
        "widgets.world-clock.layout.help",
    ),
    Opt::Part("here", "widgets.setting.here", false),
    Opt::Part("day", "widgets.setting.day-difference", true),
    Opt::Part("offset", "widgets.setting.offset", false),
];

const WORD_CLOCK_OPTIONS: &[Opt] = &[
    Opt::Choice(
        "style",
        "widgets.setting.style",
        &[
            "widgets.word-clock.style.text",
            "widgets.word-clock.style.grid",
        ],
        "widgets.word-clock.style.text",
        "widgets.word-clock.style.help",
    ),
    Opt::Size("core.size.fit", true),
    Opt::Choice(
        "lead",
        "widgets.setting.word-clock-lead",
        &[
            "widgets.word-clock.lead.none",
            "widgets.word-clock.lead.same",
            "widgets.word-clock.lead.caption",
        ],
        "widgets.word-clock.lead.same",
        "widgets.word-clock.lead.help",
    ),
    Opt::Choice(
        "quarters",
        "widgets.setting.quarters",
        &[
            "widgets.word-clock.quarters.past",
            "widgets.word-clock.quarters.next",
        ],
        "widgets.word-clock.quarters.past",
        "widgets.word-clock.quarters.help",
    ),
    Opt::Part("capitals", "widgets.setting.capitals", false),
    Opt::Part("dots", "widgets.setting.minute-dots", false),
];

const GREETING_OPTIONS: &[Opt] = &[
    Opt::Text(
        "name",
        "widgets.greeting.setting.name",
        "",
        "widgets.greeting.setting.name.help",
    ),
    Opt::Choice(
        "line",
        "widgets.greeting.setting.line",
        &["widgets.greeting.line.none", "widgets.greeting.line.date"],
        "widgets.greeting.line.date",
        "widgets.greeting.line.help",
    ),
    Opt::Part("icon", "widgets.greeting.setting.icon", false),
    Opt::Choice(
        "face",
        "widgets.setting.font",
        FACES,
        "widgets.face.ui",
        "widgets.setting.face-font.help",
    ),
    Opt::Size("core.size.fit", true),
];

const LOGO_OPTIONS: &[Opt] = &[
    Opt::Part("name", "widgets.logo.setting.name", false),
    Opt::Choice(
        "face",
        "widgets.setting.font",
        FACES,
        "widgets.face.ui",
        "widgets.setting.face-font.help",
    ),
    Opt::Size("core.size.medium", true),
];

const PICTURE_OPTIONS: &[Opt] = &[
    Opt::Folder(
        "folder",
        "widgets.picture.setting.folder",
        "Pictures",
        "widgets.picture.setting.folder.help",
    ),
    Opt::Choice(
        "change",
        "widgets.setting.change",
        &["widgets.change.daily", "widgets.change.shown"],
        "widgets.change.daily",
        "widgets.setting.change.help",
    ),
    Opt::Choice(
        "fit",
        "widgets.picture.setting.fit",
        &["widgets.picture.fit.fill", "widgets.picture.fit.whole"],
        "widgets.picture.fit.fill",
        "widgets.picture.fit.help",
    ),
    Opt::Part("caption", "widgets.picture.setting.caption", false),
];

const NOTE_OPTIONS: &[Opt] = &[
    Opt::Text(
        "text",
        "widgets.note.setting.text",
        "",
        "widgets.note.setting.text.help",
    ),
    Opt::Text(
        "heading",
        "widgets.note.setting.heading",
        "",
        "widgets.note.setting.heading.help",
    ),
    Opt::Choice(
        "face",
        "widgets.setting.font",
        FACES,
        "widgets.face.ui",
        "widgets.setting.face-font.help",
    ),
    Opt::Size("core.size.fit", true),
];

const QUOTE_OPTIONS: &[Opt] = &[
    Opt::Choice(
        "source",
        "widgets.quote.setting.source",
        &[
            "widgets.quote.source.shipped",
            "widgets.quote.source.own",
            "widgets.quote.source.both",
        ],
        "widgets.quote.source.both",
        "widgets.quote.setting.source.help",
    ),
    Opt::Choice(
        "change",
        "widgets.setting.change",
        &["widgets.change.daily", "widgets.change.shown"],
        "widgets.change.daily",
        "widgets.setting.change.help",
    ),
    Opt::Part("author", "widgets.quote.setting.author", true),
    Opt::Part("marks", "widgets.quote.setting.marks", true),
    Opt::Part("mark", "widgets.quote.setting.mark", false),
    Opt::Part("caption", "widgets.quote.setting.caption", false),
    Opt::Choice(
        "face",
        "widgets.setting.font",
        FACES,
        "widgets.face.italic",
        "widgets.setting.face-font.help",
    ),
    Opt::Size("core.size.fit", true),
];

const TODO_OPTIONS: &[Opt] = &[
    Opt::Number(
        "count",
        "widgets.todo.setting.count",
        1,
        12,
        5,
        "widgets.todo.count.help",
    ),
    Opt::Choice(
        "done",
        "widgets.todo.setting.done",
        &["widgets.todo.done.struck", "widgets.todo.done.hidden"],
        "widgets.todo.done.struck",
        "widgets.todo.done.help",
    ),
    Opt::Choice(
        "box",
        "widgets.todo.setting.box",
        &[
            "widgets.todo.box.circle",
            "widgets.todo.box.square",
            "widgets.todo.box.none",
        ],
        "widgets.todo.box.circle",
        "widgets.todo.box.help",
    ),
    Opt::Part("caption", "widgets.todo.setting.caption", true),
    Opt::Size("core.size.small", false),
];

/// Every widget of the pack.
pub const WIDGETS: &[Def] = &[
    Def {
        name: "day",
        token: 100,
        category: category::TIME,
        live: true,
        align: "core.align.left",
        look_last: false,
        options: DAY_OPTIONS,
    },
    Def {
        name: "tile-clock",
        token: 101,
        category: category::TIME,
        live: true,
        align: "core.align.center",
        look_last: false,
        options: TILES_OPTIONS,
    },
    Def {
        name: "calendar",
        token: 102,
        category: category::TIME,
        live: true,
        align: "core.align.center",
        look_last: false,
        options: CALENDAR_OPTIONS,
    },
    Def {
        name: "countdown",
        token: 103,
        category: category::TIME,
        live: true,
        align: "core.align.left",
        look_last: false,
        options: COUNTDOWN_OPTIONS,
    },
    Def {
        name: "year-progress",
        token: 104,
        category: category::TIME,
        live: true,
        align: "core.align.left",
        look_last: false,
        options: YEAR_OPTIONS,
    },
    Def {
        name: "analogue-clock",
        token: 105,
        category: category::TIME,
        live: true,
        align: "core.align.center",
        look_last: false,
        options: ANALOGUE_OPTIONS,
    },
    Def {
        name: "moon",
        token: 106,
        category: category::TIME,
        live: true,
        align: "core.align.left",
        look_last: false,
        options: MOON_OPTIONS,
    },
    Def {
        name: "world-clock",
        token: 107,
        category: category::TIME,
        live: true,
        align: "core.align.left",
        look_last: false,
        options: WORLD_OPTIONS,
    },
    Def {
        name: "word-clock",
        token: 108,
        category: category::TIME,
        live: true,
        align: "core.align.left",
        look_last: false,
        options: WORD_CLOCK_OPTIONS,
    },
    Def {
        name: "greeting",
        token: 109,
        category: category::PERSONAL,
        live: true,
        align: "core.align.center",
        look_last: true,
        options: GREETING_OPTIONS,
    },
    Def {
        name: "note",
        token: 110,
        category: category::PERSONAL,
        live: false,
        align: "core.align.center",
        look_last: true,
        options: NOTE_OPTIONS,
    },
    Def {
        name: "quote",
        token: 111,
        category: category::WORDS,
        live: true,
        align: "core.align.center",
        look_last: true,
        options: QUOTE_OPTIONS,
    },
    Def {
        name: "todo",
        token: 112,
        category: category::PERSONAL,
        live: true,
        align: "core.align.left",
        look_last: true,
        options: TODO_OPTIONS,
    },
    Def {
        name: "battery",
        token: 113,
        category: category::DEVICE,
        live: true,
        align: "core.align.center",
        look_last: false,
        options: BATTERY_OPTIONS,
    },
    Def {
        name: "library",
        token: 114,
        category: category::BOOKS,
        live: true,
        align: "core.align.center",
        look_last: false,
        options: LIBRARY_OPTIONS,
    },
    Def {
        name: "highlight",
        token: 115,
        category: category::WORDS,
        live: true,
        align: "core.align.center",
        look_last: false,
        options: HIGHLIGHT_OPTIONS,
    },
    Def {
        name: "favourites",
        token: 116,
        category: category::BOOKS,
        live: false,
        align: "core.align.center",
        look_last: false,
        options: FAVOURITES_OPTIONS,
    },
    Def {
        name: "series",
        token: 117,
        category: category::BOOKS,
        live: false,
        align: "core.align.center",
        look_last: false,
        options: SERIES_OPTIONS,
    },
    Def {
        name: "next",
        token: 118,
        category: category::BOOKS,
        live: false,
        align: "core.align.left",
        look_last: false,
        options: NEXT_OPTIONS,
    },
    Def {
        name: "progress",
        token: 119,
        category: category::BOOKS,
        live: false,
        align: "core.align.center",
        look_last: false,
        options: PROGRESS_OPTIONS,
    },
    Def {
        name: "reading",
        token: 120,
        category: category::BOOKS,
        live: false,
        align: "core.align.left",
        look_last: false,
        options: READING_OPTIONS,
    },
    Def {
        name: "logo",
        token: 121,
        category: category::OTHER,
        live: false,
        align: "core.align.center",
        look_last: true,
        options: LOGO_OPTIONS,
    },
    Def {
        name: "picture",
        token: 122,
        category: category::PERSONAL,
        live: false,
        align: "core.align.center",
        look_last: true,
        options: PICTURE_OPTIONS,
    },
];

/// The widgets and their settings groups and settings.
pub fn widgets<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    for widget in WIDGETS {
        let group = ["widgets.", widget.name].concat();
        let title = ["widgets.", widget.name].concat();
        d.group(GroupDecl::new(widget.name, &title))?;
        let mut spec = WidgetDecl::new(widget.name, &title, widget.category, widget.token)
            .map_err(|_| DeclareError::TooLong("widget"))?;
        if widget.live {
            spec = spec.live();
        }
        // No `previews`: the pack's widgets are offered in the Add picker; a
        // widget made only to preview a settings group is never listed there.
        d.widget(spec)?;
        declare_all(d, widget.name, &group, widget.all())?;
        d.submit()?;
    }
    Ok(())
}

/// The to-do list's own screen.
pub fn screens<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.screen(
        ScreenDecl::new("todo.list", "widgets.todo", TODO_SCREEN).instances(Instances::Single),
    )?;
    d.submit()
}

/// The action that opens it, with its icon.
pub fn navigation<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.action(ActionDecl::new("todo.open", "widgets.todo", TODO_OPEN))?;
    d.icon("widgets.todo.open", "list-check")?;
    d.submit()
}

/// The minute tick, for widgets that show the time.
pub fn events<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.subscribe_event(EventKind::Tick, TICK)?;
    for kind in [
        EventKind::BookOpened,
        EventKind::PageTurned,
        EventKind::BookClosed,
        EventKind::Suspend,
        EventKind::Stopping,
    ] {
        d.subscribe_event(kind, READING)?;
    }
    // The answer to the logo picture asked for, and a change of the picture set.
    d.subscribe_event(EventKind::ImageReady, TICK)?;
    d.subscribe_event(EventKind::SettingChanged, TICK)?;
    d.submit()?;
    Ok(())
}
