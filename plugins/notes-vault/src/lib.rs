//! Notes vault: a folder of Markdown notes read on the device, as a vault
//! of notes keeps them (it works with Obsidian vaults: plain `.md` files in
//! folders, with `[[wikilinks]]`, front matter and `#tags`).
//!
//! - *Notes vault* in the tools: the vault's folders, its latest changes,
//!   its tags, and a search in the notes' titles. A note opens in the
//!   reader; its links to other notes (by name, as a vault writes them)
//!   open them, and *Back* returns.
//! - Home widgets: the recent notes, a pinned note, a note of the day, a
//!   random note, the tags, and a search (see `widgets`).
//!
//! The vault is read as it is on the card, never changed. Its folders are
//! listed as they are; the latest changes, the tags and the search come
//! from an index of the notes made in the background, a piece at a time
//! while the vault's page is open, and kept in a file (see `index`): a
//! vault of thousands of notes costs time the first time, and a window of
//! rows of memory, never its size.

#![no_std]

extern crate alloc;

#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and the declaration test also use the builders"
)]
pub mod declarations;
mod home;
mod index;
mod lines;
mod list;
mod markdown;
mod native_entry;
#[doc(hidden)]
pub mod native_model;
/// The interface-7 notes vault (slice 1; see `PORTING-7.md`).
pub mod native_plugin;
mod note;
mod page;
mod source;
mod tags;
mod widgets;

pub use markdown::MarkdownDocument;
#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;

#[cfg(test)]
mod native_parity_tests;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::string::{String, ToString};

use paperpaw_sdk::builtin::menus;
use paperpaw_sdk::document::Document;
use paperpaw_sdk::setting::{Setting, Text};
use paperpaw_sdk::{
    Command, Env, FixedAction, FormatSpec, Id, Manifest, MenuEntry, Params, Plugin, RegisterError,
    Registrar, ScreenSpec, SettingExtra, SettingSpec,
};

pub const PLUGIN: Id = Id::new_static("notes-vault");
/// The settings: the vault's folder.
pub const GROUP: Id = Id::new_static("notes-vault.group");
/// The folder of the vault, chosen with the folder chooser.
pub const FOLDER: Setting<Text> = Setting::new("notes-vault.folder");
/// Opens the vault's page.
/// The Markdown notes are written in, which replaces the built-in format
/// for `.md` while this plugin is on (a later format wins an extension).
pub const MARKDOWN: Id = Id::new_static("notes-vault.markdown");
pub const OPEN: Id = Id::new_static("notes-vault.open");
/// The vault's page: where to go, and the index.
pub(crate) const HOME_SCREEN: Id = Id::new_static("notes-vault.screen");
/// A list of notes: folder, latest changes, tag, search results.
pub(crate) const LIST_SCREEN: Id = Id::new_static("notes-vault.list");
/// The list of tags.
pub(crate) const TAGS_SCREEN: Id = Id::new_static("notes-vault.tags");

static ON_FOLDER: SettingExtra =
    SettingExtra::new().editor(paperpaw_sdk::builtin::screens::FOLDER_EDITOR);

static SETTINGS: [SettingSpec; 1] =
    [
        SettingSpec::text(FOLDER.static_str(), "notes-vault.setting.folder", "", GROUP)
            .with_extra(&ON_FOLDER)
            .description("notes-vault.setting.folder.help"),
    ];

static ACTIONS: [FixedAction; 1] = [FixedAction::new(OPEN, "notes-vault.title")
    .handler(|_, _, out| out.push(Command::OpenScreen(HOME_SCREEN, Params::new())))];
static ENTRIES: [MenuEntry; 1] = [MenuEntry::new(menus::TOOLS, OPEN).order(38)];
static ICONS: [(Id, &str); 2] = [(OPEN, "note"), (GROUP, "note")];

static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

/// The vault's folder, as set, without slashes at its ends; empty where
/// none is chosen.
pub(crate) fn vault(env: &Env<'_>) -> String {
    env.settings()
        .read(&FOLDER)
        .trim()
        .trim_matches('/')
        .to_string()
}

#[derive(Debug, Default)]
pub struct NotesVault;

impl Plugin for NotesVault {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "Notes vault", paperpaw_sdk::package_version!())
            .description(
                "Reads a folder of Markdown notes by folders, tags and latest changes, with \
                 widgets for the home screen. Works with Obsidian vaults.",
            )
            .how(&HOW)
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        // What is converted of each note, moved and removed with it.
        r.add_book_cache(Env::plugin_dir(PLUGIN.as_str()), "md-")?;
        r.add_format(FormatSpec::new(
            MARKDOWN,
            "notes-vault.markdown",
            alloc::vec![Cow::Borrowed("md"), Cow::Borrowed("markdown")],
            Box::new(|storage, path, options| {
                markdown::MarkdownDocument::open(storage, path, options.publisher_styles)
                    .map(|d| Box::new(d) as Box<dyn Document>)
            }),
        ))?;
        r.add_setting_group({
            static SPEC: paperpaw_sdk::SettingGroupSpec =
                paperpaw_sdk::SettingGroupSpec::new(GROUP, "notes-vault.title");
            &SPEC
        })?;
        r.add_settings(&SETTINGS)?;
        r.add_actions(&ACTIONS)?;
        r.add_menu_entries(&ENTRIES)?;
        r.add_icons(&ICONS)?;
        r.add_screen(ScreenSpec::new(
            HOME_SCREEN,
            "notes-vault.title",
            Box::new(|env, _| Box::new(home::HomeScreen::new(env))),
        ))?;
        r.add_screen(ScreenSpec::new(
            LIST_SCREEN,
            "notes-vault.title",
            Box::new(|env, params| Box::new(list::ListScreen::new(env, params))),
        ))?;
        r.add_screen(ScreenSpec::new(
            TAGS_SCREEN,
            "notes-vault.tags",
            Box::new(|env, _| Box::new(home::TagsScreen::new(env))),
        ))?;
        widgets::register(r)?;
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
