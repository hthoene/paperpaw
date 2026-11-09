//! Reading goal: the daily goal of the reading statistics (`stats.goal`,
//! 20 minutes at first where this plugin is), and a cat whose mood follows
//! how much of it was read today:
//! sad while nothing was, meh after a little, content from half the goal
//! (`reading-goal.content`, in percent), happy once it is reached
//! (`reading-goal.happy`).
//!
//! - *Reading cat* (`reading-goal.widget`), a widget for the home screen
//!   and the sleep screen's widgets: the cat of today's mood, as large as
//!   its place leaves, and below it the minutes read and the goal (part
//!   `minutes`). A tap opens the goal's setting; where the statistics are
//!   off it offers to switch them on, where they are missing to get them
//!   from the store.
//! - A sleep screen of its own (*Today's reading cat* among the sleep
//!   screen's choices): the cat large in the middle, the minutes and the
//!   goal below it.
//!
//! The pictures are PaperPaw's cats (see `NOTICE`), or one's own: the
//! folder `reading-goal.pictures` names holds `sad`, `meh`, `content` and
//! `happy` (`.png`, `.jpg`), any size; those missing are the cats'. A data
//! plugin brings a set of its own as such a folder and names it in its
//! `settings.ini` (`[reading-goal] pictures = plugins/<id>/cats`).
//!
//! Today's reading comes from the reading statistics (the `stats`
//! plugin), which tell it as they record it ([`events::READ_TODAY`]);
//! it is kept in `today.txt` (`<day> <seconds>`, version 1), while a book
//! is open in memory only, written when it closes or the device sleeps or
//! stops: not at every page turned. Without
//! them, nothing is counted, and the widget says so. Without a goal (set
//! to none), the cat goes by [`DEFAULT_GOAL`] minutes and only the
//! minutes read show.

// The heap word is assembly (`native_plugin!`); Xtensa still gates it.
#![cfg_attr(target_arch = "xtensa", feature(asm_experimental_arch))]
#![no_std]

extern crate alloc;

#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and the declaration test also use the builders"
)]
mod declarations;
mod native_entry;
#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;
/// The interface-7 plugin.
pub mod native_plugin;

pub mod model;
mod native_widget;
pub mod store;

mod mood;
mod pictures;
mod sleep;
mod widget;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::String;
use core::cell::Cell;

use paperpaw_sdk::builtin::{events, slots};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::setting::{Setting, Text};
use paperpaw_sdk::ui::choice::Picked;
use paperpaw_sdk::{
    Command, Env, Id, Manifest, Plugin, RegisterError, Registrar, ScreenSpec, SettingExtra,
    SettingSpec, Storage,
};

pub use mood::Mood;

pub const PLUGIN: Id = Id::new_static("reading-goal");
/// The daily goal: the reading statistics' setting, `stats.goal.<minutes>`
/// or `stats.goal.off`.
pub const GOAL: &str = "stats.goal";
/// The goal where this plugin is: 20 minutes.
const GOAL_DEFAULT: &str = "stats.goal.20";
/// The minutes the cat goes by without a goal.
pub const DEFAULT_GOAL: u32 = 20;
/// The reading statistics' plugin.
const STATS: Id = Id::new_static("stats");
/// The reading statistics' settings, where the goal the cat follows is set.
const STATS_GROUP: Id = Id::new_static("stats.group");
/// The store's action that opens it.
const STORE: Id = Id::new_static("store.open");
/// The answer to the offer of the statistics.
pub(crate) const OFFERED: Event<Picked> = Event::new("reading-goal.offered");
/// Setting: from which share of the goal the cat is content, in percent.
pub const CONTENT: Setting<i64> = Setting::new("reading-goal.content");
/// Setting: from which share of the goal the cat is happy, in percent.
pub const HAPPY: Setting<i64> = Setting::new("reading-goal.happy");
/// Setting: a folder of one's own pictures for the moods (empty: the
/// cats).
pub const PICTURES: Setting<Text> = Setting::new("reading-goal.pictures");
/// The sleep screen's mode of this plugin (its screen).
pub const SLEEP_SCREEN: Id = Id::new_static(SLEEP_NAME);
const SLEEP_NAME: &str = "reading-goal.sleep";
/// What the sleep screen shows (the `sleep` plugin's setting).
const SLEEP_SHOW: Id = Id::new_static("sleep.show");

const SECTION: &str = "reading-goal.title";
static PERCENT: SettingExtra = SettingExtra::new()
    .section(SECTION)
    .unit("core.unit.percent")
    .step(5)
    .slider();
static FOLDER: SettingExtra = SettingExtra::new()
    .section(SECTION)
    .editor_slot(slots::CHOOSE_FOLDER);

static SETTINGS: [SettingSpec; 3] = [
    SettingSpec::number(
        CONTENT.static_str(),
        "reading-goal.setting.content",
        5,
        95,
        50,
        STATS_GROUP,
    )
    .advanced()
    .with_extra(&PERCENT)
    .description("reading-goal.setting.content.help"),
    SettingSpec::number(
        HAPPY.static_str(),
        "reading-goal.setting.happy",
        50,
        200,
        100,
        STATS_GROUP,
    )
    .advanced()
    .with_extra(&PERCENT)
    .description("reading-goal.setting.happy.help"),
    SettingSpec::text(
        PICTURES.static_str(),
        "reading-goal.setting.pictures",
        "",
        STATS_GROUP,
    )
    .advanced()
    .with_extra(&FOLDER)
    .description("reading-goal.setting.pictures.help"),
];

