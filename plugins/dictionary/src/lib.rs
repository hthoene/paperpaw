//! The dictionary: looks up a word in the StarDict dictionaries of a
//! folder and shows the definitions in a panel over the lower part of the
//! screen.
//!
//! Bound to the `core.lookup` slot, which the reader opens for a word
//! (long press, or *Look up a word* in its menu). Dictionaries are folders
//! with `.ifo`, `.idx`, `.dict(.dz)` and optional `.syn` files, as used by
//! KOReader, GoldenDict and many e-readers; they are found anywhere below
//! the dictionary folder. The first lookup prepares a small index for each
//! dictionary in the background; lookups then take a few milliseconds and
//! little memory (see `paperpaw-stardict`).
//!
//! The panel also lists what other plugins' lookup providers find (see
//! `PORTING-7.md` for what of the online sources is still to come). The
//! StarDict dictionaries are a provider too, for a lookup screen of another
//! plugin.
//!
//! Buttons: next/previous page or up/down scroll long definitions, confirm
//! and back close. Touch: the right part scrolls, the left part goes back
//! (or closes on the first page), outside closes.

#![no_std]

extern crate alloc;

#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and the declaration test also use the builders"
)]
pub mod declarations;
#[doc(hidden)]
pub mod native_disk;
mod native_entry;
#[doc(hidden)]
pub mod native_folders;
#[doc(hidden)]
pub mod native_model;
#[doc(hidden)]
pub mod native_panel;
/// The interface-7 dictionary (see `PORTING-7.md`).
pub mod native_plugin;
#[doc(hidden)]
pub mod native_popup;
#[doc(hidden)]
pub mod native_source;

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
