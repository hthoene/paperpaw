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
pub enum Refusal {
    /// Not an ELF file, or one this check does not read (32-bit, big
    /// endian, not a shared object): refused, never guessed at.
    NotSupported,
    /// A header or table lies (partly) outside the file.
    Truncated,
    /// Built for another machine than the one asked for.
    OtherMachine,
    /// The file has a way to run code, or to load more code, while it
    /// loads. The text names the mechanism.
    RunsCodeAtLoad(&'static str),
}
