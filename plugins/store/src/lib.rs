//! The plugin store: the plugins of a plugin library (paperpaw.org's by
//! default) for this device, by group as the website shows them, with a
//! search, each with a page to install, update or remove it (ADR 0023).
//! Fonts are one more entry, which opens the fonts browser where it is
//! installed.
//!
//! The list is read as it streams into an index kept on the storage, so
//! it may be as long as it likes; a plugin's files go straight to the
//! storage, one after another, each checked ([`paperpaw_sdk::install`]).
//! Plugins with code are listed where PaperPaw loads them and a build
//! fits; their code is checked whole and signed by its author (`trust`,
//! ADR 0024). Built-in plugins are not listed, nor a plugin that would
//! take the place of one: those come with the firmware.

#![no_std]

extern crate alloc;

mod catalog;
#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and the declaration test also use the builders"
)]
pub mod declarations;
mod fetch;
mod letters;
mod list;
#[doc(hidden)]
pub mod native_device;
mod native_entry;
#[doc(hidden)]
pub mod native_index;
#[doc(hidden)]
pub mod native_item;
#[doc(hidden)]
pub mod native_list;
#[doc(hidden)]
pub mod native_load;
#[doc(hidden)]
pub mod native_model;
#[doc(hidden)]
pub mod native_page;
/// The interface-7 plugin store (slice 1; see `PORTING-7.md`).
pub mod native_plugin;
#[doc(hidden)]
pub mod native_queue;
#[doc(hidden)]
pub mod native_refusal;
#[doc(hidden)]
pub mod native_request;
#[doc(hidden)]
pub mod native_sort;
mod needs;
mod origin;
mod page;
mod queue;
mod request;
mod requires;
mod revoked;
mod room;
mod rows;
mod setup;
mod shots;
mod source;
mod styles;
mod trust;
mod view;

#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;

#[cfg(test)]
mod native_parity_tests;

use alloc::borrow::Cow;
use alloc::boxed::Box;

use paperpaw_sdk::builtin::{events, groups, menus, params};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::install::{Installer, Progress, Received};
use paperpaw_sdk::replies::{Confirmed, HttpReply};
use paperpaw_sdk::setting::Setting;
use paperpaw_sdk::{
    Arg, Command, Commands, Env, FixedAction, FixedScreen, Id, Manifest, MenuEntry, Params, Plugin,
    PluginInfo, PluginProblem, PluginState, RegisterError, Registrar, SettingSpec,
};
use trust::Verdict;

pub const PLUGIN: Id = Id::new_static("store");
/// Opens the store.
pub const OPEN: Id = Id::new_static("store.open");
const LIST_SCREEN: Id = Id::new_static("store.list");
const PAGE_SCREEN: Id = Id::new_static("store.plugin");
const VIEW_SCREEN: Id = Id::new_static("store.view");
const SOURCE_SCREEN: Id = Id::new_static("store.source");
const SHOTS_SCREEN: Id = Id::new_static("store.shots");
const FETCH_SCREEN: Id = Id::new_static("store.fetch");
const LETTERS_SCREEN: Id = Id::new_static("store.letters");
/// The fonts browser's list, offered as the entry *Fonts*.
pub(crate) const FONTS: Id = Id::new_static("fonts-browser.open");
/// Fonts with larger sizes, from the reading settings: the fonts browser,
/// or its page here where it is not installed (a store plugin on lean
/// firmware).
pub const DISCOVER_FONTS: Id = Id::new_static("store.discover-fonts");
/// The fonts browser's id.
const FONTS_PLUGIN: &str = "fonts-browser";
/// The answer to what *Discover fonts* asks where the fonts browser does
/// not run; the question's `then` says what a yes does.
const FONTS_ANSWER: Event<Confirmed> = Event::new("store.fonts-answered");
/// The plugin manager's list of other places to get plugins.
const MANAGER_MORE: Id = Id::new_static("plugin-manager.more");

/// Where the list of plugins comes from.
pub const CATALOG: Setting<paperpaw_sdk::setting::Text> = Setting::new("store.catalog");

/// Whether the store lists community plugins too: published by their
/// authors from GitHub, which nobody reviewed (off unless set).
pub const COMMUNITY: Setting<bool> = Setting::new("store.community");

