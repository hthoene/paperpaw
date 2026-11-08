//! The screen lock: a PIN, a pattern or a password asked for when
//! PaperPaw starts and when the device wakes up, before anything else can
//! be used. Off by default.
//!
//! - Set up under Settings › System › *Screen lock*: the code in use is
//!   asked for first, then the kind (off, PIN, pattern, password) and the
//!   new code twice. Patterns are offered on touch screens.
//! - The code is kept as a salted hash among the passwords and keys,
//!   never as it is. Like those, it keeps the device from casual use, not
//!   from someone who takes its files.
//! - Locked, the lock screen takes every input: buttons, taps, swipes in
//!   from the edges. Only going to sleep, refreshing the screen and night
//!   mode pass, so a locked device still sleeps. Closed while locked
//!   (e.g. by a sleep screen closing as the device wakes), it opens again
//!   at once.
//! - After five wrong tries, the next one waits half a minute; the count
//!   is kept on the card, so starting anew does not skip the wait.
//! - The kind is changed on the lock's own page, which asks for the code
//!   in use first: changed elsewhere (a plain list of choices, *Reset all
//!   settings*), it goes back to the kind whose code is kept. With no code
//!   kept any more, the lock screen lets the device be used.
//! - *Lock now* (`lock.now`) locks at once, from any button, gesture,
//!   menu or rule.
//! - The touch lock ([`touch`]): taps and swipes do nothing while it is
//!   on, the buttons still work, e.g. to read with a hand on the screen.

#![no_std]

extern crate alloc;

mod code;
#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and the declaration test also use the builders"
)]
pub mod declarations;
mod native_entry;
#[doc(hidden)]
pub mod native_flow;
#[doc(hidden)]
pub mod native_model;
#[doc(hidden)]
pub mod native_pad;
/// The interface-7 screen lock (slice 1; see `PORTING-7.md`).
pub mod native_plugin;
mod screen;
pub mod touch;

#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;

#[cfg(test)]
mod native_parity_tests;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::string::String;
use core::cell::Cell;

use paperpaw_sdk::builtin::events;
use paperpaw_sdk::setting::{Setting, Text};
use paperpaw_sdk::{
    ActionSpec, Command, Commands, Env, Id, Instant, Manifest, Params, Plugin, RegisterError,
    Registrar, ScreenSpec, SettingSpec,
};

pub const PLUGIN: Id = Id::new_static("lock");
/// Its settings' group (Settings, Device, Screen lock).
pub const GROUP: Id = Id::new_static("lock.group");
/// Locks at once.
pub const NOW: Id = Id::new_static("lock.now");
/// The kind of lock: `lock.kind.off`, `…pin`, `…pattern` or `…password`
/// (ids, as choices are, translated to their names).
pub const KIND: Setting<Text> = Setting::new("lock.kind");
/// Whether the lock is asked for after every wake-up (the default), or
/// only after a real start: power on or restart (`STARTED`; a wake-up is
/// `RESUME`).
pub const ON_WAKE: Setting<bool> = Setting::new("lock.on-wake");
const SCREEN: Id = Id::new_static("lock.screen");

/// Whether the device is locked and how unlocking went so far: shared by
/// the plugin's handlers and its screens, so that a lock screen made
/// anew (turned, reopened) goes on where the last one was.
#[derive(Debug, Default)]
pub(crate) struct State {
    pub(crate) locked: Cell<bool>,
    /// Wrong tries in a row; [`screen::TRIES`] while a wait is due.
    pub(crate) failures: Cell<u8>,
    /// Until when the next try waits.
    pub(crate) wait_until: Cell<Option<Instant>>,
}

pub(crate) type Shared = Rc<State>;

/// The wrong tries in a row, kept across starts (a number).
const TRIES_FILE: &str = "tries.txt";

fn tries_path() -> String {
    alloc::format!("{}/{TRIES_FILE}", Env::plugin_dir(PLUGIN.as_str()))
}

/// The wrong tries in a row as the card keeps them.
pub(crate) fn read_tries(storage: &dyn paperpaw_sdk::Storage) -> u8 {
    paperpaw_sdk::read_file(storage, &tries_path(), 16)
        .ok()
        .and_then(|n| String::from_utf8(n).ok()?.trim().parse::<u8>().ok())
        .unwrap_or(0)
}

/// Keeps `tries` on the card (none: the file goes). Not kept, the count
/// still holds until PaperPaw starts anew; nothing to tell.
pub(crate) fn keep_tries(storage: &dyn paperpaw_sdk::Storage, tries: u8) {
    if tries == 0 {
        if storage.exists(&tries_path()) {
            let _ = storage.remove(&tries_path());
        }
    } else {
        let _ = storage.write(&tries_path(), alloc::format!("{tries}").as_bytes());
    }
}

/// The kinds; the setting stores them after [`KIND_PREFIX`].
pub(crate) const KINDS: [&str; 4] = ["off", "pin", "pattern", "password"];
pub(crate) const KIND_PREFIX: &str = "lock.kind.";
/// The kinds as the setting stores them: [`KIND_PREFIX`] and the kind.
const KIND_VALUES: [&str; KINDS.len()] = [
    "lock.kind.off",
    "lock.kind.pin",
    "lock.kind.pattern",
    "lock.kind.password",
];