pub(crate) fn data(name: &str) -> String {
    format!("{}/{name}", Env::plugin_dir(PLUGIN.as_str()))
}

/// Whether the reading statistics count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Statistics {
    Counting,
    /// There, switched off.
    Off,
    /// Not there (not installed).
    Missing,
}

/// What is known of today's reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Today {
    /// The goal in minutes, if one is set.
    pub goal: Option<u32>,
    /// Minutes read today.
    pub minutes: u32,
    pub mood: Mood,
    pub statistics: Statistics,
}

impl Today {
    /// Today as the statistics told it last, with the settings.
    pub(crate) fn now(env: &Env<'_>, kept: &Kept) -> Today {
        // `stats.goal.30`: 30; `stats.goal.off`: none.
        let goal = env
            .settings()
            .text(GOAL)
            .rsplit('.')
            .next()
            .and_then(|m| m.parse::<u32>().ok())
            .filter(|&m| m > 0);
        let seconds = match (kept.told(env.storage()), today(env)) {
            (Some((day, seconds)), Some(today)) if day == today => seconds,
            _ => 0,
        };
        let content = env.settings().read(&CONTENT).clamp(1, 99) as u32;
        let happy = env.settings().read(&HAPPY).clamp(1, 1000) as u32;
        let statistics = match env.plugins().iter().find(|p| p.manifest.id == STATS) {
            Some(p) if p.enabled => Statistics::Counting,
            Some(_) => Statistics::Off,
            None => Statistics::Missing,
        };
        let by = goal.unwrap_or(DEFAULT_GOAL);
        Today {
            goal,
            minutes: seconds / 60,
            mood: Mood::of(seconds, by, content, happy),
            statistics,
        }
    }
}

/// Today: days since 1970 by the local time, if the device knows it.
fn today(env: &Env<'_>) -> Option<i32> {
    let unix = paperpaw_sdk::time::unix_seconds(env)?;
    Some(paperpaw_sdk::time::local_seconds(env, unix).div_euclid(86_400) as i32)
}

/// The day and the seconds the statistics told last (`today.txt`).
fn told(storage: &dyn Storage) -> Option<(i32, u32)> {
    let text = paperpaw_sdk::read_file(storage, &data("today.txt"), 64).ok()?;
    let text = core::str::from_utf8(&text).ok()?;
    let (day, seconds) = text.trim().split_once(' ')?;
    Some((day.parse().ok()?, seconds.parse().ok()?))
}

/// What the statistics told last of today, and whether a book is open:
/// then it is kept here and written to `today.txt` only later.
#[derive(Debug, Default)]
pub(crate) struct Kept {
    /// `None` until they told something since the start.
    told: Cell<Option<(i32, u32)>>,
    reading: Cell<bool>,
}

impl Kept {
    /// What the statistics told last: kept here, else in `today.txt`.
    pub(crate) fn told(&self, storage: &dyn Storage) -> Option<(i32, u32)> {
        self.told.get().or_else(|| told(storage))
    }

    /// Writes what was told, where `today.txt` holds something else.
    fn write(&self, storage: &dyn Storage) {
        if let Some((day, seconds)) = self.told.get()
            && told(storage) != Some((day, seconds))
        {
            let text = format!("{day} {seconds}\n");
            let _ = storage.write(&data("today.txt"), text.as_bytes());
        }
    }
}

/// The reading goal plugin.
#[derive(Debug, Default)]
pub struct ReadingGoal;

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

impl Plugin for ReadingGoal {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "Reading goal", paperpaw_sdk::package_version!())
            .description(
                "Minutes to read a day, and a cat on the home screen whose mood follows today's reading.",
            )
            .how(&HOW)
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_settings(&SETTINGS)?;
        // One daily goal, the statistics': 20 minutes where this plugin is.
        r.set_setting_default(Id::new_static(GOAL), Cow::Borrowed(GOAL_DEFAULT))?;
        r.on(OFFERED, |_, picked, out| match picked.option.as_str() {
            // Switched on, at the next start: started anew at once.
            "switch-on" => {
                out.push(Command::SetPluginEnabled(STATS, true));
                out.push(Command::Restart);
            }
            "store" => out.push(Command::run(STORE)),
            _ => {}
        })?;
        let kept = Rc::new(Kept::default());
        {
            let kept = Rc::clone(&kept);
            r.on(events::READ_TODAY, move |env, read, _| {
                kept.told.set(Some((read.day, read.seconds)));
                // Not while pages turn: the card is slow to write to.
                if !kept.reading.get() {
                    kept.write(env.storage());
                }
            })?;
        }
        {
            let kept = Rc::clone(&kept);
            r.on(events::BOOK_OPENED, move |_, _, _| kept.reading.set(true))?;
        }
        {
            let kept = Rc::clone(&kept);
            r.on(events::BOOK_CLOSED, move |env, _, _| {
                kept.reading.set(false);
                kept.write(env.storage());
            })?;
        }
        // Asleep or restarting the device may lose its power.
        for event in [events::SUSPEND, events::STOPPING] {
            let kept = Rc::clone(&kept);
            r.on(event, move |env, _, _| kept.write(env.storage()))?;
        }
        widget::register(r, &kept)?;
        let screen = ScreenSpec::new(
            SLEEP_SCREEN,
            "reading-goal.sleep",
            Box::new(move |env, _| Box::new(sleep::GoalSleep::new(env, &kept))),
        );
        r.add_screen(screen)?;
        r.add_choice(SLEEP_SHOW, SLEEP_NAME)?;
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