/// The plugin library's address, for developers and libraries of one's
/// own; and whether it lists community plugins, guarded: no other
/// plugin's defaults, presets or commands switch it on.
static SETTINGS: [SettingSpec; 2] = [
    SettingSpec::text(
        CATALOG.static_str(),
        "store.setting.catalog",
        catalog::OFFICIAL_LIST,
        groups::DEVELOPER,
    )
    .description("store.setting.catalog.help"),
    SettingSpec::switch(
        COMMUNITY.static_str(),
        "store.setting.community",
        false,
        groups::SYSTEM,
    )
    .description("store.setting.community.help")
    .guarded(),
];

/// Whether the reader asked for community plugins too.
pub(crate) fn community(env: &Env<'_>) -> bool {
    env.settings().read(&COMMUNITY)
}

/// The answer to a file's download.
const DOWNLOADED: Event<HttpReply> = Event::new("store.downloaded");

/// Installs plugins; its job is kept in this plugin's folder.
pub(crate) const INSTALLER: Installer = Installer::new(PLUGIN, DOWNLOADED);

/// The plugin store.
#[derive(Debug, Default)]
pub struct Store;

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

static ACTIONS: [FixedAction; 2] = [
    FixedAction::new(OPEN, "store.title")
        .handler(|_, _, out| out.push(Command::OpenScreen(LIST_SCREEN, Params::new()))),
    FixedAction::new(DISCOVER_FONTS, "store.discover-fonts").handler(discover_fonts),
];

/// Whether a plugin that is to run did not start (or was held back for
/// good): what its start recorded, as *Manage plugins* says it. Held back
/// for this start only (safe mode, switched on) it waits for a restart.
fn failed_to_start(info: &PluginInfo) -> bool {
    info.state == PluginState::Failed
        || info
            .problem
            .is_some_and(|problem| !(problem == PluginProblem::SafeMode && info.enabled))
}

/// Why `info` did not start, as the plugin manager says it, with where to
/// read more and what can be done.
fn why_not(env: &Env<'_>, info: &PluginInfo) -> alloc::string::String {
    let i18n = env.i18n();
    let key = match info.problem {
        Some(PluginProblem::OtherBuild) => "plugin-manager.code.other-build",
        Some(PluginProblem::Damaged) => "plugin-manager.code.damaged",
        Some(PluginProblem::Mismatch) => "plugin-manager.code.mismatch",
        // As the plugin manager says it: a smaller layout (the X4 after an
        // SD install) gets the web flasher's much larger one named.
        Some(PluginProblem::NoRoom)
            if env.native().is_some_and(|_| {
                paperpaw_sdk::native::code_room(env.storage()).is_some_and(|r| r.larger.is_some())
            }) =>
        {
            "plugin-manager.code.no-room-flash"
        }
        Some(PluginProblem::NoRoom) => "plugin-manager.code.no-room",
        Some(PluginProblem::Unsupported) => "plugin-manager.code.unsupported",
        Some(PluginProblem::SafeMode) => "plugin-manager.code.safe-mode",
        Some(PluginProblem::Crashed) => "plugin-manager.code.crashed",
        Some(PluginProblem::NotInstalled) => "plugin-manager.code.not-installed",
        Some(PluginProblem::Changed) => "plugin-manager.code.changed",
        Some(PluginProblem::OlderInterface { .. }) => "plugin-manager.code.older",
        Some(PluginProblem::NeedsNewerPaperPaw) => "plugin-manager.code.needs-newer",
        Some(PluginProblem::Revoked) => "plugin-manager.code.revoked",
        _ => "plugin-manager.failed",
    };
    // The plugin manager is always here; its words are the same ones the
    // reader finds there.
    let reason = i18n
        .get(key)
        .unwrap_or_else(|| i18n.tr("store.fonts.failed.reason"));
    alloc::format!("{reason}\n{}", i18n.tr("store.fonts.failed.help"))
}

