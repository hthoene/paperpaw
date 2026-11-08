//! Weather: the weather now and the next days as widgets for the home
//! screen (or any screen showing widgets), from Open-Meteo
//! (<https://open-meteo.com>, free and without a key).
//!
//! - *Weather now* (`now`): a symbol, the temperature, the condition,
//!   today's highest and lowest, and more to choose (wind, rain, ...).
//! - *Forecast* (`forecast`): the next days in a row, or as a list in
//!   tall places.
//!
//! Nothing is sent anywhere until the reader sets a place (by its name,
//! found with Open-Meteo's search, see `place`); then only its
//! coordinates. The weather is fetched while a weather widget is placed
//! on a page (or was just shown), at most each hour or as set, and only
//! while there is a network: where PaperPaw manages Wi-Fi it does not
//! switch the radio on for it, but waits until it is on. A tap on a
//! widget fetches it at once. The last answer is kept in a small file,
//! so the widgets show it at once after a start, with its age when old.

#![no_std]

extern crate alloc;
#[cfg(test)]
extern crate std;

mod data;
#[path = "../declarations.rs"]
pub mod declarations;
mod draw;
mod forecast;
pub mod native_answers;
mod native_draw;
mod native_entry;
mod native_fetch;
mod native_forecast;
mod native_now;
mod native_place;
pub mod native_plugin;
pub mod native_shown;
mod native_symbol;
pub mod native_weather;
#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;
mod now;
mod place;
mod symbol;

/// Test-only: refuses chosen allocations to prove the fallible paths.
#[cfg(test)]
#[path = "../../test-support/fail_alloc.rs"]
mod fail_alloc;
#[cfg(test)]
#[global_allocator]
static ALLOC: fail_alloc::Failing = fail_alloc::Failing;
#[cfg(test)]
mod native_answers_tests;
#[cfg(test)]
mod native_parity_tests;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use core::cell::RefCell;

use paperpaw_sdk::builtin::{events, params};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::replies::HttpReply;
use paperpaw_sdk::setting::{Setting, Text};
use paperpaw_sdk::widgets;
use paperpaw_sdk::{
    ActionSpec, Command, Commands, Env, FixedAction, FixedScreen, HttpRequest, Id, Instant,
    Manifest, Params, Plugin, RegisterError, Registrar, SettingSpec, time,
};

use data::{Temperature, Wind};

pub const PLUGIN: Id = Id::new_static("weather");
/// The settings of the weather: the place, units, how often.
pub const GROUP: Id = Id::new_static("weather.group");
/// The place's name, set with its screen ([`PLACE_SCREEN`]).
pub const PLACE: Setting<Text> = Setting::new("weather.place");
/// The place's coordinates, `latitude,longitude`: all that is sent.
pub const COORDINATES: Setting<Text> = Setting::new("weather.coordinates");
/// Degrees Celsius or Fahrenheit: one of [`TEMPERATURES`].
pub const TEMPERATURE: Setting<Text> = Setting::new("weather.temperature");
pub const TEMPERATURES: [&str; 2] = ["weather.unit.celsius", "weather.unit.fahrenheit"];
/// The unit of wind speeds: one of [`WINDS`].
pub const WIND: Setting<Text> = Setting::new("weather.wind");
pub const WINDS: [&str; 4] = [
    "weather.unit.kmh",
    "weather.unit.mph",
    "weather.unit.ms",
    "weather.unit.knots",
];
/// Hours between fetches at most: one of [`EVERY_HOURS`].
pub const EVERY: Setting<Text> = Setting::new("weather.every");
pub const EVERY_HOURS: [&str; 4] = [
    "weather.every.1",
    "weather.every.2",
    "weather.every.3",
    "weather.every.6",
];
/// The settings of the weather but the place, kept in the program.
/// The place, edited on its own screen.
static ON_PLACE_SCREEN: paperpaw_sdk::SettingExtra =
    paperpaw_sdk::SettingExtra::new().editor(PLACE_SCREEN);
