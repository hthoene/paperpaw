//! Firmware updates, on devices whose firmware PaperPaw is (the X4): a
//! page in the settings finds an update file in the folder `updates` on
//! the card, says what it is and who signed it, and has the shell install
//! it (see `paperpaw_sdk::firmware`). An update found at start is offered
//! once; after the restart the plugin says how it went and removes the
//! file. The page also asks the update server for a newer firmware and
//! downloads it into `updates` ([`online`]), and then the code plugins
//! built for it (`fetch`), which the updated firmware installs at its
//! first start.
//!
//! Updates signed by PaperPaw install at once; one's own builds, signed
//! with one's own key, once the reader accepted that key, shown by its
//! fingerprint, at each install. An older version installs only when the
//! reader chooses it, told it is older. An update without some of the
//! plugins built in now says which, and the reader chooses to update
//! without them or to wait. An update whose plugins have no builds for it
//! yet waits for them (module `waiting`). Where the reader chose it,
//! the device looks for updates by itself about once a week, at a cheap
//! moment (module `look`).

#![no_std]

extern crate alloc;

mod count;
mod fetch;
mod found;
mod look;
pub mod online;
mod page;
mod waiting;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;

use count::Trigger;
use paperpaw_sdk::builtin::{events, groups};
use paperpaw_sdk::firmware::Firmware;
use paperpaw_sdk::{
    Arg, Command, Commands, Env, FixedAction, FixedScreen, Id, ItemState, Manifest, MenuEntry,
    MenuNeed, Params, Plugin, RegisterError, Registrar, SettingSpec, Storage,
};

pub use look::LOOK;

pub const PLUGIN: Id = Id::new_static("update");
/// Its settings' group (Settings, Device, Updates).
pub const GROUP: Id = Id::new_static("update.group");
/// Opens the page.
pub const OPEN: Id = Id::new_static("update.open");
/// Asks the update server for an update and offers one it names (e.g.
/// from a rule); says nothing otherwise.
pub const CHECK: Id = Id::new_static("update.check");
const PAGE: Id = Id::new_static("update.page");
/// Declared work: a check no reader waits for (a rule's).
pub(crate) const CHECK_DUE: paperpaw_sdk::event::Event =
    paperpaw_sdk::event::Event::new("update.check-due");
/// The update being installed (its path and version), kept through the
/// restart to say how it went.
const INSTALLING: &str = "installing.txt";
/// The update offered at start last (its path and version; `online` and
/// the version for one named by the update server): offered once.
const OFFERED: &str = "offered.txt";

/// The update server's address, for developers and servers of one's own;
/// looking for updates by itself; betas.
static SETTINGS: [SettingSpec; 3] = [
    SettingSpec::text(
        online::SERVER.static_str(),
        "update.setting.server",
        online::DEFAULT_SERVER,
        groups::DEVELOPER,
    )
    .description("update.setting.server.help"),
    SettingSpec::switch(look::LOOK.static_str(), "update.setting.look", false, GROUP)
        .description("update.setting.look.help"),
    SettingSpec::switch(
        online::BETAS.static_str(),
        "update.setting.betas",
        false,
        GROUP,
    )
    .description("update.setting.betas.help"),
];

/// `version` as the update's texts show it: a beta as `0.5.0 Beta 1`,
/// calmly, and every other version as it is.
pub(crate) fn shown_version(i18n: &paperpaw_sdk::Translator, version: &str) -> String {
    match paperpaw_sdk::firmware::beta(version) {
        Some((release, number)) => i18n.format(
            "update.version.beta",
            &[("version", Arg::Str(release)), ("number", Arg::Str(number))],
        ),
        None => String::from(version),
    }
}

fn file(name: &str) -> String {
    format!("{}/{name}", Env::plugin_dir(PLUGIN.as_str()))
}

/// Two lines of a small file of the plugin's.
fn read_pair(storage: &dyn Storage, name: &str) -> Option<(String, String)> {
    let text = paperpaw_sdk::read_file(storage, &file(name), 4096).ok()?;
    let text = String::from_utf8(text).ok()?;
    let mut lines = text.lines();
    Some((String::from(lines.next()?), String::from(lines.next()?)))
}

pub(crate) fn write_pair(storage: &dyn Storage, name: &str, first: &str, second: &str) {
    let _ = storage.create_dir(&Env::plugin_dir(PLUGIN.as_str()));
    let _ = storage.write(&file(name), format!("{first}\n{second}\n").as_bytes());
}

/// At start: how the update installed before went, and an update to
/// offer. A download the restart cut off is let go.
fn started(env: &Env<'_>, firmware: &Firmware, out: &mut Commands) {
    let storage = env.storage();
    online::clear_download(storage);
    fetch::interrupted(storage, firmware);
    // An update named before and installed since (or older now): no
    // longer one.
    if online::has_offer(storage) && online::offer(storage, firmware).is_none() {
        online::clear_offer(storage);
    }
    if let Some((path, version)) = read_pair(storage, INSTALLING) {
        let _ = storage.remove(&file(INSTALLING));
        if !firmware.undone && version == firmware.version {
            let _ = storage.remove(&path);
            out.push(Command::Announce(env.i18n().format(
                "update.done",
                &[(
                    "version",
                    Arg::Str(&crate::shown_version(env.i18n(), &version)),
                )],
            )));
            return;
        }
    }
    if firmware.undone {
        out.push(Command::Announce(env.i18n().format(
            "update.undone",
            &[(
                "version",
                Arg::Str(&crate::shown_version(env.i18n(), &firmware.version)),
            )],
        )));
        return;
    }
    // An update waiting for plugins: asked for again where due; a week
    // unseen, the page opens once.
    if waiting::at_start(env, firmware, out) {
        out.push(Command::OpenScreen(PAGE, Params::new()));
        return;
    }
    // A newer update on the card, not offered before.
    if let Ok(update) = found::find(storage, firmware)
        && update.is_newer(firmware)
        && read_pair(storage, OFFERED) != Some((update.path.clone(), update.info.version.clone()))
    {
        write_pair(storage, OFFERED, &update.path, &update.info.version);
        out.push(Command::OpenScreen(PAGE, Params::new()));
    }
}

