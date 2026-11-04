//! Power: which power mode is worn (ADR 0025). The reader chooses
//! Normal, Saving, Offline, or Automatic, which goes by the battery:
//! Saving below one level, Offline below another, Normal while charging.
//! The modes themselves are presets of the kind
//! [`MODE_KIND`](paperpaw_sdk::power::MODE_KIND), worn by the kernel
//! ([`Command::Wear`]); this plugin only decides which, silently.
//!
//! - *Power* in the quick panel: the four as segments, by buttons and by
//!   touch alike.
//! - Settings › System › Power: the mode, Automatic's two levels and
//!   *Normal while charging*.
//! - A status item, *Power mode*: a leaf while saving, Wi-Fi crossed out
//!   while offline; nothing in Normal, so the lines stay calm.

#![no_std]

extern crate alloc;

use alloc::borrow::Cow;
use alloc::boxed::Box;

use paperpaw_sdk::builtin::{events, menus};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::power::{self, PowerLevel, Wear};
use paperpaw_sdk::setting::{Setting, Text};
use paperpaw_sdk::ui::choice::Picked;
use paperpaw_sdk::{
    Command, Env, Id, Manifest, MenuEntry, Plugin, RegisterError, Registrar, SettingSpec,
    SettingValue, StatusItemSpec, StatusValue,
};

pub const PLUGIN: Id = Id::new_static("power");
const GROUP: Id = Id::new_static("power.group");
/// A charge, in percent.
static PERCENT: paperpaw_sdk::SettingExtra =
    paperpaw_sdk::SettingExtra::new().unit("core.unit.percent");
/// Opens the power settings: what a tap on the mode's status item runs.
pub const OPEN: Id = Id::new_static("power.open");
/// Which mode: one of `MODES`.
pub const MODE: Setting<Text> = Setting::new("power.mode");
/// Automatic: Saving below this percentage of charge.
pub const SAVING_BELOW: Setting<i64> = Setting::new("power.saving-below");
/// Automatic: Offline below this percentage.
pub const OFFLINE_BELOW: Setting<i64> = Setting::new("power.offline-below");
/// Automatic: Normal while charging.
pub const NORMAL_WHILE_CHARGING: Setting<bool> = Setting::new("power.normal-while-charging");
const STATUS: Id = Id::new_static("power.status");
/// Opens the power menu: what the power button held runs.
pub const MENU: Id = Id::new_static("power.menu");
/// The option chosen in the power menu.
const CHOSEN: Event<Picked> = Event::new("power.menu.chosen");
/// The power button held.
const HELD: &str = "[keymap.global]\nlong.power = power.menu\n";
/// The touch lock's switch and action (the lock plugin's, which may be
/// off or absent: the menu then has no entry for it).
const TOUCH_LOCK: &str = "lock.touch";
const TOUCH_TOGGLE: Id = Id::new_static("lock.touch-toggle");

/// The choices of [`MODE`], translation keys too.
const AUTOMATIC: &str = "power.mode.auto";
const MODES: [&str; 4] = [
    AUTOMATIC,
    "power.mode.normal",
    "power.mode.saving",
    "power.mode.offline",
];
/// Automatic changes back only this far above a level, so that it does
/// not go to and fro.
const MARGIN: i64 = 5;

static SETTINGS: [SettingSpec; 4] = [
    SettingSpec::choice(
        MODE.static_str(),
        "power.setting.mode",
        &MODES,
        AUTOMATIC,
        GROUP,
    )
    .description("power.setting.mode.help"),
    SettingSpec::number(
        SAVING_BELOW.static_str(),
        "power.setting.saving-below",
        5,
        50,
        20,
        GROUP,
    )
    .with_extra(&PERCENT)
    .description("power.setting.saving-below.help"),
    SettingSpec::number(
        OFFLINE_BELOW.static_str(),
        "power.setting.offline-below",
        0,
        30,
        10,
        GROUP,
    )
    .with_extra(&PERCENT)
    .description("power.setting.offline-below.help"),
    SettingSpec::switch(
        NORMAL_WHILE_CHARGING.static_str(),
        "power.setting.normal-while-charging",
        true,
        GROUP,
    )
    .description("power.setting.normal-while-charging.help"),
];

/// The mode to wear now, and whether the battery chose it.
pub fn choose(env: &Env<'_>) -> (Id, bool) {
    let built_in = |level: PowerLevel| match level {
        PowerLevel::Normal => power::NORMAL,
        PowerLevel::Saving => power::SAVING,
        PowerLevel::Offline => power::OFFLINE,
        // A level to come: everything as configured.
        _ => power::NORMAL,
    };
    let chosen = env.settings().read(&MODE);
    let level = match chosen {
        "power.mode.normal" => Some(PowerLevel::Normal),
        "power.mode.saving" => Some(PowerLevel::Saving),
        "power.mode.offline" => Some(PowerLevel::Offline),
        _ => None,
    };
    if let Some(level) = level {
        return (built_in(level), false);
    }
    (built_in(automatic(env)), true)
}

/// What Automatic chooses by the battery: Normal without one.
fn automatic(env: &Env<'_>) -> PowerLevel {
    let Some(battery) = env.status().battery else {
        return PowerLevel::Normal;
    };
    if battery.charging && env.settings().read(&NORMAL_WHILE_CHARGING) {
        return PowerLevel::Normal;
    }
    // The system's own saver (Android's) on: Saving at least.
    let least = if battery.saver {
        PowerLevel::Saving
    } else {
        PowerLevel::Normal
    };
    let percent = i64::from(battery.percent);
    // Wearing one already: kept until the charge is clearly above it.
    let now = env.power().level;
    let below = |level: &Setting<i64>, current: PowerLevel| {
        let margin = if now >= current { MARGIN } else { 0 };
        percent < env.settings().read(level) + margin
    };
    if below(&OFFLINE_BELOW, PowerLevel::Offline) {
        PowerLevel::Offline
    } else if below(&SAVING_BELOW, PowerLevel::Saving) {
        PowerLevel::Saving
    } else {
        least
    }
}

