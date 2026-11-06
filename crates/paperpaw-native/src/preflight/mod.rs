//! Acceptance before any plugin code can run (ADR 0034, section 6).
//!
//! On a host target a plugin is a shared library, and `dlopen` runs its
//! initialisers while it loads. So what is checked is checked on the
//! file's bytes, before it is loaded, from the structures the system's
//! loader itself reads (not from section names, which can be absent or
//! wrong). Nothing here runs plugin code or allocates.

mod elf;
#[cfg(test)]
mod tests;

pub use elf::{DataExport, check_elf64_le, data_export};
#[cfg(feature = "kernel")]
pub use elf::{FreestandingRefusal, check_freestanding};

/// The dynamic symbols a host plugin library may define, and no others: the
/// metadata locator and the entry. The one list for the loader and for the
/// packager's check ([`check_freestanding`]), so no caller types the two
/// names a second time; a test ties it to `abi/abi.def`. It lives here, not
/// in `host_glue`, because it is a rule of the check and not part of the flat
/// plugin boundary, whose surface it must not widen. Behind the `kernel`
/// feature (ADR 0020): the frozen interface of this crate does not grow.
#[cfg(feature = "kernel")]
pub const PLUGIN_EXPORTS: &[&str] = &[
    crate::host_glue::METADATA_SYMBOL,
    crate::host_glue::ENTRY_SYMBOL,
];

#[cfg(all(test, feature = "kernel"))]
mod export_list_tests {
    extern crate std;

    #[test]
    fn plugin_exports_are_exactly_the_symbols_abi_def_names() {
        let def = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../abi/abi.def"));
        for (key, name) in [
            ("symbol", crate::host_glue::METADATA_SYMBOL),
            ("entry_symbol", crate::host_glue::ENTRY_SYMBOL),
        ] {
            assert!(def.contains(&std::format!("\"{key}\": \"{name}\"")));
        }
        assert_eq!(
            super::PLUGIN_EXPORTS,
            &["PAPERPAW_METADATA", "PAPERPAW_ENTRY"][..]
        );
    }
}

/// Why a file is refused before it is loaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Refusal {
    /// Not an ELF file, or one this check does not read (32-bit, big
    /// endian, not a shared object): refused, never guessed at.
    NotSupported,
    /// A header or table lies (partly) outside the file.
    Truncated,
    /// Built for another machine than the one asked for.
    OtherMachine,
    /// No check exists yet for this platform's libraries: refused, not
    /// accepted (a shell may let a user opt in, in the open).
    UnsupportedPlatform,
    /// Loadable segments that overlap, are too many, or are laid out so that
    /// the file and the system loader could read it differently; a missing or
    /// executable-stack request.
    Segments,
    /// The dynamic section is missing, appears twice, is not where its address
    /// maps, or lies in a segment the system loader cannot write.
    DynamicSection,
    /// A dynamic tag that is unknown, repeated, or names a table this check
    /// does not read.
    DynamicTag,
    /// A relocation table the system loader would fault on, or that cannot be
    /// read as the check reads it.
    Relocation,
    /// The symbol tables or their hash tables disagree or are inconsistent.
    Symbols,
    /// The string table is empty, unterminated, or a name lies outside it.
    Strings,
    /// The file has a way to run code, or to load more code, while it
    /// loads. The text names the mechanism.
    RunsCodeAtLoad(&'static str),
}
