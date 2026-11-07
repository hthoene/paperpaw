//! Rules: when something happens, PaperPaw does something. For example
//! "every day at 22:00: night mode on", "on weekdays at 7:00: sync the
//! catalogs", "when the battery falls below 20 %: the light off", "when
//! the battery reaches 80 %, only while charging: a message to unplug"
//! (where the device cannot stop charging by itself).
//!
//! - When: PaperPaw starts; at a time of day (every day, on weekdays, at
//!   weekends or on one day); every so many minutes; a book is opened,
//!   closed or read to its end; the device wakes up or goes to sleep; the
//!   battery falls below a percentage or reaches one (once, until it fell
//!   clearly below again); charging begins or ends; a switch
//!   (any on/off setting) is turned on or off.
//! - Then: run any action (from any plugin), turn any switch on or off,
//!   set a number (e.g. the light) or show a message.
//! - Only if (optional): between two times of day, while reading or not,
//!   on battery or while charging, while a switch is on or off.
//!
//! The rules are listed under Settings › Customise › Rules: tapping one
//! pauses it, holding it offers a condition or to remove it, *Add rule*
//! asks when and what. Times
//! come as work at a time (`Work::at`, see `paperpaw_sdk::work`), so
//! they apply whatever screen is open; they are set again when the clock
//! becomes known or the time zone changes. Rules that repeat count the
//! minutes from the start, clock or not.
//!
//! Rules set off by a setting another rule changed follow each other at
//! most [`MAX_CHAIN`] deep: two rules that undo each other stop there.
//! The rules are read once and kept; where the file cannot be read, none
//! apply and the list says so instead of saving over it.

#![no_std]

extern crate alloc;

#[cfg(test)]
extern crate std;
/// Test-only: refuses chosen allocations to prove the fallible paths.
#[cfg(test)]
#[path = "../../test-support/fail_alloc.rs"]
mod fail_alloc;
#[cfg(test)]
#[global_allocator]
static ALLOC: fail_alloc::Failing = fail_alloc::Failing;

mod native_data;
mod native_entry;
pub mod native_plugin;
pub mod native_rules;
#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;
mod rule;
mod screens;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};

use paperpaw_sdk::builtin::{events, menus, settings};
use paperpaw_sdk::work::Work;
use paperpaw_sdk::{
    Arg, Command, Commands, Env, FixedAction, FixedScreen, Id, Manifest, MenuEntry, Params, Plugin,
    RegisterError, Registrar, ScreenSpec, SettingValue, time,
};

use crate::rule::{Condition, EVERY_DAY, Effect, Levels, Rule, Store, Trigger, WEEKDAYS, WEEKENDS};

pub const PLUGIN: Id = Id::new_static("rules");
/// Opens the list of rules.
pub const OPEN: Id = Id::new_static("rules.open");
const LIST_SCREEN: Id = Id::new_static("rules.list");
const PICK_SCREEN: Id = Id::new_static("rules.pick");
/// The timer of the rules with a time of day; parameter [`MINUTE`].
const TIME: Id = Id::new_static("rules.time");
const MINUTE: &str = "minute";
/// The timer of the rules that repeat: each minute while there are some.
const TICK: Id = Id::new_static("rules.tick");

/// The names of the days, Sunday first.
pub(crate) const DAY_NAMES: [&str; 7] = [
    "rules.days.0",
    "rules.days.1",
    "rules.days.2",
    "rules.days.3",
    "rules.days.4",
    "rules.days.5",
    "rules.days.6",
];
/// How many rules may follow each other through the settings they
/// change: rules that undo each other stop there.
pub const MAX_CHAIN: u8 = 4;

/// What the handlers and the list of rules share.
#[derive(Debug, Default)]
pub(crate) struct State {
    pub(crate) store: Store,
    /// Whether a book is open, as the reading events tell.
    reading: Cell<bool>,
    /// The settings rules changed, not yet heard of, each with how many
    /// rules in a row led to the change.
    chain: RefCell<Vec<(Id, u8)>>,
}