/// Wears the mode chosen now, if another is worn.
fn wear(env: &Env<'_>, out: &mut paperpaw_sdk::Commands) {
    let (mode, automatic) = choose(env);
    if env.power().mode == mode && env.power().automatic == automatic {
        return;
    }
    let wear = Wear::new(mode);
    out.push(Command::Wear(if automatic {
        wear.automatic()
    } else {
        wear
    }));
}

#[derive(Debug)]
pub struct Power;

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

static ACTIONS: [paperpaw_sdk::FixedAction; 2] = [
    paperpaw_sdk::FixedAction::new(OPEN, "power.title").handler(|_, _, out| {
        let page =
            paperpaw_sdk::Params::new().with(paperpaw_sdk::builtin::params::ID, GROUP.as_str());
        out.push(paperpaw_sdk::Command::OpenSlot(
            paperpaw_sdk::builtin::slots::SETTINGS,
            page,
        ));
    }),
    paperpaw_sdk::FixedAction::new(MENU, "power.menu.title").handler(|env, _, out| {
        let title = env.i18n().tr("power.menu.title");
        out.push(
            paperpaw_sdk::ask::choice(title, &CHOSEN)
                .options("power.menu.", &menu_options(env))
                .command(),
        );
    }),
];

/// What the power menu offers here: sleep and restart always, switching
/// off where PaperPaw does so, and on a touch device (with the touch
/// lock about) locking touch or, while it is locked, unlocking it:
/// the buttons work meanwhile, so the menu is the way out.
pub fn menu_options(env: &Env<'_>) -> alloc::vec::Vec<&'static str> {
    let mut options = alloc::vec!["sleep"];
    if env.caps().power_off {
        options.push("off");
    }
    options.push("restart");
    if env.device().input.touch
        && let Some(SettingValue::Bool(locked)) = env.settings().get(TOUCH_LOCK)
    {
        options.push(if *locked { "unlock" } else { "lock" });
    }
    options
}

/// What the option chosen in the power menu does. Sleeping and switching
/// off show the sleep screen first, as the power button's short press does:
/// e-ink keeps it, so a device that is off does not look as if it were on
/// (#146).
fn chosen(env: &Env<'_>, option: &str, out: &mut paperpaw_sdk::Commands) {
    let sleep_screen = env.has_slot(&paperpaw_sdk::builtin::slots::SLEEP);
    let (sleeps, switches_off) = (env.caps().sleep, env.caps().power_off);
    match option {
        "sleep" => {
            if sleep_screen && sleeps {
                out.push(Command::open_slot(paperpaw_sdk::builtin::slots::SLEEP));
            }
            out.push(Command::Sleep);
        }
        "off" => {
            if sleep_screen && switches_off {
                // `off`: the sleep screen's parameter (plugin `sleep`).
                out.push(Command::OpenSlot(
                    paperpaw_sdk::builtin::slots::SLEEP,
                    paperpaw_sdk::Params::new().with("off", "1"),
                ));
            }
            out.push(Command::PowerOff);
        }
        "restart" => out.push(Command::Restart),
        "lock" | "unlock" => out.push(Command::run(TOUCH_TOGGLE)),
        _ => {}
    }
}
/// The mode in the quick panel, as segments.
static ENTRIES: [MenuEntry; 1] =
    [MenuEntry::new(menus::QUICK, Id::new_static(MODE.static_str())).order(30)];
static ICONS: [(Id, &str); 2] = [(GROUP, "leaf"), (OPEN, "leaf")];

impl Plugin for Power {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "Power", paperpaw_sdk::package_version!())
            .description(
                "Chooses the power mode, by hand or by the battery: Normal, Saving, Offline.",
            )
            .how(&HOW)
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_setting_group({
            static SPEC: paperpaw_sdk::SettingGroupSpec =
                paperpaw_sdk::SettingGroupSpec::new(GROUP, "power.title").order(86);
            &SPEC
        })?;
        r.add_icons(&ICONS)?;
        r.add_settings(&SETTINGS)?;
        r.add_menu_entries(&ENTRIES)?;
        // Tapped, the power settings.
        r.add_status_item(
            StatusItemSpec::new(
                STATUS,
                "power.status.item",
                Box::new(|env, _| {
                    let icon = match env.power().level {
                        PowerLevel::Normal => return None,
                        PowerLevel::Saving => "leaf",
                        PowerLevel::Offline => "wifi-off",
                        _ => return None,
                    };
                    Some(StatusValue::Icon(Cow::Borrowed(icon)))
                }),
            )
            .tap(OPEN),
        )?;
        r.add_actions(&ACTIONS)?;
        r.add_keymap(HELD)?;
        r.on(CHOSEN, |env, picked, out| chosen(env, &picked.option, out))?;
        // Silently: the mode changes, only the status mark says so.
        for event in [
            events::STARTED.id(),
            events::STATUS_CHANGED.id(),
            events::RESUME.id(),
        ] {
            r.subscribe(event.clone(), Box::new(|env, _, out| wear(env, out)))?;
        }
        r.on(events::SETTING_CHANGED, |env, changed, out| {
            if changed.id.namespace() == PLUGIN.as_str() {
                wear(env, out);
            }
        })?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}
