//! First-time help: the hints plugins name for their screens (see
//! [`paperpaw_sdk::help`]), shown over a screen the first time it opens
//! on this device, once: its zones framed with what a tap there does, the
//! rest in a box. A tap or a button closes them; *No more hints* switches
//! them all off (`help.show`, on at first). *Show hints again*, in the
//! system settings, has each shown once more.
//!
//! The screens shown are kept in `seen.txt`, a screen's id a line
//! (version 1).

#![no_std]

extern crate alloc;

mod overlay;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;

use paperpaw_sdk::builtin::{events, groups, params};
use paperpaw_sdk::help;
use paperpaw_sdk::setting::Setting;
use paperpaw_sdk::{
    Command, Commands, Env, FixedScreen, Id, Manifest, Params, Plugin, RegisterError, Registrar,
    SettingExtra, SettingSpec, Storage, StorageError,
};

pub const PLUGIN: Id = Id::new_static("help");
/// Setting: hints show the first time a screen opens.
pub const SHOW: Setting<bool> = Setting::new("help.show");
/// The hints over a screen; parameter `id` names it.
pub const OVERLAY: Id = Id::new_static("help.overlay");
/// What *Show hints again* opens: it has them shown again, and says so.
const AGAIN: Id = Id::new_static("help.again");

const SECTION: &str = "help.section";
static IN_HELP: SettingExtra = SettingExtra::new().section(SECTION);
static AGAIN_EXTRA: SettingExtra = SettingExtra::new().section(SECTION).editor(AGAIN);

static SETTINGS: [SettingSpec; 2] = [
    SettingSpec::switch(SHOW.static_str(), "help.setting.show", true, groups::SYSTEM)
        .with_extra(&IN_HELP)
        .description("help.setting.show.help"),
    SettingSpec::text("help.again", "help.setting.again", "", groups::SYSTEM)
        .with_extra(&AGAIN_EXTRA)
        .description("help.setting.again.help"),
];

fn seen_file() -> String {
    format!("{}/seen.txt", Env::plugin_dir(PLUGIN.as_str()))
}

/// Whether the hints of `screen` were shown on this device. Where what
/// was shown cannot be read (the memory card taken out, #145), they count
/// as shown: a card PaperPaw cannot read is not one it never saw.
pub(crate) fn seen(storage: &dyn Storage, screen: &str) -> bool {
    match paperpaw_sdk::read_file(storage, &seen_file(), 64 * 1024) {
        Ok(text) => {
            core::str::from_utf8(&text).is_ok_and(|t| t.lines().any(|l| l.trim() == screen))
        }
        Err(error) => error != StorageError::NotFound,
    }
}

/// The hints of `screen` were shown; whether that is kept.
pub(crate) fn mark_seen(storage: &dyn Storage, screen: &str) -> bool {
    seen(storage, screen)
        || storage
            .append(&seen_file(), format!("{screen}\n").as_bytes())
            .is_ok()
}

/// The hints of `screen` count as not shown: they were covered before
/// the reader could read them.
pub(crate) fn forget_seen(storage: &dyn Storage, screen: &str) {
    let Ok(text) = paperpaw_sdk::read_file(storage, &seen_file(), 64 * 1024) else {
        return;
    };
    let text = String::from_utf8_lossy(&text);
    let kept: String = text
        .lines()
        .filter(|line| line.trim() != screen)
        .map(|line| format!("{line}\n"))
        .collect();
    let _ = storage.write(&seen_file(), kept.as_bytes());
}

/// The plugin.
#[derive(Debug, Default)]
pub struct Help;

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

static SCREENS: [FixedScreen; 2] = [
    FixedScreen::new(OVERLAY, "help.title", |env, params| {
        Box::new(overlay::Overlay::new(env, params))
    }),
    FixedScreen::new(AGAIN, "help.setting.again", |env, _| {
        Box::new(overlay::Again::new(env))
    }),
];

impl Plugin for Help {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "Hints", paperpaw_sdk::package_version!())
            .description("Shows what a screen's controls do, the first time it opens.")
            .how(&HOW)
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_settings(&SETTINGS)?;
        r.on(events::SCREEN_SHOWN, |env, shown, out| {
            covered(env, &shown.screen, out);
            if !help::waits(env, shown.screen.as_str()) {
                explain(env, &shown.screen, out);
            }
        })?;
        r.on(help::READY, |env, ready, out| {
            explain(env, &ready.screen, out)
        })?;
        r.add_screens(&SCREENS)?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

/// Opens the hints over `screen`, where it has some, they are on, they
/// were not shown yet and the first-run setup is not under way: a screen
/// passed in its course (the quick panel opened by a held button) shows
/// them the first time it opens after it.
fn explain(env: &Env<'_>, screen: &Id, out: &mut Commands) {
    let in_setup = env
        .stack()
        .iter()
        .any(|entry| entry.slot.as_ref() == Some(&paperpaw_sdk::builtin::slots::SETUP));
    if !env.settings().read(&SHOW)
        || in_setup
        || screen.namespace() == PLUGIN.as_str()
        || help::hints(env, screen.as_str()).is_empty()
        || seen(env.storage(), screen.as_str())
    {
        return;
    }
    let params = Params::new().with(params::ID, screen.as_str());
    out.push(Command::OpenScreen(OVERLAY, params));
}

/// Closes the hints that another screen, `shown`, now covers (a
/// plugin's question, a notice opened over them): what they explain is
/// hidden, so they show the next time instead.
fn covered(env: &Env<'_>, shown: &Id, out: &mut Commands) {
    if *shown == OVERLAY {
        return;
    }
    for (index, entry) in env.stack().iter().enumerate() {
        if entry.screen != OVERLAY {
            continue;
        }
        if let Some(below) = index.checked_sub(1).and_then(|i| env.stack().get(i)) {
            forget_seen(env.storage(), below.screen.as_str());
        }
        out.push(Command::Close(entry.key));
    }
}

/// Has every screen's hints shown again.
pub(crate) fn again(storage: &dyn Storage) {
    let _ = storage.remove(&seen_file());
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
