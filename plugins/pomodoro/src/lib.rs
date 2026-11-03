//! Pomodoro: a focus timer with work and break rounds.
//!
//! - One state (phase, end time, rounds, the day's counts) is shared by
//!   every part and kept in a small file, so a round survives a restart.
//! - A timer (`Work::at`) ends the phase exactly on time, whatever screen
//!   is open or none, says so with a notice and, as set, a flash or a
//!   blink of the front light, and may start the next phase by itself.
//! - *Pomodoro* (the Tools menu): a large countdown, a progress bar, the
//!   dots of the set and Start/Pause, Skip and Reset. It redraws once a
//!   minute (every ten seconds in the last one), never more.
//! - Widgets for the home screen (and every screen showing widgets):
//!   *Pomodoro timer* (phase and time left, a tap starts or pauses),
//!   *Pomodoro button* (Start/Pause as a button) and *Pomodoro today*
//!   (finished rounds, the daily goal, the week). They read the state
//!   when it changes and each minute while the timer runs; none holds the
//!   device awake.
//! - A status item shows the minutes left in any status line.
//!
//! Works with the settings as they come: 25, 5 and 15 minutes, four
//! rounds. Advanced: auto-start, the end signal, a daily goal and keeping
//! the screen awake during a round.

#![no_std]

extern crate alloc;

mod phase;
mod screen;
mod signal;
mod state;
mod widget;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{events, menus};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::setting::Setting;
use paperpaw_sdk::{
    ActionSpec, Command, Commands, Env, Id, Manifest, MenuEntry, Params, Plugin, RegisterError,
    Registrar, ScreenSpec, SettingExtra, SettingGroupSpec, SettingSpec, StatusItemSpec,
    StatusValue,
};

pub use phase::{Lengths, Log, Phase};
pub use state::State;

use state::Shared;

pub const PLUGIN: Id = Id::new_static("pomodoro");
pub const OPEN: Id = Id::new_static("pomodoro.open");
/// Starts or pauses the timer.
pub const TOGGLE: Id = Id::new_static("pomodoro.toggle");
/// Skips to the next phase.
pub const SKIP: Id = Id::new_static("pomodoro.skip-action");
pub const SCREEN: Id = Id::new_static("pomodoro.screen");
/// A phase is over: its timer.
pub(crate) const DONE: Event<()> = Event::new("pomodoro.done");
const GROUP: Id = Id::new_static("pomodoro.group");

pub const FOCUS_MIN: Setting<i64> = Setting::new("pomodoro.focus");
pub const SHORT_MIN: Setting<i64> = Setting::new("pomodoro.short");
pub const LONG_MIN: Setting<i64> = Setting::new("pomodoro.long");
pub const ROUNDS: Setting<i64> = Setting::new("pomodoro.rounds");
/// Start the next phase by itself.
pub const AUTO: Setting<bool> = Setting::new("pomodoro.auto");
/// The end signal: one of [`signal::SIGNALS`].
pub const SIGNAL: Setting<paperpaw_sdk::setting::Text> = Setting::new("pomodoro.signal");
/// Rounds a day to reach; 0 for no goal.
pub const GOAL: Setting<i64> = Setting::new("pomodoro.goal");
/// Keeps the screen on while a round runs and its screen is open.
pub const AWAKE: Setting<bool> = Setting::new("pomodoro.awake");

const SECTION: &str = "pomodoro.section.more";
static MORE: SettingExtra = SettingExtra::new().section(SECTION);

static SETTINGS: [SettingSpec; 8] = [
    SettingSpec::number(
        FOCUS_MIN.static_str(),
        "pomodoro.setting.focus",
        1,
        90,
        25,
        GROUP,
    )
    .with_extra(&UNIT_MINUTES)
    .description("pomodoro.setting.focus.help"),
    SettingSpec::number(
        SHORT_MIN.static_str(),
        "pomodoro.setting.short",
        1,
        30,
        5,
        GROUP,
    )
    .with_extra(&UNIT_MINUTES)
    .description("pomodoro.setting.short.help"),
    SettingSpec::number(
        LONG_MIN.static_str(),
        "pomodoro.setting.long",
        1,
        60,
        15,
        GROUP,
    )
    .with_extra(&UNIT_MINUTES)
    .description("pomodoro.setting.long.help"),
    SettingSpec::number(
        ROUNDS.static_str(),
        "pomodoro.setting.rounds",
        2,
        8,
        4,
        GROUP,
    )
    .description("pomodoro.setting.rounds.help"),
    SettingSpec::switch(AUTO.static_str(), "pomodoro.setting.auto", false, GROUP)
        .description("pomodoro.setting.auto.help"),
    SettingSpec::choice(
        SIGNAL.static_str(),
        "pomodoro.setting.signal",
        &signal::SIGNALS,
        signal::NOTICE,
        GROUP,
    )
    .advanced()
    .with_extra(&MORE)
    .description("pomodoro.setting.signal.help"),
    SettingSpec::number(GOAL.static_str(), "pomodoro.setting.goal", 0, 24, 0, GROUP)
        .advanced()
        .with_extra(&MORE)
        .description("pomodoro.setting.goal.help"),
    SettingSpec::switch(AWAKE.static_str(), "pomodoro.setting.awake", false, GROUP)
        .advanced()
        .with_extra(&MORE)
        .description("pomodoro.setting.awake.help"),
];

static UNIT_MINUTES: SettingExtra = SettingExtra::new().unit("core.unit.minutes");

