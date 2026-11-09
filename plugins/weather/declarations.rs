//! What the weather plugin declares: the settings group, the place (edited
//! on its own screen), the units and how often the weather is fetched, the
//! coordinates, the place's screen, the actions to choose the place and to
//! fetch, the icons, the two widgets with their settings and the events the
//! plugin subscribes to. `build.rs` turns it into the image's batches.
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EditorDecl, EventKind, ExtraDecl, GroupDecl, Kind,
    ScreenDecl, SpecDecl, WidgetDecl,
};
pub use paperpaw_ui_kit::widget::options::Opt;
use paperpaw_ui_kit::widget::options::{declare_all, shared_look};

/// Callback tokens: stable within this plugin.
pub const CHOOSE_PLACE_TOKEN: u32 = 1;
pub const REFRESH_TOKEN: u32 = 2;
pub const PLACE_SCREEN: u32 = 10;
/// The token of the lifecycle events (start, resume, a setting changed).
pub const LIFECYCLE_TOKEN: u32 = 3;
/// The token of the end of a request (`core.net`).
pub const HTTP_TOKEN: u32 = 4;
/// The token of the work that looks whether the weather is due.
#[allow(dead_code, reason = "only the plugin schedules work; the build script sees the declarations alone")]
pub const WORK_TOKEN: u32 = 5;
/// The widget *Weather now*.
pub const NOW_TOKEN: u32 = 20;
/// The widget *Forecast*.
pub const FORECAST_TOKEN: u32 = 21;

/// The settings group, as the settings name it.
const GROUP: &str = "weather.group";
pub const TEMPERATURES: [&str; 2] = ["weather.unit.celsius", "weather.unit.fahrenheit"];
pub const WINDS: [&str; 4] = [
    "weather.unit.kmh",
    "weather.unit.mph",
    "weather.unit.ms",
    "weather.unit.knots",
];
pub const EVERY_HOURS: [&str; 4] = [
    "weather.every.1",
    "weather.every.2",
    "weather.every.3",
    "weather.every.6",
];

/// The group, the place, the units, how often, and the coordinates.
pub fn settings<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    let mut group = GroupDecl::new("group", "weather.title");
    group.order = 88;
    d.group(group)?;
    let mut place = SpecDecl::new("place", Kind::Text(""), GROUP, "weather.setting.place");
    place.help = "weather.setting.place.help";
    d.spec(place)?;
    let mut on_screen = ExtraDecl::new("place");
    on_screen.editor = Some(EditorDecl::Screen("place-screen"));
    d.extra(on_screen)?;
    for (name, choices, title, help) in [
        (
            "temperature",
            &TEMPERATURES[..],
            "weather.setting.temperature",
            "weather.setting.temperature.help",
        ),
        (
            "wind",
            &WINDS[..],
            "weather.setting.wind",
            "weather.setting.wind.help",
        ),
        (
            "every",
            &EVERY_HOURS[..],
            "weather.setting.every",
            "weather.setting.every.help",
        ),
    ] {
        let kind = Kind::Choice {
            default: choices[0],
            choices,
        };
        let mut spec = SpecDecl::new(name, kind, GROUP, title);
        spec.help = help;
        d.spec(spec)?;
    }
    let mut coordinates = SpecDecl::new(
        "coordinates",
        Kind::Text(""),
        GROUP,
        "weather.setting.coordinates",
    );
    coordinates.help = "weather.setting.coordinates.help";
    coordinates.advanced = true;
    d.spec(coordinates)?;
    d.submit()?;
    Ok(())
}

/// The screen that chooses the place.
pub fn screens<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.screen(ScreenDecl::new(
        "place-screen",
        "weather.setting.place",
        PLACE_SCREEN,
    ))?;
    d.submit()?;
    Ok(())
}

/// The actions and the icons.
pub fn navigation<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.action(ActionDecl::new(
        "choose-place",
        "weather.setting.place",
        CHOOSE_PLACE_TOKEN,
    ))?;
    d.action(ActionDecl::new("refresh", "weather.refresh", REFRESH_TOKEN))?;
    d.icon("weather.group", "sun")?;
    d.icon("weather.refresh", "refresh")?;
    d.submit()?;
    Ok(())
}

