//! Native identity of the widget pack: its widgets, the settings group of
//! each, and the tick that keeps clocks right. Callback tokens are stable
//! within this plugin; every widget's settings are declared here, none at
//! run time.
use paperpaw_sdk::native::declare::{Declarations, DeclareError, EventKind, GroupDecl, WidgetDecl};
pub use paperpaw_ui_kit::widget::options::Opt;
use paperpaw_ui_kit::widget::options::{FACES, declare_all};

/// The callback token of the minute tick.
pub const TICK: u32 = 1;

/// The widget categories of the host's catalogue.
pub mod category {
    pub const TIME: &str = "core.widgets.time";
    pub const PERSONAL: &str = "core.widgets.personal";
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
    pub options: &'static [Opt],
}

const DAY_OPTIONS: &[Opt] = &[
    Opt::Align("core.align.left"),
    Opt::Frame("core.frame.none"),
    Opt::Padding,
    Opt::Paper,
    Opt::Ink,
    Opt::Corners,
    Opt::FrameLine,
    Opt::Shadow,
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
    Opt::Align("core.align.center"),
    Opt::Frame("core.frame.none"),
    Opt::Padding,
    Opt::Paper,
    Opt::Ink,
    Opt::Corners,
    Opt::FrameLine,
    Opt::Shadow,
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
    Opt::Align("core.align.center"),
    Opt::Frame("core.frame.none"),
    Opt::Padding,
    Opt::Paper,
    Opt::Ink,
    Opt::Corners,
    Opt::FrameLine,
    Opt::Shadow,
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
    Opt::Align("core.align.left"),
    Opt::Frame("core.frame.none"),
    Opt::Padding,
    Opt::Paper,
    Opt::Ink,
    Opt::Corners,
    Opt::FrameLine,
    Opt::Shadow,
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
    Opt::Align("core.align.left"),
    Opt::Frame("core.frame.none"),
    Opt::Padding,
    Opt::Paper,
    Opt::Ink,
    Opt::Corners,
    Opt::FrameLine,
    Opt::Shadow,
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
    Opt::Align("core.align.center"),
    Opt::Frame("core.frame.none"),
    Opt::Padding,
    Opt::Paper,
    Opt::Ink,
    Opt::Corners,
    Opt::FrameLine,
    Opt::Shadow,
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

const MOON_OPTIONS: &[Opt] = &[
    Opt::Align("core.align.left"),
    Opt::Frame("core.frame.none"),
    Opt::Padding,
    Opt::Paper,
    Opt::Ink,
    Opt::Corners,
    Opt::FrameLine,
    Opt::Shadow,
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
    Opt::Align("core.align.left"),
    Opt::Frame("core.frame.none"),
    Opt::Padding,
    Opt::Paper,
    Opt::Ink,
    Opt::Corners,
    Opt::FrameLine,
    Opt::Shadow,
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
    Opt::Align("core.align.left"),
    Opt::Frame("core.frame.none"),
    Opt::Padding,
    Opt::Paper,
    Opt::Ink,
    Opt::Corners,
    Opt::FrameLine,
    Opt::Shadow,
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
    Opt::Align("core.align.center"),
    Opt::Frame("core.frame.none"),
    Opt::Padding,
    Opt::Paper,
    Opt::Ink,
    Opt::Corners,
    Opt::FrameLine,
    Opt::Shadow,
];

/// Every widget of the pack.
pub const WIDGETS: &[Def] = &[
    Def {
        name: "day",
        token: 100,
        category: category::TIME,
        live: true,
        options: DAY_OPTIONS,
    },
    Def {
        name: "tile-clock",
        token: 101,
        category: category::TIME,
        live: true,
        options: TILES_OPTIONS,
    },
    Def {
        name: "calendar",
        token: 102,
        category: category::TIME,
        live: true,
        options: CALENDAR_OPTIONS,
    },
    Def {
        name: "countdown",
        token: 103,
        category: category::TIME,
        live: true,
        options: COUNTDOWN_OPTIONS,
    },
    Def {
        name: "year-progress",
        token: 104,
        category: category::TIME,
        live: true,
        options: YEAR_OPTIONS,
    },
    Def {
        name: "analogue-clock",
        token: 105,
        category: category::TIME,
        live: true,
        options: ANALOGUE_OPTIONS,
    },
    Def {
        name: "moon",
        token: 106,
        category: category::TIME,
        live: true,
        options: MOON_OPTIONS,
    },
    Def {
        name: "world-clock",
        token: 107,
        category: category::TIME,
        live: true,
        options: WORLD_OPTIONS,
    },
    Def {
        name: "word-clock",
        token: 108,
        category: category::TIME,
        live: true,
        options: WORD_CLOCK_OPTIONS,
    },
    Def {
        name: "greeting",
        token: 109,
        category: category::PERSONAL,
        live: true,
        options: GREETING_OPTIONS,
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
        d.widget(
            spec.previews(&group)
                .map_err(|_| DeclareError::TooLong("widget preview"))?,
        )?;
        declare_all(d, widget.name, &group, widget.options)?;
        d.submit()?;
    }
    Ok(())
}

/// The minute tick, for widgets that show the time.
pub fn events<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.subscribe_event(EventKind::Tick, TICK)?;
    d.submit()?;
    Ok(())
}
