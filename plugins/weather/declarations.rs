//! What the weather plugin declares (slice 1), compared with the interface-6
//! registration by the declaration tests: the settings group, the place
//! (edited on its own screen), the units and how often the weather is
//! fetched, the coordinates, the place's screen, the actions to choose the
//! place and to fetch, and the icons. The two widgets and their settings
//! follow with the widget pack's kit (`PORTING-7.md`).
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EditorDecl, ExtraDecl, GroupDecl, Kind, ScreenDecl,
    SpecDecl,
};

/// Callback tokens: stable within this plugin.
pub const CHOOSE_PLACE_TOKEN: u32 = 1;
pub const REFRESH_TOKEN: u32 = 2;
pub const PLACE_SCREEN: u32 = 10;

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