/// Opens the lock screen, if a lock is set and it is not open yet.
fn lock(env: &Env<'_>, state: &State, out: &mut Commands) {
    if state.locked.get() || code::active(env).is_none() {
        return;
    }
    state.locked.set(true);
    out.push(Command::OpenScreen(
        SCREEN,
        Params::new().with(screen::MODE, screen::UNLOCK),
    ));
}

/// The screen lock plugin.
#[derive(Debug, Default)]
pub struct Lock;

/// The kind, chosen (and its code set) on the lock's page.
static ON_PAGE: paperpaw_sdk::SettingExtra = paperpaw_sdk::SettingExtra::new().editor(SCREEN);

/// The kind of lock, and whether waking up asks for it.
static SETTINGS: [SettingSpec; 2] = [
    SettingSpec::choice(
        KIND.static_str(),
        "lock.setting.kind",
        &KIND_VALUES,
        KIND_VALUES[0],
        GROUP,
    )
    .description("lock.setting.kind.help")
    .guarded()
    .with_extra(&ON_PAGE),
    SettingSpec::switch(ON_WAKE.static_str(), "lock.setting.on-wake", true, GROUP)
        .description("lock.setting.on-wake.help")
        .guarded(),
];

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

static ICONS: [(Id, &str); 1] = [(NOW, "lock")];

impl Plugin for Lock {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "Screen lock", paperpaw_sdk::package_version!())
            .description(
                "A PIN, a pattern or a password when the device wakes up; touch locked while reading.",
            )
            .how(&HOW)
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let state: Shared = Rc::new(State::default());
        let shared = state.clone();
        r.add_screen(ScreenSpec::new(
            SCREEN,
            "lock.setting.kind",
            Box::new(move |env, params| {
                Box::new(screen::LockScreen::new(env, params, shared.clone()))
            }),
        ))?;
        // Its settings on a page of their own (Settings, Device).
        r.add_setting_group({
            static SPEC: paperpaw_sdk::SettingGroupSpec =
                paperpaw_sdk::SettingGroupSpec::new(GROUP, "lock.plugin.name").order(92);
            &SPEC
        })?;
        r.add_icon(GROUP, Cow::Borrowed("lock"))?;
        r.add_settings(&SETTINGS)?;
        touch::register(r, state.clone())?;
        let now = state.clone();
        // Sealed: no other plugin's handler may lock in its place.
        r.add_action(
            ActionSpec::new(NOW, "lock.now")
                .handler(move |env: &Env<'_>, _, out: &mut Commands| {
                    lock(env, &now, out);
                })
                .sealed(),
        )?;
        r.add_icons(&ICONS)?;
        let start = state.clone();
        r.on(events::STARTED, move |env, _, out| {
            // A wait due when PaperPaw stopped is waited anew.
            let tries = read_tries(env.storage());
            start.failures.set(tries);
            if tries >= screen::TRIES {
                start
                    .wait_until
                    .set(Some(env.now().add_millis(screen::WAIT_MS)));
            }
            lock(env, &start, out);
        })?;
        r.on(events::RESUME, move |env, _, out| {
            if env.settings().read(&ON_WAKE) {
                lock(env, &state, out);
            }
        })?;
        // The kind goes with the code kept: changed without it, back.
        r.on(events::SETTING_CHANGED, |env, changed, out| {
            if changed.id != *KIND.id() {
                return;
            }
            let Some(kept) = code::kept_kind(env) else {
                return;
            };
            let value = alloc::format!("{KIND_PREFIX}{kept}");
            if env.settings().read(&KIND) != value {
                out.push(Command::set(&KIND, value.into()));
                out.push(Command::Notify(env.i18n().tr("lock.on-its-page").into()));
            }
        })?;
        // The first-run setup: a lock, off unless chosen.
        r.add_setup_step(
            paperpaw_sdk::setup::SetupStepSpec::new(
                Id::new_static("lock.setup"),
                "",
                paperpaw_sdk::setup::SetupPage::Setting(KIND.into()),
            )
            .order(70),
        )?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use paperpaw_i18n::{Catalog, check};

    /// The setting's values are the kinds after the prefix.
    #[test]
    fn kind_values_follow_kinds() {
        for (value, kind) in super::KIND_VALUES.iter().zip(super::KINDS) {
            assert_eq!(*value, alloc::format!("{}{kind}", super::KIND_PREFIX));
        }
    }

    /// What version 1 wrote stays readable (see `docs/data-files.md`).
    #[test]
    fn the_tries_of_version_1_are_read() {
        let storage = paperpaw_sdk::MemoryStorage::new().with(
            &super::tries_path(),
            include_str!("../testdata/tries.txt.v1"),
        );
        assert_eq!(super::read_tries(&storage), 3);
    }

    #[test]
    fn translations_are_complete() {
        let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
        let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
        let issues = check::compare(&en, &de);
        assert!(issues.is_empty(), "{issues:?}");
    }
}