/// The values of the symbol's size.
pub const SYMBOLS: [&str; 3] = [
    "weather.symbol.large",
    "weather.symbol.small",
    "weather.symbol.none",
];
/// The values of where the symbol sits.
pub const PLACINGS: [&str; 3] = [
    "weather.placing.auto",
    "weather.placing.beside",
    "weather.placing.above",
];
/// How the symbols are drawn.
pub const STYLES: [&str; 2] = ["weather.style.line", "weather.style.solid"];
/// The first day the forecast shows.
pub const STARTS: [&str; 2] = ["weather.start.today", "weather.start.tomorrow"];
/// The forecast's layouts.
pub const LAYOUTS: [&str; 3] = [
    "weather.layout.auto",
    "weather.layout.row",
    "weather.layout.list",
];
/// How the forecast names weekdays.
pub const NAMES: [&str; 3] = [
    "weather.names.short",
    "weather.names.initial",
    "weather.names.long",
];

const STYLE: Opt = Opt::Choice(
    "style",
    "weather.setting.style",
    &STYLES,
    STYLES[0],
    "weather.setting.style.help",
);

/// The settings of *Weather now* after the shared look, in their order.
pub const NOW_OPTIONS: &[Opt] = &[
    Opt::Size("core.size.fit", true),
    Opt::Choice(
        "symbol",
        "weather.setting.symbol",
        &SYMBOLS,
        SYMBOLS[0],
        "weather.setting.symbol.help",
    ),
    Opt::Choice(
        "placing",
        "weather.setting.placing",
        &PLACINGS,
        PLACINGS[0],
        "weather.setting.placing.help",
    ),
    STYLE,
    Opt::Part("place", "weather.part.place", true),
    Opt::Part("condition", "weather.part.condition", true),
    Opt::Part("range", "weather.part.range", true),
    Opt::Part("feels", "weather.part.feels", false),
    Opt::Part("wind", "weather.part.wind", false),
    Opt::Part("rain", "weather.part.rain", false),
    Opt::Part("humidity", "weather.part.humidity", false),
    Opt::Part("sun", "weather.part.sun", false),
    Opt::Part("unit", "weather.part.unit", false),
    Opt::Part("age", "weather.part.age", true),
];

/// The settings of *Forecast* after the shared look, in their order.
pub const FORECAST_OPTIONS: &[Opt] = &[
    Opt::Size("core.size.fit", true),
    Opt::Number(
        "days",
        "weather.setting.days",
        3,
        7,
        5,
        "weather.setting.days.help",
    ),
    Opt::Choice(
        "start",
        "weather.setting.start",
        &STARTS,
        STARTS[0],
        "weather.setting.start.help",
    ),
    Opt::Choice(
        "layout",
        "weather.setting.layout",
        &LAYOUTS,
        LAYOUTS[0],
        "weather.setting.layout.help",
    ),
    Opt::Choice(
        "names",
        "weather.setting.names",
        &NAMES,
        NAMES[0],
        "weather.setting.names.help",
    ),
    STYLE,
    Opt::Part("symbol", "weather.part.symbol", true),
    Opt::Part("low", "weather.part.low", true),
    Opt::Part("rain", "weather.part.rain-days", false),
    Opt::Part("bars", "weather.part.bars", true),
    Opt::Part("today", "weather.part.today", true),
    Opt::Part("place", "weather.part.place", false),
    Opt::Part("lines", "weather.part.lines", false),
];

/// The two widgets, their settings groups and their settings.
pub fn widgets<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    for (name, title, token, options) in [
        ("now", "weather.widget.now", NOW_TOKEN, NOW_OPTIONS),
        (
            "forecast",
            "weather.widget.forecast",
            FORECAST_TOKEN,
            FORECAST_OPTIONS,
        ),
    ] {
        let group = ["weather.", name].concat();
        d.group(GroupDecl::new(name, title))?;
        let spec = WidgetDecl::new(name, title, "core.widgets.world", token)
            .map_err(|_| DeclareError::TooLong("widget"))?;
        d.widget(
            spec.previews(&group)
                .map_err(|_| DeclareError::TooLong("widget preview"))?,
        )?;
        let look = shared_look("core.align.center");
        declare_all(
            d,
            name,
            &group,
            look.into_iter().chain(options.iter().copied()),
        )?;
        if name == "forecast" {
            let mut days = ExtraDecl::new("forecast.days");
            days.unit = "weather.unit.days";
            d.extra(days)?;
        }
        d.submit()?;
    }
    Ok(())
}

/// The events the plugin watches: the start and the wake (the weather may be
/// due), a setting that changed (the place, the units) and the end of a request.
pub fn events<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    for kind in [
        EventKind::Started,
        EventKind::Resume,
        EventKind::SettingChanged,
    ] {
        d.subscribe_event(kind, LIFECYCLE_TOKEN)?;
    }
    d.subscribe_event(EventKind::HttpDone, HTTP_TOKEN)?;
    d.submit()?;
    Ok(())
}