/// *Discover fonts*: the fonts browser's list where it runs. Otherwise
/// the reader is told why not (it is not installed, it did not start, it
/// is switched off, or it waits for a restart), with the one thing to do
/// about it; the store's page of it is not the answer to a wish for fonts,
/// and a plugin that failed never falls back to it unsaid.
fn discover_fonts(env: &Env<'_>, _: &Params, out: &mut Commands) {
    if env.actions().contains(FONTS.as_str()) {
        out.push(Command::run(FONTS));
        return;
    }
    // The question, what it adds, the word for yes and for no, and what yes does.
    let (question, help, yes, no, then) = match env.plugins().get(FONTS_PLUGIN) {
        None => (
            "store.fonts.missing",
            "store.fonts.missing.help",
            "store.fonts.show",
            "store.fonts.not-now",
            "page",
        ),
        Some(info) if failed_to_start(info) => {
            let i18n = env.i18n();
            out.push(
                paperpaw_sdk::ask::confirm(i18n.tr("store.fonts.failed"), &FONTS_ANSWER)
                    .text(why_not(env, info))
                    .yes(i18n.tr("store.fonts.show"))
                    .no(i18n.tr("store.fonts.not-now"))
                    .focus_yes()
                    .with("then", "page")
                    .command(),
            );
            return;
        }
        Some(info) if !info.enabled => (
            "store.fonts.off",
            "store.fonts.off.help",
            "store.fonts.switch-on",
            "store.fonts.not-now",
            "on",
        ),
        Some(info) if info.restart_needed() => (
            "store.fonts.restart",
            "store.fonts.restart.help",
            "store.restart",
            "store.fonts.later",
            "restart",
        ),
        // Switched on and not running, or failed: its page offers what
        // can be done (remove it, install it over).
        Some(_) => {
            out.push(fonts_page(env));
            return;
        }
    };
    let i18n = env.i18n();
    out.push(
        paperpaw_sdk::ask::confirm(i18n.tr(question), &FONTS_ANSWER)
            .text(i18n.tr(help))
            .yes(i18n.tr(yes))
            .no(i18n.tr(no))
            .focus_yes()
            .with("then", then)
            .command(),
    );
}

/// The store's page of the fonts browser, titled while it is fetched.
fn fonts_page(env: &Env<'_>) -> Command {
    let page = Params::new()
        .with(params::ID, FONTS_PLUGIN)
        .with(params::TITLE, env.i18n().tr("store.fonts.name"));
    Command::OpenScreen(PAGE_SCREEN, page)
}

/// What the reader chose to do about the fonts browser.
fn fonts_answered(env: &Env<'_>, answer: &Confirmed, out: &mut Commands) {
    if !answer.yes {
        return;
    }
    match answer.context.get("then") {
        Some("on") => {
            if let Ok(id) = Id::parse(FONTS_PLUGIN) {
                out.push(Command::SetPluginEnabled(id, true));
                out.push(Command::Restart);
            }
        }
        Some("restart") => out.push(Command::Restart),
        _ => out.push(fonts_page(env)),
    }
}

static ENTRIES: [MenuEntry; 3] = [
    // With the tools, and in the plugin manager.
    MenuEntry::new(menus::TOOLS, OPEN).order(90),
    MenuEntry::new(MANAGER_MORE, OPEN).order(10),
    // On the page of the text, below the font size, whose largest
    // sizes are the font's.
    MenuEntry::new(groups::READING_TEXT, DISCOVER_FONTS),
];
static ICONS: [(Id, &str); 2] = [(OPEN, "cloud-download"), (DISCOVER_FONTS, "typography")];

static SCREENS: [FixedScreen; 7] = [
    FixedScreen::new(PAGE_SCREEN, "store.title", |env, params| {
        Box::new(page::PageScreen::new(env, params))
    }),
    FixedScreen::new(LETTERS_SCREEN, "store.letter", |env, params| {
        Box::new(letters::LettersScreen::new(env, params))
    }),
    FixedScreen::new(VIEW_SCREEN, "store.title", |env, params| {
        Box::new(view::ViewScreen::new(env, params))
    }),
    FixedScreen::new(SOURCE_SCREEN, "store.source", |env, params| {
        Box::new(source::SourceScreen::new(env, params))
    }),
    FixedScreen::new(FETCH_SCREEN, "store.fetch.title", |env, params| {
        Box::new(fetch::FetchScreen::new(env, params))
    })
    .instances(paperpaw_sdk::Instances::SINGLE),
    FixedScreen::new(SHOTS_SCREEN, "store.shots", |env, params| {
        Box::new(shots::ShotsScreen::new(env, params))
    }),
    FixedScreen::new(LIST_SCREEN, "store.title", |env, _| {
        Box::new(list::ListScreen::new(env))
    })
    .instances(paperpaw_sdk::Instances::SINGLE),
];