/// The lengths the settings say.
pub fn lengths(env: &Env<'_>) -> Lengths {
    let s = env.settings();
    let read = |setting: &Setting<i64>, most: i64| s.read(setting).clamp(1, most) as u32;
    Lengths {
        focus: read(&FOCUS_MIN, 90),
        short: read(&SHORT_MIN, 30),
        long: read(&LONG_MIN, 60),
        rounds: read(&ROUNDS, 8),
    }
}

/// The daily goal in rounds, if one is set.
pub fn goal(env: &Env<'_>) -> Option<u32> {
    u32::try_from(env.settings().read(&GOAL))
        .ok()
        .filter(|&g| g > 0)
}

/// Says that what the widgets show changed.
pub(crate) fn widgets_changed() -> [Command; 3] {
    [widget::TIMER, widget::BUTTON, widget::TODAY].map(|id| paperpaw_sdk::widgets::changed(&id))
}

fn push_all(out: &mut Commands, cmds: impl IntoIterator<Item = Command>) {
    for c in cmds {
        out.push(c);
    }
}

/// The notice for a phase that just ended, and the commands of its end
/// signal.
fn finished(env: &Env<'_>, was: Phase, st: &State, out: &mut Vec<Command>) {
    let i18n = env.i18n();
    let reached = was == Phase::Focus && goal(env) == Some(st.log.today());
    let notice = match (was, reached) {
        (Phase::Focus, true) => i18n.plural(
            "pomodoro.notice.goal",
            i64::from(st.log.today()),
            &[("count", paperpaw_sdk::Arg::Int(i64::from(st.log.today())))],
        ),
        (Phase::Focus, false) => String::from(i18n.tr("pomodoro.notice.break")),
        _ => String::from(i18n.tr("pomodoro.notice.focus")),
    };
    out.push(Command::Notify(notice));
    signal::give(env, env.settings().text(SIGNAL.as_str()), out);
}

/// The Pomodoro plugin.
#[derive(Debug, Default)]
pub struct Pomodoro;

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

impl Plugin for Pomodoro {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "Pomodoro", paperpaw_sdk::package_version!())
            .description("A focus timer with work and break rounds, widgets and a daily count.")
            .how(&HOW)
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let shared: Shared = Shared::default();

        // At start-up and on waking: go on where the saved state left off.
        for event in [events::STARTED, events::RESUME] {
            let s = shared.clone();
            r.on(event, move |env, _, out| {
                let mut cmds = Vec::new();
                s.borrow_mut().load(env, &mut cmds);
                cmds.extend(widgets_changed());
                push_all(out, cmds);
            })?;
        }
        for event in [events::SUSPEND, events::STOPPING] {
            let s = shared.clone();
            r.on(event, move |env, _, _| s.borrow().save(env))?;
        }

        r.add_action(
            ActionSpec::new(OPEN, "pomodoro.title").handler(|_, _, out| {
                out.push(Command::OpenScreen(SCREEN, Params::new()));
            }),
        )?;
        r.add_menu_entry(MenuEntry::new(menus::TOOLS, OPEN).order(80))?;
        r.add_icon(OPEN, Cow::Borrowed("clock"))?;

        let s = shared.clone();
        r.add_action(ActionSpec::new(TOGGLE, "pomodoro.action.toggle").handler(
            move |env: &Env<'_>, _, out: &mut Commands| {
                let mut cmds = Vec::new();
                s.borrow_mut().toggle(env, &mut cmds);
                cmds.extend(widgets_changed());
                push_all(out, cmds);
            },
        ))?;
        let s = shared.clone();
        r.add_action(ActionSpec::new(SKIP, "pomodoro.action.skip").handler(
            move |env: &Env<'_>, _, out: &mut Commands| {
                s.borrow_mut().advance(env, false);
                out.push(Command::CancelWork(DONE.id().clone(), None));
                push_all(out, widgets_changed());
            },
        ))?;

        let made = shared.clone();
        r.add_screen(ScreenSpec::new(
            SCREEN,
            "pomodoro.title",
            Box::new(move |env, _| Box::new(screen::PomodoroScreen::new(env, made.clone()))),
        ))?;

        // The time is up: tell the reader, go on to the next phase.
        let s = shared.clone();
        r.on(DONE, move |env, _, out| {
            let mut st = s.borrow_mut();
            if !st.running {
                return;
            }
            let was = st.phase;
            st.advance(env, true);
            let mut cmds = Vec::new();
            finished(env, was, &st, &mut cmds);
            // The next phase starts by itself only when asked.
            if env.settings().read(&AUTO) {
                st.start(env, &mut cmds);
            }
            cmds.extend(widgets_changed());
            push_all(out, cmds);
        })?;
        r.on(signal::BLINK_BACK, |_, params, out| {
            if let Some(command) = signal::back(params) {
                out.push(command);
            }
        })?;

        r.add_setting_group({
            static SPEC: SettingGroupSpec =
                SettingGroupSpec::new(GROUP, "pomodoro.group").order(90);
            &SPEC
        })?;
        r.add_icon(GROUP, Cow::Borrowed("clock"))?;
        r.add_settings(&SETTINGS)?;
        widget::register(r, &shared)?;

        // Minutes left, in any status line.
        let s = shared.clone();
        r.add_status_item(
            StatusItemSpec::new(
                Id::new_static("pomodoro.status"),
                "pomodoro.status",
                Box::new(move |env, _| {
                    let st = s.borrow();
                    if !st.running {
                        return None;
                    }
                    let whole = lengths(env).ms(st.phase);
                    let mins = phase::minutes_left(st.remaining(env.now(), whole));
                    Some(StatusValue::Text(format!("{mins} min")))
                }),
            )
            .tap(OPEN),
        )?;

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