pub(crate) type Shared = Rc<State>;

impl State {
    /// Runs the rules whose trigger `applies` and whose condition holds;
    /// `depth` rules in a row led here.
    fn run(
        &self,
        env: &Env<'_>,
        depth: u8,
        out: &mut Commands,
        applies: impl Fn(&Trigger) -> bool,
    ) {
        let reading = self.reading.get();
        self.store.with(env.storage(), |rules| {
            for rule in rules {
                if !rule.on || !applies(&rule.trigger) || !holds(env, &rule.condition, reading) {
                    continue;
                }
                let (id, value) = match &rule.effect {
                    Effect::Action(action) => {
                        out.push(Command::run(action.clone()));
                        continue;
                    }
                    Effect::Message(text) => {
                        out.push(Command::Announce(text.clone()));
                        continue;
                    }
                    Effect::Switch(id, on) => (id, SettingValue::Bool(*on)),
                    Effect::Number(id, value) => (id, SettingValue::Int(*value)),
                };
                if env.settings().get(id.as_str()) != Some(&value) {
                    self.caused(id, depth.saturating_add(1));
                    out.push(Command::SetSetting(id.clone(), value));
                }
            }
        });
    }

    /// Setting `id` is changed by the `depth`-th rule in a row.
    fn caused(&self, id: &Id, depth: u8) {
        let mut chain = self.chain.borrow_mut();
        chain.retain(|(other, _)| other != id);
        chain.push((id.clone(), depth));
    }

    /// How many rules in a row led to setting `id` changing: none where
    /// the change is not a rule's.
    fn depth_of(&self, id: &Id) -> u8 {
        let mut chain = self.chain.borrow_mut();
        let found = chain.iter().position(|(other, _)| other == id);
        found.map_or(0, |at| chain.remove(at).1)
    }
}

/// Whether `condition` holds now.
fn holds(env: &Env<'_>, condition: &Condition, reading: bool) -> bool {
    let switch = |id: &Id| env.settings().get(id.as_str()) == Some(&SettingValue::Bool(true));
    let charging = env.status().battery.map(|b| b.charging);
    match condition {
        Condition::Always => true,
        Condition::Between { from, to } => time::local_time(env).is_some_and(|t| {
            rule::between(u16::from(t.hour) * 60 + u16::from(t.minute), *from, *to)
        }),
        Condition::Reading => reading,
        Condition::NotReading => !reading,
        Condition::OnBattery => charging == Some(false),
        Condition::Charging => charging == Some(true),
        Condition::SwitchOn(id) => switch(id),
        Condition::SwitchOff(id) => !switch(id),
    }
}

/// Sets the timers: to the next time of day a rule waits for, and to the
/// next minute while rules repeat.
pub(crate) fn schedule(env: &Env<'_>, store: &Store, out: &mut Commands) {
    schedule_time(env, store, out);
    schedule_tick(env, store, out);
}

/// Sets the timer to the next time of day a rule waits for, if the clock
/// is known.
fn schedule_time(env: &Env<'_>, store: &Store, out: &mut Commands) {
    let Some(now) = time::local_time(env) else {
        return;
    };
    let minute_now = u16::from(now.hour) * 60 + u16::from(now.minute);
    let next = store.with(env.storage(), |rules| {
        rules
            .iter()
            .filter(|r| r.on)
            .filter_map(|r| match r.trigger {
                Trigger::Time { minute, .. } => {
                    Some((rule::minutes_until(minute_now, minute), minute))
                }
                _ => None,
            })
            .min()
    });
    if let Some((ahead, at)) = next {
        // The start of that minute.
        let at_minute = env
            .now()
            .add_millis(u64::from(ahead) * 60_000 - u64::from(now.second) * 1000);
        out.push(Command::work(
            Work::at(TIME, at_minute).with(Params::new().with(MINUTE, at.to_string())),
        ));
    }
}