static PLACE_SETTING: [SettingSpec; 1] =
    [
        SettingSpec::text(PLACE.static_str(), "weather.setting.place", "", GROUP)
            .with_extra(&ON_PLACE_SCREEN)
            .description("weather.setting.place.help"),
    ];

static SETTINGS: [SettingSpec; 4] = [
    SettingSpec::choice(
        "weather.temperature",
        "weather.setting.temperature",
        &TEMPERATURES,
        TEMPERATURES[0],
        GROUP,
    )
    .description("weather.setting.temperature.help"),
    SettingSpec::choice(
        "weather.wind",
        "weather.setting.wind",
        &WINDS,
        WINDS[0],
        GROUP,
    )
    .description("weather.setting.wind.help"),
    SettingSpec::choice(
        "weather.every",
        "weather.setting.every",
        &EVERY_HOURS,
        EVERY_HOURS[0],
        GROUP,
    )
    .description("weather.setting.every.help"),
    SettingSpec::text(
        "weather.coordinates",
        "weather.setting.coordinates",
        "",
        GROUP,
    )
    .description("weather.setting.coordinates.help")
    .advanced(),
];
/// Fetches the weather now.
pub const REFRESH: Id = Id::new_static("weather.refresh");
/// Opens the place's screen.
pub const CHOOSE_PLACE: Id = Id::new_static("weather.choose-place");
/// The screen that chooses the place (the editor of [`PLACE`]).
pub const PLACE_SCREEN: Id = Id::new_static("weather.place-screen");
/// The forecast's answer.
const FETCHED: Event<HttpReply> = Event::new("weather.fetched");
/// The timer: time to look whether the weather is due.
const TICK: Event<()> = Event::new("weather.tick");
/// Parameter of a fetch the reader asked for: tell how it went.
const TOLD: &str = "told";
/// The widgets.
const WIDGETS: [&str; 2] = ["weather.now", "weather.forecast"];
/// No new try sooner after a fetch failed, in seconds.
const RETRY: i64 = 15 * 60;
/// A fetch under way is not asked again for this long, in milliseconds.
const PATIENCE: u64 = 2 * 60 * 1000;
/// A widget updated this recently counts as shown, in milliseconds.
const SEEN: u64 = 5 * 60 * 1000;

fn data(name: &str) -> String {
    format!("{}/{name}", Env::plugin_dir(PLUGIN.as_str()))
}

/// Where the last weather is kept.
fn weather_file() -> String {
    data("weather.txt")
}

/// What the handlers and the widgets share.
#[derive(Debug, Default)]
pub(crate) struct State {
    /// The weather kept, read from its file once.
    weather: Option<data::Weather>,
    loaded: bool,
    /// Counts changes of the weather, so widgets know to read it anew.
    generation: u32,
    /// A fetch is under way, since then.
    asked: Option<Instant>,
    /// The last fetch failed, then.
    failed: Option<Instant>,
    /// The weather was fetched then, in this run (for its age where the
    /// time of day is not known).
    fetched_at: Option<Instant>,
    /// A widget was shown then.
    seen: Option<Instant>,
}

pub(crate) type Shared = Rc<RefCell<State>>;

impl State {
    fn load(&mut self, env: &Env<'_>) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        self.weather = paperpaw_sdk::read_file(env.storage(), &weather_file(), 4 * 1024)
            .ok()
            .and_then(|bytes| data::Weather::from_text(&String::from_utf8_lossy(&bytes)));
    }

    /// The weather kept for the place set, if any.
    pub(crate) fn weather(&mut self, env: &Env<'_>) -> Option<&data::Weather> {
        self.load(env);
        let coordinates = env.settings().read(&COORDINATES);
        self.weather
            .as_ref()
            .filter(|w| w.coordinates == coordinates.trim())
    }

    /// How old the weather is, in seconds, if that is known.
    pub(crate) fn age(&mut self, env: &Env<'_>) -> Option<i64> {
        let fetched = self.weather(env).map_or(0, |w| w.fetched);
        match (time::unix_seconds(env), fetched) {
            (Some(now), fetched) if fetched > 0 => Some(now - fetched),
            _ => self
                .fetched_at
                .map(|at| (env.now().since(at) / 1000) as i64),
        }
    }
}

