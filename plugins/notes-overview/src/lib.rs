//! All notes: the bookmarks and highlights the reader keeps,
//! seen across the library, and written out in Kindle's `My Clippings.txt`
//! form.
//!
//! *Tools › All notes* lists the books that have notes with how
//! many; a book opens its notes, a note opens the book there. At the end
//! of the list, *Export as My Clippings.txt* writes the notes of every book
//! to `Exports/My Clippings.txt` beside the library folder.
//!
//! It reads the library's books and their annotations through the library
//! service (`core.library`), a page at a time, so a large library costs
//! time, not memory (see `native_scan`).

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
