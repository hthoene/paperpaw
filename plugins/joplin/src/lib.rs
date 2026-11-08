//! Joplin: a book's highlights and notes kept as a note in Joplin, one
//! note per book, updated when its notes are exported again (*Export as
//! Markdown* at the end of a book's bookmarks and highlights), in a
//! notebook named in the settings.
//!
//! The notes go to where Joplin synchronises to, so they reach every
//! device the user runs Joplin on at its next sync, with no computer
//! running: Joplin Cloud, Joplin Server or a WebDAV folder such as
//! Nextcloud (see [`Service`]). *Joplin* in the tools sets up the
//! account; the password is kept with the other passwords and keys (see
//! `paperpaw_sdk::secrets`). What cannot be sent (no connection, Joplin
//! down) waits and is sent later: with the next export, by the action
//! *Send to Joplin* (also for rules), and when Wi-Fi comes back where
//! PaperPaw manages it. Sync targets with Joplin's end-to-end encryption
//! get nothing, rather than notes in plain text.
//!
//! The notes are read and written in pieces and sent from a file, and
//! Joplin's answers are saved to files of which only the end is read, so
//! this works on small devices too. Shipped switched off: switch it on
//! in the plugins.
//!
//! It reads the Markdown the reader writes (as the `readwise` plugin
//! does), so it works with any reader that writes the same and emits
//! `core.notes-exported`.

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

mod item;
mod kept;
mod locks;
mod markdown;
pub mod native_joplin;
#[cfg(test)]
mod native_parity_tests;
#[cfg(test)]
mod native_read_tests;
mod send;
mod service;
mod setup;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::string::String;
use core::cell::RefCell;

use paperpaw_sdk::builtin::{events, groups, menus};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::replies::HttpReply;
use paperpaw_sdk::setting::{Setting, Text};
use paperpaw_sdk::{
    ActionSpec, Command, Commands, Env, FixedAction, FixedScreen, Id, Manifest, MenuEntry, Params,
    Plugin, RegisterError, Registrar, SettingExtra, SettingSpec,
};

pub use send::DEFAULT_NOTEBOOK;
pub use service::Service;

use send::{Run, Start};

pub const PLUGIN: Id = Id::new_static("joplin");
/// Opens the set-up.
pub const OPEN: Id = Id::new_static("joplin.open");
/// Sends the notes that wait.
pub const SEND: Id = Id::new_static("joplin.send");
/// Where the notes go ([`Service::key`]), the address (Joplin Server's,
/// or the WebDAV folder), the e-mail or user name, the notebook.
pub const SERVICE: Setting<Text> = Setting::new("joplin.service");
pub const ADDRESS: Setting<Text> = Setting::new("joplin.address");
pub const USER: Setting<Text> = Setting::new("joplin.user");
pub const NOTEBOOK: Setting<Text> = Setting::new("joplin.notebook");
/// The password, among the passwords and keys.
pub const PASSWORD: &str = "joplin.password";
pub(crate) const SCREEN: Id = Id::new_static("joplin.setup");
/// Emitted with a message (`text`) when a sending ends.
pub(crate) const DONE: Event<Finished> = Event::new("joplin.done");

paperpaw_sdk::event_data! {
    /// How a sending ended ([`DONE`]), in words.
    pub(crate) struct Finished {
        pub(crate) text: String,
    }
}

/// The account's settings, all set on the set-up.
static ON_SETUP: SettingExtra = SettingExtra::new().editor(SCREEN);
static SETTINGS: [SettingSpec; 4] = [
    SettingSpec::choice(
        SERVICE.static_str(),
        "joplin.service",
        &Service::KEYS,
        Service::KEYS[0],
        groups::LIBRARY,
    )
    .with_extra(&ON_SETUP)
    .kept_on_reset()
    .description("joplin.service.help"),
    SettingSpec::text(ADDRESS.static_str(), "joplin.address", "", groups::LIBRARY)
        .with_extra(&ON_SETUP)
        .kept_on_reset()
        .advanced()
        .description("joplin.address.help"),
    SettingSpec::text(USER.static_str(), "joplin.user", "", groups::LIBRARY)
        .with_extra(&ON_SETUP)
        .kept_on_reset()
        .advanced()
        .description("joplin.user.help"),
    SettingSpec::text(
        NOTEBOOK.static_str(),
        "joplin.notebook",
        DEFAULT_NOTEBOOK,
        groups::LIBRARY,
    )
    .with_extra(&ON_SETUP)
    .kept_on_reset()
    .advanced()
    .description("joplin.notebook.help"),
];

/// The Joplin plugin.
#[derive(Debug, Default)]
pub struct Joplin;

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