/// What a widget tells while it has no weather to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Waiting {
    /// No place is set.
    NoPlace,
    /// Fetching it.
    Fetching,
    /// It could not be fetched.
    Failed,
    /// It comes once there is a network.
    NoNetwork,
}

impl Waiting {
    pub(crate) fn key(self) -> &'static str {
        match self {
            Waiting::NoPlace => "weather.no-place",
            Waiting::Fetching => "weather.fetching",
            Waiting::Failed => "weather.failed",
            Waiting::NoNetwork => "weather.no-network",
        }
    }
}

/// What the widgets show: the weather, or why there is none, and the
/// place's name and the weather's age.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Shown {
    pub weather: Option<data::Weather>,
    pub waiting: Waiting,
    pub place: String,
    /// Its age, in seconds, when it is old (see `data::is_old`).
    pub old: Option<i64>,
    /// Today, in days since 1970, if the device knows it.
    pub today: Option<i32>,
    pub generation: u32,
}

impl Shown {
    /// What the widgets show now; the state notes that a widget is shown.
    pub(crate) fn read(env: &Env<'_>, state: &Shared) -> Self {
        let mut state = state.borrow_mut();
        state.seen = Some(env.now());
        let weather = state.weather(env).cloned();
        let age = state.age(env);
        let place = env.settings().read(&PLACE).trim().to_string();
        let waiting = if data::split_coordinates(env.settings().read(&COORDINATES)).is_none() {
            Waiting::NoPlace
        } else if state.asked.is_some() {
            Waiting::Fetching
        } else if state.failed.is_some() {
            Waiting::Failed
        } else if online(env) {
            Waiting::Fetching
        } else {
            Waiting::NoNetwork
        };
        // Its age, in whole minutes, so that the minutes between changes
        // draw nothing anew.
        let old = age
            .filter(|&age| weather.is_some() && data::is_old(age, every(env)))
            .map(|age| age / 60 * 60);
        let today =
            time::local_time(env).map(|t| data::days_from_civil(t.year, t.month, t.day) as i32);
        Shown {
            weather: if waiting == Waiting::NoPlace {
                None
            } else {
                weather
            },
            waiting,
            place,
            old,
            today,
            generation: state.generation,
        }
    }

    /// The age as text: "3 h ago", "2 days ago".
    pub(crate) fn age_text(&self, env: &Env<'_>) -> Option<String> {
        let age = self.old?;
        let hours = age / 3600;
        Some(if hours < 48 {
            env.i18n().plural("weather.age.hours", hours, &[])
        } else {
            env.i18n().plural("weather.age.days", hours / 24, &[])
        })
    }
}

/// The unit of temperatures chosen.
pub(crate) fn temperature_unit(env: &Env<'_>) -> Temperature {
    if env.settings().read(&TEMPERATURE) == TEMPERATURES[1] {
        Temperature::Fahrenheit
    } else {
        Temperature::Celsius
    }
}

/// The unit of wind speeds chosen.
pub(crate) fn wind_unit(env: &Env<'_>) -> Wind {
    match WINDS.iter().position(|w| *w == env.settings().read(&WIND)) {
        Some(1) => Wind::Mph,
        Some(2) => Wind::Ms,
        Some(3) => Wind::Knots,
        _ => Wind::Kmh,
    }
}

/// Seconds between fetches at most.
fn every(env: &Env<'_>) -> i64 {
    env.settings()
        .read(&EVERY)
        .rsplit('.')
        .next()
        .and_then(|h| h.parse::<i64>().ok())
        .unwrap_or(1)
        .clamp(1, 24)
        * 3600
}

/// Whether the network can be used now: where PaperPaw manages Wi-Fi,
/// only while it is in a network (the radio is not switched on for the
/// weather).
fn online(env: &Env<'_>) -> bool {
    match env.wifi() {
        Some(wifi) => wifi.state.is_online(),
        None => env.caps().network,
    }
}

