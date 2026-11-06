//! Builds and checks PaperPaw's hyphenation data plugins.
//!
//! A hyphenation plugin is a folder without code:
//!
//! ```text
//! hyphenation-de/
//!   plugin.ini              [plugin] id, name, description, version, formats = 5
//!                           [hyphenation] de = hyphenation/de.pphp
//!   i18n/en.ini, de.ini     the plugin's name and description
//!   how/en.txt, de.txt      how it works (for people; the kernel does not read it)
//!   hyphenation/de.pphp     the compiled table (PPHP version 1)
//!   hyphenation/README.md   where the patterns come from, and their licence
//! ```
//!
//! The `[hyphenation]` section is the file kind of
//! `docs/abi/proposals/declarative-registration.md`: a line for each
//! language, `<BCP 47 tag> = <relative path>`, at most [`MAX_LANGUAGES`] a
//! plugin, one table (at most [`MAX_TABLE`] bytes) that two tags may share.
//! [`check`] looks at a folder as the loader and `paperpaw-dev package` will:
//! on the host, without running anything.

mod check;
mod packages;
mod table;
mod write;

pub use check::{Problem, check};
pub use packages::{FORMATS, PACKAGES, Package, VERSION};
pub use table::table_check;
pub use write::{build, build_all};

/// Most languages (lines of `[hyphenation]`) of one plugin.
pub const MAX_LANGUAGES: usize = 8;
/// The largest table file, in bytes (provisional until the X4 gate).
pub const MAX_TABLE: u64 = 2 * 1024 * 1024;
/// The longest path in a plugin folder, in bytes.
pub const MAX_PATH: usize = 100;
/// The longest `plugin.ini`, in bytes.
pub const MAX_MANIFEST: u64 = 4 * 1024;
/// The longest translation catalog or text file, in bytes.
pub const MAX_FILE: u64 = 64 * 1024;
/// The longest BCP 47 tag, in bytes.
pub const MAX_TAG: usize = 35;

#[cfg(test)]
mod tests;
