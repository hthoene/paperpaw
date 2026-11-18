//! A simple browser for reading the web and fetching books and fonts.
//! Off until switched on in the plugin manager.
//!
//! - The start screen offers *Search or enter address*, the bookmarks and
//!   the pages read lately. Holding an entry (with buttons: a long press
//!   on confirm) offers what can be done with it.
//! - A web page opens in the reader, which shows its reading matter like
//!   a book (see the HTML format). Its links lead to further pages;
//!   *Back* returns to the page before, and from the first one to the
//!   start screen.
//! - Books (EPUB, text) go to the library, fonts to the fonts folder
//!   (usable after a restart), anything else to `Downloads`.
//!
//! Bound to the slot `core.browser`, so the reader opens links out of
//! books here; another browser plugin can take its place.
//!
//! A native interface-7 plugin only (`native_plugin`): the network is
//! `core.net`, books go into the library with `library.import`, and what the
//! plugin decides without a host is in `native_web`, `native_store` and
//! `native_save` (`PORTING-7.md` names what waits for the host).

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
pub mod declarations;
mod native_entry;
#[cfg(test)]
mod native_format_tests;
/// The interface-7 browser.
pub mod native_plugin;
pub mod native_save;
pub mod native_store;
#[cfg(test)]
mod native_tests;
pub mod native_web;

#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;

use paperpaw_sdk::Id;

pub const PLUGIN: Id = Id::new_static("browser");

/// Help text published by the host during atomic native admission.
pub static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];
/// Catalog sources published before native callbacks.
pub static CATALOGS: [(&str, &str); 2] = [
    ("en", include_str!("../i18n/en.ini")),
    ("de", include_str!("../i18n/de.ini")),
];

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