/// Whether a weather widget shows in any place of widgets (the home
/// screen's pages, the sleep screen).
fn placed(env: &Env<'_>) -> bool {
    WIDGETS
        .iter()
        .any(|widget| env.widgets().shown(env, widget))
}

/// Why the weather is looked at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Why {
    /// The timer, a start, the network: fetched if due.
    Due,
    /// The reader asked for it.
    Asked,
}

/// Fetches the weather if `why` calls for it, and sets the timer for the
/// next look.
fn check(env: &Env<'_>, state: &Shared, why: Why, out: &mut Commands) {
    let coordinates = env.settings().read(&COORDINATES).trim().to_string();
    let Some(url) = data::forecast_url(&coordinates) else {
        return;
    };
    let mut state = state.borrow_mut();
    // Longer while saving; in Offline nothing by itself.
    let base = every(env);
    let every = match env.power().every(base as u64) {
        Some(every) => every as i64,
        None if why == Why::Due => return,
        None => base,
    };
    let age = state.age(env);
    let new_place = state.weather(env).is_none();
    let next = data::until_due(age, every, 5 * 60);
    // The next look: once due, when the radio is on anyway; nothing
    // wakes the device or the radio for the weather.
    let look = paperpaw_sdk::work::Work::network(TICK.id().clone())
        .optional()
        .after(core::time::Duration::from_secs(next as u64))
        .at_moments(paperpaw_sdk::work::Moments::WAKE | paperpaw_sdk::work::Moments::ONLINE);
    out.push(Command::work(look));
    if state.asked.is_some_and(|at| env.now().since(at) < PATIENCE) {
        return;
    }
    if why == Why::Due {
        let shown = state.seen.is_some_and(|at| env.now().since(at) < SEEN);
        let failed_lately = state
            .failed
            .is_some_and(|at| (env.now().since(at) / 1000) < RETRY as u64);
        if !(placed(env) || shown)
            || !online(env)
            || failed_lately
            || !(new_place || data::due(age, every))
        {
            return;
        }
    }
    let mut request = HttpRequest::new("GET", &url, FETCHED)
        .header(
            String::from("User-Agent"),
            String::from("PaperPaw/0.1 (e-reader)"),
        )
        .with(Params::new().with(params::ID, coordinates));
    if why == Why::Asked {
        request = request.param(TOLD, "1");
        out.push(Command::Notify(
            env.i18n().tr("weather.fetching").to_string(),
        ));
    }
    state.asked = Some(env.now());
    state.generation = state.generation.wrapping_add(1);
    out.push(Command::http(request));
}

/// The forecast answered: kept, and shown by the widgets.
fn fetched(env: &Env<'_>, state: &Shared, reply: &HttpReply, out: &mut Commands) {
    let mut state = state.borrow_mut();
    state.asked = None;
    state.generation = state.generation.wrapping_add(1);
    let coordinates = reply.context.get(params::ID).unwrap_or("").to_string();
    let fetched = time::unix_seconds(env).unwrap_or(0);
    let weather = reply
        .ok()
        .then(|| data::parse_forecast(&reply.body, &coordinates, fetched))
        .flatten();
    let told = reply.context.get(TOLD).is_some();
    // Shown at once, the new weather or that it failed.
    out.push(widgets::changed(&now::ID));
    out.push(widgets::changed(&forecast::ID));
    match weather {
        Some(weather) => {
            let _ = env.storage().create_dir(&Env::plugin_dir(PLUGIN.as_str()));
            let _ = env
                .storage()
                .write(&weather_file(), weather.to_text().as_bytes());
            state.weather = Some(weather);
            state.loaded = true;
            state.failed = None;
            state.fetched_at = Some(env.now());
        }
        None => {
            state.failed = Some(env.now());
            if told {
                out.push(Command::Notify(env.i18n().tr("weather.failed").to_string()));
            }
        }
    }
}

