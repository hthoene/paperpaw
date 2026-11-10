//! OPDS catalogs: online libraries such as Project Gutenberg, Calibre-Web,
//! Kavita or Komga, browsed and searched like folders; their books are
//! downloaded into the library.
//!
//! - *Catalogs* lists them. Tapping one browses it; holding it (with
//!   buttons: the menu button) offers to sync, edit or remove it.
//! - A catalog has an address and, if it needs them, a user name and a
//!   password. Marked for syncing, *Sync catalogs* downloads its books
//!   that are not on the device yet into its folder (see `native_sync`); the
//!   action `opds.sync` can also be put into menus and rules.
//! - Books go to the library folder when downloaded one by one, in the
//!   format PaperPaw reads best of those offered.
//!
//! Feeds are read in pieces and books written straight to storage, so
//! catalogs of any size work on small devices too. This is a native plugin
//! (interface 7): the readers and engines are host-free where they can be
//! (`native_model`, `native_json`, `native_dav`, `native_search`,
//! `native_catalogs`, `native_covers`) or drive the host's services through
//! small facades (`native_web`, `native_browse`, `native_sync`), and
//! `native_plugin` is the screens. `PORTING-7.md` says what is not ported.

// The heap word is assembly (`native_plugin!`); Xtensa still gates it.
#![cfg_attr(target_arch = "xtensa", feature(asm_experimental_arch))]
#![no_std]

extern crate alloc;

#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and the declaration test also use the builders"
)]
pub mod declarations;
#[doc(hidden)]
pub mod native_browse;
#[doc(hidden)]
pub mod native_catalogs;
#[doc(hidden)]
pub mod native_covers;
#[doc(hidden)]
pub mod native_dav;
#[doc(hidden)]
pub mod native_download;
mod native_entry;
#[doc(hidden)]
pub mod native_files;
#[doc(hidden)]
pub mod native_json;
#[doc(hidden)]
pub mod native_keep;
#[doc(hidden)]
pub mod native_model;
/// The interface-7 catalogs (see `PORTING-7.md`).
pub mod native_plugin;
#[doc(hidden)]
pub mod native_search;
#[doc(hidden)]
pub mod native_sync;
#[doc(hidden)]
pub mod native_web;

#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;

/// The plugin's id.
pub const PLUGIN: &str = "opds";

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixtures")]
    use paperpaw_i18n::{Catalog, check};

    #[test]
    fn translations_are_complete() {
        let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
        let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
        let issues = check::compare(&en, &de);
        assert!(issues.is_empty(), "{issues:?}");
    }
}