static ACTIONS: [FixedAction; 1] = [FixedAction::new(OPEN, "joplin.title")
    .handler(|_, _, out| out.push(Command::OpenScreen(SCREEN, Params::new())))];
static ENTRIES: [MenuEntry; 1] = [MenuEntry::new(menus::TOOLS, OPEN).order(49)];
static ICONS: [(Id, &str); 2] = [(OPEN, "note"), (SEND, "upload")];

static SCREENS: [FixedScreen; 1] = [FixedScreen::new(SCREEN, "joplin.title", |env, _| {
    Box::new(setup::SetupScreen::new(env))
})
.instances(paperpaw_sdk::Instances::SINGLE)];

impl Plugin for Joplin {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "Book notes sync", paperpaw_sdk::package_version!())
            .description("Keeps each book's highlights and notes in a note. Works with Joplin (Joplin Cloud, Joplin Server or WebDAV).")
            .how(&HOW)
            .uses(paperpaw_sdk::Uses::NETWORK)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let run: Rc<RefCell<Run>> = Rc::default();
        r.add_settings(&SETTINGS)?;
        r.add_screens(&SCREENS)?;
        r.add_actions(&ACTIONS)?;
        let state = run.clone();
        r.add_action(ActionSpec::new(SEND, "joplin.send").handler(
            move |env: &Env<'_>, _, out: &mut Commands| {
                if env.reader() {
                    state.borrow_mut().start(env, Start::Action, out);
                } else {
                    let due = SEND_DUE.id().clone();
                    out.push(Command::work(paperpaw_sdk::work::Work::when_cheap(
                        due,
                        core::time::Duration::from_secs(3600),
                    )));
                }
            },
        ))?;
        let state = run.clone();
        r.on(SEND_DUE, move |env, (), out| {
            state.borrow_mut().start(env, Start::Action, out);
        })?;
        r.add_menu_entries(&ENTRIES)?;
        r.add_icons(&ICONS)?;

        // Any reader's notes, once exported as Markdown.
        let state = run.clone();
        r.on(events::NOTES_EXPORTED, move |env, exported, out| {
            if kept::add_waiting(env.storage(), &exported.path).is_ok() {
                state.borrow_mut().start(env, Start::Export, out);
                retry_later(env, out);
            }
        })?;
        // Notes still waiting: at the next connection, at the latest in
        // half an hour (longer while saving).
        let state = run.clone();
        r.on(RETRY_DUE, move |env, (), out| {
            state.borrow_mut().start(env, Start::Retry, out);
            retry_later(env, out);
        })?;
        r.on(events::STARTED, |env, _, out| retry_later(env, out))?;
        let state = run.clone();
        r.on(events::SETTING_CHANGED, move |_, changed, _| {
            if [SERVICE.as_str(), ADDRESS.as_str(), USER.as_str()].contains(&changed.id.as_str()) {
                state.borrow_mut().account_changed();
            }
        })?;
        let state = run.clone();
        r.on(setup::ENTERED, move |_, typed, out| {
            if setup::entered(typed, out) {
                state.borrow_mut().account_changed();
            }
        })?;
        type Step = fn(&mut Run, &Env<'_>, &HttpReply, &mut Commands);
        let steps: [(Event<HttpReply>, Step); 7] = [
            (send::SIGNED_IN, Run::signed_in),
            (send::CHECKED, Run::checked),
            (send::LOCKS, Run::locks_listed),
            (send::NOTEBOOK_FOUND, Run::notebook_found),
            (send::NOTEBOOK_MADE, Run::notebook_made),
            (send::NOTE_FOUND, Run::note_found),
            (send::NOTE_SENT, Run::note_sent),
        ];
        for (event, step) in steps {
            let state = run.clone();
            r.on(event, move |env, reply, out| {
                step(&mut state.borrow_mut(), env, reply, out);
            })?;
        }
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

/// Declared work: notes still waiting, sent again.
const RETRY_DUE: paperpaw_sdk::event::Event = paperpaw_sdk::event::Event::new("joplin.retry");
/// Declared work: a send no reader waits for (a rule's).
const SEND_DUE: paperpaw_sdk::event::Event = paperpaw_sdk::event::Event::new("joplin.send-due");

/// Declares the next try while notes wait.
fn retry_later(env: &Env<'_>, out: &mut Commands) {
    if !kept::waiting(env.storage()).is_empty() {
        let work = paperpaw_sdk::work::Work::network(RETRY_DUE.id().clone())
            .within(core::time::Duration::from_secs(30 * 60))
            .at_moments(paperpaw_sdk::work::Moments::ONLINE);
        out.push(Command::work(work));
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
