//! The library: the books under the library folder, as a calm list.
//!
//! It opens with the book read last as a card, so going on reading is one
//! confirm or one tap; below it every book with its cover, title, author and
//! how far it is read. What is shown (all books, those being read, unread or
//! finished ones, the shelves) is chosen above the books. Series, folders,
//! favourites, authors and tags open as lists of their own. Holding a book
//! offers what can be done with it: open, finished, information, the archive,
//! delete.
//!
//! The library folder is a setting; its editor (`core.choose-folder`) browses
//! the card's folders, so no path has to be typed.
//!
//! This is an interface-7 plugin: its screens run on `core.ui`, its books come
//! from the host's library service (`core.library`) and its covers from the
//! host's image service. What waits for the host or the SDK is in
//! `PORTING-7.md`. Bound to `core.library`, and to `core.home` only as a
//! fallback when no home screen plugin is installed.

// What goes wrong with the user's data is told, never dropped unseen:
// a result left unread says why (see CONTRIBUTING.md).
#![deny(clippy::let_underscore_must_use)]
#![no_std]

extern crate alloc;

#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and the declaration test also use the builders"
)]
pub mod declarations;
#[doc(hidden)]
pub mod native_cells;
#[doc(hidden)]
pub mod native_choose;
#[doc(hidden)]
pub mod native_covers;
mod native_draw;
mod native_entry;
#[doc(hidden)]
pub mod native_info;
#[doc(hidden)]
pub mod native_list;
#[doc(hidden)]
pub mod native_model;
/// The interface-7 library plugin (see `PORTING-7.md`).
pub mod native_plugin;
#[doc(hidden)]
pub mod native_sheet;
#[doc(hidden)]
pub mod native_shelves;
#[doc(hidden)]
pub mod native_states;

#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;

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