/// Sets the timer to a minute from now while rules repeat: they count
/// minutes from the start, so they need no clock.
fn schedule_tick(env: &Env<'_>, store: &Store, out: &mut Commands) {
    let repeats = store.with(env.storage(), |rules| {
        rules
            .iter()
            .any(|r| r.on && matches!(r.trigger, Trigger::Every(_)))
    });
    if repeats {
        out.push(Command::work(Work::at(TICK, env.now().add_millis(60_000))));
    }
}

/// The days of a time of day, in words: "every day", "Monday to Friday".
fn days_text(env: &Env<'_>, days: u8) -> String {
    let key = match days {
        EVERY_DAY => "rules.days.every",
        WEEKDAYS => "rules.days.weekdays",
        WEEKENDS => "rules.days.weekends",
        _ => {
            let names: Vec<&str> = (0..7)
                .filter(|d| days & (1 << d) != 0)
                .map(|d| env.i18n().tr(DAY_NAMES[d]))
                .collect();
            return names.join(", ");
        }
    };
    env.i18n().tr(key).to_string()
}

/// The name of a setting.
fn setting_title<'e>(env: &Env<'e>, id: &'e Id) -> &'e str {
    let title = env
        .settings()
        .specs()
        .find(|s| s.id == *id)
        .map_or(id.as_str(), |s| s.title.as_ref());
    env.i18n().tr(title)
}

/// When a rule applies, with its condition, in words.
fn when_text(env: &Env<'_>, rule: &Rule) -> String {
    let when = trigger_text(env, &rule.trigger);
    match condition_text(env, &rule.condition) {
        Some(condition) => env.i18n().format(
            "rules.when-if",
            &[
                ("when", Arg::Str(&when)),
                ("condition", Arg::Str(&condition)),
            ],
        ),
        None => when,
    }
}

/// A condition in words; `None` for always.
fn condition_text(env: &Env<'_>, condition: &Condition) -> Option<String> {
    let tr = |key: &str| env.i18n().tr(key).to_string();
    Some(match condition {
        Condition::Always => return None,
        Condition::Between { from, to } => env.i18n().format(
            "rules.if.between",
            &[
                (
                    "from",
                    Arg::Str(&env.i18n().localize_digits(&rule::clock(*from))),
                ),
                (
                    "to",
                    Arg::Str(&env.i18n().localize_digits(&rule::clock(*to))),
                ),
            ],
        ),
        Condition::Reading => tr("rules.if.reading"),
        Condition::NotReading => tr("rules.if.not-reading"),
        Condition::OnBattery => tr("rules.if.battery"),
        Condition::Charging => tr("rules.if.charging"),
        Condition::SwitchOn(id) => env.i18n().format(
            "rules.if.switch-on",
            &[("setting", Arg::Str(setting_title(env, id)))],
        ),
        Condition::SwitchOff(id) => env.i18n().format(
            "rules.if.switch-off",
            &[("setting", Arg::Str(setting_title(env, id)))],
        ),
    })
}

/// When a rule applies, in words.
fn trigger_text(env: &Env<'_>, trigger: &Trigger) -> String {
    let tr = |key: &str| env.i18n().tr(key).to_string();
    match trigger {
        Trigger::Start => tr("rules.when.start"),
        Trigger::Time { minute, days } => {
            let time = env
                .i18n()
                .localize_digits(&rule::clock(*minute))
                .into_owned();
            let days = days_text(env, *days);
            env.i18n().format(
                "rules.when.time",
                &[("time", Arg::Str(&time)), ("days", Arg::Str(&days))],
            )
        }
        Trigger::Every(minutes) => env.i18n().plural(
            "rules.when.every",
            i64::from(*minutes),
            &[("count", Arg::Int(i64::from(*minutes)))],
        ),
        Trigger::BookOpened => tr("rules.when.book"),
        Trigger::BookClosed => tr("rules.when.book-closed"),
        Trigger::BookFinished => tr("rules.when.book-finished"),
        Trigger::Resume => tr("rules.when.resume"),
        Trigger::Sleep => tr("rules.when.sleep"),
        Trigger::BatteryBelow(percent) => env.i18n().format(
            "rules.when.battery",
            &[("percent", Arg::Int(i64::from(*percent)))],
        ),
        Trigger::BatteryAtLeast(percent) => env.i18n().format(
            "rules.when.battery-at",
            &[("percent", Arg::Int(i64::from(*percent)))],
        ),
        Trigger::Charging => tr("rules.when.charging"),
        Trigger::Unplugged => tr("rules.when.unplugged"),
        Trigger::SwitchOn(id) => env.i18n().format(
            "rules.when.switch-on",
            &[("setting", Arg::Str(setting_title(env, id)))],
        ),
        Trigger::SwitchOff(id) => env.i18n().format(
            "rules.when.switch-off",
            &[("setting", Arg::Str(setting_title(env, id)))],
        ),
    }
}

