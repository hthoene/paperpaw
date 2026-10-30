//! The weather widgets on the standard app's home screen in other time
//! zones and with other settings: which day is today follows the
//! device's zone, and the forecast's first day, names of days and parts
//! shown follow what is set.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_core::Kernel;
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation, SystemStatus,
    WallClock,
};
use paperpaw_sdk::MemoryStorage;

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test-x4",
    "Test X4",
    DisplayInfo::new(480, 800, 220, PixelFormat::Mono1, Rotation::Deg0, None),
    InputCaps::new(
        true,
        false,
        false,
        &["back", "confirm", "left", "right", "up", "down", "power"],
    ),
    MemoryClass::Small,
);

const KEPT: &str = ".paperpaw/plugins/weather/weather.txt";
/// When the kept weather was fetched: Wednesday 2026-10-14, 10:30 UTC.
const FETCHED: i64 = 1_791_973_800;
const PLACE: &str = "[weather]\nplace = Berlin\ncoordinates = 52.52,13.41\n";

/// The texts of the home screen with the weather widgets, the weather
/// kept, the clock at the time it was fetched in a zone `offset` minutes
/// from UTC, `config` added.
fn texts(offset: i32, config: &str) -> Vec<String> {
    let storage = MemoryStorage::new()
        .with(".paperpaw/plugins/setup/done", "")
        .with(KEPT, include_str!("../testdata/weather.txt.v1"));
    let config = format!(
        "[core]\nlanguage = en\n[help]\nshow = false\n\
         [home]\npage-1 = weather.now 0/0 4x3, weather.forecast 0/3 4x3\n{PLACE}{config}"
    );
    let builder = Kernel::builder(DEVICE)
        .storage(storage)
        .network(true)
        .user_config(config);
    let mut kernel = paperpaw_app::install(builder, paperpaw_app::Fonts::Packs)
        .build()
        .unwrap();
    let clock = WallClock::new(Instant(0), FETCHED * 1000, Some(offset));
    let _ = kernel.set_status(Instant(0), SystemStatus::new(Some(clock), None));
    let _ = kernel.start(Instant(0));
    let _ = kernel.tick(Instant(61_000));
    kernel.render(&mut |_, _| {});
    kernel.describe().into_iter().map(|p| p.label).collect()
}

fn has(texts: &[String], text: &str) -> bool {
    texts.iter().any(|t| t == text)
}

#[test]
fn today_is_the_devices_day() {
    // In Berlin it is the 14th, a Wednesday: today and Thursday.
    let berlin = texts(120, "");
    assert!(has(&berlin, "Today") && has(&berlin, "Thu"), "{berlin:?}");
    assert!(!has(&berlin, "Wed"), "{berlin:?}");
    assert!(has(&berlin, "H 15° · L 6°"), "{berlin:?}");

    // Far west it is still Tuesday the 13th: the kept days start
    // tomorrow, none of them is today.
    let west = texts(-12 * 60, "");
    assert!(!has(&west, "Today"), "{west:?}");
    assert!(has(&west, "Wed") && has(&west, "Thu"), "{west:?}");

    // Far east it is already Thursday the 15th: Wednesday is gone, and
    // today's range is Thursday's.
    let east = texts(14 * 60, "");
    assert!(has(&east, "Today") && has(&east, "Fri"), "{east:?}");
    assert!(!has(&east, "Wed") && !has(&east, "Thu"), "{east:?}");
    assert!(has(&east, "H 11° · L 5°"), "{east:?}");
}

#[test]
fn the_forecast_starts_tomorrow_if_set() {
    let texts = texts(120, "[weather.forecast]\nstart = weather.start.tomorrow\n");
    assert!(!has(&texts, "Today"), "{texts:?}");
    assert!(has(&texts, "Thu") && has(&texts, "Fri"), "{texts:?}");
}

#[test]
fn weekdays_are_named_as_set() {
    let long = texts(
        120,
        "[weather.forecast]\nnames = weather.names.long\nlayout = weather.layout.list\n",
    );
    assert!(has(&long, "Thursday") && has(&long, "Friday"), "{long:?}");
    let initials = texts(120, "[weather.forecast]\nnames = weather.names.initial\n");
    // Initials, also for today.
    assert!(!has(&initials, "Today"), "{initials:?}");
    assert!(has(&initials, "T") && has(&initials, "F"), "{initials:?}");
}

#[test]
fn parts_are_shown_as_set() {
    let shown = texts(
        120,
        "[weather.forecast]\nrain = on\nplace = on\n\
         [weather.now]\nsize = core.size.small\nfeels = on\nwind = on\nunit = on\n",
    );
    // The chance of rain of each day; the place above the forecast too.
    assert!(has(&shown, "10 %") && has(&shown, "85 %"), "{shown:?}");
    assert_eq!(
        shown.iter().filter(|t| *t == "Berlin").count(),
        2,
        "{shown:?}"
    );
    // The temperature with its unit; how warm it feels and the wind on
    // a line.
    assert!(has(&shown, "12°C"), "{shown:?}");
    assert!(has(&shown, "Feels like 10°C · Wind 15 km/h"), "{shown:?}");
    // In other units, converted from what was kept.
    let units = texts(
        120,
        "[weather]\ntemperature = weather.unit.fahrenheit\nwind = weather.unit.ms\n\
         [weather.now]\nsize = core.size.small\nwind = on\n",
    );
    assert!(has(&units, "54°"), "{units:?}");
    assert!(units.iter().any(|t| t.contains("Wind 4 m/s")), "{units:?}");
}