/// The update plugin.
#[derive(Debug, Default)]
pub struct Update;

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

static ACTIONS: [FixedAction; 2] = [
    FixedAction::new(OPEN, "update.title")
        .handler(|_, _, out| out.push(Command::OpenScreen(PAGE, Params::new()))),
    FixedAction::new(CHECK, "update.check").handler(|env, _, out| {
        if env.reader() {
            online::check(env, true, Trigger::Manual, out);
        } else {
            let due = CHECK_DUE.id().clone();
            out.push(Command::work(paperpaw_sdk::work::Work::when_cheap(
                due,
                core::time::Duration::from_secs(3600),
            )));
        }
    }),
];
/// The first row of the *Updates* settings, with its state.
static ENTRIES: [MenuEntry; 1] = [MenuEntry::new(GROUP, OPEN)];
static ICONS: [(Id, &str); 2] = [(CHECK, "cloud-download"), (OPEN, "circle-arrow-up")];

static SCREENS: [FixedScreen; 1] = [FixedScreen::new(PAGE, "update.title", |env, _| {
    Box::new(page::UpdatePage::new(env))
})
.instances(paperpaw_sdk::Instances::SINGLE)];

impl Plugin for Update {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "Update", paperpaw_sdk::package_version!())
            .description(
                "Installs firmware updates, from the card or downloaded from the internet.",
            )
            .how(&HOW)
            .uses(paperpaw_sdk::Uses::NETWORK)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        // Its settings on a page of their own (Settings, Device).
        r.add_setting_group({
            static SPEC: paperpaw_sdk::SettingGroupSpec =
                paperpaw_sdk::SettingGroupSpec::new(GROUP, "update.group").order(94);
            &SPEC
        })?;
        r.add_icon(GROUP, Cow::Borrowed("cloud-download"))?;
        r.set_menu_need(GROUP, MenuNeed::Firmware)?;
        r.add_settings(&SETTINGS)?;
        // Only where PaperPaw updates the firmware.
        r.set_menu_need(online::SERVER.id().clone(), MenuNeed::Firmware)?;
        r.add_screens(&SCREENS)?;
        r.add_actions(&ACTIONS)?;
        r.on(CHECK_DUE, |env, (), out| {
            online::check(env, true, Trigger::Auto, out);
        })?;
        // Looking by itself: declared at each start and when the setting
        // changes.
        r.on(look::LOOK_DUE, |env, (), out| look::due(env, out))?;
        r.on(events::SETTING_CHANGED, |env, changed, out| {
            if changed.id == *look::LOOK.id() {
                look::plan(env, out);
            }
        })?;
        r.set_menu_need(look::LOOK.id().clone(), MenuNeed::Firmware)?;
        // The first-run setup asks it, as the setting it is.
        r.add_setup_step(
            paperpaw_sdk::setup::SetupStepSpec::new(
                Id::new_static("update.setup"),
                "",
                paperpaw_sdk::setup::SetupPage::Setting(look::LOOK.id().clone()),
            )
            .order(57)
            .applies(alloc::boxed::Box::new(|env| env.firmware().is_some())),
        )?;
        r.set_menu_need(CHECK, MenuNeed::Firmware)?;
        r.add_icons(&ICONS)?;
        // Answers and downloads go on while the page is closed.
        r.on(online::CHECKED, online::checked)?;
        r.on(online::DOWNLOADED, online::downloaded)?;
        r.on(fetch::FETCHED, fetch::received)?;
        r.add_menu_entries(&ENTRIES)?;
        r.set_menu_need(OPEN, MenuNeed::Firmware)?;
        // An update the server named waits quietly: a badge on the entry.
        r.set_item_state(OPEN, |env| {
            if online::has_offer(env.storage()) {
                ItemState::new().badge(String::from(env.i18n().tr("update.badge")))
            } else {
                ItemState::new()
            }
        })?;
        r.on(events::STARTED, |env, _, out| {
            if let Some(firmware) = env.firmware() {
                started(env, firmware, out);
            }
            look::plan(env, out);
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

    /// What version 1 wrote stays readable (see `docs/data-files.md`).
    #[test]
    fn pairs_of_version_1_are_read() {
        let storage = paperpaw_sdk::MemoryStorage::new().with(
            &super::file(super::INSTALLING),
            include_str!("../testdata/installing.txt.v1"),
        );
        assert_eq!(
            super::read_pair(&storage, super::INSTALLING),
            Some((
                alloc::string::String::from("updates/paperpaw-x4-0.4.0.bin"),
                alloc::string::String::from("0.4.0")
            ))
        );
    }
}