/// What a rule does, in words.
fn effect_text(env: &Env<'_>, effect: &Effect) -> String {
    match effect {
        Effect::Action(id) => {
            let title = env
                .actions()
                .get(id.as_str())
                .map_or(id.as_str(), |a| a.title());
            env.i18n().tr(title).to_string()
        }
        Effect::Switch(id, on) => {
            let state = env.i18n().tr(if *on { "rules.on" } else { "rules.off" });
            env.i18n().format(
                "rules.do.switch",
                &[
                    ("setting", Arg::Str(setting_title(env, id))),
                    ("state", Arg::Str(state)),
                ],
            )
        }
        Effect::Number(id, value) => env.i18n().format(
            "rules.do.number",
            &[
                ("setting", Arg::Str(setting_title(env, id))),
                ("value", Arg::Int(*value)),
            ],
        ),
        Effect::Message(text) => env
            .i18n()
            .format("rules.do.message", &[("text", Arg::Str(text))]),
    }
}

/// The rules plugin.
#[derive(Debug, Default)]
pub struct Rules;

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

static ACTIONS: [FixedAction; 1] = [FixedAction::new(OPEN, "rules.title")
    .handler(|_, _, out| out.push(Command::OpenScreen(LIST_SCREEN, Params::new())))];
static ENTRIES: [MenuEntry; 1] = [MenuEntry::new(menus::SETTINGS, OPEN).order(230)];
static ICONS: [(Id, &str); 1] = [(OPEN, "automation")];

static SCREENS: [FixedScreen; 1] = [FixedScreen::new(
    PICK_SCREEN,
    "rules.title",
    |env, params| Box::new(screens::PickScreen::new(env, params)),
)];

