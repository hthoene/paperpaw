//! Customizing menus on the device: reorder, remove and add entries of any
//! menu, or return it to how its plugins define it. Changes apply at once
//! and are saved in `[menus]` of `paperpaw.ini`.
//!
//! One cannot lock oneself out: a menu that leads to the settings (directly
//! or through submenus) keeps doing so, and this editor stays on the
//! settings page. Back arrows and buttons do not depend on menus anyway.
//! Nor can a menu be put into one of its own submenus: such a circle
//! would hide both from the list. Menus caught in one all the same (by
//! an edited `paperpaw.ini`) are listed, and *Reset all* resets every
//! menu changed, listed or not.
//!
//!
//! The plugin is the interface-7 plugin [`native_plugin::NativeMenuEditor`];
//! the interface-6 plugin is gone.

// The heap word is assembly (`native_plugin!`); Xtensa still gates it.
#![cfg_attr(target_arch = "xtensa", feature(asm_experimental_arch))]
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

mod native_entry;
/// The menus, their entries and the edits, without the host.
pub mod native_menu;
#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;
/// The interface-7 plugin.
pub mod native_plugin;

/// How it works, by language.
pub static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

#[cfg(test)]
mod tests {
    #[test]
    fn translations_are_complete() {
        use paperpaw_i18n::{Catalog, check};
        let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
        let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
        let issues = check::compare(&en, &de);
        assert!(issues.is_empty(), "{issues:?}");
    }
}
