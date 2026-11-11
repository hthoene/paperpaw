//! Fonts from the internet: a catalog of fonts (paperpaw.org's by
//! default), each name shown in its own font; one is shown larger with a
//! sentence in it, downloaded as a data plugin and read with at once.
//!
//! Nothing of a font is built in: devices that draw scalable fonts get
//! its font files, the others (the X4) font packs made for them, read a
//! glyph at a time from the storage. *More fonts* is offered where the
//! reading font and the status lines' font are chosen, and *Your own
//! fonts*, which shows how to read with a font file of one's own.

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

#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and the declaration test also use the builders"
)]
mod declarations;
pub mod native_catalog;
mod native_entry;
pub mod native_job;
mod native_own;
pub mod native_plan;
/// The interface-7 plugin.
pub mod native_plugin;
mod native_sample;
mod native_view;
#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;

mod catalog;
mod font;
mod install;
mod list;
mod samples;

use alloc::borrow::Cow;
use alloc::boxed::Box;

use paperpaw_sdk::builtin::{events, groups, settings};
use paperpaw_sdk::setting::Setting;
use paperpaw_sdk::{
    Command, Env, FixedAction, FixedScreen, Id, Manifest, MenuEntry, Params, Plugin, RegisterError,
    Registrar, SettingSpec,
};

pub const PLUGIN: Id = Id::new_static("fonts-browser");
/// Opens the list of fonts.
pub const OPEN: Id = Id::new_static("fonts-browser.open");
/// Shows how to read with a font of one's own.
pub const OWN: Id = Id::new_static("fonts-browser.own");
const LIST_SCREEN: Id = Id::new_static("fonts-browser.list");
const FONT_SCREEN: Id = Id::new_static("fonts-browser.font");

/// Where the catalog of fonts comes from.
pub const CATALOG: Setting<paperpaw_sdk::setting::Text> = Setting::new("fonts-browser.catalog");

/// The font library's address, for developers and libraries of one's own.
static SETTINGS: [SettingSpec; 1] = [SettingSpec::text(
    CATALOG.static_str(),
    "fonts-browser.setting.catalog",
    "https://paperpaw.org/api/v1/fonts.json",
    groups::DEVELOPER,
)
.description("fonts-browser.setting.catalog.help")];

/// The fonts browser.
#[derive(Debug, Default)]
pub struct FontsBrowser;

/// How it works, by language (see `Manifest::how`), kept once for both entries.
use native_own::HOW;

static ACTIONS: [FixedAction; 2] = [
    FixedAction::new(OPEN, "fonts-browser.more")
        .handler(|_, _, out| out.push(Command::OpenScreen(LIST_SCREEN, Params::new()))),
    FixedAction::new(OWN, "fonts-browser.own").handler(show_own),
];
/// After the fonts to choose from, where they are chosen.
static ENTRIES: [MenuEntry; 4] = [
    MenuEntry::new(Id::new_static(settings::READING_FONT.static_str()), OPEN),
    MenuEntry::new(Id::new_static(settings::READING_FONT.static_str()), OWN),
    MenuEntry::new(Id::new_static(settings::STATUS_FONT.static_str()), OPEN),
    MenuEntry::new(Id::new_static(settings::STATUS_FONT.static_str()), OWN),
];
static ICONS: [(Id, &str); 2] = [(OPEN, "typography"), (OWN, "folder-open")];

/// Opens the part of the plugin's help (its *How it works*) that says how
/// to read with a font file of one's own, from after its heading on, as a
/// page.
fn show_own(env: &Env<'_>, _: &Params, out: &mut paperpaw_sdk::Commands) {
    let i18n = env.i18n();
    let heading = i18n.tr("fonts-browser.own.heading");
    let text = env
        .plugins()
        .get(PLUGIN.as_str())
        .and_then(|info| info.how(i18n))
        .and_then(|how| {
            how.find(heading)
                .map(|at| alloc::string::String::from(how[at + heading.len()..].trim_start()))
        })
        .map(|how| alloc::format!("{}\n\n{how}\n", i18n.tr("fonts-browser.own")));
    let file = alloc::format!("{}/own.txt", Env::plugin_dir(PLUGIN.as_str()));
    match text.map(|text| paperpaw_sdk::readers::show_text(env.storage(), &file, &text)) {
        Some(Ok(open)) => out.push(open),
        _ => out.push(Command::Notify(i18n.tr("fonts-browser.how-failed").into())),
    }
}

static SCREENS: [FixedScreen; 2] = [
    FixedScreen::new(FONT_SCREEN, "fonts-browser.title", |env, params| {
        Box::new(font::FontScreen::new(env, params))
    }),
    FixedScreen::new(LIST_SCREEN, "fonts-browser.title", |env, _| {
        Box::new(list::ListScreen::new(env))
    })
    .instances(paperpaw_sdk::Instances::SINGLE),
];

impl Plugin for FontsBrowser {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "More fonts", paperpaw_sdk::package_version!()).description(
                "Shows fonts from the internet, each with a sample, and downloads them to read with.",
            ).how(&HOW).uses(paperpaw_sdk::Uses::NETWORK)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_settings(&SETTINGS)?;
        r.add_screens(&SCREENS)?;
        r.add_actions(&ACTIONS)?;
        r.add_menu_entries(&ENTRIES)?;
        r.add_icons(&ICONS)?;
        // Downloads go on while no screen of the plugin is open; one cut
        // off by a restart is begun anew from the font's page.
        r.on(install::DOWNLOADED, install::received)?;
        r.on(events::STARTED, |env, _, _| {
            install::INSTALLER.cancel(env.storage())
        })?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