impl Plugin for Rules {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "Rules", paperpaw_sdk::package_version!())
            .description("When something happens, does something: e.g. night mode at 22:00.")
            .how(&HOW)
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    #[expect(
        clippy::too_many_lines,
        reason = "longer than the limit from before it was set; split it when it is next changed"
    )]
    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let state: Shared = Rc::new(State::default());
        let shared = state.clone();
        r.add_screen(
            ScreenSpec::new(
                LIST_SCREEN,
                "rules.title",
                Box::new(move |env, _| Box::new(screens::RulesScreen::new(env, shared.clone()))),
            )
            .instances(paperpaw_sdk::Instances::SINGLE),
        )?;
        r.add_screens(&SCREENS)?;
        r.add_actions(&ACTIONS)?;
        r.add_menu_entries(&ENTRIES)?;
        r.add_icons(&ICONS)?;
        let started = state.clone();
        r.on(events::STARTED, move |env, _, out| {
            started.run(env, 0, out, |t| *t == Trigger::Start);
            schedule(env, &started.store, out);
        })?;
        for (event, trigger, open) in [
            (
                Id::from(events::BOOK_OPENED),
                Trigger::BookOpened,
                Some(true),
            ),
            (events::BOOK_CLOSED.into(), Trigger::BookClosed, Some(false)),
            (events::BOOK_FINISHED.into(), Trigger::BookFinished, None),
            (events::SUSPEND.into(), Trigger::Sleep, None),
        ] {
            let state = state.clone();
            r.subscribe(
                event,
                Box::new(move |env: &Env<'_>, _: &Params, out: &mut Commands| {
                    if let Some(open) = open {
                        state.reading.set(open);
                    }
                    state.run(env, 0, out, |t| *t == trigger);
                }),
            )?;
        }
        let resumed = state.clone();
        r.on(events::RESUME, move |env, _, out| {
            resumed.run(env, 0, out, |t| *t == Trigger::Resume);
            // The clock may have moved on while asleep.
            schedule_time(env, &resumed.store, out);
        })?;
        let timed = state.clone();
        r.subscribe(
            TIME,
            Box::new(move |env: &Env<'_>, params: &Params, out: &mut Commands| {
                let today = time::local_time(env).map_or(0, |t| t.weekday);
                if let Some(at) = params.get(MINUTE).and_then(|m| m.parse::<u16>().ok()) {
                    timed.run(env, 0, out, |t| {
                        matches!(t, Trigger::Time { minute, days }
                            if *minute == at && days & (1 << today) != 0)
                    });
                }
                schedule_time(env, &timed.store, out);
            }),
        )?;
        // Minutes since the start, for the rules that repeat.
        let minutes = Rc::new(Cell::new(0u32));
        let ticked = state.clone();
        r.subscribe(
            TICK,
            Box::new(move |env: &Env<'_>, _: &Params, out: &mut Commands| {
                let now = minutes.get() + 1;
                minutes.set(now);
                ticked.run(
                    env,
                    0,
                    out,
                    |t| matches!(t, Trigger::Every(every) if now.is_multiple_of(u32::from(*every))),
                );
                schedule_tick(env, &ticked.store, out);
            }),
        )?;
        // The battery as it was last, and the levels it has reached: a
        // charge falling below a mark or reaching one, and charging
        // beginning or ending, compare with it. The level is the best the
        // device knows: a fuel gauge's (X4 Pro, phones) or an estimate from
        // the voltage (X4), which reads high while charging.
        let battery: Rc<Cell<Option<(u8, bool, Levels)>>> = Rc::new(Cell::new(None));
        let status = state.clone();
        r.on(events::STATUS_CHANGED, move |env, _, out| {
            // The clock became known, was set or moved to another zone:
            // the time of day waited for comes at another moment.
            schedule_time(env, &status.store, out);
            let Some((percent, charging)) = env.status().battery.map(|b| (b.percent, b.charging))
            else {
                return;
            };
            let Some((was, was_charging, mut levels)) = battery.get() else {
                battery.set(Some((percent, charging, Levels::up_to(percent))));
                return;
            };
            let reached = levels.reach(percent);
            battery.set(Some((percent, charging, levels)));
            status.run(env, 0, out, |t| match t {
                Trigger::BatteryBelow(mark) => was >= *mark && percent < *mark,
                Trigger::BatteryAtLeast(mark) => reached.contains(*mark),
                Trigger::Charging => !was_charging && charging,
                Trigger::Unplugged => was_charging && !charging,
                _ => false,
            });
        })?;
        r.on(events::SETTING_CHANGED, move |env, changed, out| {
            let depth = state.depth_of(&changed.id);
            let id = changed.id.as_str();
            if id == settings::TIME_ZONE.as_str() || id == settings::TIME_ZONE_RULE.as_str() {
                schedule_time(env, &state.store, out);
                return;
            }
            let Some(SettingValue::Bool(on)) = env.settings().get(id) else {
                return;
            };
            // Rules undoing each other's changes stop after a few.
            if depth >= MAX_CHAIN {
                return;
            }
            let on = *on;
            state.run(env, depth, out, |t| match t {
                Trigger::SwitchOn(s) => on && s.as_str() == id,
                Trigger::SwitchOff(s) => !on && s.as_str() == id,
                _ => false,
            });
        })?;
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
