//! All notes: the bookmarks and highlights the reader keeps,
//! seen across the library, and written out in Kindle's `My Clippings.txt`
//! form.
//!
//! *Tools › All notes* lists the books that have notes with how
//! many; a book opens its notes, a note opens the book there. At the end
//! of the list, *Export as My Clippings.txt* writes the notes of every book
//! to `Exports/My Clippings.txt` beside the library folder (see
//! `clippings`).
//!
//! It reads what [`paperpaw_sdk::annotations`] keeps, and the library's
//! books through the library service: the folder of notes files is listed
//! once, then the books pass one at a time, so a large library costs time,
//! not memory (see `scan`). No SDK change is needed.

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
pub mod export_file;
pub mod model;
mod native_entry;
pub mod native_export;
#[cfg(test)]
mod native_export_tests;
pub mod native_library;
pub mod native_notes;
#[cfg(test)]
mod native_notes_tests;
pub mod native_plugin;
pub mod native_run;
pub mod native_scan;
#[cfg(test)]
mod native_scan_tests;
pub mod native_staged;

#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;

mod clippings;
mod scan;
mod screen;

use alloc::borrow::Cow;
use alloc::boxed::Box;

use paperpaw_sdk::builtin::menus;
use paperpaw_sdk::{
    Command, FixedAction, Id, Manifest, MenuEntry, Params, Plugin, RegisterError, Registrar,
    ScreenSpec,
};

pub const PLUGIN: Id = Id::new_static("notes-overview");
/// The action that opens the list of books.
pub const OPEN: Id = Id::new_static("notes-overview.open");
/// The list of books with notes.
pub(crate) const SCREEN: Id = Id::new_static("notes-overview.screen");
/// The notes of one book (parameter `path`).
pub(crate) const BOOK: Id = Id::new_static("notes-overview.book");

static ACTIONS: [FixedAction; 1] = [FixedAction::new(OPEN, "notes-overview.title")
    .handler(|_, _, out| out.push(Command::OpenScreen(SCREEN, Params::new())))];
static ENTRIES: [MenuEntry; 1] = [MenuEntry::new(menus::TOOLS, OPEN).order(37)];
static ICONS: [(Id, &str); 1] = [(OPEN, "bookmarks")];

static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

#[derive(Debug)]
pub struct NotesOverview;

impl Plugin for NotesOverview {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "All notes", paperpaw_sdk::package_version!())
            .description("The bookmarks and highlights of all books in one list, and an export as My Clippings.txt.")
            .how(&HOW)
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_actions(&ACTIONS)?;
        r.add_menu_entries(&ENTRIES)?;
        r.add_icons(&ICONS)?;
        r.add_screen(ScreenSpec::new(
            SCREEN,
            "notes-overview.title",
            Box::new(|env, _| Box::new(screen::NotesScreen::books(env))),
        ))?;
        r.add_screen(ScreenSpec::new(
            BOOK,
            "notes-overview.title",
            Box::new(|env, params| Box::new(screen::NotesScreen::of_book(env, params))),
        ))?;
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
