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

pub use elf::check_elf64_le;

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
