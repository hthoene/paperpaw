//! Native identity of the widget pack: its widgets, the settings group of
//! each, and the tick that keeps clocks right. Callback tokens are stable
//! within this plugin; every widget's settings are declared here, none at
//! run time.
use paperpaw_sdk::native::declare::{
    Declarations, DeclareError, EventKind, ExtraDecl, GroupDecl, Kind, SpecDecl, WidgetDecl,
};

/// The callback token of the minute tick.
pub const TICK: u32 = 1;

/// The widget categories of the host's catalogue.
pub mod category {
    pub const TIME: &str = "core.widgets.time";
}

const FRAMES: &[&str] = &[
    "core.frame.none",
    "core.frame.line",
    "core.frame.card",
    "core.frame.dark",
];
const SIZES: &[&str] = &[
    "core.size.small",
    "core.size.medium",
    "core.size.large",
    "core.size.fit",
];
const PADDINGS: &[&str] = &[
    "core.padding.auto",
    "core.padding.none",
    "core.padding.small",
    "core.padding.normal",
    "core.padding.large",
];
const ALIGNMENTS: &[&str] = &["core.align.left", "core.align.center", "core.align.right"];
const PAPERS: &[&str] = &[
    "core.plate.normal",
    "core.plate.inverted",
    "core.plate.light-gray",
    "core.plate.dark-gray",
    "core.plate.none",
];
const INKS: &[&str] = &["core.ink.auto", "core.tone.black", "core.tone.white"];
const CORNERS: &[&str] = &[
    "core.corners.auto",
    "core.corners.square",
    "core.corners.small",
    "core.corners.round",
    "core.corners.soft",
];
const LINES: &[&str] = &[
    "core.lines.auto",
    "core.lines.thin",
    "core.lines.medium",
    "core.lines.thick",
];
const SHADOWS: &[&str] = &["core.shadow.none", "core.shadow.hard"];

/// The values of a date-format setting.
pub const DATE_FORMATS: &[&str] = &[
    "widgets.date-format.long",
    "widgets.date-format.weekday",
    "widgets.date-format.numeric",
    "widgets.date-format.iso",
];

/// One setting of a widget's group; the key is relative to the widget.
#[derive(Debug, Clone, Copy)]
pub enum Opt {
    /// Where the text sits; the default is one of the alignments.
    Align(&'static str),
    /// The frame, with its default.
    Frame(&'static str),
    Padding,
    Paper,
    Ink,
    Corners,
    FrameLine,
    Shadow,
    /// The text size, with its default and whether "as large as fits" is offered.
    Size(&'static str, bool),
    /// A switch for a part of what the widget shows: key, title, default.
    Part(&'static str, &'static str, bool),
    /// A choice: key, title, values, default, help.
    Choice(
        &'static str,
        &'static str,
        &'static [&'static str],
        &'static str,
        &'static str,
    ),
    /// How dates are written: the default is one of [`DATE_FORMATS`].
    DateFormat(&'static str),
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
];

/// A setting as declared: its key after the widget's, kind, title and help,
/// and whether the choice keeps values its options lack (colours).
struct Row<'a> {
    suffix: &'a str,
    kind: Kind<'a>,
    title: &'a str,
    help: &'a str,
    keeps: bool,
}

fn choice<'a>(
    suffix: &'a str,
    (default, choices): (&'a str, &'a [&'a str]),
    title: &'a str,
    help: &'a str,
) -> Row<'a> {
    Row {
        suffix,
        kind: Kind::Choice { default, choices },
        title,
        help,
        keeps: false,
    }
}

fn row(option: Opt) -> Row<'static> {
    match option {
        Opt::Align(default) => choice(
            "align",
            (default, ALIGNMENTS),
            "core.setting.align",
            "core.setting.align.help",
        ),
        Opt::Frame(default) => choice(
            "frame",
            (default, FRAMES),
            "core.setting.frame",
            "core.setting.frame.help",
        ),
        Opt::Padding => choice(
            "padding",
            (PADDINGS[0], PADDINGS),
            "core.setting.padding",
            "core.setting.padding.help",
        ),
        Opt::Paper => Row {
            keeps: true,
            ..choice(
                "paper",
                (PAPERS[0], PAPERS),
                "core.setting.paper",
                "core.setting.paper.help",
            )
        },
        Opt::Ink => Row {
            keeps: true,
            ..choice(
                "ink",
                (INKS[0], INKS),
                "core.setting.ink",
                "core.setting.ink.help",
            )
        },
        Opt::Corners => choice(
            "corners",
            (CORNERS[0], CORNERS),
            "core.setting.corners",
            "core.setting.corners.help",
        ),
        Opt::FrameLine => choice(
            "frame-line",
            (LINES[0], LINES),
            "core.setting.frame-line",
            "core.setting.frame-line.help",
        ),
        Opt::Shadow => choice(
            "drop-shadow",
            (SHADOWS[0], SHADOWS),
            "core.setting.drop-shadow",
            "core.setting.drop-shadow.help",
        ),
        Opt::Size(default, fit) => choice(
            "size",
            (default, if fit { SIZES } else { &SIZES[..3] }),
            "core.setting.size",
            "core.setting.size.help",
        ),
        Opt::Part(key, title, default) => Row {
            suffix: key,
            kind: Kind::Bool(default),
            title,
            help: "core.setting.part.help",
            keeps: false,
        },
        Opt::Choice(key, title, values, default, help) => {
            choice(key, (default, values), title, help)
        }
        Opt::DateFormat(default) => choice(
            "date-format",
            (default, DATE_FORMATS),
            "widgets.setting.date-format",
            "widgets.setting.date-format.help",
        ),
    }
}

fn declare_option(
    d: &mut Declarations<impl FnMut(&[u8]) -> Result<(), i32>>,
    widget: &Def,
    group: &str,
    option: Opt,
) -> Result<(), DeclareError> {
    let row = row(option);
    let relative = [widget.name, ".", row.suffix].concat();
    let mut spec = SpecDecl::new(&relative, row.kind, group, row.title);
    spec.help = row.help;
    d.spec(spec)?;
    if row.keeps {
        let mut extra = ExtraDecl::new(&relative);
        extra.keeps_unknown = true;
        d.extra(extra)?;
    }
    Ok(())
}

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
        for option in widget.options {
            declare_option(d, widget, &group, *option)?;
        }
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
