//! Translation: selected text translated online by DeepL, bound to the
//! slot `core.translate` (the reader offers *Translate* for selected text
//! when there is a network). Other services can bind there instead.
//!
//! DeepL needs an API key of one's own: free at deepl.com for up to
//! 500,000 characters a month. Without one, the panel says so and a tap
//! asks for it; it is kept with the other passwords and keys (see
//! `paperpaw_sdk::secrets`), where it can be changed or removed. The text
//! is translated into the language chosen (setting `translate.to`), by
//! default PaperPaw's own.
//!
//! The translation shows over the page like a dictionary entry: the page
//! buttons or taps scroll it, confirm or a tap at its end closes it.
//!
//! This is an interface-7 native plugin ([`native_plugin::NativeTranslate`]);
//! see `PORTING-7.md`.

#![no_std]

extern crate alloc;

#[cfg(test)]
extern crate std;

#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and the declaration tests also use the builders"
)]
pub mod declarations;
pub mod native_ask;
mod native_entry;
#[doc(hidden)]
pub mod native_model;
#[cfg(test)]
mod native_model_tests;
pub mod native_panel;
/// The interface-7 translation plugin (slice 1; see `PORTING-7.md`).
pub mod native_plugin;

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