/// The place changed: the weather of another place goes.
fn place_changed(env: &Env<'_>, state: &Shared, out: &mut Commands) {
    {
        let mut s = state.borrow_mut();
        s.generation = s.generation.wrapping_add(1);
        s.failed = None;
        s.asked = None;
        if data::split_coordinates(env.settings().read(&COORDINATES)).is_none() {
            s.weather = None;
            s.fetched_at = None;
            let _ = env.storage().remove(&weather_file());
            return;
        }
    }
    check(env, state, Why::Asked, out);
}

/// The weather plugin.
#[derive(Debug, Default)]
pub struct Weather;

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

static ACTIONS: [FixedAction; 1] = [FixedAction::new(CHOOSE_PLACE, "weather.setting.place")
    .handler(|_, _, out| out.push(Command::OpenScreen(PLACE_SCREEN, Params::new())))];
static ICONS: [(Id, &str); 2] = [(GROUP, "sun"), (REFRESH, "refresh")];

static SCREENS: [FixedScreen; 1] = [FixedScreen::new(
    PLACE_SCREEN,
    "weather.setting.place",
    |env, _| Box::new(place::PlaceScreen::new(env)),
)];

impl Plugin for Weather {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "Weather", paperpaw_sdk::package_version!())
            .description("The weather now and the next days as widgets, from Open-Meteo.")
            .how(&HOW)
            .uses(paperpaw_sdk::Uses::NETWORK)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let state: Shared = Rc::new(RefCell::new(State::default()));
        // The data's licence asks for its source to be named.
        r.contribute(
            Id::new_static("about.lines"),
            "Weather data by Open-Meteo.com (https://open-meteo.com), under CC BY 4.0.",
        )?;
        r.add_setting_group({
            static SPEC: paperpaw_sdk::SettingGroupSpec =
                paperpaw_sdk::SettingGroupSpec::new(GROUP, "weather.title").order(88);
            &SPEC
        })?;
        r.add_icons(&ICONS)?;
        r.add_settings(&PLACE_SETTING)?;
        r.add_settings(&SETTINGS)?;
        r.add_screens(&SCREENS)?;
        r.add_actions(&ACTIONS)?;
        let s = state.clone();
        r.add_action(ActionSpec::new(REFRESH, "weather.refresh").handler(
            move |env: &Env<'_>, _, out: &mut Commands| {
                if data::split_coordinates(env.settings().read(&COORDINATES)).is_none() {
                    out.push(Command::OpenScreen(PLACE_SCREEN, Params::new()));
                } else {
                    check(env, &s, Why::Asked, out);
                }
            },
        ))?;

        // When to look: at the start, on waking, once in a network, by
        // the timer, and when a widget is placed or the place changes.
        for event in [events::STARTED, events::RESUME, TICK] {
            let s = state.clone();
            r.on(event, move |env, _, out| check(env, &s, Why::Due, out))?;
        }
        let s = state.clone();
        r.on(events::SETTING_CHANGED, move |env, changed, out| {
            let id = changed.id.as_str();
            if id == COORDINATES.as_str() {
                place_changed(env, &s, out);
            } else if id == EVERY.as_str() || env.settings().text(id).contains("weather.") {
                check(env, &s, Why::Due, out);
            }
        })?;
        let s = state.clone();
        r.on(FETCHED, move |env, reply, out| fetched(env, &s, reply, out))?;

        now::register(r, &state)?;
        forecast::register(r, &state)?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

/// What a tap on a widget does: choose the place if none is set, else
/// fetch the weather now.
pub(crate) fn tap(shown: &Shown) -> Command {
    if shown.waiting == Waiting::NoPlace {
        Command::OpenScreen(PLACE_SCREEN, Params::new())
    } else {
        Command::run(REFRESH)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_i18n::{Catalog, check};

    #[test]
    fn translations_are_complete() {
        let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
        let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
        let issues = check::compare(&en, &de);
        assert!(issues.is_empty(), "{issues:?}");
    }

    #[test]
    fn hours_between_fetches() {
        for value in EVERY_HOURS {
            let hours: i64 = value.rsplit('.').next().unwrap().parse().unwrap();
            assert!((1..=24).contains(&hours));
        }
    }
}