impl Plugin for Store {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "Plugin store", paperpaw_sdk::package_version!())
            .description("Finds, installs, updates and removes plugins for this device.")
            .how(&HOW)
            .uses(paperpaw_sdk::Uses::NETWORK)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_settings(&SETTINGS)?;
        r.add_screens(&SCREENS)?;
        r.add_actions(&ACTIONS)?;
        r.add_menu_entries(&ENTRIES)?;
        r.add_icons(&ICONS)?;
        // Downloads go on while no screen of the store is open; one cut
        // off by a restart is begun anew from the plugin's page.
        r.on(DOWNLOADED, received)?;
        r.on(FONTS_ANSWER, fonts_answered)?;
        r.on(queue::FETCHED, queue::fetched)?;
        // The first-run setup's offer of the plugins most readers add.
        setup::register(r)?;
        // Plugins another plugin asks for (the reader's packs of letters).
        request::register(r)?;
        r.on(events::STARTED, |env, _, out| {
            // A download cut off by the restart is begun anew from its
            // page, or first when the queue goes on.
            queue::interrupted(env);
            INSTALLER.cancel(env.storage());
            // The setup's plugins go on without the store being opened:
            // the reader asked for them.
            if queue::has_quiet(env) {
                queue::next(env, out);
            }
            // The faces of Literata the firmware leaves out, fetched once.
            styles::offer(env, out);
        })?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

/// A file arrived (or did not): once all are here, the plugin installed
/// if it may be (see [`trust`]), and said; then the next update waiting
/// in the queue ([`queue`]) has its turn.
fn received(env: &Env<'_>, reply: &HttpReply, out: &mut Commands) {
    let mut said = paperpaw_sdk::Commands::new();
    // Plugins put in quietly by the first-run setup ask for nothing more;
    // those another plugin asked for may (a chain of requirements).
    let quiet = queue::quiet(env) && !queue::requested(env);
    let mut required = alloc::vec::Vec::new();
    let installed = match INSTALLER.received(env, reply, out) {
        Received::Complete(done) => {
            // Read before it is put in its place.
            let folder = INSTALLER.folder(env.storage()).unwrap_or_default();
            required = requires::missing(env, &folder, &done.plugin);
            (finish(env, &done, &mut said), done.name, done.plugin)
        }
        Received::Failed(job) => {
            said.push(failed(env, &job.name));
            (false, job.name, job.plugin)
        }
        Received::Next(_) | Received::Other => return,
        _ => return,
    };
    // Plugins another plugin asked for: told to it, not said here.
    if queue::requested(env) {
        request::outcome(env, &installed.2, installed.0);
        out.extend(
            said.into_iter()
                .filter(|c| !matches!(c, Command::Notify(_))),
        );
    // A quiet queue (the first-run setup's) says it all once, at its end:
    // its notices wait, what installs goes on.
    } else if queue::quiet(env) {
        queue::note(env, &installed.1, installed.0);
        out.extend(
            said.into_iter()
                .filter(|c| !matches!(c, Command::Notify(_))),
        );
    } else {
        out.extend(said);
    }
    queue::next(env, out);
    // What the plugin installed requires, and the device lacks, is offered.
    if installed.0 && !quiet {
        requires::offer(required, out);
    }
}

/// Installs the plugin all of whose files are here, if it may be, and
/// says so; whether it was installed.
fn finish(env: &Env<'_>, done: &Progress, out: &mut Commands) -> bool {
    let name = [("name", Arg::Str(&done.name))];
    let folder = INSTALLER.folder(env.storage()).unwrap_or_default();
    let (key, installed) = match trust::verdict(env, &done.plugin, &folder) {
        Verdict::Install { code } => {
            let before = env.plugins().get(&done.plugin).is_some();
            if trust::install(env, code, out).is_none() {
                INSTALLER.cancel(env.storage());
                out.push(failed(env, &done.name));
                return false;
            }
            let key = match (code.is_some(), before) {
                (true, true) => "store.updated-code",
                (true, false) => "store.installed-code",
                (false, true) => "store.updated",
                (false, false) => "store.installed",
            };
            (key, true)
        }
        Verdict::Refused(refusal) => {
            INSTALLER.cancel(env.storage());
            (trust::refusal_key(env, refusal), false)
        }
    };
    out.push(Command::Notify(env.i18n().format(key, &name)));
    installed
}

fn failed(env: &Env<'_>, name: &str) -> Command {
    Command::Notify(
        env.i18n()
            .format("store.failed", &[("name", Arg::Str(name))]),
    )
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
