//! Battery use: records the battery while the device is awake, with what
//! it did meanwhile (screen refreshes, input, pages turned), and shows how
//! much it uses an hour awake and for 100 pages, and how long it lasts.
//!
//! The record is a log on the device's storage (see its `log` module), kept for
//! analysis on a computer too: where the battery goes is what to make
//! leaner next.

#![no_std]

extern crate alloc;

mod log;
mod view;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::rc::Rc;
use core::cell::RefCell;

use paperpaw_sdk::builtin::{events, groups, menus};
use paperpaw_sdk::{
    Command, Env, FixedAction, Id, Manifest, MenuEntry, Params, Plugin, RegisterError, Registrar,
    ScreenSpec, SettingSpec, time,
};

use log::{Kind, Reading};
use paperpaw_sdk::setting::Setting;

pub const PLUGIN: Id = Id::new_static("battery");
/// Records the battery's use.
pub const ENABLED: Setting<bool> = Setting::new("battery.enabled");
/// Shows what each plugin took (for developers).
pub const BY_PLUGIN: Setting<bool> = Setting::new("battery.by-plugin");
/// Opens the view.
pub const OPEN: Id = Id::new_static("battery.open");
const SCREEN: Id = Id::new_static("battery.screen");

/// What is counted between readings.
#[derive(Debug, Default)]
struct State {
    /// Pages turned since start-up.
    pages: u32,
    /// A time awake was started in the log.
    awake: bool,
    /// When what each plugin took was last noted.
    plugins_noted: Option<u64>,
}

type Shared = Rc<RefCell<State>>;

/// The battery and what was done, now.
fn reading(env: &Env<'_>, state: &State) -> Reading {
    let battery = env.status().battery;
    Reading {
        ms: env.now().millis(),
        millivolts: battery.and_then(|b| b.millivolts),
        percent: battery.map(|b| b.percent),
        charging: battery.is_some_and(|b| b.charging),
        usage: log::Counts::of(env.usage()),
        pages: state.pages,
        unix: time::unix_seconds(env),
    }
}

fn dir() -> alloc::string::String {
    Env::plugin_dir(PLUGIN.as_str())
}

/// Appends `text` to the log `name` (with `header` first), starting a new
/// one when it is full. A log that cannot become the old one (the old
/// one in the way) starts anew all the same: it never grows past
/// [`log::MAX_BYTES`] by much. Lines not written are lost, nothing else:
/// the log is a record, not the user's data.
fn append(env: &Env<'_>, name: &str, old: &str, header: &str, text: &str) {
    let storage = env.storage();
    let path = format!("{}/{name}", dir());
    match storage.size(&path) {
        Ok(size) if size >= log::MAX_BYTES => {
            let old = format!("{}/{old}", dir());
            if storage.rename(&path, &old).is_err() {
                let _ = storage.remove(&old);
                if storage.rename(&path, &old).is_err() {
                    let _ = storage.remove(&path);
                }
            }
            let _ = storage.append(&path, header.as_bytes());
        }
        Ok(_) => {}
        Err(_) => {
            let _ = storage.append(&path, header.as_bytes());
        }
    }
    let _ = storage.append(&path, text.as_bytes());
}

/// Adds a line to the log, and what each plugin took when due.
fn record(env: &Env<'_>, state: &mut State, kind: Kind) {
    if !env.settings().read(&ENABLED) {
        return;
    }
    // Awake already: a start that ends a sleep is told twice (it started,
    // and the device woke), recorded once.
    if kind == Kind::Start && state.awake {
        return;
    }
    let now = reading(env, state);
    append(
        env,
        log::FILE,
        log::OLD,
        log::HEADER,
        &log::line(kind, &now),
    );
    let due = state
        .plugins_noted
        .is_none_or(|at| now.ms.saturating_sub(at) >= log::PLUGINS_EVERY_MS);
    if kind == Kind::End || (kind == Kind::Sample && due) {
        state.plugins_noted = Some(now.ms);
        append(
            env,
            log::PLUGINS,
            log::PLUGINS_OLD,
            log::PLUGINS_HEADER,
            &log::plugin_lines(now.ms, &env.plugin_usage()),
        );
        // On the card every ten minutes and at sleep or a restart: an
        // empty battery ends it all without warning, and a write to the
        // card each minute wears it for little.
        let _ = env.storage().sync();
    }
    state.awake = kind != Kind::End;
}

/// The battery use plugin.
#[derive(Debug, Default)]
pub struct Battery;

/// Whether the battery is recorded, and by plugin.
static SETTINGS: [SettingSpec; 2] = [
    SettingSpec::switch(
        ENABLED.static_str(),
        "battery.setting.enabled",
        true,
        // With the power settings (Settings, Device, Power).
        Id::new_static("power.group"),
    )
    .description("battery.setting.enabled.help"),
    SettingSpec::switch(
        BY_PLUGIN.static_str(),
        "battery.setting.by-plugin",
        true,
        groups::DEVELOPER,
    )
    .description("battery.setting.by-plugin.help"),
];

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

static ACTIONS: [FixedAction; 1] = [FixedAction::new(OPEN, "battery.title")
    .handler(|_, _, out| out.push(Command::OpenScreen(SCREEN, Params::new())))];
static ENTRIES: [MenuEntry; 1] = [MenuEntry::new(menus::TOOLS, OPEN).order(37)];
static ICONS: [(Id, &str); 1] = [(OPEN, "battery")];

impl Plugin for Battery {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "Battery use", paperpaw_sdk::package_version!())
            .description("Records the battery and shows how long it lasts.")
            .how(&HOW)
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let state: Shared = Rc::new(RefCell::new(State::default()));
        r.add_settings(&SETTINGS)?;

        // Recording: a time awake from start-up or waking to sleep or a
        // restart.
        for (event, kind) in [
            (events::STARTED, Kind::Start),
            (events::RESUME, Kind::Start),
            (events::SUSPEND, Kind::End),
            (events::STOPPING, Kind::End),
        ] {
            let s = state.clone();
            r.subscribe(
                event,
                Box::new(move |env, _, _| record(env, &mut s.borrow_mut(), kind)),
            )?;
        }
        let s = state.clone();
        r.on(events::BATTERY_MEASURED, move |env, _, _| {
            let mut state = s.borrow_mut();
            if !state.awake {
                record(env, &mut state, Kind::Start);
            }
            record(env, &mut state, Kind::Sample);
        })?;
        let s = state.clone();
        r.subscribe(
            events::PAGE_TURNED,
            Box::new(move |_, _, _| {
                let mut state = s.borrow_mut();
                state.pages = state.pages.saturating_add(1);
            }),
        )?;

        // The view.
        let s = state.clone();
        r.add_screen(
            ScreenSpec::new(
                SCREEN,
                "battery.title",
                Box::new(move |env, _| {
                    let now = reading(env, &s.borrow());
                    Box::new(view::BatteryView::new(env, now, &dir()))
                }),
            )
            .instances(paperpaw_sdk::Instances::SINGLE),
        )?;
        r.add_actions(&ACTIONS)?;
        r.add_menu_entries(&ENTRIES)?;
        r.add_icons(&ICONS)?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use paperpaw_i18n::{Catalog, check};

    #[test]
    fn translations_are_complete() {
        let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
        let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
        let issues = check::compare(&en, &de);
        assert!(issues.is_empty(), "{issues:?}");
    }
}
